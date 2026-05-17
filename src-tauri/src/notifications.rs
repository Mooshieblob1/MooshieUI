//! Notification system — global and per-user notifications.
//!
//! Stores notifications in `{app_data_dir}/notifications.json`. Notifications
//! can be global (visible to all users) or targeted to a specific user.
//! They are delivered via the SSE event stream and persisted to disk.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::RwLock;

use crate::config;

/// A notification entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notification {
    /// Unique ID for this notification.
    pub id: String,
    /// "global" or a specific username.
    pub target: String,
    /// Short title for the notification.
    pub title: String,
    /// Optional longer message body.
    pub body: Option<String>,
    /// Notification type for icon/color: "info", "success", "warning", "error".
    #[serde(default = "default_notif_type")]
    pub kind: String,
    /// Whether the notification has been read/dismissed.
    #[serde(default)]
    pub read: bool,
    /// ISO 8601 timestamp when the notification was created.
    pub created_at: String,
}

fn default_notif_type() -> String {
    "info".to_string()
}

/// On-disk notification database.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NotificationDatabase {
    pub notifications: Vec<Notification>,
}

/// In-memory notification state.
pub struct NotificationState {
    db: RwLock<NotificationDatabase>,
}

impl NotificationState {
    pub fn new() -> Self {
        let db = load_notifications().unwrap_or_default();
        Self {
            db: RwLock::new(db),
        }
    }

    /// Create a new notification.
    pub fn create(
        &self,
        target: &str,
        title: &str,
        body: Option<&str>,
        kind: &str,
    ) -> Notification {
        let id = format!(
            "notif_{}",
            uuid::Uuid::new_v4().to_string().replace("-", "")[..12].to_string()
        );
        let now = Utc::now().to_rfc3339();
        let notification = Notification {
            id,
            target: target.to_string(),
            title: title.to_string(),
            body: body.map(|s| s.to_string()),
            kind: kind.to_string(),
            read: false,
            created_at: now,
        };

        {
            let mut db = self.db.write().unwrap();
            db.notifications.push(notification.clone());
        }
        self.save();
        notification
    }

    /// Get notifications for a user (global + user-specific, unread first).
    pub fn get_for_user(&self, username: &str) -> Vec<Notification> {
        let db = self.db.read().unwrap();
        let mut results: Vec<Notification> = db
            .notifications
            .iter()
            .filter(|n| n.target == "global" || n.target.eq_ignore_ascii_case(username))
            .cloned()
            .collect();
        // Sort: unread first, then newest first
        results.sort_by(|a, b| {
            b.read
                .cmp(&a.read)
                .then_with(|| b.created_at.cmp(&a.created_at))
        });
        results
    }

    /// Get unread count for a user.
    pub fn unread_count(&self, username: &str) -> usize {
        let db = self.db.read().unwrap();
        db.notifications
            .iter()
            .filter(|n| {
                !n.read && (n.target == "global" || n.target.eq_ignore_ascii_case(username))
            })
            .count()
    }

    /// Mark a notification as read.
    pub fn mark_read(&self, notification_id: &str) -> Result<(), String> {
        let mut db = self.db.write().unwrap();
        let notif = db
            .notifications
            .iter_mut()
            .find(|n| n.id == notification_id)
            .ok_or_else(|| "Notification not found".to_string())?;
        notif.read = true;
        drop(db);
        self.save();
        Ok(())
    }

    /// Mark all notifications for a user as read.
    pub fn mark_all_read(&self, username: &str) {
        let mut db = self.db.write().unwrap();
        for notif in &mut db.notifications {
            if notif.target == "global" || notif.target.eq_ignore_ascii_case(username) {
                notif.read = true;
            }
        }
        drop(db);
        self.save();
    }

    fn save(&self) {
        let db = self.db.read().unwrap();
        if let Err(e) = save_notifications(&db) {
            log::error!("Failed to save notifications: {}", e);
        }
    }
}

fn notifications_path() -> Option<PathBuf> {
    config::app_data_dir().map(|d| d.join("notifications.json"))
}

fn load_notifications() -> Option<NotificationDatabase> {
    let path = notifications_path()?;
    if !path.exists() {
        return Some(NotificationDatabase::default());
    }
    let data = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&data).ok()
}

fn save_notifications(db: &NotificationDatabase) -> Result<(), String> {
    let path = notifications_path().ok_or("No app data dir")?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let data = serde_json::to_string_pretty(db).map_err(|e| e.to_string())?;
    std::fs::write(&path, data).map_err(|e| e.to_string())?;
    Ok(())
}
