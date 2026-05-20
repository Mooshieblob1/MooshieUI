//! Local account authentication for LAN mode.
//!
//! Stores accounts in `{app_data_dir}/auth.json` with Argon2id-hashed passwords.
//! Sessions are tracked via random bearer tokens held in memory.
//!
//! Legacy SHA-256 hashes (64 hex chars) are accepted on login and
//! transparently upgraded to Argon2id.

use argon2::password_hash::SaltString;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::RwLock;

use crate::config;

/// Default storage limit per user: 1 GB.
const DEFAULT_STORAGE_LIMIT: u64 = 1024 * 1024 * 1024;

/// Default image expiry: 7 days in seconds.
pub const DEFAULT_EXPIRY_SECS: u64 = 7 * 24 * 60 * 60;

/// Session TTL: 7 days.
const SESSION_TTL_SECS: i64 = 7 * 24 * 60 * 60;

fn default_storage_limit() -> u64 {
    DEFAULT_STORAGE_LIMIT
}

/// A stored user account.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub username: String,
    /// Argon2id hash (PHC string). Legacy accounts may still hold a 64-char
    /// hex SHA-256 hash — these are verified and upgraded on login.
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
    /// Whether this user can access the Model Hub (download models from CivitAI).
    /// Admins and moderators always have access; this flag controls user-role accounts.
    #[serde(default)]
    pub can_use_modelhub: bool,
}

fn default_role() -> String {
    "user".to_string()
}

/// On-disk auth database.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AuthDatabase {
    pub accounts: Vec<Account>,
}

/// A persisted session entry with TTL metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionEntry {
    pub username: String,
    /// ISO 8601 timestamp when the session was created.
    pub created_at: String,
}

/// Auth state with persistent sessions.
pub struct AuthState {
    db: RwLock<AuthDatabase>,
    /// Active session tokens → session entry. Persisted to disk so tokens
    /// survive server restarts (enables "remember me"). Expired entries are
    /// pruned on load and periodically on validation.
    sessions: RwLock<HashMap<String, SessionEntry>>,
    /// Per-user last activity timestamp (username → Instant).
    last_activity: RwLock<HashMap<String, std::time::Instant>>,
}

impl Default for AuthState {
    fn default() -> Self {
        Self::new()
    }
}

impl AuthState {
    pub fn new() -> Self {
        let mut db = load_auth_db().unwrap_or_default();

        // One-time migration: normalise all stored usernames to lowercase
        let mut db_changed = false;
        for account in &mut db.accounts {
            let lower = account.username.to_ascii_lowercase();
            if account.username != lower {
                log::info!(
                    "Migrating account username '{}' → '{}'",
                    account.username,
                    lower
                );
                account.username = lower;
                db_changed = true;
            }
        }
        // Deduplicate after lowering (keep the first occurrence)
        {
            let mut seen = std::collections::HashSet::new();
            let before = db.accounts.len();
            db.accounts.retain(|a| seen.insert(a.username.clone()));
            if db.accounts.len() != before {
                log::warn!(
                    "Removed {} duplicate accounts after case-normalisation",
                    before - db.accounts.len()
                );
                db_changed = true;
            }
        }
        if db_changed {
            if let Err(e) = save_auth_db(&db) {
                log::error!("Failed to persist auth DB after username migration: {}", e);
            }
        }

        let mut sessions = load_sessions().unwrap_or_default();

        // Normalise session usernames to lowercase
        let mut sessions_changed = false;
        for entry in sessions.values_mut() {
            let lower = entry.username.to_ascii_lowercase();
            if entry.username != lower {
                entry.username = lower;
                sessions_changed = true;
            }
        }

        // Rename mixed-case gallery directories to lowercase
        if let Some(gallery_base) = config::gallery_dir() {
            let users_dir = gallery_base.join("users");
            if users_dir.is_dir() {
                if let Ok(entries) = std::fs::read_dir(&users_dir) {
                    for entry in entries.flatten() {
                        if let Some(name) = entry.file_name().to_str() {
                            let lower = name.to_ascii_lowercase();
                            if name != lower && entry.path().is_dir() {
                                let target = users_dir.join(&lower);
                                if target.exists() {
                                    log::warn!(
                                        "Cannot rename gallery '{}' → '{}': target already exists",
                                        name,
                                        lower
                                    );
                                } else {
                                    log::info!(
                                        "Renaming gallery directory '{}' → '{}'",
                                        name,
                                        lower
                                    );
                                    if let Err(e) = std::fs::rename(entry.path(), &target) {
                                        log::error!(
                                            "Failed to rename gallery dir '{}': {}",
                                            name,
                                            e
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // Prune sessions for accounts that no longer exist
        let valid_usernames: std::collections::HashSet<&str> =
            db.accounts.iter().map(|a| a.username.as_str()).collect();
        sessions.retain(|_, entry| valid_usernames.contains(entry.username.as_str()));
        // Prune expired sessions
        let now = Utc::now();
        sessions.retain(|_, entry| {
            chrono::DateTime::parse_from_rfc3339(&entry.created_at)
                .map(|t| (now - t.with_timezone(&Utc)).num_seconds() < SESSION_TTL_SECS)
                .unwrap_or(false) // drop entries with unparseable timestamps
        });

        if sessions_changed {
            if let Err(e) = save_sessions(&sessions) {
                log::error!("Failed to persist sessions after username migration: {}", e);
            }
        }

        Self {
            db: RwLock::new(db),
            sessions: RwLock::new(sessions),
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
        let username = username.to_ascii_lowercase();
        let mut db = self.db.write().unwrap();
        if db
            .accounts
            .iter()
            .any(|a| a.username.eq_ignore_ascii_case(&username))
        {
            return Err("Username already exists".to_string());
        }
        db.accounts.push(Account {
            username: username.clone(),
            password_hash: hash_password(password),
            must_change_password: temp,
            role: "user".to_string(),
            created_at: Utc::now().to_rfc3339(),
            last_online: None,
            storage_limit_bytes: DEFAULT_STORAGE_LIMIT,
            can_use_modelhub: false,
        });
        save_auth_db(&db)?;
        Ok(())
    }

    /// Authenticate and return a session token plus whether a password change
    /// is required.
    pub fn login(&self, username: &str, password: &str) -> Result<(String, bool), String> {
        let username = username.to_ascii_lowercase();
        let db = self.db.read().unwrap();
        let account = db
            .accounts
            .iter()
            .find(|a| a.username.eq_ignore_ascii_case(&username))
            .ok_or("Invalid username or password")?;

        if !verify_password(password, &account.password_hash) {
            return Err("Invalid username or password".to_string());
        }

        let must_change = account.must_change_password;

        // Transparently upgrade legacy SHA-256 hash to Argon2id
        if is_legacy_sha256(&account.password_hash) {
            drop(db); // release read lock
            let mut db = self.db.write().unwrap();
            if let Some(acc) = db
                .accounts
                .iter_mut()
                .find(|a| a.username.eq_ignore_ascii_case(&username))
            {
                acc.password_hash = hash_password(password);
                let _ = save_auth_db(&db);
            }
        }

        let token = generate_token();
        {
            let mut sessions = self.sessions.write().unwrap();
            sessions.insert(
                token.clone(),
                SessionEntry {
                    username: username.to_string(),
                    created_at: Utc::now().to_rfc3339(),
                },
            );
            if let Err(e) = save_sessions(&sessions) {
                log::error!("Failed to persist sessions after login: {}", e);
            }
        }
        Ok((token, must_change))
    }

    /// Validate a session token. Returns the username if valid and not expired.
    pub fn validate_token(&self, token: &str) -> Option<String> {
        let sessions = self.sessions.read().unwrap();
        if let Some(entry) = sessions.get(token) {
            // Check TTL
            let now = Utc::now();
            let expired = chrono::DateTime::parse_from_rfc3339(&entry.created_at)
                .map(|t| (now - t.with_timezone(&Utc)).num_seconds() >= SESSION_TTL_SECS)
                .unwrap_or(true);
            if expired {
                drop(sessions);
                let mut sessions = self.sessions.write().unwrap();
                sessions.remove(token);
                if let Err(e) = save_sessions(&sessions) {
                    log::error!("Failed to persist sessions after TTL prune: {}", e);
                }
                return None;
            }
            Some(entry.username.clone())
        } else {
            None
        }
    }

    /// Invalidate a session token.
    pub fn logout(&self, token: &str) {
        let mut sessions = self.sessions.write().unwrap();
        sessions.remove(token);
        if let Err(e) = save_sessions(&sessions) {
            log::error!("Failed to persist sessions after logout: {}", e);
        }
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
            .find(|a| a.username.eq_ignore_ascii_case(username))
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
            .find(|a| a.username.eq_ignore_ascii_case(username))
            .ok_or("Account not found")?;
        account.role = role.to_string();
        save_auth_db(&db)?;
        Ok(())
    }

    /// Get the modelhub access flag for a user account.
    pub fn get_modelhub_access(&self, username: &str) -> Option<bool> {
        let db = self.db.read().unwrap();
        db.accounts
            .iter()
            .find(|a| a.username.eq_ignore_ascii_case(username))
            .map(|a| a.can_use_modelhub)
    }

    /// Set the modelhub access flag for a user account.
    pub fn set_modelhub_access(&self, username: &str, allowed: bool) -> Result<(), String> {
        let mut db = self.db.write().unwrap();
        let account = db
            .accounts
            .iter_mut()
            .find(|a| a.username.eq_ignore_ascii_case(username))
            .ok_or("Account not found")?;
        account.can_use_modelhub = allowed;
        save_auth_db(&db)?;
        Ok(())
    }

    /// Delete an account by username.
    pub fn delete_account(&self, username: &str) -> Result<(), String> {
        let mut db = self.db.write().unwrap();
        let before = db.accounts.len();
        db.accounts
            .retain(|a| !a.username.eq_ignore_ascii_case(username));
        if db.accounts.len() == before {
            return Err("Account not found".to_string());
        }
        save_auth_db(&db)?;
        // Also remove any active sessions for this user
        let mut sessions = self.sessions.write().unwrap();
        sessions.retain(|_, entry| !entry.username.eq_ignore_ascii_case(username));
        if let Err(e) = save_sessions(&sessions) {
            log::error!("Failed to persist sessions after account deletion: {}", e);
        }
        Ok(())
    }

    /// Get the storage limit in bytes for a user.
    pub fn get_storage_limit(&self, username: &str) -> u64 {
        let db = self.db.read().unwrap();
        db.accounts
            .iter()
            .find(|a| a.username.eq_ignore_ascii_case(username))
            .map(|a| a.storage_limit_bytes)
            .unwrap_or(DEFAULT_STORAGE_LIMIT)
    }

    /// Set the storage limit in bytes for a user. Admin/moderator only.
    pub fn set_storage_limit(&self, username: &str, limit_bytes: u64) -> Result<(), String> {
        let mut db = self.db.write().unwrap();
        let account = db
            .accounts
            .iter_mut()
            .find(|a| a.username.eq_ignore_ascii_case(username))
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
            .find(|a| a.username.eq_ignore_ascii_case(username))
            .ok_or("Account not found")?;

        if !verify_password(current_password, &account.password_hash) {
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
            .find(|a| a.username.eq_ignore_ascii_case(username))
            .ok_or("Account not found")?;

        account.password_hash = hash_password(temp_password);
        account.must_change_password = true;
        save_auth_db(&db)?;
        // Revoke existing sessions for this user so they must re-login
        let mut sessions = self.sessions.write().unwrap();
        sessions.retain(|_, entry| !entry.username.eq_ignore_ascii_case(username));
        if let Err(e) = save_sessions(&sessions) {
            log::error!("Failed to persist sessions after password reset: {}", e);
        }
        Ok(())
    }

    /// Update the last-activity timestamp for a user.
    pub fn touch_activity(&self, username: &str) {
        let mut map = self.last_activity.write().unwrap();
        map.insert(username.to_ascii_lowercase(), std::time::Instant::now());
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
            if activity.contains_key(&account.username.to_ascii_lowercase()) {
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

    /// List all users with their role, online/offline status, timestamps, storage limit, and modelhub access.
    /// A user is "online" if their last activity was within `threshold`.
    pub fn list_users_status(
        &self,
        threshold: std::time::Duration,
    ) -> Vec<(String, String, bool, String, Option<String>, u64, bool)> {
        let db = self.db.read().unwrap();
        let activity = self.last_activity.read().unwrap();
        db.accounts
            .iter()
            .map(|a| {
                let online = activity
                    .get(&a.username.to_ascii_lowercase())
                    .is_some_and(|t| t.elapsed() < threshold);
                (
                    a.username.clone(),
                    a.role.clone(),
                    online,
                    a.created_at.clone(),
                    a.last_online.clone(),
                    a.storage_limit_bytes,
                    a.can_use_modelhub,
                )
            })
            .collect()
    }
}

/// Returns true if the stored hash is a legacy 64-character hex SHA-256 string.
fn is_legacy_sha256(hash: &str) -> bool {
    hash.len() == 64 && hash.chars().all(|c| c.is_ascii_hexdigit())
}

/// Hash a password with Argon2id (returns a PHC-format string including salt).
fn hash_password(password: &str) -> String {
    let salt = SaltString::generate(&mut argon2::password_hash::rand_core::OsRng);
    let argon2 = Argon2::default();
    argon2
        .hash_password(password.as_bytes(), &salt)
        .expect("Argon2 hashing should not fail")
        .to_string()
}

/// Verify a password against a stored hash.
/// Supports both Argon2id (PHC string) and legacy SHA-256 (64 hex chars).
fn verify_password(password: &str, stored_hash: &str) -> bool {
    if is_legacy_sha256(stored_hash) {
        // Legacy path: plain SHA-256 comparison
        let mut hasher = Sha256::new();
        hasher.update(password.as_bytes());
        let computed = format!("{:x}", hasher.finalize());
        computed == stored_hash
    } else {
        // Argon2id verification
        match PasswordHash::new(stored_hash) {
            Ok(parsed) => Argon2::default()
                .verify_password(password.as_bytes(), &parsed)
                .is_ok(),
            Err(_) => false,
        }
    }
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

// --- Session persistence ---

fn sessions_db_path() -> Option<PathBuf> {
    config::app_data_dir().map(|d| d.join("sessions.json"))
}

fn load_sessions() -> Result<HashMap<String, SessionEntry>, String> {
    let path = sessions_db_path().ok_or("No app data dir")?;
    if !path.exists() {
        return Ok(HashMap::new());
    }
    let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    // Try new format first (token → SessionEntry)
    if let Ok(entries) = serde_json::from_str::<HashMap<String, SessionEntry>>(&content) {
        return Ok(entries);
    }
    // Fall back to legacy format (token → username string) — migrate on load
    if let Ok(legacy) = serde_json::from_str::<HashMap<String, String>>(&content) {
        let now = Utc::now().to_rfc3339();
        let migrated: HashMap<String, SessionEntry> = legacy
            .into_iter()
            .map(|(token, username)| {
                (
                    token,
                    SessionEntry {
                        username,
                        created_at: now.clone(),
                    },
                )
            })
            .collect();
        return Ok(migrated);
    }
    Err("Failed to parse sessions.json".to_string())
}

fn save_sessions(sessions: &HashMap<String, SessionEntry>) -> Result<(), String> {
    let path = sessions_db_path().ok_or("No app data dir")?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(sessions).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| e.to_string())?;
    Ok(())
}
