//! Tauri desktop commands for single-ZIP backup and restore.

use std::sync::Arc;

use tauri::State;

use crate::backup::{
    apply_backup_admin_data_bytes, build_backup_zip_with_manifest, inspect_backup_zip_bytes,
    ApplyBackupAdminOptions, ApplyBackupAdminResult, BackupBuildParams, BackupBundleInput,
    BackupInspection, BackupManifest, MAX_ZIP_UPLOAD_BYTES,
};
use crate::commands::wildcards;
use crate::error::AppError;
use crate::state::AppState;

/// Export a single-ZIP backup to `dest_path`.
#[tauri::command]
pub async fn export_backup_zip(
    state: State<'_, Arc<AppState>>,
    dest_path: String,
    bundle: BackupBundleInput,
    include_admin_data: bool,
) -> Result<BackupManifest, AppError> {
    let app_config = if include_admin_data {
        Some(state.config.read().await.clone())
    } else {
        None
    };

    let dest = std::path::PathBuf::from(dest_path.trim());
    if dest.as_os_str().is_empty() {
        return Err(AppError::Other("Missing destination path".into()));
    }

    tokio::task::spawn_blocking(move || -> Result<BackupManifest, AppError> {
        let wc_files = if include_admin_data {
            wildcards::list()?.files
        } else {
            Vec::new()
        };
        let (bytes, manifest) = build_backup_zip_with_manifest(BackupBuildParams {
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            username: "admin".to_string(),
            bundle,
            app_config,
            wildcards: wc_files,
        })
        .map_err(AppError::Other)?;
        if let Some(parent) = dest.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        std::fs::write(&dest, &bytes)?;
        Ok(manifest)
    })
    .await
    .map_err(|e| AppError::Other(e.to_string()))?
}

fn read_bounded_zip(src_path: &str) -> Result<Vec<u8>, AppError> {
    let trimmed = src_path.trim();
    if trimmed.is_empty() {
        return Err(AppError::Other("Missing source path".into()));
    }
    let path = std::path::Path::new(trimmed);
    let meta = std::fs::metadata(path)?;
    if meta.len() > MAX_ZIP_UPLOAD_BYTES as u64 {
        return Err(AppError::Other(
            "Backup archive exceeds 64 MB size limit".into(),
        ));
    }
    Ok(std::fs::read(path)?)
}

/// Inspect a backup `.zip` file on disk and return its manifest + parsed payloads.
#[tauri::command]
pub async fn inspect_backup_zip(src_path: String) -> Result<BackupInspection, AppError> {
    tokio::task::spawn_blocking(move || -> Result<BackupInspection, AppError> {
        let bytes = read_bounded_zip(&src_path)?;
        inspect_backup_zip_bytes(&bytes).map_err(AppError::Other)
    })
    .await
    .map_err(|e| AppError::Other(e.to_string()))?
}

/// Apply admin-level configuration and wildcard files from a backup `.zip` on disk.
#[tauri::command]
pub async fn apply_backup_admin_data(
    state: State<'_, Arc<AppState>>,
    src_path: String,
    opts: ApplyBackupAdminOptions,
) -> Result<ApplyBackupAdminResult, AppError> {
    let bytes = tokio::task::spawn_blocking(move || read_bounded_zip(&src_path))
        .await
        .map_err(|e| AppError::Other(e.to_string()))??;
    apply_backup_admin_data_bytes(&state, &bytes, &opts)
        .await
        .map_err(AppError::Other)
}
