//! Model request queue — allows non-mod users to request models for download.
//!
//! Stores requests in `{app_data_dir}/model_requests.json`. Mods/admins can
//! view, approve (trigger download), or deny (with optional reason) requests.
//! Denied requests generate notifications for the requesting user.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::RwLock;

use crate::config;

/// Status of a model request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RequestStatus {
    Pending,
    Approved,
    Denied,
}

/// A single model download request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelRequest {
    /// Unique ID for this request.
    pub id: String,
    /// Username of the requester.
    pub username: String,
    /// CivitAI model ID.
    pub model_id: u64,
    /// Display name of the model.
    pub model_name: String,
    /// Model type (Checkpoint, LORA, etc.).
    pub model_type: String,
    /// CivitAI model URL.
    pub model_url: String,
    /// Specific file to download (name + URL).
    pub file_name: String,
    pub file_url: String,
    /// File size in KB.
    pub file_size_kb: f64,
    /// Category for installation (checkpoints, loras, etc.).
    pub category: String,
    /// Current status.
    pub status: RequestStatus,
    /// Who handled the request (mod/admin username).
    pub handled_by: Option<String>,
    /// Reason for denial (shown to requester).
    pub deny_reason: Option<String>,
    /// ISO 8601 timestamp when the request was created.
    pub created_at: String,
    /// ISO 8601 timestamp when the request was handled.
    pub handled_at: Option<String>,
}

/// On-disk model request database.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModelRequestDatabase {
    pub requests: Vec<ModelRequest>,
}

/// In-memory model request state.
pub struct ModelRequestState {
    db: RwLock<ModelRequestDatabase>,
}

impl ModelRequestState {
    pub fn new() -> Self {
        let db = load_model_requests().unwrap_or_default();
        Self {
            db: RwLock::new(db),
        }
    }

    /// Add a new pending request. Returns the created request.
    pub fn add_request(
        &self,
        username: &str,
        model_id: u64,
        model_name: &str,
        model_type: &str,
        model_url: &str,
        file_name: &str,
        file_url: &str,
        file_size_kb: f64,
        category: &str,
    ) -> ModelRequest {
        let id = format!(
            "req_{}",
            uuid::Uuid::new_v4().to_string().replace("-", "")[..12].to_string()
        );
        let now = Utc::now().to_rfc3339();
        let request = ModelRequest {
            id,
            username: username.to_string(),
            model_id,
            model_name: model_name.to_string(),
            model_type: model_type.to_string(),
            model_url: model_url.to_string(),
            file_name: file_name.to_string(),
            file_url: file_url.to_string(),
            file_size_kb,
            category: category.to_string(),
            status: RequestStatus::Pending,
            handled_by: None,
            deny_reason: None,
            created_at: now,
            handled_at: None,
        };

        {
            let mut db = self.db.write().unwrap();
            db.requests.push(request.clone());
        }
        self.save();
        request
    }

    /// Get all requests, optionally filtered by status.
    pub fn get_requests(&self, status: Option<RequestStatus>) -> Vec<ModelRequest> {
        let db = self.db.read().unwrap();
        match status {
            Some(s) => db
                .requests
                .iter()
                .filter(|r| r.status == s)
                .cloned()
                .collect(),
            None => db.requests.clone(),
        }
    }

    /// Get requests for a specific user.
    pub fn get_requests_for_user(&self, username: &str) -> Vec<ModelRequest> {
        let db = self.db.read().unwrap();
        db.requests
            .iter()
            .filter(|r| r.username.eq_ignore_ascii_case(username))
            .cloned()
            .collect()
    }

    /// Approve a request (mod/admin action).
    pub fn approve_request(
        &self,
        request_id: &str,
        handled_by: &str,
    ) -> Result<ModelRequest, String> {
        let mut db = self.db.write().unwrap();
        let req = db
            .requests
            .iter_mut()
            .find(|r| r.id == request_id)
            .ok_or_else(|| "Request not found".to_string())?;

        if req.status != RequestStatus::Pending {
            return Err("Request is not pending".to_string());
        }

        req.status = RequestStatus::Approved;
        req.handled_by = Some(handled_by.to_string());
        req.handled_at = Some(Utc::now().to_rfc3339());
        let result = req.clone();
        drop(db);
        self.save();
        Ok(result)
    }

    /// Deny a request with an optional reason.
    pub fn deny_request(
        &self,
        request_id: &str,
        handled_by: &str,
        reason: Option<&str>,
    ) -> Result<ModelRequest, String> {
        let mut db = self.db.write().unwrap();
        let req = db
            .requests
            .iter_mut()
            .find(|r| r.id == request_id)
            .ok_or_else(|| "Request not found".to_string())?;

        if req.status != RequestStatus::Pending {
            return Err("Request is not pending".to_string());
        }

        req.status = RequestStatus::Denied;
        req.handled_by = Some(handled_by.to_string());
        req.deny_reason = reason.map(|s| s.to_string());
        req.handled_at = Some(Utc::now().to_rfc3339());
        let result = req.clone();
        drop(db);
        self.save();
        Ok(result)
    }

    fn save(&self) {
        let db = self.db.read().unwrap();
        if let Err(e) = save_model_requests(&db) {
            log::error!("Failed to save model requests: {}", e);
        }
    }
}

fn model_requests_path() -> Option<PathBuf> {
    config::app_data_dir().map(|d| d.join("model_requests.json"))
}

fn load_model_requests() -> Option<ModelRequestDatabase> {
    let path = model_requests_path()?;
    if !path.exists() {
        return Some(ModelRequestDatabase::default());
    }
    let data = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&data).ok()
}

fn save_model_requests(db: &ModelRequestDatabase) -> Result<(), String> {
    let path = model_requests_path().ok_or("No app data dir")?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let data = serde_json::to_string_pretty(db).map_err(|e| e.to_string())?;
    std::fs::write(&path, data).map_err(|e| e.to_string())?;
    Ok(())
}
