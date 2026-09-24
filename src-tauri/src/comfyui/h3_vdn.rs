//! Optional VDN-H3 package and checkpoint, shared by desktop and HTTP handlers.
//! Downloads only the branch/adapters, never the 72 GB Diffusers base model.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use crate::config::ServerMode;
use crate::state::AppState;

pub const PACKAGE: &str = "ComfyUI-VDN-H3";
pub const CHECKPOINT: &str = "stage-dmd-step-250";
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum VdnPrecision {
    #[default]
    Bf16,
    Int8,
}

impl VdnPrecision {
    pub fn checkpoint(self) -> &'static str {
        match self {
            Self::Bf16 => CHECKPOINT,
            Self::Int8 => "stage-dmd-step-250-int8-convrot",
        }
    }

    fn files(self) -> Vec<CheckpointFile> {
        FILES
            .iter()
            .map(|file| {
                if self == Self::Bf16 {
                    return *file;
                }
                match file.path {
                    "linear_branch/model.safetensors" => CheckpointFile {
                        path: "linear_branch/model_int8_convrot_comfyui.safetensors",
                        bytes: 2304371056,
                        sha256: Some(
                            "1fa18c3ebd94caa804ae3dc3a93df7ae069d8f5c363a00b6eb5288112c0f5abc",
                        ),
                    },
                    "adapters/default/adapter_spec.json" => CheckpointFile {
                        path: "adapters/default/adapter_config.json",
                        ..*file
                    },
                    "adapters/turbo/adapter_spec.json" => CheckpointFile {
                        path: "adapters/turbo/adapter_config.json",
                        ..*file
                    },
                    _ => *file,
                }
            })
            .collect()
    }

    fn url(self, path: &str) -> String {
        match self {
            Self::Bf16 => format!("https://huggingface.co/OpenVDN/vdn-minimax-h3/resolve/{MODEL_REVISION}/{CHECKPOINT}/{path}"),
            Self::Int8 => format!("https://huggingface.co/drbaph/vdn-minimax-h3-int8-convrot-comfyui/resolve/3dc26acabfa4cfc808faf6be0bf20b5e070897a4/{path}"),
        }
    }
}
const NODE_REVISION: &str = "3eb63496c24ca70faaf8a14b6c75fcb480e34bf1";
const MODEL_REVISION: &str = "6d052b3e9cc6b5d29009ac0ba71bc4ccb1fd4a14";
static INSTALL_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Clone, Copy)]
pub(crate) struct CheckpointFile {
    pub(crate) path: &'static str,
    pub(crate) bytes: u64,
    pub(crate) sha256: Option<&'static str>,
}

// Hugging Face blob metadata at MODEL_REVISION. JSON is also revision-pinned.
const FILES: &[CheckpointFile] = &[
    CheckpointFile {
        path: "model_spec.json",
        bytes: 25705,
        sha256: None,
    },
    CheckpointFile {
        path: "linear_branch/config.json",
        bytes: 465,
        sha256: None,
    },
    CheckpointFile {
        path: "linear_branch/model.safetensors",
        bytes: 4279428112,
        sha256: Some("dec6981c7874f5b3bc92d1a02e256b673a3b3499dc1a124714bb3b19da602855"),
    },
    CheckpointFile {
        path: "adapters/default/adapter_spec.json",
        bytes: 415,
        sha256: None,
    },
    CheckpointFile {
        path: "adapters/default/adapter_model.safetensors",
        bytes: 334026912,
        sha256: Some("58558fef506f88bb41649242de9b9b3a365da806b51b2e96afbbe1625222058a"),
    },
    CheckpointFile {
        path: "adapters/turbo/adapter_spec.json",
        bytes: 22264,
        sha256: None,
    },
    CheckpointFile {
        path: "adapters/turbo/adapter_model.safetensors",
        bytes: 851452696,
        sha256: Some("24fc93c82fe84dc45d0627f4e72c637bc387d282ba18f60ed3b7f8c81089392c"),
    },
];

#[derive(Serialize)]
pub struct VdnStatus {
    pub precision: VdnPrecision,
    pub ready: bool,
    pub node_loaded: bool,
    pub can_install: bool,
    pub download_bytes: u64,
}

/// Check the live backend, including the checkpoint dropdown. A cloned package
/// alone is insufficient: it may have failed to import or need a restart.
fn node_ready(info: &Value, precision: VdnPrecision) -> (bool, bool) {
    let inputs = &info["ApplyVDNH3"]["input"]["required"];
    let loaded = [
        "model",
        "vdn_checkpoint",
        "apply_turbo_adapter",
        "strength",
        "lora_mode",
        "branch_weights",
        "retain_buffers",
        "attention_backend",
        "verbose",
    ]
    .iter()
    .all(|key| inputs.get(key).is_some());
    let checkpoint = inputs["vdn_checkpoint"][0].as_array().is_some_and(|names| {
        names
            .iter()
            .any(|name| name.as_str() == Some(precision.checkpoint()))
    });
    (loaded, loaded && checkpoint)
}

fn checkpoint_complete(root: &Path, precision: VdnPrecision) -> bool {
    precision.files().iter().all(|file| {
        std::fs::metadata(root.join(file.path))
            .is_ok_and(|meta| meta.is_file() && meta.len() == file.bytes)
    })
}

impl AppState {
    pub async fn h3_vdn_status(&self, precision: VdnPrecision) -> VdnStatus {
        let (path, local) = {
            let config = self.config.read().await;
            (
                config.comfyui_path.clone(),
                matches!(config.server_mode, ServerMode::AutoLaunch),
            )
        };
        let info = self
            .api_get("/object_info/ApplyVDNH3")
            .await
            .unwrap_or(Value::Null);
        let (node_loaded, ready) = node_ready(&info, precision);
        VdnStatus {
            precision,
            ready: ready
                && (!local
                    || checkpoint_complete(
                        &Path::new(&path)
                            .join("models/vdn")
                            .join(precision.checkpoint()),
                        precision,
                    )),
            node_loaded,
            can_install: local && !path.trim().is_empty(),
            download_bytes: precision.files().iter().map(|file| file.bytes).sum(),
        }
    }

    pub async fn install_h3_vdn(
        &self,
        precision: VdnPrecision,
        on_progress: &(dyn Fn(&str, &str, bool) + Send + Sync),
    ) -> Result<(), String> {
        let _install = INSTALL_LOCK
            .try_lock()
            .map_err(|_| "VDN installation is already running")?;
        let path = {
            let config = self.config.read().await;
            if !matches!(config.server_mode, ServerMode::AutoLaunch) {
                return Err(
                    "Install VDN on the remote ComfyUI server, then refresh its connection.".into(),
                );
            }
            if config.comfyui_path.trim().is_empty() {
                return Err("ComfyUI path is not configured".into());
            }
            PathBuf::from(&config.comfyui_path)
        };
        on_progress("package", "Installing VDN-H3 nodes...", false);
        install_package(&self.http_client, &path).await?;
        let root = path.join("models/vdn").join(precision.checkpoint());
        for spec in &precision.files() {
            let dest = root.join(spec.path);
            if std::fs::metadata(&dest).is_ok_and(|meta| meta.is_file() && meta.len() == spec.bytes)
            {
                continue;
            }
            let url = precision.url(spec.path);
            on_progress("download", &format!("Downloading {}...", spec.path), false);
            download_file(&self.http_client, &url, &dest, spec, on_progress).await?;
        }
        on_progress(
            "done",
            "VDN files are ready. Checking the ComfyUI connection...",
            true,
        );
        Ok(())
    }
}

async fn install_package(client: &reqwest::Client, comfyui: &Path) -> Result<(), String> {
    let parent = comfyui.join("custom_nodes");
    let target = parent.join(PACKAGE);
    // Preserve independently installed/modified node packages.
    if target.join("__init__.py").is_file() {
        return Ok(());
    }
    if target.exists() {
        return Err(format!(
            "The existing {PACKAGE} directory is incomplete. Repair it before installing VDN."
        ));
    }
    let url = format!("https://github.com/Saganaki22/ComfyUI-VDN-H3/archive/{NODE_REVISION}.zip");
    let bytes = client
        .get(url)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .bytes()
        .await
        .map_err(|e| e.to_string())?;
    // Extraction is small but blocking. A unique staging folder is renamed only
    // after all files are present, so failed installs can be retried.
    tokio::task::spawn_blocking(move || {
        std::fs::create_dir_all(&parent).map_err(|e| e.to_string())?;
        let staging = parent.join(format!(".mooshie-vdn-{}", uuid::Uuid::new_v4()));
        let result = (|| {
            let mut zip =
                zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| e.to_string())?;
            for index in 0..zip.len() {
                let mut entry = zip.by_index(index).map_err(|e| e.to_string())?;
                let enclosed = entry.enclosed_name().ok_or("Unsafe VDN archive path")?;
                let relative: PathBuf = enclosed.components().skip(1).collect();
                if relative.as_os_str().is_empty() {
                    continue;
                }
                let dest = staging.join(relative);
                if entry.is_dir() {
                    std::fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
                } else {
                    std::fs::create_dir_all(dest.parent().ok_or("Invalid archive entry")?)
                        .map_err(|e| e.to_string())?;
                    let mut out = std::fs::File::create(dest).map_err(|e| e.to_string())?;
                    std::io::copy(&mut entry, &mut out).map_err(|e| e.to_string())?;
                }
            }
            if !staging.join("__init__.py").is_file() || !staging.join("vdn_h3/nodes.py").is_file()
            {
                return Err("VDN archive is missing its node entry point".into());
            }
            std::fs::write(staging.join(".mooshieui-revision"), NODE_REVISION)
                .map_err(|e| e.to_string())?;
            std::fs::rename(&staging, &target).map_err(|e| e.to_string())
        })();
        // Only the uniquely allocated staging directory is removed.
        if staging.exists() {
            let _ = std::fs::remove_dir_all(&staging);
        }
        result
    })
    .await
    .map_err(|e| e.to_string())?
}

fn verify_download(spec: &CheckpointFile, bytes: u64, digest: &str) -> Result<(), String> {
    if bytes != spec.bytes {
        return Err(format!(
            "{}: downloaded {bytes} bytes, expected {}. Retry installation.",
            spec.path, spec.bytes
        ));
    }
    if spec.sha256.is_some_and(|expected| expected != digest) {
        return Err(format!(
            "{}: download checksum mismatch. Retry installation.",
            spec.path
        ));
    }
    Ok(())
}

pub(crate) async fn download_file(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    spec: &CheckpointFile,
    on_progress: &(dyn Fn(&str, &str, bool) + Send + Sync),
) -> Result<(), String> {
    let mut request = client.get(url);
    if let Some(token) = super::client::huggingface_token_for_url(url) {
        request = request.bearer_auth(token);
    }
    let mut response = request
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?;
    tokio::fs::create_dir_all(dest.parent().ok_or("Invalid model destination")?)
        .await
        .map_err(|e| e.to_string())?;
    let partial = dest.with_extension("part");
    let result = async {
        let mut file = tokio::fs::File::create(&partial)
            .await
            .map_err(|e| e.to_string())?;
        let mut hasher = Sha256::new();
        let mut downloaded = 0u64;
        let mut last_emit = std::time::Instant::now();
        while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
            if downloaded + chunk.len() as u64 > spec.bytes {
                return Err(format!("{}: download exceeds expected size", spec.path));
            }
            file.write_all(&chunk).await.map_err(|e| e.to_string())?;
            hasher.update(&chunk);
            downloaded += chunk.len() as u64;
            if last_emit.elapsed().as_millis() >= 250 {
                on_progress(
                    "download",
                    &format!("{}: {}%", spec.path, downloaded * 100 / spec.bytes),
                    false,
                );
                last_emit = std::time::Instant::now();
            }
        }
        file.flush().await.map_err(|e| e.to_string())?;
        drop(file);
        verify_download(spec, downloaded, &hex::encode(hasher.finalize()))?;
        tokio::fs::rename(&partial, dest)
            .await
            .map_err(|e| e.to_string())
    }
    .await;
    if result.is_err() {
        let _ = tokio::fs::remove_file(partial).await;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn int8_manifest_uses_its_own_branch_and_metadata_names() {
        let files = VdnPrecision::Int8.files();
        assert_eq!(
            files.iter().map(|file| file.bytes).sum::<u64>(),
            3_489_899_513
        );
        assert_eq!(
            VdnPrecision::Bf16
                .files()
                .iter()
                .map(|file| file.bytes)
                .sum::<u64>(),
            5_464_956_569
        );
        assert!(files.iter().any(
            |f| f.path.ends_with("model_int8_convrot_comfyui.safetensors") && f.sha256.is_some()
        ));
        assert_eq!(
            files
                .iter()
                .filter(|f| f.path.ends_with("adapter_config.json"))
                .count(),
            2
        );
        assert!(!files.iter().any(|f| f.path.ends_with("adapter_spec.json")));
        assert_ne!(
            VdnPrecision::Int8.checkpoint(),
            VdnPrecision::Bf16.checkpoint()
        );
        assert!(!VdnPrecision::Int8
            .url("model_spec.json")
            .contains("stage-dmd"));
        assert!(serde_json::from_str::<VdnPrecision>("\"other\"").is_err());
    }

    #[test]
    fn node_presence_without_checkpoint_is_not_ready() {
        assert_eq!(node_ready(&Value::Null, VdnPrecision::Bf16), (false, false));
        let mut inputs = serde_json::Map::new();
        for key in [
            "model",
            "vdn_checkpoint",
            "apply_turbo_adapter",
            "strength",
            "lora_mode",
            "branch_weights",
            "retain_buffers",
            "attention_backend",
            "verbose",
        ] {
            inputs.insert(key.into(), json!([[]]));
        }
        let mut info = json!({"ApplyVDNH3": {"input": {"required": inputs}}});
        assert_eq!(node_ready(&info, VdnPrecision::Bf16), (true, false));
        info["ApplyVDNH3"]["input"]["required"]["vdn_checkpoint"] = json!([[CHECKPOINT]]);
        assert_eq!(node_ready(&info, VdnPrecision::Bf16), (true, true));
        assert_eq!(node_ready(&info, VdnPrecision::Int8), (true, false));
        info["ApplyVDNH3"]["input"]["required"]["vdn_checkpoint"] =
            json!([[VdnPrecision::Int8.checkpoint()]]);
        assert_eq!(node_ready(&info, VdnPrecision::Int8), (true, true));
        assert_eq!(node_ready(&info, VdnPrecision::Bf16), (true, false));
    }

    #[test]
    fn incomplete_checkpoint_or_bad_download_is_rejected() {
        let root = std::env::temp_dir().join(format!("vdn-test-{}", uuid::Uuid::new_v4()));
        assert!(!checkpoint_complete(&root, VdnPrecision::Bf16));
        let spec = CheckpointFile {
            path: "test",
            bytes: 3,
            sha256: Some("abc"),
        };
        assert!(verify_download(&spec, 2, "abc").is_err());
        assert!(verify_download(&spec, 3, "bad").is_err());
        assert!(verify_download(&spec, 3, "abc").is_ok());
    }

    #[tokio::test]
    async fn failed_transfer_leaves_no_checkpoint_and_retry_installs_verified_bytes() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/weights", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            // First response has the right size but wrong hash. Then a retry succeeds.
            for body in ["bad", "abc"] {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = [0u8; 2048];
                let _ = socket.read(&mut request).await.unwrap();
                socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: 3\r\nConnection: close\r\n\r\n{body}").as_bytes()).await.unwrap();
            }
        });
        let root = std::env::temp_dir().join(format!("vdn-download-test-{}", uuid::Uuid::new_v4()));
        let dest = root.join("adapters/default/model.safetensors");
        let file = CheckpointFile {
            path: "adapters/default/model.safetensors",
            bytes: 3,
            sha256: Some("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"),
        };
        let client = reqwest::Client::new();
        let progress = |_: &str, _: &str, _: bool| {};
        assert!(download_file(&client, &url, &dest, &file, &progress)
            .await
            .unwrap_err()
            .contains("checksum"));
        assert!(!dest.exists());
        assert!(!dest.with_extension("part").exists());
        download_file(&client, &url, &dest, &file, &progress)
            .await
            .unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"abc");
        assert!(!dest.with_extension("part").exists());
        server.await.unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
}
