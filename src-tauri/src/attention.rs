//! SageAttention v2 install resolution, shared by the setup wizard and the
//! settings install command.
//!
//! SageAttention 2.x has never been published to PyPI (the `sageattention`
//! package there stops at 1.0.6), so a plain `uv pip install
//! "sageattention>=2"` can never resolve. On Windows we instead install a
//! prebuilt wheel from the community woct0rdho/SageAttention releases,
//! matched to the venv's torch/CUDA/Python build; on Linux we compile the
//! pinned upstream source tag (which requires the CUDA toolkit).

use std::path::Path;

use serde::Deserialize;

use crate::comfyui::process::tokio_command_no_window;

/// Pinned upstream source spec, compiled with nvcc on non-Windows platforms.
pub const SAGE2_LINUX_GIT_SPEC: &str = "git+https://github.com/thu-ml/SageAttention.git@v2.2.0";

/// GitHub releases of the community-maintained Windows wheels for SageAttention 2.x.
const WHEEL_RELEASES_API: &str =
    "https://api.github.com/repos/woct0rdho/SageAttention/releases?per_page=20";

/// The venv's installed PyTorch build, as needed to pick a compatible wheel.
#[derive(Debug)]
pub struct TorchBuild {
    pub torch: (u32, u32, u32),
    /// CUDA tag from torch's local version, e.g. "cu128".
    pub cuda: String,
    pub python: (u32, u32),
}

impl TorchBuild {
    pub fn describe(&self) -> String {
        format!(
            "torch {}.{}.{}+{}, Python {}.{}",
            self.torch.0, self.torch.1, self.torch.2, self.cuda, self.python.0, self.python.1
        )
    }
}

/// A prebuilt wheel picked from the release assets.
pub struct ResolvedWheel {
    pub url: String,
    pub file_name: String,
}

/// Ask the venv's Python for the installed torch version and interpreter
/// version. Errors are user-facing strings.
pub async fn probe_torch_build(venv_python: &Path) -> Result<TorchBuild, String> {
    let mut cmd = tokio_command_no_window(venv_python);
    cmd.args([
        "-c",
        "import sys, torch; print(torch.__version__); print('%d.%d' % sys.version_info[:2])",
    ]);
    let output = cmd
        .output()
        .await
        .map_err(|e| format!("failed to run the venv's python: {}", e))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "could not detect the venv's PyTorch build (is PyTorch installed?): {}",
            stderr.trim()
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut lines = stdout.lines();
    let torch_version = lines.next().unwrap_or("").trim().to_string();
    let py_version = lines.next().unwrap_or("").trim().to_string();

    let (torch_part, local) = torch_version
        .split_once('+')
        .unwrap_or((torch_version.as_str(), ""));
    if !local.starts_with("cu") {
        return Err(format!(
            "the venv's PyTorch ({}) is not a CUDA build; SageAttention v2 requires CUDA PyTorch",
            torch_version
        ));
    }
    let torch = parse_version3(torch_part).ok_or_else(|| {
        format!(
            "could not parse the venv's PyTorch version: {}",
            torch_version
        )
    })?;
    let python = parse_version2(&py_version)
        .ok_or_else(|| format!("could not parse the venv's Python version: {}", py_version))?;
    Ok(TorchBuild {
        torch,
        cuda: local.to_string(),
        python,
    })
}

/// Find the best prebuilt Windows wheel for the given torch build.
///
/// Preference order: exact torch version match, then same torch major.minor,
/// then the highest "andhigher" floor at or below the installed torch; ties
/// break toward newer SageAttention versions and higher post releases.
pub async fn resolve_sage2_windows_wheel(
    client: &reqwest::Client,
    build: &TorchBuild,
) -> Result<ResolvedWheel, String> {
    #[derive(Deserialize)]
    struct Release {
        assets: Vec<Asset>,
    }
    #[derive(Deserialize)]
    struct Asset {
        name: String,
        browser_download_url: String,
    }

    let releases: Vec<Release> = client
        .get(WHEEL_RELEASES_API)
        .header(reqwest::header::USER_AGENT, "MooshieUI")
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| {
            format!(
                "could not reach github.com to look up SageAttention v2 wheels: {}",
                e
            )
        })?
        .error_for_status()
        .map_err(|e| format!("GitHub wheel lookup failed: {}", e))?
        .json()
        .await
        .map_err(|e| format!("could not parse the GitHub wheel list: {}", e))?;

    let mut best: Option<(WheelRank, ResolvedWheel)> = None;
    for release in &releases {
        for asset in &release.assets {
            let Some(info) = parse_wheel_name(&asset.name) else {
                continue;
            };
            let Some(rank) = rank_wheel(&info, build) else {
                continue;
            };
            if best.as_ref().is_none_or(|(b, _)| rank > *b) {
                best = Some((
                    rank,
                    ResolvedWheel {
                        url: asset.browser_download_url.clone(),
                        file_name: asset.name.clone(),
                    },
                ));
            }
        }
    }
    best.map(|(_, w)| w).ok_or_else(|| {
        format!(
            "no prebuilt SageAttention v2 wheel matches this environment ({}); \
             see https://github.com/woct0rdho/SageAttention/releases for supported combinations",
            build.describe()
        )
    })
}

/// Compatibility metadata parsed from a wheel filename such as
/// `sageattention-2.2.0+cu128torch2.9.1.post6-cp310-abi3-win_amd64.whl` or
/// `sageattention-2.2.0+cu128torch2.10.0andhigher.post6-cp310-abi3-win_amd64.whl`.
struct WheelInfo {
    sage: (u32, u32, u32),
    cuda: String,
    torch: (u32, u32, u32),
    and_higher: bool,
    post: u32,
    py_floor: (u32, u32),
}

/// (compat category, sage version, post, torch) — higher is better.
type WheelRank = (u8, (u32, u32, u32), u32, (u32, u32, u32));

fn rank_wheel(info: &WheelInfo, build: &TorchBuild) -> Option<WheelRank> {
    if info.cuda != build.cuda || info.py_floor > build.python {
        return None;
    }
    let category = if info.and_higher {
        if info.torch > build.torch {
            return None;
        }
        0
    } else if info.torch == build.torch {
        2
    } else if (info.torch.0, info.torch.1) == (build.torch.0, build.torch.1) {
        1
    } else {
        return None;
    };
    Some((category, info.sage, info.post, info.torch))
}

fn parse_wheel_name(name: &str) -> Option<WheelInfo> {
    let rest = name.strip_prefix("sageattention-")?.strip_suffix(".whl")?;
    // Wheel filename layout: {version}-{python tag}-{abi tag}-{platform tag}
    let mut parts = rest.split('-');
    let version = parts.next()?;
    let py_tag = parts.next()?;
    let abi_tag = parts.next()?;
    let platform = parts.next()?;
    if parts.next().is_some() || platform != "win_amd64" || abi_tag != "abi3" {
        return None;
    }
    let py_floor = parse_cp_tag(py_tag)?;

    // Version layout: {sage}+cu{cuda}torch{torch}[andhigher][.post{N}]
    let (sage_str, local) = version.split_once('+')?;
    let sage = parse_version3(sage_str)?;
    if sage.0 != 2 {
        return None;
    }
    let local = local.strip_prefix("cu")?;
    let torch_pos = local.find("torch")?;
    let cuda = format!("cu{}", &local[..torch_pos]);
    let mut torch_part = &local[torch_pos + "torch".len()..];
    let mut post = 0u32;
    if let Some(idx) = torch_part.find(".post") {
        post = torch_part[idx + ".post".len()..].parse().ok()?;
        torch_part = &torch_part[..idx];
    }
    let and_higher = torch_part.ends_with("andhigher");
    let torch_str = torch_part.strip_suffix("andhigher").unwrap_or(torch_part);
    let torch = parse_version3(torch_str)?;
    Some(WheelInfo {
        sage,
        cuda,
        torch,
        and_higher,
        post,
        py_floor,
    })
}

/// "2.9.1" → (2, 9, 1); a missing patch component defaults to 0.
fn parse_version3(s: &str) -> Option<(u32, u32, u32)> {
    let mut it = s.split('.');
    let major = it.next()?.parse().ok()?;
    let minor = it.next()?.parse().ok()?;
    let patch = match it.next() {
        Some(p) => p.parse().ok()?,
        None => 0,
    };
    if it.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

/// "3.11" → (3, 11)
fn parse_version2(s: &str) -> Option<(u32, u32)> {
    let (a, b) = s.split_once('.')?;
    Some((a.parse().ok()?, b.parse().ok()?))
}

/// "cp310" → (3, 10). CPython major versions are single-digit.
fn parse_cp_tag(s: &str) -> Option<(u32, u32)> {
    let digits = s.strip_prefix("cp")?;
    if digits.len() < 2 {
        return None;
    }
    let major = digits[..1].parse().ok()?;
    let minor = digits[1..].parse().ok()?;
    Some((major, minor))
}
