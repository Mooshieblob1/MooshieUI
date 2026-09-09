//! The managed Apple Silicon runtime, shared by desktop and server builds.

pub const MACOS_PYTHON: &str = include_str!("../../runtime/macos-python.txt");
pub const MACOS_CONSTRAINTS: &str = include_str!("../../runtime/macos-constraints.txt");

/// Mac CPU and MPS share the same wheels, so preserve the selected execution mode.
pub fn set_macos_cpu_mode(args: &mut Vec<String>, cpu: bool) {
    args.retain(|arg| arg != "--cpu");
    if cpu {
        args.push("--cpu".into());
    }
}

/// Fail before spawning ComfyUI with options that cannot run on Apple MPS.
pub fn validate_macos_args(args: &[String]) -> Result<(), String> {
    for arg in args {
        let flag = arg.split('=').next().unwrap_or(arg);
        if matches!(
            flag,
            "--use-sage-attention"
                | "--use-flash-attention"
                | "--cuda-device"
                | "--directml"
                | "--fp8_e4m3fn-unet"
                | "--fp8_e5m2-unet"
                | "--fp8_e4m3fn-text-enc"
                | "--fp8_e5m2-text-enc"
        ) {
            return Err(format!(
                "ComfyUI option {flag} is not supported by the managed Mac runtime. \
                 Remove it from Settings > Extra arguments and use default attention and precision."
            ));
        }
    }
    Ok(())
}

/// All app-managed pip paths call this, including custom-node and package installs.
pub fn apply_constraints(cmd: &mut tokio::process::Command) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let base = crate::config::app_data_dir()
            .ok_or_else(|| "Cannot locate the Mac runtime data directory".to_string())?
            .join("runtime");
        std::fs::create_dir_all(&base).map_err(|e| e.to_string())?;
        let path = base.join("macos-constraints.txt");
        if std::fs::read_to_string(&path).ok().as_deref() != Some(MACOS_CONSTRAINTS) {
            // Rename on macOS is atomic: concurrent installers never read a
            // partially written constraint file.
            let temporary = base.join(format!("constraints-{}.tmp", uuid::Uuid::new_v4()));
            std::fs::write(&temporary, MACOS_CONSTRAINTS).map_err(|e| e.to_string())?;
            std::fs::rename(&temporary, &path).map_err(|e| e.to_string())?;
        }
        cmd.arg("--constraint").arg(path);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = cmd;
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn validate_host() -> Result<(), String> {
    if !cfg!(target_arch = "aarch64") {
        return Err("Local Mac setup requires the native Apple Silicon app. Intel Macs can use a remote MooshieUI server in their browser.".into());
    }
    let output = std::process::Command::new("/usr/bin/sw_vers")
        .arg("-productVersion")
        .output()
        .map_err(|e| format!("Cannot determine the macOS version: {e}"))?;
    let version = String::from_utf8_lossy(&output.stdout);
    let major = version
        .trim()
        .split('.')
        .next()
        .and_then(|s| s.parse::<u32>().ok());
    if !output.status.success() || !matches!(major, Some(14..)) {
        return Err("Local Apple Silicon generation requires macOS 14 or later.".into());
    }
    Ok(())
}

#[cfg(target_os = "macos")]
pub async fn verify_python(python: &std::path::Path, require_mps: bool) -> Result<String, String> {
    let output = tokio::process::Command::new(python)
        .args(["-c", include_str!("../../runtime/macos_probe.py")])
        .output()
        .await
        .map_err(|e| format!("Cannot verify the Mac Python runtime: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "Mac runtime verification failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let report: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("Invalid Mac runtime report: {e}"))?;
    if report["architecture"] != "arm64" {
        return Err("Python is not ARM-native. Re-run setup with the Apple Silicon app.".into());
    }
    if report["python"] != MACOS_PYTHON.trim() {
        return Err(
            "The Mac Python version does not match the managed runtime. Re-run setup to repair it."
                .into(),
        );
    }
    for line in MACOS_CONSTRAINTS.lines() {
        if let Some((name, version)) = line.split_once("==") {
            if report["packages"][name] != version {
                return Err(format!(
                    "Mac runtime requires {name}=={version}. Re-run setup to repair it."
                ));
            }
        }
    }
    if require_mps && report["mps_operation"] != true {
        return Err(format!("Apple Metal could not execute a tensor operation. Select CPU explicitly to continue without acceleration. Runtime report: {text}"));
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mac_cpu_selection_survives_restart_and_can_switch_back_to_metal() {
        let mut args = vec!["--fp32-vae".into(), "--cpu".into()];
        set_macos_cpu_mode(&mut args, true);
        assert_eq!(args, ["--fp32-vae", "--cpu"]);
        set_macos_cpu_mode(&mut args, false);
        assert_eq!(args, ["--fp32-vae"]);
    }

    #[test]
    fn mac_rejects_cuda_and_fp8_launch_overrides() {
        for arg in [
            "--cuda-device=0",
            "--use-sage-attention",
            "--use-flash-attention",
            "--fp8_e4m3fn-unet",
        ] {
            assert!(validate_macos_args(&[arg.into()]).is_err());
        }
        assert!(validate_macos_args(&["--cpu".into(), "--fp32-vae".into()]).is_ok());
    }

    #[test]
    fn managed_mac_runtime_pins_match() {
        assert_eq!(MACOS_PYTHON.trim().split('.').count(), 3);
        let pins: Vec<_> = MACOS_CONSTRAINTS
            .lines()
            .filter_map(|s| s.split_once("=="))
            .collect();
        assert_eq!(pins.len(), 3);
        assert_eq!(pins[0].0, "torch");
        assert_eq!(pins[2], ("torchaudio", pins[0].1));
    }
}
