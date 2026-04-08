//! Local account authentication for LAN mode.
//!
//! Stores accounts in `{app_data_dir}/auth.json` with bcrypt-hashed passwords.
//! Sessions are tracked via random bearer tokens held in memory.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::RwLock;

use crate::config;

/// A stored user account.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub username: String,
    /// SHA-256 hash of the password (hex-encoded). Not bcrypt for simplicity
    /// in MVP — upgrade to argon2 later.
    pub password_hash: String,
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
}

impl AuthState {
    pub fn new() -> Self {
        let db = load_auth_db().unwrap_or_default();
        Self {
            db: RwLock::new(db),
            sessions: RwLock::new(HashMap::new()),
        }
    }

    /// Check if any accounts exist.
    pub fn has_accounts(&self) -> bool {
        let db = self.db.read().unwrap();
        !db.accounts.is_empty()
    }

    /// Create a new account. Returns error if username already exists.
    pub fn create_account(&self, username: &str, password: &str) -> Result<(), String> {
        let mut db = self.db.write().unwrap();
        if db.accounts.iter().any(|a| a.username == username) {
            return Err("Username already exists".to_string());
        }
        db.accounts.push(Account {
            username: username.to_string(),
            password_hash: hash_password(password),
        });
        save_auth_db(&db)?;
        Ok(())
    }

    /// Authenticate and return a session token.
    pub fn login(&self, username: &str, password: &str) -> Result<String, String> {
        let db = self.db.read().unwrap();
        let account = db
            .accounts
            .iter()
            .find(|a| a.username == username)
            .ok_or("Invalid username or password")?;

        if account.password_hash != hash_password(password) {
            return Err("Invalid username or password".to_string());
        }

        let token = generate_token();
        self.sessions
            .write()
            .unwrap()
            .insert(token.clone(), username.to_string());
        Ok(token)
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
