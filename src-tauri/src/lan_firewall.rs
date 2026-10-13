//! Windows Firewall rule that lets other devices on the home network reach the
//! embedded web server in LAN mode.
//!
//! Windows asks about a new listening app once. Anyone who dismisses that
//! prompt gets inbound block rules for the exe, and block rules beat allow
//! rules, so LAN mode silently stops working. The fix removes those block
//! rules and adds one allow rule for this exe on private and domain networks
//! (the same scope as ticking "Private networks" in the Windows prompt). A
//! program rule rather than a port rule also covers the mDNS responder behind
//! `mooshieui.local` and a fallback port. Public networks are left alone.
//!
//! Other platforms have no firewall in the way by default, so they report
//! [`FirewallState::Unsupported`].

use serde::Serialize;

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FirewallState {
    /// Not Windows: nothing to manage.
    Unsupported,
    /// An enabled inbound allow rule covers this exe on private networks.
    Allowed,
    /// No rule either way. Windows will prompt (or quietly block) when the
    /// server binds on all interfaces.
    Missing,
    /// An inbound block rule covers this exe on private networks.
    Blocked,
    /// The rules could not be read.
    Unknown,
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct FirewallStatus {
    pub state: FirewallState,
    /// The network carrying the default route is a Windows "Public" network,
    /// where the private-network rule does not apply.
    pub public_network: bool,
}

/// Display name of the allow rule this module manages.
#[cfg_attr(not(any(windows, test)), allow(dead_code))]
const RULE_NAME: &str = "MooshieUI LAN access";

/// `ERROR_CANCELLED`: the launcher's exit code when the UAC prompt is declined.
#[cfg_attr(not(any(windows, test)), allow(dead_code))]
const EXIT_CANCELLED: i32 = 1223;

#[derive(Debug, PartialEq, Eq)]
pub enum AllowError {
    Unsupported,
    /// The user said no at the UAC prompt.
    Cancelled,
    Failed(String),
}

impl std::fmt::Display for AllowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AllowError::Unsupported => write!(f, "Firewall rules are only managed on Windows"),
            AllowError::Cancelled => write!(f, "The Windows admin prompt was declined"),
            AllowError::Failed(msg) => write!(f, "Could not update Windows Firewall: {msg}"),
        }
    }
}

/// Read the current rule state. Needs no elevation.
pub async fn status() -> FirewallStatus {
    tokio::task::spawn_blocking(status_blocking)
        .await
        .unwrap_or(FirewallStatus {
            state: FirewallState::Unknown,
            public_network: false,
        })
}

/// Remove this exe's private-network block rules and add the allow rule.
/// Shows one UAC prompt on the PC and waits for it.
pub async fn allow() -> Result<FirewallStatus, AllowError> {
    tokio::task::spawn_blocking(allow_blocking)
        .await
        .map_err(|e| AllowError::Failed(e.to_string()))?
}

#[cfg(not(windows))]
fn status_blocking() -> FirewallStatus {
    FirewallStatus {
        state: FirewallState::Unsupported,
        public_network: false,
    }
}

#[cfg(not(windows))]
fn allow_blocking() -> Result<FirewallStatus, AllowError> {
    Err(AllowError::Unsupported)
}

#[cfg(windows)]
fn status_blocking() -> FirewallStatus {
    let unknown = FirewallStatus {
        state: FirewallState::Unknown,
        public_network: false,
    };
    let Ok(exe) = std::env::current_exe() else {
        return unknown;
    };
    match run_powershell(&status_script(&exe)) {
        Ok(out) if out.status.success() => {
            parse_status(&String::from_utf8_lossy(&out.stdout)).unwrap_or(unknown)
        }
        Ok(out) => {
            log::warn!(
                "lan_firewall: status check failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            );
            unknown
        }
        Err(e) => {
            log::warn!("lan_firewall: could not run PowerShell: {e}");
            unknown
        }
    }
}

#[cfg(windows)]
fn allow_blocking() -> Result<FirewallStatus, AllowError> {
    let exe = std::env::current_exe().map_err(|e| AllowError::Failed(e.to_string()))?;
    let out = run_powershell(&elevate_script(&allow_script(&exe)))
        .map_err(|e| AllowError::Failed(e.to_string()))?;
    match out.status.code() {
        Some(0) => {
            log::info!("lan_firewall: allow rule added for {}", exe.display());
            Ok(status_blocking())
        }
        Some(EXIT_CANCELLED) => Err(AllowError::Cancelled),
        code => Err(AllowError::Failed(format!(
            "PowerShell exited with {code:?}"
        ))),
    }
}

#[cfg(windows)]
fn run_powershell(script: &str) -> std::io::Result<std::process::Output> {
    crate::comfyui::process::std_command_no_window("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-EncodedCommand",
            &encode_command(script),
        ])
        .output()
}

/// `-EncodedCommand` takes base64 of the UTF-16LE script, which sidesteps
/// every layer of command-line quoting.
#[cfg_attr(not(any(windows, test)), allow(dead_code))]
fn encode_command(script: &str) -> String {
    use base64::Engine;
    let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// A path for a single-quoted PowerShell literal: every quote character
/// PowerShell accepts as one (ASCII and the U+2018..U+201B family) is doubled.
#[cfg(any(windows, test))]
fn ps_quote(path: &std::path::Path) -> String {
    let mut quoted = String::new();
    for ch in path.display().to_string().chars() {
        if matches!(ch, '\'' | '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}') {
            quoted.push(ch);
        }
        quoted.push(ch);
    }
    quoted
}

/// Pipeline selecting this exe's enabled inbound rules. Rules Windows creates
/// from its own prompt store the path in whatever case it was launched with,
/// so match case-insensitively after expanding environment variables.
#[cfg(any(windows, test))]
fn exe_rules_pipeline(exe: &std::path::Path) -> String {
    format!(
        "Get-NetFirewallApplicationFilter | Where-Object {{ $_.Program -and \
         [Environment]::ExpandEnvironmentVariables($_.Program) -ieq '{exe}' }} | \
         Get-NetFirewallRule | Where-Object {{ $_.Direction -eq 'Inbound' -and $_.Enabled -eq 'True' }}",
        exe = ps_quote(exe),
    )
}

/// Prints `<allow rules> <block rules> <public 0|1>`, counting rules that
/// apply on private networks (profile Any = 0, or the Private bit = 2).
#[cfg(any(windows, test))]
fn status_script(exe: &std::path::Path) -> String {
    format!(
        "$ErrorActionPreference = 'SilentlyContinue'\n\
         $rules = @({rules} | Where-Object {{ $p = [int]$_.Profile; $p -eq 0 -or ($p -band 2) }})\n\
         $allow = @($rules | Where-Object {{ $_.Action -eq 'Allow' }}).Count\n\
         $block = @($rules | Where-Object {{ $_.Action -eq 'Block' }}).Count\n\
         $public = 0\n\
         $route = Get-NetRoute -DestinationPrefix '0.0.0.0/0' | Sort-Object RouteMetric | Select-Object -First 1\n\
         if ($route) {{ $np = Get-NetConnectionProfile -InterfaceIndex $route.ifIndex; if ($np.NetworkCategory -eq 'Public') {{ $public = 1 }} }}\n\
         Write-Output \"$allow $block $public\"\n",
        rules = exe_rules_pipeline(exe),
    )
}

/// Runs elevated: drop this exe's block rules that touch private or domain
/// networks (profile Any = 0, Domain = 1, Private = 2), then replace the
/// managed allow rule.
#[cfg(any(windows, test))]
fn allow_script(exe: &std::path::Path) -> String {
    format!(
        "$ErrorActionPreference = 'Stop'\n\
         try {{\n\
         {rules} | Where-Object {{ $_.Action -eq 'Block' -and ([int]$_.Profile -eq 0 -or ([int]$_.Profile -band 3)) }} | Remove-NetFirewallRule -ErrorAction SilentlyContinue\n\
         Get-NetFirewallRule -DisplayName '{name}' -ErrorAction SilentlyContinue | Remove-NetFirewallRule\n\
         New-NetFirewallRule -DisplayName '{name}' -Description 'Lets devices on your home network open MooshieUI in LAN mode.' \
         -Direction Inbound -Action Allow -Program '{exe}' -Profile Private,Domain | Out-Null\n\
         exit 0\n\
         }} catch {{ exit 1 }}\n",
        rules = exe_rules_pipeline(exe),
        name = RULE_NAME,
        exe = ps_quote(exe),
    )
}

/// Runs `inner` in an elevated, hidden PowerShell and exits with its code,
/// or with [`EXIT_CANCELLED`] when the UAC prompt is declined.
#[cfg(any(windows, test))]
fn elevate_script(inner: &str) -> String {
    format!(
        "try {{\n\
         $p = Start-Process -FilePath 'powershell.exe' -Verb RunAs -WindowStyle Hidden -Wait -PassThru \
         -ArgumentList '-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-EncodedCommand','{encoded}'\n\
         exit $p.ExitCode\n\
         }} catch {{ exit {cancelled} }}\n",
        encoded = encode_command(inner),
        cancelled = EXIT_CANCELLED,
    )
}

/// Parse `status_script` output. A block rule wins over any allow rule, as it
/// does in Windows Firewall itself.
#[cfg_attr(not(any(windows, test)), allow(dead_code))]
fn parse_status(stdout: &str) -> Option<FirewallStatus> {
    let line = stdout.lines().rev().find(|l| !l.trim().is_empty())?;
    let mut parts = line.split_whitespace().map(str::parse::<u32>);
    let (Some(Ok(allow)), Some(Ok(block)), Some(Ok(public)), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return None;
    };
    let state = if block > 0 {
        FirewallState::Blocked
    } else if allow > 0 {
        FirewallState::Allowed
    } else {
        FirewallState::Missing
    };
    Some(FirewallStatus {
        state,
        public_network: public > 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn block_rules_win_over_allow_rules() {
        let s = parse_status("2 1 0\r\n").unwrap();
        assert_eq!(s.state, FirewallState::Blocked);
        assert!(!s.public_network);
    }

    #[test]
    fn allow_missing_and_public_are_read() {
        assert_eq!(parse_status("1 0 0").unwrap().state, FirewallState::Allowed);
        let missing = parse_status("0 0 1").unwrap();
        assert_eq!(missing.state, FirewallState::Missing);
        assert!(missing.public_network);
    }

    #[test]
    fn noise_before_the_counts_is_ignored_but_bad_output_is_rejected() {
        assert_eq!(
            parse_status("WARNING: something\n1 0 0\n\n").unwrap().state,
            FirewallState::Allowed
        );
        assert_eq!(parse_status(""), None);
        assert_eq!(parse_status("1 0"), None);
        assert_eq!(parse_status("1 0 0 4"), None);
        assert_eq!(parse_status("a b c"), None);
    }

    #[test]
    fn encoded_command_is_base64_utf16le() {
        // "ab" -> 61 00 62 00
        assert_eq!(encode_command("ab"), "YQBiAA==");
    }

    #[test]
    fn exe_path_quotes_are_doubled_in_every_script() {
        let exe = Path::new(r"C:\Users\Cole's PC\MooshieUI\mooshieui.exe");
        let quoted = r"C:\Users\Cole''s PC\MooshieUI\mooshieui.exe";
        assert!(status_script(exe).contains(quoted));
        let allow = allow_script(exe);
        assert!(allow.contains(&format!("-Program '{quoted}'")));
        assert!(!allow.contains("Cole's"));
    }

    #[test]
    fn allow_rule_stays_off_public_networks() {
        let script = allow_script(Path::new(r"C:\a.exe"));
        assert!(script.contains("-Profile Private,Domain"));
        assert!(!script.contains("Public"));
    }

    #[test]
    fn elevation_wraps_the_encoded_inner_script() {
        let outer = elevate_script("exit 0");
        assert!(outer.contains("-Verb RunAs"));
        assert!(outer.contains(&encode_command("exit 0")));
        assert!(outer.contains("exit 1223"));
    }
}
