//! Pre-update ComfyUI snapshots and rollback support.
//!
//! Captures the active ComfyUI commit, installed git custom-node revisions,
//! and frozen Python environment before an in-app update runs so the user can
//! roll back if a new ComfyUI release breaks an extension or model stack.
//!
//! Compiled in both `desktop` and `server` builds; keep `tauri` imports out.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const SNAPSHOT_DIR_NAME: &str = "comfyui-snapshots";
const SNAPSHOT_VERSION: u32 = 1;
const MAX_SNAPSHOTS: usize = 3;
const FREEZE_TIMEOUT_SECS: u64 = 45;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotNodeRev {
    pub name: String,
    pub revision: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComfyUiSnapshot {
    pub id: String,
    pub version: u32,
    pub created_unix: i64,
    pub comfyui_version: Option<String>,
    pub comfyui_commit: String,
    pub target_ref: String,
    pub custom_nodes: Vec<SnapshotNodeRev>,
    pub python_packages: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComfyUiSnapshotSummary {
    pub id: String,
    pub created_unix: i64,
    pub comfyui_version: Option<String>,
    pub comfyui_commit: String,
    pub target_ref: String,
    pub package_count: usize,
    pub custom_node_count: usize,
}

impl ComfyUiSnapshot {
    pub fn summary(&self) -> ComfyUiSnapshotSummary {
        ComfyUiSnapshotSummary {
            id: self.id.clone(),
            created_unix: self.created_unix,
            comfyui_version: self.comfyui_version.clone(),
            comfyui_commit: self.comfyui_commit.clone(),
            target_ref: self.target_ref.clone(),
            package_count: self
                .python_packages
                .lines()
                .filter(|line| !line.trim().is_empty())
                .count(),
            custom_node_count: self.custom_nodes.len(),
        }
    }
}

pub fn snapshots_dir(base: &Path) -> PathBuf {
    base.join(SNAPSHOT_DIR_NAME)
}

pub fn is_valid_commit_sha(sha: &str) -> bool {
    sha.len() == 40 && sha.bytes().all(|b| b.is_ascii_hexdigit())
}

pub fn validate_snapshot_id(id: &str) -> Result<(), String> {
    if id.is_empty() || id.len() > 64 {
        return Err("Invalid snapshot identifier".into());
    }
    if !id
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Err("Invalid snapshot identifier".into());
    }
    Ok(())
}

/// Filter editable/local-path lines out of `pip freeze` output so a snapshot
/// can be replayed cleanly with `uv pip install -r` without broken host paths.
pub fn filter_freeze_lines(raw: &str) -> String {
    raw.lines()
        .map(str::trim)
        .filter(|line| {
            !line.is_empty()
                && !line.starts_with('#')
                && !line.starts_with("-e ")
                && !line.starts_with("--")
                && !line.starts_with("-f ")
                && !line.contains(" @ file://")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn is_safe_node_dirname(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains('/')
        && !name.contains('\\')
        && !name.contains('\0')
}

fn resolve_snapshot_file(base: &Path, id: &str) -> Result<PathBuf, String> {
    validate_snapshot_id(id)?;
    let root = snapshots_dir(base);
    let candidate = root.join(format!("{id}.json"));
    let meta = std::fs::symlink_metadata(&candidate)
        .map_err(|_| format!("Snapshot '{id}' was not found"))?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err("Invalid snapshot entry".into());
    }
    let root_canon = root
        .canonicalize()
        .map_err(|e| format!("Snapshots directory unavailable: {e}"))?;
    let file_canon = candidate
        .canonicalize()
        .map_err(|e| format!("Snapshot unavailable: {e}"))?;
    if !file_canon.starts_with(&root_canon) || file_canon.parent() != Some(root_canon.as_path()) {
        return Err("Invalid snapshot path".into());
    }
    Ok(file_canon)
}

pub fn load_snapshot(base: &Path, id: &str) -> Result<ComfyUiSnapshot, String> {
    let path = resolve_snapshot_file(base, id)?;
    let raw = std::fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read ComfyUI snapshot: {e}"))?;
    let snapshot: ComfyUiSnapshot =
        serde_json::from_str(&raw).map_err(|e| format!("Corrupt ComfyUI snapshot: {e}"))?;
    validate_snapshot_id(&snapshot.id)?;
    if snapshot.version != SNAPSHOT_VERSION {
        return Err(format!("Unsupported snapshot version {}", snapshot.version));
    }
    if !is_valid_commit_sha(&snapshot.comfyui_commit) {
        return Err("Snapshot contains an invalid ComfyUI commit hash".into());
    }
    Ok(snapshot)
}

pub fn list_snapshots(base: &Path) -> Vec<ComfyUiSnapshotSummary> {
    let dir = snapshots_dir(base);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut items = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if validate_snapshot_id(stem).is_err() {
            continue;
        }
        if let Ok(snap) = load_snapshot(base, stem) {
            items.push(snap.summary());
        }
    }
    items.sort_by(|a, b| {
        b.created_unix
            .cmp(&a.created_unix)
            .then_with(|| b.id.cmp(&a.id))
    });
    items
}

pub fn save_snapshot(
    base: &Path,
    snapshot: &ComfyUiSnapshot,
) -> Result<ComfyUiSnapshotSummary, String> {
    validate_snapshot_id(&snapshot.id)?;
    if !is_valid_commit_sha(&snapshot.comfyui_commit) {
        return Err("Cannot save snapshot without a full ComfyUI commit SHA".into());
    }
    let dir = snapshots_dir(base);
    std::fs::create_dir_all(&dir).map_err(|e| format!("Cannot create snapshots folder: {e}"))?;
    let final_path = dir.join(format!("{}.json", snapshot.id));
    let tmp_path = dir.join(format!(
        ".{}.{}.tmp",
        snapshot.id,
        uuid::Uuid::new_v4().simple()
    ));
    let bytes = serde_json::to_vec_pretty(snapshot)
        .map_err(|e| format!("Failed to serialize snapshot: {e}"))?;
    std::fs::write(&tmp_path, bytes).map_err(|e| format!("Failed to write snapshot: {e}"))?;
    if let Err(err) = std::fs::rename(&tmp_path, &final_path) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(format!("Failed to finalize snapshot: {err}"));
    }
    prune_old_snapshots(base, MAX_SNAPSHOTS);
    Ok(snapshot.summary())
}

pub fn prune_old_snapshots(base: &Path, keep: usize) {
    let dir = snapshots_dir(base);
    let list = list_snapshots(base);
    for stale in list.into_iter().skip(keep) {
        let path = dir.join(format!("{}.json", stale.id));
        if std::fs::symlink_metadata(&path)
            .map(|m| m.is_file() && !m.file_type().is_symlink())
            .unwrap_or(false)
        {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[cfg(feature = "desktop")]
async fn freeze_python_packages(uv_path: &Path, python_path: &Path) -> String {
    use std::time::Duration;

    if !python_path.is_file() {
        return String::new();
    }

    if uv_path.is_file() {
        let mut cmd = crate::comfyui::process::tokio_command_no_window(uv_path);
        cmd.args(["pip", "freeze", "--python"])
            .arg(python_path)
            .kill_on_drop(true);
        if let Ok(Ok(out)) =
            tokio::time::timeout(Duration::from_secs(FREEZE_TIMEOUT_SECS), cmd.output()).await
        {
            if out.status.success() {
                return filter_freeze_lines(&String::from_utf8_lossy(&out.stdout));
            }
        }
    }

    let mut cmd = crate::comfyui::process::tokio_command_no_window(python_path);
    cmd.args(["-m", "pip", "freeze"]).kill_on_drop(true);
    if let Ok(Ok(out)) =
        tokio::time::timeout(Duration::from_secs(FREEZE_TIMEOUT_SECS), cmd.output()).await
    {
        if out.status.success() {
            return filter_freeze_lines(&String::from_utf8_lossy(&out.stdout));
        }
    }

    String::new()
}

#[cfg(feature = "desktop")]
pub(crate) async fn capture_snapshot(
    base: &Path,
    comfyui_dir: &Path,
    git_path: &Path,
    python_path: &Path,
    uv_path: &Path,
    target_ref: &str,
) -> Result<ComfyUiSnapshotSummary, String> {
    if !comfyui_dir.join(".git").exists() {
        return Err("ComfyUI installation has no Git checkout to snapshot yet".into());
    }
    let dir_str = comfyui_dir
        .to_str()
        .ok_or_else(|| "Invalid ComfyUI directory path".to_string())?;
    let comfyui_commit = crate::comfyui::git::run_git(
        git_path,
        &["-C", dir_str, "rev-parse", "--verify", "HEAD"],
        None,
    )
    .await?;
    if !is_valid_commit_sha(&comfyui_commit) {
        return Err("ComfyUI Git HEAD did not resolve to a valid commit SHA".into());
    }

    let version_info = crate::comfyui_version::comfyui_version_info(comfyui_dir);
    let mut custom_nodes = Vec::new();
    let custom_nodes_dir = comfyui_dir.join("custom_nodes");
    if let Ok(entries) = std::fs::read_dir(&custom_nodes_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() || !path.join(".git").exists() {
                continue;
            }
            let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            if !is_safe_node_dirname(name) {
                continue;
            }
            let Some(node_str) = path.to_str() else {
                continue;
            };
            if let Ok(rev) = crate::comfyui::git::run_git(
                git_path,
                &["-C", node_str, "rev-parse", "--verify", "HEAD"],
                None,
            )
            .await
            {
                if is_valid_commit_sha(&rev) {
                    custom_nodes.push(SnapshotNodeRev {
                        name: name.to_string(),
                        revision: rev,
                    });
                }
            }
        }
    }
    custom_nodes.sort_by(|a, b| a.name.cmp(&b.name));

    let python_packages = freeze_python_packages(uv_path, python_path).await;
    let created_unix = chrono::Utc::now().timestamp();
    let suffix = &uuid::Uuid::new_v4().simple().to_string()[..8];
    let id = format!("{created_unix}-{suffix}");

    let snapshot = ComfyUiSnapshot {
        id,
        version: SNAPSHOT_VERSION,
        created_unix,
        comfyui_version: version_info.installed,
        comfyui_commit,
        target_ref: target_ref.to_string(),
        custom_nodes,
        python_packages,
    };
    save_snapshot(base, &snapshot)
}

/// Validates that a saved custom node folder name cannot traverse directories.
pub fn validate_node_dirname(name: &str) -> bool {
    is_safe_node_dirname(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "mooshie-snapshot-{name}-{}",
                uuid::Uuid::new_v4().simple()
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn sample_snapshot(id: &str, created_unix: i64) -> ComfyUiSnapshot {
        ComfyUiSnapshot {
            id: id.into(),
            version: SNAPSHOT_VERSION,
            created_unix,
            comfyui_version: Some("0.46.0".into()),
            comfyui_commit: "a".repeat(40),
            target_ref: "v0.46.2".into(),
            custom_nodes: vec![SnapshotNodeRev {
                name: "ComfyUI-GGUF".into(),
                revision: "b".repeat(40),
            }],
            python_packages: "torch==2.6.0\nnumpy==1.26.4".into(),
        }
    }

    #[test]
    fn filters_editable_and_file_urls_from_freeze() {
        let raw = "\
# comment\n\
torch==2.6.0\n\
-e /home/user/repo\n\
my_local @ file:///tmp/wheel.whl\n\
--extra-index-url https://example.com\n\
numpy==1.26.4\n\
gitpkg @ https://github.com/org/repo.git\n\
";
        assert_eq!(
            filter_freeze_lines(raw),
            "torch==2.6.0\nnumpy==1.26.4\ngitpkg @ https://github.com/org/repo.git"
        );
    }

    #[test]
    fn rejects_traversal_and_invalid_snapshot_ids() {
        let scratch = Scratch::new("traversal");
        for bad in ["", "../escape", "sub/id", "sub\\id", "bad.json", "a b"] {
            assert!(validate_snapshot_id(bad).is_err(), "{bad:?}");
            assert!(load_snapshot(&scratch.0, bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn saves_lists_sorted_and_prunes_to_three_newest() {
        let scratch = Scratch::new("prune");
        for (id, ts) in [
            ("100-aaaa", 100),
            ("300-cccc", 300),
            ("200-bbbb", 200),
            ("400-dddd", 400),
        ] {
            save_snapshot(&scratch.0, &sample_snapshot(id, ts)).unwrap();
        }
        let listed = list_snapshots(&scratch.0);
        let ids: Vec<_> = listed.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, vec!["400-dddd", "300-cccc", "200-bbbb"]);
        assert_eq!(listed[0].package_count, 2);
        assert_eq!(listed[0].custom_node_count, 1);

        let loaded = load_snapshot(&scratch.0, "300-cccc").unwrap();
        assert_eq!(loaded.comfyui_version.as_deref(), Some("0.46.0"));
        assert!(load_snapshot(&scratch.0, "100-aaaa").is_err());
    }
}
