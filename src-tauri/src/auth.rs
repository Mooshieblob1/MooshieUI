//! Local account authentication for LAN mode.
//!
//! Stores accounts in `{app_data_dir}/auth.json` with bcrypt-hashed passwords.
//! Sessions are tracked via random bearer tokens held in memory.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::RwLock;

use crate::config;

/// Default storage limit per user: 2 GB.
const DEFAULT_STORAGE_LIMIT: u64 = 2 * 1024 * 1024 * 1024;

/// Default image expiry: 7 days in seconds.
pub const DEFAULT_EXPIRY_SECS: u64 = 7 * 24 * 60 * 60;

fn default_storage_limit() -> u64 {
    DEFAULT_STORAGE_LIMIT
}

/// A stored user account.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub username: String,
    /// SHA-256 hash of the password (hex-encoded). Not bcrypt for simplicity
    /// in MVP — upgrade to argon2 later.
    pub password_hash: String,
    /// When true the user must pick a new password on next login.
    #[serde(default)]
    pub must_change_password: bool,
    /// Account role: "user" (default) or "moderator".
    #[serde(default = "default_role")]
    pub role: String,
    /// ISO 8601 timestamp when the account was created.
    #[serde(default)]
    pub created_at: String,
    /// ISO 8601 timestamp of the last time the user was active (persisted periodically).
    #[serde(default)]
    pub last_online: Option<String>,
    /// Maximum gallery storage in bytes. Default 2 GB. Admins/mods can expand.
    #[serde(default = "default_storage_limit")]
    pub storage_limit_bytes: u64,
}

fn default_role() -> String {
    "user".to_string()
}

/// On-disk auth database.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AuthDatabase {
    pub accounts: Vec<Account>,
}

/// In-memory auth state.
pub struct AuthState {
    db: RwLock<AuthDatabase>,
    /// Active session tokens → username.
    sessions: RwLock<HashMap<String, String>>,
    /// Per-user last activity timestamp (username → Instant).
    last_activity: RwLock<HashMap<String, std::time::Instant>>,
}

impl AuthState {
    pub fn new() -> Self {
        let db = load_auth_db().unwrap_or_default();
        Self {
            db: RwLock::new(db),
            sessions: RwLock::new(HashMap::new()),
            last_activity: RwLock::new(HashMap::new()),
        }
    }

    /// Check if any accounts exist.
    pub fn has_accounts(&self) -> bool {
        let db = self.db.read().unwrap();
        !db.accounts.is_empty()
    }

    /// Create a new account. Returns error if username already exists.
    pub fn create_account(&self, username: &str, password: &str) -> Result<(), String> {
        self.create_account_ex(username, password, false)
    }

    /// Create a new account with optional temporary-password flag.
    pub fn create_account_ex(
        &self,
        username: &str,
        password: &str,
        temp: bool,
    ) -> Result<(), String> {
        let mut db = self.db.write().unwrap();
        if db.accounts.iter().any(|a| a.username == username) {
            return Err("Username already exists".to_string());
        }
        db.accounts.push(Account {
            username: username.to_string(),
            password_hash: hash_password(password),
            must_change_password: temp,
            role: "user".to_string(),
            created_at: Utc::now().to_rfc3339(),
            last_online: None,
            storage_limit_bytes: DEFAULT_STORAGE_LIMIT,
        });
        save_auth_db(&db)?;
        Ok(())
    }

    /// Authenticate and return a session token plus whether a password change
    /// is required.
    pub fn login(&self, username: &str, password: &str) -> Result<(String, bool), String> {
        let db = self.db.read().unwrap();
        let account = db
            .accounts
            .iter()
            .find(|a| a.username == username)
            .ok_or("Invalid username or password")?;

        if account.password_hash != hash_password(password) {
            return Err("Invalid username or password".to_string());
        }

        let must_change = account.must_change_password;
        let token = generate_token();
        self.sessions
            .write()
            .unwrap()
            .insert(token.clone(), username.to_string());
        Ok((token, must_change))
    }

    /// Validate a session token. Returns the username if valid.
    pub fn validate_token(&self, token: &str) -> Option<String> {
        let sessions = self.sessions.read().unwrap();
        sessions.get(token).cloned()
    }

    /// Invalidate a session token.
    pub fn logout(&self, token: &str) {
        self.sessions.write().unwrap().remove(token);
    }

    /// List all account usernames.
    pub fn list_accounts(&self) -> Vec<String> {
        let db = self.db.read().unwrap();
        db.accounts.iter().map(|a| a.username.clone()).collect()
    }

    /// List accounts with their roles.
    pub fn list_accounts_with_roles(&self) -> Vec<(String, String)> {
        let db = self.db.read().unwrap();
        db.accounts
            .iter()
            .map(|a| (a.username.clone(), a.role.clone()))
            .collect()
    }

    /// Get the role of a specific account.
    pub fn get_account_role(&self, username: &str) -> Option<String> {
        let db = self.db.read().unwrap();
        db.accounts
            .iter()
            .find(|a| a.username == username)
            .map(|a| a.role.clone())
    }

    /// Set the role of an account. Valid roles: "user", "moderator", "admin".
    pub fn set_account_role(&self, username: &str, role: &str) -> Result<(), String> {
        if role != "user" && role != "moderator" && role != "admin" {
            return Err("Invalid role. Must be 'user', 'moderator', or 'admin'.".to_string());
        }
        let mut db = self.db.write().unwrap();
        let account = db
            .accounts
            .iter_mut()
            .find(|a| a.username == username)
            .ok_or("Account not found")?;
        account.role = role.to_string();
        save_auth_db(&db)?;
        Ok(())
    }

    /// Delete an account by username.
    pub fn delete_account(&self, username: &str) -> Result<(), String> {
        let mut db = self.db.write().unwrap();
        let before = db.accounts.len();
        db.accounts.retain(|a| a.username != username);
        if db.accounts.len() == before {
            return Err("Account not found".to_string());
        }
        save_auth_db(&db)?;
        // Also remove any active sessions for this user
        let mut sessions = self.sessions.write().unwrap();
        sessions.retain(|_, u| u != username);
        Ok(())
    }

    /// Get the storage limit in bytes for a user.
    pub fn get_storage_limit(&self, username: &str) -> u64 {
        let db = self.db.read().unwrap();
        db.accounts
            .iter()
            .find(|a| a.username == username)
            .map(|a| a.storage_limit_bytes)
            .unwrap_or(DEFAULT_STORAGE_LIMIT)
    }

    /// Set the storage limit in bytes for a user. Admin/moderator only.
    pub fn set_storage_limit(&self, username: &str, limit_bytes: u64) -> Result<(), String> {
        let mut db = self.db.write().unwrap();
        let account = db
            .accounts
            .iter_mut()
            .find(|a| a.username == username)
            .ok_or("Account not found")?;
        account.storage_limit_bytes = limit_bytes;
        save_auth_db(&db)?;
        Ok(())
    }

    /// Change a user's own password. Requires the current password for
    /// verification. Clears the `must_change_password` flag.
    pub fn change_password(
        &self,
        username: &str,
        current_password: &str,
        new_password: &str,
    ) -> Result<(), String> {
        if new_password.len() < 4 {
            return Err("New password must be at least 4 characters".to_string());
        }
        let mut db = self.db.write().unwrap();
        let account = db
            .accounts
            .iter_mut()
            .find(|a| a.username == username)
            .ok_or("Account not found")?;

        if account.password_hash != hash_password(current_password) {
            return Err("Current password is incorrect".to_string());
        }
        account.password_hash = hash_password(new_password);
        account.must_change_password = false;
        save_auth_db(&db)?;
        Ok(())
    }

    /// Admin: set a temporary password on an account, forcing the user to
    /// choose a new one at next login.
    pub fn reset_password(&self, username: &str, temp_password: &str) -> Result<(), String> {
        if temp_password.len() < 4 {
            return Err("Temporary password must be at least 4 characters".to_string());
        }
        let mut db = self.db.write().unwrap();
        let account = db
            .accounts
            .iter_mut()
            .find(|a| a.username == username)
            .ok_or("Account not found")?;

        account.password_hash = hash_password(temp_password);
        account.must_change_password = true;
        save_auth_db(&db)?;
        // Revoke existing sessions for this user so they must re-login
        let mut sessions = self.sessions.write().unwrap();
        sessions.retain(|_, u| u != username);
        Ok(())
    }

    /// Update the last-activity timestamp for a user.
    pub fn touch_activity(&self, username: &str) {
        let mut map = self.last_activity.write().unwrap();
        map.insert(username.to_string(), std::time::Instant::now());
    }

    /// Persist all accumulated `last_online` timestamps to the auth database.
    /// Call periodically and on shutdown to avoid losing online-status data.
    pub fn flush_last_online(&self) {
        let activity = self.last_activity.read().unwrap();
        if activity.is_empty() {
            return;
        }
        let now = Utc::now();
        let mut db = self.db.write().unwrap();
        let mut changed = false;
        for account in &mut db.accounts {
            if activity.contains_key(&account.username) {
                let ts = now.to_rfc3339();
                if account.last_online.as_deref() != Some(&ts) {
                    account.last_online = Some(ts);
                    changed = true;
                }
            }
        }
        if changed {
            let _ = save_auth_db(&db);
        }
    }

    /// List all users with their role, online/offline status, and timestamps.
    /// A user is "online" if their last activity was within `threshold`.
    pub fn list_users_status(
        &self,
        threshold: std::time::Duration,
    ) -> Vec<(String, String, bool, String, Option<String>)> {
        let db = self.db.read().unwrap();
        let activity = self.last_activity.read().unwrap();
        db.accounts
            .iter()
            .map(|a| {
                let online = activity
                    .get(&a.username)
                    .map_or(false, |t| t.elapsed() < threshold);
                (
                    a.username.clone(),
                    a.role.clone(),
                    online,
                    a.created_at.clone(),
                    a.last_online.clone(),
                )
            })
            .collect()
    }
}

fn hash_password(password: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(password.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn generate_token() -> String {
    use rand::RngExt;
    let mut rng = rand::rng();
    let bytes: Vec<u8> = (0..32).map(|_| rng.random::<u8>()).collect();
    hex::encode(bytes)
}

fn auth_db_path() -> Option<PathBuf> {
    config::app_data_dir().map(|d| d.join("auth.json"))
}

fn load_auth_db() -> Result<AuthDatabase, String> {
    let path = auth_db_path().ok_or("No app data dir")?;
    if !path.exists() {
        return Ok(AuthDatabase::default());
    }
    let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    serde_json::from_str(&content).map_err(|e| e.to_string())
}

fn save_auth_db(db: &AuthDatabase) -> Result<(), String> {
    let path = auth_db_path().ok_or("No app data dir")?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(db).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| e.to_string())?;
    Ok(())
}
