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

use crate::comfyui::process::tokio_command_no_window;

/// Pinned upstream source spec, compiled with nvcc on non-Windows platforms.
pub const SAGE2_LINUX_GIT_SPEC: &str = "git+https://github.com/thu-ml/SageAttention.git@v2.2.0";

/// Where the community-maintained Windows wheels are published.
const WHEEL_RELEASE_BASE: &str = "https://github.com/woct0rdho/SageAttention/releases/download";

/// A community Windows wheel the app is allowed to install.
struct PinnedWheel {
    /// Release tag the asset is published under.
    tag: &'static str,
    file_name: &'static str,
    /// SHA-256 of the asset, enforced by uv/pip via the URL's `#sha256=` fragment.
    sha256: &'static str,
}

/// Explicit allowlist of prebuilt SageAttention v2 wheels, each pinned to its
/// release and SHA-256 (hashed from the downloaded assets).
///
/// The wheel used to be chosen at runtime from whatever the community repo's
/// latest releases held and installed unverified, so a replaced asset or a
/// compromised release would run arbitrary native code in the venv. Every
/// CUDA/torch combination cannot be pinned up front, so this lists exactly the
/// wheels the ranking in [`rank_wheel`] picked for each supported build when
/// it was frozen (the newest post release per CUDA/torch pairing, including
/// the `andhigher` stable-ABI builds covering torch >= 2.10). A build outside
/// the list gets a clear "no pinned wheel" error instead of an unvetted wheel.
/// Add entries deliberately, with the asset's SHA-256.
const SAGE2_WINDOWS_WHEELS: &[PinnedWheel] = &[
    PinnedWheel {
        tag: "v2.2.0-windows.post3",
        file_name: "sageattention-2.2.0+cu124torch2.5.1.post3-cp39-abi3-win_amd64.whl",
        sha256: "b8dc7014d4046f3ee1db774acf8a0f956e5b6620e33cec2d02ab57427e0ed31a",
    },
    PinnedWheel {
        tag: "v2.2.0-windows.post3",
        file_name: "sageattention-2.2.0+cu126torch2.6.0.post3-cp39-abi3-win_amd64.whl",
        sha256: "04cc82fbac669e3958650deea8a04843dd0dea005603c4e7241d01bdfce27ab6",
    },
    PinnedWheel {
        tag: "v2.2.0-windows.post3",
        file_name: "sageattention-2.2.0+cu128torch2.7.1.post3-cp39-abi3-win_amd64.whl",
        sha256: "7acbe7e274282e1ca9927e9e123bcdd512875492640fe55dce3c8b992506ec91",
    },
    PinnedWheel {
        tag: "v2.2.0-windows.post3",
        file_name: "sageattention-2.2.0+cu128torch2.8.0.post3-cp39-abi3-win_amd64.whl",
        sha256: "7dabcd00e63229b28f046c5a69ec37cf4756afb375dbadd1975dadec045ae21c",
    },
    PinnedWheel {
        tag: "v2.2.0-windows.post3",
        file_name: "sageattention-2.2.0+cu128torch2.9.0.post3-cp39-abi3-win_amd64.whl",
        sha256: "eb555644b49eacb26f336bbdf0701779a24c5639a620c963f48a14ab65c2c29c",
    },
    PinnedWheel {
        tag: "v2.2.0-windows.post3",
        file_name: "sageattention-2.2.0+cu130torch2.9.0.post3-cp39-abi3-win_amd64.whl",
        sha256: "5d6b5799379e3ae66ffd9acd5dcce4a7a17f4e9114a9c24d4e09b69aead20068",
    },
    PinnedWheel {
        tag: "v2.2.0-windows.post6",
        file_name: "sageattention-2.2.0+cu128torch2.9.1.post6-cp310-abi3-win_amd64.whl",
        sha256: "867862a3c582a058caf825bedeff6e36cabafe6ca954236db5544b78c626f9e9",
    },
    PinnedWheel {
        tag: "v2.2.0-windows.post6",
        file_name: "sageattention-2.2.0+cu130torch2.9.1.post6-cp310-abi3-win_amd64.whl",
        sha256: "5ee9c3fe060481f27bb001b32d8983c468c9b6b03095d84344d4cc4ec44fd74a",
    },
    PinnedWheel {
        tag: "v2.2.0-windows.post6",
        file_name: "sageattention-2.2.0+cu128torch2.10.0andhigher.post6-cp310-abi3-win_amd64.whl",
        sha256: "103e06df49486daa87c3338bf2490ada2b074da1422f2b90566a8f2f81a6b76e",
    },
    PinnedWheel {
        tag: "v2.2.0-windows.post6",
        file_name: "sageattention-2.2.0+cu130torch2.10.0andhigher.post6-cp310-abi3-win_amd64.whl",
        sha256: "1635283f5c01ec3cda58a784d0d7eabbcaffaf9511d1b263db4750e1ed7958bb",
    },
];

impl PinnedWheel {
    fn resolved(&self) -> ResolvedWheel {
        ResolvedWheel {
            // The fragment makes uv (and pip) verify the download against the
            // pinned hash; a mismatch aborts the install.
            url: format!(
                "{WHEEL_RELEASE_BASE}/{}/{}#sha256={}",
                self.tag, self.file_name, self.sha256
            ),
            file_name: self.file_name.to_string(),
        }
    }
}

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

/// Find the best pinned prebuilt Windows wheel for the given torch build.
///
/// Preference order: exact torch version match, then same torch major.minor,
/// then the highest "andhigher" floor at or below the installed torch; ties
/// break toward newer SageAttention versions and higher post releases.
///
/// Only [`SAGE2_WINDOWS_WHEELS`] is considered; nothing is looked up online.
/// The client parameter is kept so existing callers need no change.
pub async fn resolve_sage2_windows_wheel(
    _client: &reqwest::Client,
    build: &TorchBuild,
) -> Result<ResolvedWheel, String> {
    select_sage2_windows_wheel(build)
}

fn select_sage2_windows_wheel(build: &TorchBuild) -> Result<ResolvedWheel, String> {
    let mut best: Option<(WheelRank, &PinnedWheel)> = None;
    for wheel in SAGE2_WINDOWS_WHEELS {
        let Some(info) = parse_wheel_name(wheel.file_name) else {
            continue;
        };
        let Some(rank) = rank_wheel(&info, build) else {
            continue;
        };
        if best.as_ref().is_none_or(|(b, _)| rank > *b) {
            best = Some((rank, wheel));
        }
    }
    best.map(|(_, w)| w.resolved()).ok_or_else(|| {
        format!(
            "no pinned prebuilt SageAttention v2 wheel matches this environment ({}). \
             MooshieUI only installs verified wheels for CUDA 12.4-13.0 builds of \
             torch 2.5-2.9 and torch 2.10 or newer; install SageAttention manually from \
             https://github.com/woct0rdho/SageAttention/releases if you need another build",
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
    // ASCII digits only: `digits[..1]` on a multi-byte first character (the
    // tag comes from a filename) was a char-boundary panic, and `parse` alone
    // would also accept a sign.
    if digits.len() < 2 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let major = digits.get(..1)?.parse().ok()?;
    let minor = digits.get(1..)?.parse().ok()?;
    Some((major, minor))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build(torch: (u32, u32, u32), cuda: &str, python: (u32, u32)) -> TorchBuild {
        TorchBuild {
            torch,
            cuda: cuda.to_string(),
            python,
        }
    }

    #[test]
    fn cp_tags_parse_ascii_digits_and_refuse_everything_else() {
        assert_eq!(parse_cp_tag("cp310"), Some((3, 10)));
        assert_eq!(parse_cp_tag("cp39"), Some((3, 9)));
        // A multi-byte first character used to panic on `digits[..1]`.
        for bad in [
            "cpé1", "cp3é", "cp٣١٠", "cp+1", "cp3-", "cp3", "cp", "py310", "",
        ] {
            assert_eq!(parse_cp_tag(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn every_pinned_wheel_parses_and_carries_a_sha256() {
        for wheel in SAGE2_WINDOWS_WHEELS {
            assert!(
                parse_wheel_name(wheel.file_name).is_some(),
                "{} must be selectable",
                wheel.file_name
            );
            assert_eq!(wheel.sha256.len(), 64, "{}", wheel.file_name);
            assert!(wheel.sha256.bytes().all(|b| b.is_ascii_hexdigit()));
            assert!(wheel.tag.starts_with("v2.2.0-windows"));
        }
    }

    #[test]
    fn resolved_url_is_pinned_to_the_release_and_hash() {
        let wheel = select_sage2_windows_wheel(&build((2, 11, 0), "cu130", (3, 12))).unwrap();
        assert_eq!(
            wheel.file_name,
            "sageattention-2.2.0+cu130torch2.10.0andhigher.post6-cp310-abi3-win_amd64.whl"
        );
        assert_eq!(
            wheel.url,
            "https://github.com/woct0rdho/SageAttention/releases/download/v2.2.0-windows.post6/\
             sageattention-2.2.0+cu130torch2.10.0andhigher.post6-cp310-abi3-win_amd64.whl\
             #sha256=1635283f5c01ec3cda58a784d0d7eabbcaffaf9511d1b263db4750e1ed7958bb"
        );
    }

    #[test]
    fn exact_torch_match_beats_andhigher_and_same_minor() {
        let exact = select_sage2_windows_wheel(&build((2, 9, 1), "cu128", (3, 12))).unwrap();
        assert!(exact.file_name.contains("cu128torch2.9.1.post6"));
        let older = select_sage2_windows_wheel(&build((2, 9, 0), "cu128", (3, 11))).unwrap();
        assert!(older.file_name.contains("cu128torch2.9.0.post3"));
        let minor = select_sage2_windows_wheel(&build((2, 7, 0), "cu128", (3, 10))).unwrap();
        assert!(minor.file_name.contains("cu128torch2.7.1.post3"));
    }

    #[test]
    fn unlisted_builds_are_refused_rather_than_guessed() {
        let err = select_sage2_windows_wheel(&build((2, 10, 0), "cu126", (3, 12)))
            .err()
            .expect("no cu126 wheel for torch 2.10");
        assert!(err.contains("no pinned prebuilt SageAttention v2 wheel"));
        // The post6 wheels need Python >= 3.10.
        assert!(select_sage2_windows_wheel(&build((2, 10, 0), "cu128", (3, 9))).is_err());
    }
}
