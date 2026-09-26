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
use std::net::{IpAddr, Ipv6Addr};
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use std::time::{Duration, Instant};

use crate::config;
use crate::notifications::NotificationState;

/// Default storage limit per user: 1 GB.
const DEFAULT_STORAGE_LIMIT: u64 = 1024 * 1024 * 1024;

/// Default image expiry: 7 days in seconds.
pub const DEFAULT_EXPIRY_SECS: u64 = 7 * 24 * 60 * 60;

/// Session TTL: 7 days.
const SESSION_TTL_SECS: i64 = 7 * 24 * 60 * 60;

/// Grace period before legacy SHA-256 password hashes are auto-upgraded on login.
const LEGACY_PASSWORD_GRACE_DAYS: i64 = 30;

/// Failed login attempts before a temporary lockout.
const MAX_LOGIN_ATTEMPTS: u32 = 15;

/// Lockout duration after too many failed logins.
const LOGIN_LOCKOUT: Duration = Duration::from_secs(15 * 60);

/// Failed logins from one client address before that address is refused for
/// `LOGIN_LOCKOUT`. Kept below `MAX_LOGIN_ATTEMPTS` so a single client can
/// never trip another account's lockout on its own.
const MAX_IP_LOGIN_ATTEMPTS: u32 = 10;

/// A client address's failures are forgotten after this long without another.
const IP_LOGIN_WINDOW: Duration = LOGIN_LOCKOUT;

/// Upper bound on tracked client addresses, so a flood of distinct sources
/// cannot grow the limiter without bound.
const MAX_TRACKED_LOGIN_IPS: usize = 4096;

/// Longest username accepted for a new account.
const MAX_USERNAME_LEN: usize = 32;

/// Persisted sessions file format (v2 stores hashed token keys).
const SESSIONS_FORMAT_VERSION: u32 = 2;

#[derive(Debug, Clone)]
struct LoginAttemptState {
    failures: u32,
    locked_until: Option<Instant>,
    last_failure: Instant,
}

impl LoginAttemptState {
    fn new(now: Instant) -> Self {
        Self {
            failures: 0,
            locked_until: None,
            last_failure: now,
        }
    }

    /// The lockout error, while a lockout is running.
    fn lockout_error(&self, now: Instant) -> Option<String> {
        let until = self.locked_until.filter(|until| now < *until)?;
        let mins = until.saturating_duration_since(now).as_secs().div_ceil(60);
        Some(format!(
            "Too many failed login attempts. Try again in about {} minute(s).",
            mins.max(1)
        ))
    }

    fn is_locked(&self, now: Instant) -> bool {
        self.locked_until.is_some_and(|until| now < until)
    }

    fn lock_expired(&self, now: Instant) -> bool {
        self.locked_until.is_some_and(|until| now >= until)
    }
}

/// Whether `username` may be given to a NEW account: 1 to `MAX_USERNAME_LEN`
/// ASCII letters, digits, `_` or `-`. The name becomes a directory under the
/// gallery and export dirs, so nothing a path parser could read as structure
/// is allowed. Only enforced at creation, so older accounts keep working.
pub fn is_valid_new_username(username: &str) -> bool {
    !username.is_empty()
        && username.len() <= MAX_USERNAME_LEN
        && username
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// The error for a name `is_valid_new_username` rejects.
pub fn invalid_new_username_error() -> String {
    format!(
        "Usernames may only contain letters, numbers, '-' and '_' (up to {} characters)",
        MAX_USERNAME_LEN
    )
}

/// The key a client address is rate-limited under. IPv4-mapped IPv6 folds onto
/// plain IPv4, and IPv6 is grouped by /64, since a single host can hand itself
/// any address inside its prefix. `None` for loopback, which is the local
/// operator and is never limited.
fn login_ip_key(ip: IpAddr) -> Option<IpAddr> {
    match ip.to_canonical() {
        ip if ip.is_loopback() => None,
        IpAddr::V6(v6) => {
            let s = v6.segments();
            Some(IpAddr::V6(Ipv6Addr::new(
                s[0], s[1], s[2], s[3], 0, 0, 0, 0,
            )))
        }
        v4 => Some(v4),
    }
}

/// Count one failed login against an account. Returns true when this failure
/// started a lockout. Failures for names that are not accounts are ignored,
/// and expired lockouts are pruned (the next failure would reset them anyway,
/// so the counting itself is unchanged).
fn record_account_failure(
    attempts: &mut HashMap<String, LoginAttemptState>,
    username: String,
    exists: bool,
    now: Instant,
) -> bool {
    attempts.retain(|_, state| !state.lock_expired(now));
    if !exists {
        return false;
    }
    let state = attempts
        .entry(username)
        .or_insert_with(|| LoginAttemptState::new(now));
    if state.is_locked(now) {
        return false;
    }
    state.failures = state.failures.saturating_add(1);
    state.last_failure = now;
    if state.failures >= MAX_LOGIN_ATTEMPTS {
        state.locked_until = Some(now + LOGIN_LOCKOUT);
        return true;
    }
    false
}

/// Count one failed login against a client address. Returns true when this
/// failure started a lockout. Stale entries are pruned first and the map is
/// capped at `MAX_TRACKED_LOGIN_IPS`.
fn record_ip_failure(
    attempts: &mut HashMap<IpAddr, LoginAttemptState>,
    key: IpAddr,
    now: Instant,
) -> bool {
    attempts.retain(|k, state| {
        *k == key
            || state.is_locked(now)
            || now.saturating_duration_since(state.last_failure) < IP_LOGIN_WINDOW
    });
    if !attempts.contains_key(&key) && attempts.len() >= MAX_TRACKED_LOGIN_IPS {
        // Evict the quietest address, preferring ones that are not locked out.
        let victim = attempts
            .iter()
            .min_by_key(|(_, state)| (state.is_locked(now), state.last_failure))
            .map(|(k, _)| *k);
        if let Some(victim) = victim {
            attempts.remove(&victim);
        }
    }
    let state = attempts
        .entry(key)
        .or_insert_with(|| LoginAttemptState::new(now));
    if state.is_locked(now) {
        return false;
    }
    if state.lock_expired(now)
        || now.saturating_duration_since(state.last_failure) >= IP_LOGIN_WINDOW
    {
        state.locked_until = None;
        state.failures = 0;
    }
    state.failures = state.failures.saturating_add(1);
    state.last_failure = now;
    if state.failures >= MAX_IP_LOGIN_ATTEMPTS {
        state.locked_until = Some(now + LOGIN_LOCKOUT);
        return true;
    }
    false
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct SessionsStore {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    sessions: HashMap<String, SessionEntry>,
}

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
    /// ISO 8601 deadline after which legacy SHA-256 password hashes are auto-upgraded on login.
    #[serde(default)]
    pub legacy_password_grace_deadline: Option<String>,
    /// Whether the one-time global notification about the legacy password migration was sent.
    #[serde(default)]
    pub legacy_password_announcement_sent: bool,
}

/// A persisted session entry with TTL metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionEntry {
    pub username: String,
    /// ISO 8601 timestamp when the session was created.
    pub created_at: String,
}

/// Auth state with persistent sessions.
///
/// Lock discipline: `db` may be held while taking `sessions`, never the other
/// way round; `last_activity` is never held together with `db`. Argon2 work
/// (hashing and verification) runs with no lock held, and callers on an async
/// runtime should run the methods that do it (`login`, `create_account*`,
/// `upgrade_password_encryption`, `change_password`, `reset_password`) on a
/// blocking thread.
pub struct AuthState {
    db: RwLock<AuthDatabase>,
    /// Active session tokens → session entry. Persisted to disk so tokens
    /// survive server restarts (enables "remember me"). Expired entries are
    /// pruned on load and periodically on validation.
    sessions: RwLock<HashMap<String, SessionEntry>>,
    /// Per-user last activity timestamp (username → Instant).
    last_activity: RwLock<HashMap<String, std::time::Instant>>,
    /// Failed login counters (username → attempt state). Only existing
    /// accounts are tracked, so this is bounded by the account list.
    login_attempts: RwLock<HashMap<String, LoginAttemptState>>,
    /// Failed login counters per client address (see `login_ip_key`).
    ip_login_attempts: RwLock<HashMap<IpAddr, LoginAttemptState>>,
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
        let has_legacy = db
            .accounts
            .iter()
            .any(|a| is_legacy_sha256(&a.password_hash));
        if has_legacy && db.legacy_password_grace_deadline.is_none() {
            let deadline = Utc::now() + chrono::Duration::days(LEGACY_PASSWORD_GRACE_DAYS);
            db.legacy_password_grace_deadline = Some(deadline.to_rfc3339());
            db_changed = true;
            log::info!(
                "Legacy password encryption migration: {} account(s) must upgrade to Argon2id by {}",
                db.accounts
                    .iter()
                    .filter(|a| is_legacy_sha256(&a.password_hash))
                    .count(),
                deadline.to_rfc3339()
            );
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

        // Computed now so the first unknown-username login is not slower than
        // the rest (see `verify_login_password`).
        let _ = dummy_password_hash();

        Self {
            db: RwLock::new(db),
            sessions: RwLock::new(sessions),
            last_activity: RwLock::new(HashMap::new()),
            login_attempts: RwLock::new(HashMap::new()),
            ip_login_attempts: RwLock::new(HashMap::new()),
        }
    }

    /// The stored hash of an account, read under a short-lived lock so the
    /// slow Argon2 check that follows runs with no lock held.
    fn stored_password_hash(&self, username: &str) -> Option<String> {
        let db = self.db.read().unwrap();
        db.accounts
            .iter()
            .find(|a| a.username.eq_ignore_ascii_case(username))
            .map(|a| a.password_hash.clone())
    }

    /// Replace an account's hash only if it still equals `expected`, so a
    /// password changed while Argon2 ran unlocked is never overwritten.
    /// Returns whether the new hash was stored (and persisted).
    fn swap_password_hash(
        &self,
        username: &str,
        expected: &str,
        new_hash: String,
        must_change_password: Option<bool>,
    ) -> Result<bool, String> {
        let mut db = self.db.write().unwrap();
        let Some(account) = db
            .accounts
            .iter_mut()
            .find(|a| a.username.eq_ignore_ascii_case(username))
        else {
            return Ok(false);
        };
        if account.password_hash != expected {
            return Ok(false);
        }
        account.password_hash = new_hash;
        if let Some(flag) = must_change_password {
            account.must_change_password = flag;
        }
        save_auth_db(&db)?;
        Ok(true)
    }

    /// Returns an error when the account is temporarily locked out.
    pub fn check_login_allowed(&self, username: &str) -> Result<(), String> {
        let username = username.to_ascii_lowercase();
        let attempts = self.login_attempts.read().unwrap();
        match attempts
            .get(&username)
            .and_then(|state| state.lockout_error(Instant::now()))
        {
            Some(err) => Err(err),
            None => Ok(()),
        }
    }

    /// Returns an error while `ip` is refused for too many failed logins.
    pub fn check_ip_login_allowed(&self, ip: IpAddr) -> Result<(), String> {
        let Some(key) = login_ip_key(ip) else {
            return Ok(());
        };
        let attempts = self.ip_login_attempts.read().unwrap();
        match attempts
            .get(&key)
            .and_then(|state| state.lockout_error(Instant::now()))
        {
            Some(err) => Err(err),
            None => Ok(()),
        }
    }

    /// Count a failed login from `ip`, whatever username it named.
    pub fn record_failed_login_ip(&self, ip: IpAddr) {
        let Some(key) = login_ip_key(ip) else {
            return;
        };
        let mut attempts = self.ip_login_attempts.write().unwrap();
        if record_ip_failure(&mut attempts, key, Instant::now()) {
            log::warn!(
                "Login lockout triggered for a client address after {} failed attempts",
                MAX_IP_LOGIN_ATTEMPTS
            );
        }
    }

    pub fn record_failed_login(&self, username: &str) {
        let username = username.to_ascii_lowercase();
        // Only real accounts are counted: otherwise anyone could grow this map
        // with made-up names, and a name that does not exist has nothing to
        // protect. Looked up before taking `login_attempts` so the two locks
        // are never held together.
        let exists = {
            let db = self.db.read().unwrap();
            db.accounts
                .iter()
                .any(|a| a.username.eq_ignore_ascii_case(&username))
        };
        let mut attempts = self.login_attempts.write().unwrap();
        if record_account_failure(&mut attempts, username, exists, Instant::now()) {
            log::warn!(
                "Login lockout triggered after {} failed attempts",
                MAX_LOGIN_ATTEMPTS
            );
        }
    }

    pub fn clear_login_attempts(&self, username: &str) {
        let username = username.to_ascii_lowercase();
        self.login_attempts.write().unwrap().remove(&username);
    }

    /// ISO 8601 deadline when legacy SHA-256 password hashes expire.
    pub fn legacy_password_deadline(&self) -> Option<String> {
        self.db
            .read()
            .unwrap()
            .legacy_password_grace_deadline
            .clone()
    }

    pub fn is_legacy_password_grace_expired(&self) -> bool {
        let db = self.db.read().unwrap();
        match db.legacy_password_grace_deadline.as_deref() {
            Some(deadline) => chrono::DateTime::parse_from_rfc3339(deadline)
                .map(|t| Utc::now() > t.with_timezone(&Utc))
                .unwrap_or(false),
            None => false,
        }
    }

    pub fn account_uses_legacy_password(&self, username: &str) -> bool {
        let db = self.db.read().unwrap();
        db.accounts
            .iter()
            .find(|a| a.username.eq_ignore_ascii_case(username))
            .is_some_and(|a| is_legacy_sha256(&a.password_hash))
    }

    /// Re-hash the user's current password with Argon2id without changing the password text.
    /// Returns `true` when upgraded, `false` when already on Argon2id.
    /// Emit a one-time global notification about the legacy password encryption migration.
    pub fn try_emit_legacy_password_announcement(&self, notifications: &NotificationState) {
        let should_emit = {
            let db = self.db.read().unwrap();
            let has_legacy = db
                .accounts
                .iter()
                .any(|a| is_legacy_sha256(&a.password_hash));
            has_legacy
                && !db.legacy_password_announcement_sent
                && db.legacy_password_grace_deadline.is_some()
        };
        if !should_emit {
            return;
        }

        let deadline = self.legacy_password_deadline().unwrap_or_default();
        let params = serde_json::json!({ "deadline": deadline });
        notifications.create_i18n(
            "global",
            "notifications.legacy_password_migration.title",
            Some("notifications.legacy_password_migration.body"),
            Some(params),
            "warning",
        );

        let mut db = self.db.write().unwrap();
        db.legacy_password_announcement_sent = true;
        if let Err(e) = save_auth_db(&db) {
            log::error!(
                "Failed to persist auth DB after legacy password announcement: {}",
                e
            );
        }
    }

    pub fn upgrade_password_encryption(
        &self,
        username: &str,
        password: &str,
    ) -> Result<bool, String> {
        self.check_login_allowed(username)?;
        let username = username.to_ascii_lowercase();
        let stored = self.stored_password_hash(&username);

        // Verify the password before revealing anything about the stored hash
        // format, so this endpoint can't be used to probe account state.
        if !verify_login_password(password, stored.as_deref()) {
            return Err("Invalid username or password".to_string());
        }
        let Some(stored) = stored else {
            return Err("Invalid username or password".to_string());
        };

        if !is_legacy_sha256(&stored) {
            self.clear_login_attempts(&username);
            return Ok(false);
        }

        let new_hash = hash_password(password);
        if !self.swap_password_hash(&username, &stored, new_hash, None)? {
            return Err("Password changed during the upgrade; try again".to_string());
        }
        self.clear_login_attempts(&username);
        Ok(true)
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
        // Hashed before the lock: Argon2 is slow and every request that
        // resolves a role reads this table.
        let password_hash = hash_password(password);
        let mut db = self.db.write().unwrap();
        if db
            .accounts
            .iter()
            .any(|a| a.username.eq_ignore_ascii_case(&username))
        {
            return Err("Username already exists".to_string());
        }
        // After the duplicate check, so re-seeding an older account whose name
        // predates this rule still reports "already exists".
        if !is_valid_new_username(&username) {
            return Err(invalid_new_username_error());
        }
        db.accounts.push(Account {
            username: username.clone(),
            password_hash,
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
        self.check_login_allowed(username)?;
        let username = username.to_ascii_lowercase();
        // Snapshot under a short read lock; Argon2 below runs unlocked.
        let (stored, must_change, grace_deadline) = {
            let db = self.db.read().unwrap();
            let account = db
                .accounts
                .iter()
                .find(|a| a.username.eq_ignore_ascii_case(&username));
            (
                account.map(|a| a.password_hash.clone()),
                account.is_some_and(|a| a.must_change_password),
                db.legacy_password_grace_deadline.clone(),
            )
        };

        if !verify_login_password(password, stored.as_deref()) {
            return Err("Invalid username or password".to_string());
        }
        let Some(stored) = stored else {
            return Err("Invalid username or password".to_string());
        };

        let was_legacy = is_legacy_sha256(&stored);
        let grace_expired = was_legacy
            && match grace_deadline.as_deref() {
                Some(deadline) => chrono::DateTime::parse_from_rfc3339(deadline)
                    .map(|t| Utc::now() > t.with_timezone(&Utc))
                    .unwrap_or(false),
                None => false,
            };

        // Hashes this password may be stored under by the time the session is
        // minted: the verified one, or its Argon2id upgrade below.
        let mut verified_hashes = vec![stored.clone()];
        if was_legacy {
            let new_hash = hash_password(password);
            verified_hashes.push(new_hash.clone());
            match self.swap_password_hash(&username, &stored, new_hash, None) {
                Err(e) => log::error!(
                    "Failed to persist Argon2id upgrade for '{}': {}",
                    username,
                    e
                ),
                // Changed concurrently: whatever replaced it is newer.
                Ok(false) => {}
                Ok(true) if grace_expired => log::info!(
                    "Auto-upgraded legacy password encryption for '{}' after grace period",
                    username
                ),
                Ok(true) => log::info!(
                    "Upgraded legacy password encryption for '{}' on login",
                    username
                ),
            }
        }

        let token = generate_token();
        {
            // Verification ran unlocked, so the password may have been changed
            // (or reset) meanwhile, revoking the account's sessions. Re-check
            // under `db` and keep it held while the session is inserted, so a
            // concurrent change either refuses this login or revokes it.
            let db = self.db.read().unwrap();
            let unchanged = db
                .accounts
                .iter()
                .find(|a| a.username.eq_ignore_ascii_case(&username))
                .is_some_and(|a| verified_hashes.contains(&a.password_hash));
            if !unchanged {
                return Err("Invalid username or password".to_string());
            }
            let mut sessions = self.sessions.write().unwrap();
            sessions.insert(
                hash_session_token(&token),
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
        let token_key = hash_session_token(token);
        let sessions = self.sessions.read().unwrap();
        if let Some(entry) = sessions.get(&token_key) {
            // Check TTL
            let now = Utc::now();
            let expired = chrono::DateTime::parse_from_rfc3339(&entry.created_at)
                .map(|t| (now - t.with_timezone(&Utc)).num_seconds() >= SESSION_TTL_SECS)
                .unwrap_or(true);
            if expired {
                drop(sessions);
                let mut sessions = self.sessions.write().unwrap();
                sessions.remove(&token_key);
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
        sessions.remove(&hash_session_token(token));
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

    /// Usernames of every account whose role is in `roles`. Used to target
    /// notifications at staff (admins/moderators) instead of broadcasting them
    /// to all users.
    pub fn usernames_with_roles(&self, roles: &[&str]) -> Vec<String> {
        let db = self.db.read().unwrap();
        db.accounts
            .iter()
            .filter(|a| roles.iter().any(|r| a.role.eq_ignore_ascii_case(r)))
            .map(|a| a.username.clone())
            .collect()
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
    /// verification. Clears the `must_change_password` flag and revokes every
    /// other session of the account; `keep_token` (the session making the
    /// change) stays signed in.
    pub fn change_password(
        &self,
        username: &str,
        current_password: &str,
        new_password: &str,
        keep_token: Option<&str>,
    ) -> Result<(), String> {
        if new_password.len() < 4 {
            return Err("New password must be at least 4 characters".to_string());
        }
        let stored = self
            .stored_password_hash(username)
            .ok_or("Account not found")?;
        if !verify_password(current_password, &stored) {
            return Err("Current password is incorrect".to_string());
        }
        let new_hash = hash_password(new_password);
        if !self.swap_password_hash(username, &stored, new_hash, Some(false))? {
            return Err("Password was changed by another session; try again".to_string());
        }

        let keep_key = keep_token.map(hash_session_token);
        let mut sessions = self.sessions.write().unwrap();
        let before = sessions.len();
        sessions.retain(|key, entry| {
            keep_session_after_password_change(key, entry, username, keep_key.as_deref())
        });
        if sessions.len() != before {
            if let Err(e) = save_sessions(&sessions) {
                log::error!("Failed to persist sessions after password change: {}", e);
            }
        }
        Ok(())
    }

    /// Admin: set a temporary password on an account, forcing the user to
    /// choose a new one at next login.
    pub fn reset_password(&self, username: &str, temp_password: &str) -> Result<(), String> {
        if temp_password.len() < 4 {
            return Err("Temporary password must be at least 4 characters".to_string());
        }
        // Hashed before the lock (see `create_account_ex`).
        let temp_hash = hash_password(temp_password);
        let mut db = self.db.write().unwrap();
        let account = db
            .accounts
            .iter_mut()
            .find(|a| a.username.eq_ignore_ascii_case(username))
            .ok_or("Account not found")?;

        account.password_hash = temp_hash;
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
        // Snapshot and release `last_activity` before taking `db`: holding
        // both, in the opposite order to other paths, can deadlock.
        let activity: std::collections::HashSet<String> =
            self.last_activity.read().unwrap().keys().cloned().collect();
        if activity.is_empty() {
            return;
        }
        let now = Utc::now();
        let mut db = self.db.write().unwrap();
        let mut changed = false;
        for account in &mut db.accounts {
            if activity.contains(&account.username.to_ascii_lowercase()) {
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
        // One lock at a time (see `flush_last_online`).
        let online_users: std::collections::HashSet<String> = self
            .last_activity
            .read()
            .unwrap()
            .iter()
            .filter(|(_, t)| t.elapsed() < threshold)
            .map(|(name, _)| name.clone())
            .collect();
        let db = self.db.read().unwrap();
        db.accounts
            .iter()
            .map(|a| {
                let online = online_users.contains(&a.username.to_ascii_lowercase());
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

/// Hash a session token (SHA-256) for storage/lookup in the session table.
fn hash_session_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Verify a password against a stored hash.
/// Supports both Argon2id (PHC string) and legacy SHA-256 (64 hex chars).
fn verify_password(password: &str, stored_hash: &str) -> bool {
    if is_legacy_sha256(stored_hash) {
        // Legacy path: constant-time SHA-256 comparison (upgraded to Argon2id on login).
        let mut hasher = Sha256::new();
        hasher.update(password.as_bytes());
        let computed = format!("{:x}", hasher.finalize());
        use subtle::ConstantTimeEq;
        computed.as_bytes().ct_eq(stored_hash.as_bytes()).into()
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

/// A real Argon2id hash of a throwaway password. Unknown usernames are
/// verified against it so a failed login costs the same Argon2 work whether
/// or not the account exists, and response timing does not reveal it.
fn dummy_password_hash() -> &'static str {
    static HASH: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    HASH.get_or_init(|| hash_password("mooshieui-login-timing-equaliser"))
}

/// Login-path password check with uniform cost: an unknown account (`None`)
/// and a legacy SHA-256 hash both also run one Argon2 verification, so the
/// time taken does not tell which usernames exist or which still use the
/// legacy format.
fn verify_login_password(password: &str, stored_hash: Option<&str>) -> bool {
    match stored_hash {
        None => {
            let _ = verify_password(password, dummy_password_hash());
            false
        }
        Some(hash) if is_legacy_sha256(hash) => {
            let _ = verify_password(password, dummy_password_hash());
            verify_password(password, hash)
        }
        Some(hash) => verify_password(password, hash),
    }
}

/// Whether a session survives its account's password change: sessions of
/// other accounts do, and so does the one that made the change.
fn keep_session_after_password_change(
    session_key: &str,
    entry: &SessionEntry,
    username: &str,
    keep_key: Option<&str>,
) -> bool {
    !entry.username.eq_ignore_ascii_case(username) || keep_key == Some(session_key)
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

#[cfg(unix)]
fn restrict_private_file_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn restrict_private_file_permissions(_path: &Path) {}

fn save_auth_db(db: &AuthDatabase) -> Result<(), String> {
    let path = auth_db_path().ok_or("No app data dir")?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(db).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| e.to_string())?;
    restrict_private_file_permissions(&path);
    Ok(())
}

// --- Session persistence ---

fn sessions_db_path() -> Option<PathBuf> {
    config::app_data_dir().map(|d| d.join("sessions.json"))
}

fn migrate_plaintext_session_keys(
    sessions: HashMap<String, SessionEntry>,
) -> HashMap<String, SessionEntry> {
    sessions
        .into_iter()
        .map(|(key, entry)| (hash_session_token(&key), entry))
        .collect()
}

struct LoadedSessions {
    sessions: HashMap<String, SessionEntry>,
    needs_save: bool,
}

fn content_looks_like_sessions_store(content: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(content)
        .ok()
        .and_then(|value| {
            value
                .as_object()
                .map(|object| object.contains_key("version") || object.contains_key("sessions"))
        })
        .unwrap_or(false)
}

fn parse_sessions_content(content: &str) -> Result<LoadedSessions, String> {
    if content_looks_like_sessions_store(content) {
        if let Ok(store) = serde_json::from_str::<SessionsStore>(content) {
            if store.version >= SESSIONS_FORMAT_VERSION {
                return Ok(LoadedSessions {
                    sessions: store.sessions,
                    needs_save: false,
                });
            }
            return Ok(LoadedSessions {
                sessions: migrate_plaintext_session_keys(store.sessions),
                needs_save: true,
            });
        }
    }
    // Legacy v1: token → SessionEntry (plaintext token keys)
    if let Ok(entries) = serde_json::from_str::<HashMap<String, SessionEntry>>(content) {
        return Ok(LoadedSessions {
            sessions: migrate_plaintext_session_keys(entries),
            needs_save: true,
        });
    }
    // Legacy v0: token → username string
    if let Ok(legacy) = serde_json::from_str::<HashMap<String, String>>(content) {
        let now = Utc::now().to_rfc3339();
        let sessions = legacy
            .into_iter()
            .map(|(token, username)| {
                (
                    hash_session_token(&token),
                    SessionEntry {
                        username,
                        created_at: now.clone(),
                    },
                )
            })
            .collect();
        return Ok(LoadedSessions {
            sessions,
            needs_save: true,
        });
    }
    Err("Failed to parse sessions.json".to_string())
}

fn load_sessions() -> Result<HashMap<String, SessionEntry>, String> {
    let path = sessions_db_path().ok_or("No app data dir")?;
    if !path.exists() {
        return Ok(HashMap::new());
    }
    let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let loaded = parse_sessions_content(&content)?;
    if loaded.needs_save {
        let _ = save_sessions(&loaded.sessions);
    }
    Ok(loaded.sessions)
}

fn save_sessions(sessions: &HashMap<String, SessionEntry>) -> Result<(), String> {
    let path = sessions_db_path().ok_or("No app data dir")?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let store = SessionsStore {
        version: SESSIONS_FORMAT_VERSION,
        sessions: sessions.clone(),
    };
    let json = serde_json::to_string_pretty(&store).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| e.to_string())?;
    restrict_private_file_permissions(&path);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session_entry(username: &str) -> SessionEntry {
        SessionEntry {
            username: username.to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn legacy_session_entry_map_is_migrated_without_being_treated_as_empty_store() {
        let token = "0123456789abcdef";
        let content = serde_json::json!({
            token: {
                "username": "Alice",
                "created_at": "2026-01-01T00:00:00Z"
            }
        })
        .to_string();

        let loaded = parse_sessions_content(&content).expect("legacy sessions should parse");
        let token_hash = hash_session_token(token);

        assert!(loaded.needs_save);
        assert_eq!(loaded.sessions.len(), 1);
        assert!(!loaded.sessions.contains_key(token));
        assert_eq!(
            loaded
                .sessions
                .get(&token_hash)
                .map(|entry| entry.username.as_str()),
            Some("Alice")
        );
    }

    #[test]
    fn current_empty_session_store_loads_without_migration() {
        let content = serde_json::json!({
            "version": SESSIONS_FORMAT_VERSION,
            "sessions": {}
        })
        .to_string();

        let loaded = parse_sessions_content(&content).expect("current sessions should parse");

        assert!(!loaded.needs_save);
        assert!(loaded.sessions.is_empty());
    }

    #[test]
    fn old_versioned_session_store_migrates_plaintext_keys() {
        let token = "fedcba9876543210";
        let content = serde_json::json!({
            "version": 1,
            "sessions": {
                token: {
                    "username": "Bob",
                    "created_at": "2026-01-01T00:00:00Z"
                }
            }
        })
        .to_string();

        let loaded = parse_sessions_content(&content).expect("old sessions store should parse");
        let token_hash = hash_session_token(token);

        assert!(loaded.needs_save);
        assert_eq!(loaded.sessions.len(), 1);
        assert_eq!(
            loaded
                .sessions
                .get(&token_hash)
                .map(|entry| entry.username.as_str()),
            Some("Bob")
        );
    }

    #[test]
    fn current_session_store_preserves_hashed_keys() {
        let token_hash = hash_session_token("already-hashed-token");
        let content = serde_json::to_string(&SessionsStore {
            version: SESSIONS_FORMAT_VERSION,
            sessions: HashMap::from([(token_hash.clone(), session_entry("Carol"))]),
        })
        .expect("sessions store should serialize");

        let loaded = parse_sessions_content(&content).expect("current sessions should parse");

        assert!(!loaded.needs_save);
        assert_eq!(
            loaded
                .sessions
                .get(&token_hash)
                .map(|entry| entry.username.as_str()),
            Some("Carol")
        );
    }

    fn test_account(username: &str, password_hash: String) -> Account {
        Account {
            username: username.to_string(),
            password_hash,
            must_change_password: false,
            role: "user".to_string(),
            created_at: Utc::now().to_rfc3339(),
            last_online: None,
            storage_limit_bytes: DEFAULT_STORAGE_LIMIT,
            can_use_modelhub: false,
        }
    }

    fn auth_with_accounts(accounts: Vec<Account>) -> AuthState {
        AuthState {
            db: RwLock::new(AuthDatabase {
                accounts,
                legacy_password_grace_deadline: None,
                legacy_password_announcement_sent: false,
            }),
            sessions: RwLock::new(HashMap::new()),
            last_activity: RwLock::new(HashMap::new()),
            login_attempts: RwLock::new(HashMap::new()),
            ip_login_attempts: RwLock::new(HashMap::new()),
        }
    }

    #[test]
    fn password_upgrade_checks_password_before_hash_format() {
        let auth = auth_with_accounts(vec![test_account("alice", hash_password("correct"))]);

        let missing = auth
            .upgrade_password_encryption("missing", "wrong")
            .unwrap_err();
        let wrong_password = auth
            .upgrade_password_encryption("alice", "wrong")
            .unwrap_err();
        assert_eq!(missing, "Invalid username or password");
        assert_eq!(wrong_password, "Invalid username or password");

        let already_modern = auth
            .upgrade_password_encryption("alice", "correct")
            .expect("correct password should be accepted");
        assert!(!already_modern);
    }
}

#[cfg(test)]
mod login_limit_tests {
    use super::*;

    fn empty_auth() -> AuthState {
        AuthState {
            db: RwLock::new(AuthDatabase::default()),
            sessions: RwLock::new(HashMap::new()),
            last_activity: RwLock::new(HashMap::new()),
            login_attempts: RwLock::new(HashMap::new()),
            ip_login_attempts: RwLock::new(HashMap::new()),
        }
    }

    fn v4(a: u8, b: u8, c: u8, d: u8) -> IpAddr {
        IpAddr::from([a, b, c, d])
    }

    #[test]
    fn unknown_usernames_are_never_counted() {
        let auth = empty_auth();
        for _ in 0..(MAX_LOGIN_ATTEMPTS * 3) {
            auth.record_failed_login("ghost");
        }
        assert!(auth.login_attempts.read().unwrap().is_empty());
        assert!(auth.check_login_allowed("ghost").is_ok());
    }

    #[test]
    fn real_accounts_keep_the_existing_lockout() {
        let mut map = HashMap::new();
        let t0 = Instant::now();
        for i in 1..MAX_LOGIN_ATTEMPTS {
            assert!(!record_account_failure(&mut map, "admin".into(), true, t0));
            assert_eq!(map["admin"].failures, i);
        }
        assert!(record_account_failure(&mut map, "admin".into(), true, t0));
        assert!(map["admin"].lockout_error(t0).is_some());
        // Further failures during the lockout neither extend nor re-trigger it.
        assert!(!record_account_failure(&mut map, "admin".into(), true, t0));
        // Once the lockout has run out the entry is pruned and counting restarts.
        let later = t0 + LOGIN_LOCKOUT;
        assert!(map["admin"].lockout_error(later).is_none());
        assert!(!record_account_failure(
            &mut map,
            "admin".into(),
            true,
            later
        ));
        assert_eq!(map["admin"].failures, 1);
    }

    #[test]
    fn expired_account_lockouts_are_pruned() {
        let mut map = HashMap::new();
        let t0 = Instant::now();
        for _ in 0..MAX_LOGIN_ATTEMPTS {
            record_account_failure(&mut map, "bob".into(), true, t0);
        }
        assert!(map.contains_key("bob"));
        record_account_failure(&mut map, "ghost".into(), false, t0 + LOGIN_LOCKOUT);
        assert!(map.is_empty());
    }

    #[test]
    fn a_single_address_cannot_trip_an_account_lockout_alone() {
        const { assert!(MAX_IP_LOGIN_ATTEMPTS < MAX_LOGIN_ATTEMPTS) };
    }

    #[test]
    fn address_limiter_locks_only_the_noisy_address() {
        let mut map = HashMap::new();
        let t0 = Instant::now();
        let noisy = v4(192, 168, 1, 50);
        for _ in 1..MAX_IP_LOGIN_ATTEMPTS {
            assert!(!record_ip_failure(&mut map, noisy, t0));
        }
        assert!(record_ip_failure(&mut map, noisy, t0));
        assert!(map[&noisy].lockout_error(t0).is_some());
        assert!(!map.contains_key(&v4(192, 168, 1, 51)));
        // The lockout ends and the count starts over.
        let later = t0 + LOGIN_LOCKOUT;
        assert!(map[&noisy].lockout_error(later).is_none());
        record_ip_failure(&mut map, noisy, later);
        assert_eq!(map[&noisy].failures, 1);
    }

    #[test]
    fn address_failures_decay_after_the_window() {
        let mut map = HashMap::new();
        let t0 = Instant::now();
        let ip = v4(10, 0, 0, 7);
        for _ in 1..MAX_IP_LOGIN_ATTEMPTS {
            record_ip_failure(&mut map, ip, t0);
        }
        record_ip_failure(&mut map, ip, t0 + IP_LOGIN_WINDOW);
        assert_eq!(map[&ip].failures, 1);
        assert!(map[&ip].locked_until.is_none());
    }

    #[test]
    fn stale_addresses_are_pruned_and_the_map_is_capped() {
        let mut map = HashMap::new();
        let t0 = Instant::now();
        for i in 0..(MAX_TRACKED_LOGIN_IPS as u32 + 50) {
            let ip = IpAddr::from((0x0a00_0000 + i).to_be_bytes());
            record_ip_failure(&mut map, ip, t0);
        }
        assert_eq!(map.len(), MAX_TRACKED_LOGIN_IPS);
        record_ip_failure(&mut map, v4(172, 16, 0, 1), t0 + IP_LOGIN_WINDOW);
        assert_eq!(map.len(), 1);
    }

    #[test]
    fn address_keys_fold_mapped_v4_and_group_v6_by_prefix() {
        assert_eq!(login_ip_key(v4(127, 0, 0, 1)), None);
        assert_eq!(login_ip_key("::1".parse().unwrap()), None);
        assert_eq!(login_ip_key("::ffff:127.0.0.1".parse().unwrap()), None);
        assert_eq!(
            login_ip_key("::ffff:192.168.1.9".parse().unwrap()),
            Some(v4(192, 168, 1, 9))
        );
        assert_eq!(
            login_ip_key("2001:db8:1:2:aaaa:bbbb:cccc:dddd".parse().unwrap()),
            login_ip_key("2001:db8:1:2::1".parse().unwrap())
        );
        assert_ne!(
            login_ip_key("2001:db8:1:2::1".parse().unwrap()),
            login_ip_key("2001:db8:1:3::1".parse().unwrap())
        );
    }

    #[test]
    fn auth_state_refuses_a_locked_address_but_never_loopback() {
        let auth = empty_auth();
        let remote = v4(192, 168, 1, 50);
        let local = v4(127, 0, 0, 1);
        for _ in 0..MAX_IP_LOGIN_ATTEMPTS {
            auth.record_failed_login_ip(remote);
            auth.record_failed_login_ip(local);
        }
        assert!(auth.check_ip_login_allowed(remote).is_err());
        assert!(auth.check_ip_login_allowed(v4(192, 168, 1, 51)).is_ok());
        assert!(auth.check_ip_login_allowed(local).is_ok());
    }

    #[test]
    fn new_usernames_are_restricted_to_a_safe_charset() {
        let longest = "a".repeat(MAX_USERNAME_LEN);
        for ok in ["alice", "Bob_2", "a-b", "x", longest.as_str()] {
            assert!(is_valid_new_username(ok), "{ok} should be accepted");
        }
        let too_long = "a".repeat(MAX_USERNAME_LEN + 1);
        for bad in [
            "",
            "john.doe",
            "D:x",
            "a/b",
            "a\\b",
            "..",
            "with space",
            "caf\u{e9}",
            too_long.as_str(),
        ] {
            assert!(!is_valid_new_username(bad), "{bad:?} should be rejected");
        }
    }
}

#[cfg(test)]
mod auth_hardening_tests {
    use super::*;

    fn auth_with(accounts: Vec<Account>) -> AuthState {
        AuthState {
            db: RwLock::new(AuthDatabase {
                accounts,
                ..AuthDatabase::default()
            }),
            sessions: RwLock::new(HashMap::new()),
            last_activity: RwLock::new(HashMap::new()),
            login_attempts: RwLock::new(HashMap::new()),
            ip_login_attempts: RwLock::new(HashMap::new()),
        }
    }

    fn account(username: &str, password_hash: String) -> Account {
        Account {
            username: username.to_string(),
            password_hash,
            must_change_password: false,
            role: "user".to_string(),
            created_at: String::new(),
            last_online: None,
            storage_limit_bytes: DEFAULT_STORAGE_LIMIT,
            can_use_modelhub: false,
        }
    }

    fn legacy_hash(password: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(password.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    #[test]
    fn unknown_accounts_never_verify() {
        assert!(!verify_login_password("anything", None));
        assert!(!verify_login_password("", None));
        // The dummy hash is a real Argon2id PHC string, so the unknown-user
        // path pays the same verification cost as a real account.
        assert!(PasswordHash::new(dummy_password_hash()).is_ok());
        assert!(!is_legacy_sha256(dummy_password_hash()));
    }

    #[test]
    fn login_verification_accepts_argon2_and_legacy_hashes() {
        let modern = hash_password("correct");
        assert!(verify_login_password("correct", Some(&modern)));
        assert!(!verify_login_password("wrong", Some(&modern)));

        let legacy = legacy_hash("correct");
        assert!(is_legacy_sha256(&legacy));
        assert!(verify_login_password("correct", Some(&legacy)));
        assert!(!verify_login_password("wrong", Some(&legacy)));
    }

    #[test]
    fn login_rejects_unknown_users_and_wrong_passwords_alike() {
        let auth = auth_with(vec![account("alice", hash_password("correct"))]);
        assert_eq!(
            auth.login("nobody", "correct").unwrap_err(),
            "Invalid username or password"
        );
        assert_eq!(
            auth.login("alice", "wrong").unwrap_err(),
            "Invalid username or password"
        );
    }

    #[test]
    fn a_hash_changed_while_argon2_ran_is_not_overwritten() {
        let auth = auth_with(vec![account("alice", "current-hash".to_string())]);
        let swapped = auth
            .swap_password_hash("alice", "stale-hash", "new-hash".to_string(), Some(false))
            .unwrap();
        assert!(!swapped);
        assert_eq!(
            auth.stored_password_hash("ALICE").as_deref(),
            Some("current-hash")
        );
        assert!(!auth
            .swap_password_hash("nobody", "x", "y".to_string(), None)
            .unwrap());
    }

    #[test]
    fn password_change_keeps_only_the_changing_session_of_that_account() {
        let entry = |username: &str| SessionEntry {
            username: username.to_string(),
            created_at: String::new(),
        };
        let keep = hash_session_token("current-token");
        // Another account's session is untouched.
        assert!(keep_session_after_password_change(
            "k1",
            &entry("bob"),
            "alice",
            Some(&keep)
        ));
        // The session making the change survives.
        assert!(keep_session_after_password_change(
            &keep,
            &entry("alice"),
            "Alice",
            Some(&keep)
        ));
        // Every other session of the account is revoked.
        assert!(!keep_session_after_password_change(
            "other",
            &entry("alice"),
            "alice",
            Some(&keep)
        ));
        assert!(!keep_session_after_password_change(
            &keep,
            &entry("alice"),
            "alice",
            None
        ));
    }

    #[test]
    fn online_status_reads_activity_without_holding_the_account_table() {
        let auth = auth_with(vec![account("alice", String::new())]);
        auth.touch_activity("Alice");
        // A writer parked on `db` must not stop the activity snapshot: the
        // status list takes `last_activity` and `db` one at a time.
        let db_guard = auth.db.read().unwrap();
        let online: Vec<String> = auth.last_activity.read().unwrap().keys().cloned().collect();
        drop(db_guard);
        assert_eq!(online, ["alice"]);
        let status = auth.list_users_status(Duration::from_secs(60));
        assert_eq!(status.len(), 1);
        assert!(status[0].2, "alice should be online");
        assert!(!auth.list_users_status(Duration::ZERO)[0].2);
    }
}
