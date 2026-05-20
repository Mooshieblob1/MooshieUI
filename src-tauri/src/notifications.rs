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
    /// Legacy read flag for single-target notifications.
    #[serde(default)]
    pub read: bool,
    /// Users who have read a global notification.
    #[serde(default)]
    pub read_by: Vec<String>,
    /// Users who have dismissed this notification.
    #[serde(default)]
    pub dismissed_by: Vec<String>,
    /// ISO 8601 timestamp when the notification was created.
    pub created_at: String,
    /// When true, `title` and optional `body` are locale keys; `params` supplies `{var}` interpolation.
    #[serde(default)]
    pub i18n: bool,
    /// Interpolation values for i18n title/body keys.
    #[serde(default)]
    pub params: Option<serde_json::Value>,
}

/// Notification view returned to clients.
#[derive(Debug, Clone, Serialize)]
pub struct UserNotification {
    pub id: String,
    pub target: String,
    pub title: String,
    pub body: Option<String>,
    pub kind: String,
    pub read: bool,
    pub created_at: String,
    #[serde(default)]
    pub i18n: bool,
    #[serde(default)]
    pub params: Option<serde_json::Value>,
}

fn default_notif_type() -> String {
    "info".to_string()
}

fn user_list_contains(users: &[String], username: &str) -> bool {
    users.iter().any(|u| u.eq_ignore_ascii_case(username))
}

fn add_user_once(users: &mut Vec<String>, username: &str) {
    if !user_list_contains(users, username) {
        users.push(username.to_string());
    }
}

fn notification_targets_user(notification: &Notification, username: &str) -> bool {
    notification.target == "global" || notification.target.eq_ignore_ascii_case(username)
}

fn notification_visible_to(notification: &Notification, username: &str) -> bool {
    notification_targets_user(notification, username)
        && !user_list_contains(&notification.dismissed_by, username)
}

fn notification_read_by(notification: &Notification, username: &str) -> bool {
    notification.read || user_list_contains(&notification.read_by, username)
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

    /// Create a new notification with plain-text title/body (legacy).
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
            read_by: Vec::new(),
            dismissed_by: Vec::new(),
            created_at: now,
            i18n: false,
            params: None,
        };
        {
            let mut db = self.db.write().unwrap();
            db.notifications.push(notification.clone());
        }
        self.save();
        notification
    }

    /// Create a notification using frontend locale keys (`title` / optional `body`) and params.
    pub fn create_i18n(
        &self,
        target: &str,
        title_key: &str,
        body_key: Option<&str>,
        params: Option<serde_json::Value>,
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
            title: title_key.to_string(),
            body: body_key.map(|s| s.to_string()),
            kind: kind.to_string(),
            read: false,
            read_by: Vec::new(),
            dismissed_by: Vec::new(),
            created_at: now,
            i18n: true,
            params,
        };

        {
            let mut db = self.db.write().unwrap();
            db.notifications.push(notification.clone());
        }
        self.save();
        notification
    }

    /// Get notifications for a user (global + user-specific, unread first).
    pub fn get_for_user(&self, username: &str) -> Vec<UserNotification> {
        let db = self.db.read().unwrap();
        let mut results: Vec<UserNotification> = db
            .notifications
            .iter()
            .filter(|n| notification_visible_to(n, username))
            .map(|notification| UserNotification {
                id: notification.id.clone(),
                target: notification.target.clone(),
                title: notification.title.clone(),
                body: notification.body.clone(),
                kind: notification.kind.clone(),
                read: notification_read_by(notification, username),
                created_at: notification.created_at.clone(),
                i18n: notification.i18n,
                params: notification.params.clone(),
            })
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
            .filter(|n| notification_visible_to(n, username) && !notification_read_by(n, username))
            .count()
    }

    /// Mark a notification as read.
    pub fn mark_read(&self, username: &str, notification_id: &str) -> Result<(), String> {
        let mut db = self.db.write().unwrap();
        let notif = db
            .notifications
            .iter_mut()
            .find(|n| n.id == notification_id && notification_visible_to(n, username))
            .ok_or_else(|| "Notification not found".to_string())?;
        if notif.target == "global" {
            add_user_once(&mut notif.read_by, username);
        } else {
            notif.read = true;
        }
        drop(db);
        self.save();
        Ok(())
    }

    /// Mark all notifications for a user as read.
    pub fn mark_all_read(&self, username: &str) {
        let mut db = self.db.write().unwrap();
        for notif in &mut db.notifications {
            if notification_visible_to(notif, username) {
                if notif.target == "global" {
                    add_user_once(&mut notif.read_by, username);
                } else {
                    notif.read = true;
                }
            }
        }
        drop(db);
        self.save();
    }

    /// Dismiss a notification for a user.
    pub fn dismiss(&self, username: &str, notification_id: &str) -> Result<(), String> {
        let mut db = self.db.write().unwrap();
        let notif = db
            .notifications
            .iter_mut()
            .find(|n| n.id == notification_id && notification_visible_to(n, username))
            .ok_or_else(|| "Notification not found".to_string())?;
        add_user_once(&mut notif.dismissed_by, username);
        drop(db);
        self.save();
        Ok(())
    }

    /// Dismiss all visible notifications for a user.
    pub fn clear_all(&self, username: &str) {
        let mut db = self.db.write().unwrap();
        for notif in &mut db.notifications {
            if notification_visible_to(notif, username) {
                add_user_once(&mut notif.dismissed_by, username);
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
