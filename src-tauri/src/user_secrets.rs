//! Per-account third-party credentials, encrypted at rest.
//!
//! Sibling to [`crate::user_prefs`]: both live under `{app_data}/users/`, but
//! this module suffixes each directory with a hash of the raw username (see
//! `sanitize_username` below) to keep lookalike accounts such as `bob` and
//! `b.o.b` from colliding, and the payload here is a credential that belongs
//! to the account holder rather than to the instance owner.
//!
//! This exists because a hosted MooshieUI serves several people from one
//! process. NovelAI's terms of service do not allow several humans to share
//! one key, so each account brings its own and the server never mixes them.
//!
//! Data stored at `{app_data_dir}/users/{sanitized}-{8hex}/secrets.json`,
//! where `{sanitized}-{8hex}` is `sanitize_username`'s output: the username
//! filtered to alphanumerics/`_`/`-`, suffixed with the first 8 hex
//! characters of the SHA-256 of the lowercased raw username.
//!
//! **Threat model.** The master key lives on the same host as the ciphertext,
//! so this is obfuscation-grade against a full host compromise, not a vault.
//! What it does buy: a leaked backup, a PVC snapshot, or a stray `cat` of the
//! data directory does not hand over anybody's NovelAI token. Point
//! `MOOSHIEUI_SECRET_KEY` at a Kubernetes Secret to separate the two.

use std::io::Write;
use std::path::PathBuf;
use std::sync::OnceLock;

use base64::Engine;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::config;

/// One encrypted value: a per-write nonce and the ciphertext, both base64.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SealedValue {
    pub nonce: String,
    pub ct: String,
}

/// On-disk shape of `secrets.json`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SecretsFile {
    /// Schema version, so a later credential type can migrate this file
    /// rather than replace it.
    #[serde(default)]
    pub version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub novelai_api_key: Option<SealedValue>,
}

const NONCE_LEN: usize = 24;
const KEY_LEN: usize = 32;

// --- crypto -------------------------------------------------------------

/// Encrypt `plaintext` under `key`, binding it to `username` as additional
/// authenticated data.
///
/// The AAD is what stops a `secrets.json` copied into another account's
/// directory from decrypting: the tag covers the username, so the move is
/// detected as tampering rather than silently honoured.
fn seal_with_nonce(
    key: &[u8; KEY_LEN],
    username: &str,
    plaintext: &str,
    nonce: &[u8; NONCE_LEN],
) -> Result<Vec<u8>, String> {
    let cipher = XChaCha20Poly1305::new(Key::from_slice(key));
    cipher
        .encrypt(
            XNonce::from_slice(nonce),
            Payload {
                msg: plaintext.as_bytes(),
                aad: username.as_bytes(),
            },
        )
        .map_err(|_| "Failed to encrypt secret".to_string())
}

/// Decrypt a sealed value. Returns `None` for every failure mode -- wrong key,
/// wrong username, truncated nonce, flipped bit, non-UTF-8 plaintext -- because
/// the caller's only recovery is to ask the user for the key again, and none of
/// these cases should ever panic or surface as a hard error.
fn open(key: &[u8; KEY_LEN], username: &str, nonce: &[u8], ct: &[u8]) -> Option<String> {
    if nonce.len() != NONCE_LEN {
        return None;
    }
    let cipher = XChaCha20Poly1305::new(Key::from_slice(key));
    let plain = cipher
        .decrypt(
            XNonce::from_slice(nonce),
            Payload {
                msg: ct,
                aad: username.as_bytes(),
            },
        )
        .ok()?;
    String::from_utf8(plain).ok()
}

/// Fresh nonce bytes. Matches the `rand` 0.10 idiom already used by
/// `auth::generate_token`.
fn random_bytes<const N: usize>() -> [u8; N] {
    use rand::RngExt;
    let mut rng = rand::rng();
    let mut out = [0u8; N];
    for byte in out.iter_mut() {
        *byte = rng.random::<u8>();
    }
    out
}

// --- master key ---------------------------------------------------------

static MASTER_KEY: OnceLock<Option<[u8; KEY_LEN]>> = OnceLock::new();

/// Resolve the master key: `MOOSHIEUI_SECRET_KEY` first, then a file on disk.
///
/// The env var wins so a hosted deployment can keep the key in a Kubernetes
/// Secret instead of on the same volume as the ciphertext, where one snapshot
/// would otherwise carry both halves.
fn master_key() -> Option<[u8; KEY_LEN]> {
    *MASTER_KEY.get_or_init(|| {
        if let Ok(encoded) = std::env::var("MOOSHIEUI_SECRET_KEY") {
            let trimmed = encoded.trim();
            if !trimmed.is_empty() {
                match base64::engine::general_purpose::STANDARD.decode(trimmed) {
                    Ok(bytes) if bytes.len() == KEY_LEN => {
                        let mut key = [0u8; KEY_LEN];
                        key.copy_from_slice(&bytes);
                        log::info!("user_secrets: master key loaded from MOOSHIEUI_SECRET_KEY");
                        return Some(key);
                    }
                    _ => {
                        // Loud, because the fallback silently changes which key
                        // decrypts existing data. Better to say so once at
                        // startup than to report every stored key as missing.
                        log::error!(
                            "MOOSHIEUI_SECRET_KEY is set but is not base64 of {KEY_LEN} bytes; \
                             falling back to the on-disk key file"
                        );
                    }
                }
            }
        }
        load_or_create_key_file()
    })
}

fn key_file_path() -> Option<PathBuf> {
    config::app_data_dir().map(|d| d.join("secrets.key"))
}

fn load_or_create_key_file() -> Option<[u8; KEY_LEN]> {
    let path = key_file_path()?;
    if let Ok(bytes) = std::fs::read(&path) {
        if bytes.len() == KEY_LEN {
            let mut key = [0u8; KEY_LEN];
            key.copy_from_slice(&bytes);
            return Some(key);
        }
        log::error!(
            "user_secrets: {:?} is {} bytes, expected {KEY_LEN}; refusing to overwrite it",
            path,
            bytes.len()
        );
        return None;
    }

    let key: [u8; KEY_LEN] = random_bytes();
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            log::error!("user_secrets: cannot create {parent:?}: {e}");
            return None;
        }
    }
    // create_new so two processes racing on first start cannot each write a
    // key and leave the loser's stored secrets undecryptable.
    match secure_create_options()
        .write(true)
        .create_new(true)
        .open(&path)
    {
        Ok(mut f) => {
            if let Err(e) = f.write_all(&key) {
                log::error!("user_secrets: cannot write {path:?}: {e}");
                return None;
            }
            restrict_permissions(&path);
            log::info!("user_secrets: generated a new master key at {path:?}");
            Some(key)
        }
        Err(_) => {
            // Lost the race, or it appeared between the read and the create.
            let bytes = std::fs::read(&path).ok()?;
            if bytes.len() != KEY_LEN {
                return None;
            }
            let mut key = [0u8; KEY_LEN];
            key.copy_from_slice(&bytes);
            Some(key)
        }
    }
}

#[cfg(unix)]
fn restrict_permissions(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Err(e) = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)) {
        log::warn!("user_secrets: cannot chmod {path:?}: {e}");
    }
}

/// Windows has no mode bits to set; the file inherits the data directory's ACL.
#[cfg(not(unix))]
fn restrict_permissions(_path: &std::path::Path) {}

/// `OpenOptions` that create a new file at mode 0600 on Unix, so there is no
/// window between file creation and a later `chmod` in which the file is
/// world-readable. Windows has no mode bits; the file inherits the data
/// directory's ACL, same as `restrict_permissions` above.
#[cfg(unix)]
fn secure_create_options() -> std::fs::OpenOptions {
    use std::os::unix::fs::OpenOptionsExt;
    let mut opts = std::fs::OpenOptions::new();
    opts.mode(0o600);
    opts
}

#[cfg(not(unix))]
fn secure_create_options() -> std::fs::OpenOptions {
    std::fs::OpenOptions::new()
}

/// A sibling temp path derived from `path`'s own file name, plus this
/// process's id and a random UUID, so a concurrent save (this account
/// saving twice at once, or another account entirely) cannot collide with it.
fn tmp_path_for(path: &std::path::Path) -> PathBuf {
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "secrets.json".to_string());
    path.with_file_name(format!(
        ".{file_name}.{}.{}.tmp",
        std::process::id(),
        uuid::Uuid::new_v4()
    ))
}

// --- storage ------------------------------------------------------------

/// Strip everything that could escape the users directory, then disambiguate.
///
/// The character filter (alphanumerics, `_` and `-` only) is the same
/// traversal guard [`crate::user_prefs`] uses: `.` and `/` are dropped
/// outright, so `../../etc/passwd` collapses to the harmless `etcpasswd`.
///
/// Unlike `user_prefs`, that filtered string is not used as the path segment
/// on its own. Account creation (`crate::auth::create_account_ex`) only
/// lowercases a username; it does not restrict its character set. That means
/// two distinct accounts, e.g. `bob` and `b.o.b`, both filter down to `bob`
/// and would otherwise collide on the same `secrets.json`, silently
/// clobbering each other's stored key. To keep lookalikes apart, the filtered
/// string is suffixed with `-` and the first 8 hex characters of the SHA-256
/// of the *lowercased raw* username (matching how `auth` stores it, not the
/// filtered form), giving each raw username its own directory.
fn sanitize_username(username: &str) -> Option<String> {
    let safe: String = username
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
        .collect();
    if safe.is_empty() {
        return None;
    }
    let digest = Sha256::digest(username.to_ascii_lowercase().as_bytes());
    let suffix = hex::encode(digest);
    Some(format!("{safe}-{}", &suffix[..8]))
}

/// The storage layer takes its root directory and master key as parameters
/// rather than reaching for `app_data_dir()` and `master_key()` itself. That
/// keeps it testable against a scratch directory: no environment variables, no
/// `OnceLock` to poison, and no chance of a test writing into the developer's
/// real data directory. The public wrappers below supply the real values.
fn secrets_path_in(root: &std::path::Path, username: &str) -> Option<PathBuf> {
    let safe = sanitize_username(username)?;
    Some(root.join("users").join(safe).join("secrets.json"))
}

fn load_file_in(root: &std::path::Path, username: &str) -> Option<SecretsFile> {
    let path = secrets_path_in(root, username)?;
    let bytes = std::fs::read(&path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn load_nai_key_in(
    root: &std::path::Path,
    master: &[u8; KEY_LEN],
    username: &str,
) -> Option<String> {
    let sealed = load_file_in(root, username)?.novelai_api_key?;
    let nonce = base64::engine::general_purpose::STANDARD
        .decode(&sealed.nonce)
        .ok()?;
    let ct = base64::engine::general_purpose::STANDARD
        .decode(&sealed.ct)
        .ok()?;
    open(master, username, &nonce, &ct)
}

fn save_nai_key_in(
    root: &std::path::Path,
    master: &[u8; KEY_LEN],
    username: &str,
    api_key: Option<&str>,
) -> Result<(), String> {
    let path = secrets_path_in(root, username).ok_or_else(|| "Invalid username".to_string())?;
    let mut file = load_file_in(root, username).unwrap_or_default();
    file.version = 1;

    // An all-whitespace key clears rather than stores: the UI's "clear" action
    // is an empty text field, and a key of spaces would otherwise be saved and
    // then rejected by NovelAiClient on every generation.
    match api_key.map(str::trim).filter(|s| !s.is_empty()) {
        Some(plaintext) => {
            let nonce: [u8; NONCE_LEN] = random_bytes();
            let ct = seal_with_nonce(master, username, plaintext, &nonce)?;
            file.novelai_api_key = Some(SealedValue {
                nonce: base64::engine::general_purpose::STANDARD.encode(nonce),
                ct: base64::engine::general_purpose::STANDARD.encode(&ct),
            });
        }
        None => file.novelai_api_key = None,
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let bytes = serde_json::to_vec_pretty(&file).map_err(|e| e.to_string())?;

    // Write to a sibling temp file, fsync it, then rename it over `path`,
    // rather than std::fs::write()'s truncate-in-place. A crash or a full
    // disk mid-write would otherwise leave a truncated secrets.json, which
    // load_file_in(...).unwrap_or_default() above would silently treat as an
    // empty file, so the next save would replace the user's stored key
    // instead of preserving it. Same-directory rename is atomic on both Unix
    // and Windows (Rust's fs::rename uses MoveFileEx with
    // MOVEFILE_REPLACE_EXISTING there). The temp file is created at mode 0600
    // so there is no unprotected window at all: the rename carries that mode
    // across.
    let tmp_path = tmp_path_for(&path);
    let write_result: Result<(), String> = (|| {
        let mut f = secure_create_options()
            .write(true)
            .create_new(true)
            .open(&tmp_path)
            .map_err(|e| e.to_string())?;
        f.write_all(&bytes).map_err(|e| e.to_string())?;
        f.sync_all().map_err(|e| e.to_string())?;
        drop(f);
        std::fs::rename(&tmp_path, &path).map_err(|e| e.to_string())?;
        Ok(())
    })();
    if write_result.is_err() {
        // Best-effort: a crash before this point already left nothing behind
        // to clean up, and a failure here is not worth surfacing over the
        // original error.
        let _ = std::fs::remove_file(&tmp_path);
    }
    write_result?;
    restrict_permissions(&path);
    Ok(())
}

fn delete_all_in(root: &std::path::Path, username: &str) -> Result<(), String> {
    let path = match secrets_path_in(root, username) {
        Some(p) => p,
        None => return Ok(()),
    };
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

// --- public API ---------------------------------------------------------

/// Read this account's NovelAI key, or `None` if it has never set one.
///
/// A missing file, an unreadable file, a corrupt blob and a master key that no
/// longer decrypts all answer the same way, because the recovery is the same:
/// prompt for the key again.
pub fn load_nai_key(username: &str) -> Option<String> {
    let root = config::app_data_dir()?;
    let master = master_key()?;
    load_nai_key_in(&root, &master, username)
}

/// Store or clear this account's NovelAI key. `None` (or an all-whitespace
/// string) clears it.
pub fn save_nai_key(username: &str, api_key: Option<&str>) -> Result<(), String> {
    let root = config::app_data_dir()
        .ok_or_else(|| "Cannot locate the server data directory.".to_string())?;
    let master = master_key().ok_or_else(|| {
        "Cannot access the secret store. Check the server data directory.".to_string()
    })?;
    save_nai_key_in(&root, &master, username, api_key)
}

/// Whether this account has a usable NovelAI key.
///
/// Decrypts rather than checking for the field's presence, so a blob left
/// undecryptable by a rotated master key reports honestly as "no key" instead
/// of enabling a UI that would then fail on every generation.
pub fn has_nai_key(username: &str) -> bool {
    load_nai_key(username).is_some()
}

/// Remove every stored secret for an account. Called when the account is
/// deleted. A missing file is success.
pub fn delete_all(username: &str) -> Result<(), String> {
    let root = match config::app_data_dir() {
        Some(r) => r,
        None => return Ok(()),
    };
    delete_all_in(&root, username)
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: [u8; 32] = [7u8; 32];
    const NONCE: [u8; 24] = [3u8; 24];

    #[test]
    fn seal_and_open_round_trip() {
        let ct = seal_with_nonce(&KEY, "alice", "pst-secret-token", &NONCE).unwrap();
        let out = open(&KEY, "alice", &NONCE, &ct);
        assert_eq!(out.as_deref(), Some("pst-secret-token"));
    }

    #[test]
    fn a_blob_sealed_for_one_user_does_not_open_as_another() {
        // The username is the AAD, so a secrets.json copied into another
        // user's directory must fail authentication rather than decrypt.
        let ct = seal_with_nonce(&KEY, "alice", "pst-secret-token", &NONCE).unwrap();
        assert!(open(&KEY, "bob", &NONCE, &ct).is_none());
    }

    #[test]
    fn a_wrong_master_key_does_not_open() {
        let ct = seal_with_nonce(&KEY, "alice", "pst-secret-token", &NONCE).unwrap();
        assert!(open(&[9u8; 32], "alice", &NONCE, &ct).is_none());
    }

    #[test]
    fn a_corrupt_ciphertext_returns_none_instead_of_panicking() {
        let mut ct = seal_with_nonce(&KEY, "alice", "pst-secret-token", &NONCE).unwrap();
        let last = ct.len() - 1;
        ct[last] ^= 0xff;
        assert!(open(&KEY, "alice", &NONCE, &ct).is_none());
    }

    #[test]
    fn a_short_nonce_returns_none_instead_of_panicking() {
        let ct = seal_with_nonce(&KEY, "alice", "pst-secret-token", &NONCE).unwrap();
        assert!(open(&KEY, "alice", &[3u8; 5], &ct).is_none());
    }

    #[test]
    fn sanitize_strips_path_traversal() {
        // Dots and slashes are filtered out entirely, so a crafted username
        // collapses to a harmless directory name with a hash suffix; none of
        // that can reintroduce a separator, a dot, or a drive-letter colon.
        let result = sanitize_username("../../etc/passwd").unwrap();
        assert!(result.starts_with("etcpasswd-"));
        assert!(!result.contains('/'));
        assert!(!result.contains('\\'));
        assert!(!result.contains('.'));
        assert!(!result.contains(':'));

        // A username that filters to nothing is still rejected outright,
        // not stored under a bare hash.
        assert_eq!(sanitize_username("..").as_deref(), None);
        assert_eq!(sanitize_username("../").as_deref(), None);
        assert_eq!(sanitize_username("").as_deref(), None);
    }

    #[test]
    fn sanitize_keeps_ordinary_and_reserved_names() {
        for name in ["alice", "_admin", "bob-2"] {
            let result = sanitize_username(name).unwrap();
            assert!(result.starts_with(&format!("{name}-")));
            // 8 hex characters after the filtered name and its separator.
            let suffix = &result[name.len() + 1..];
            assert_eq!(suffix.len(), 8);
            assert!(suffix.chars().all(|c| c.is_ascii_hexdigit()));
            assert!(!result.contains('/'));
            assert!(!result.contains('\\'));
            assert!(!result.contains(':'));
        }
    }

    /// A unique scratch directory. Deliberately does NOT use `app_data_dir()`
    /// or any env var, so these tests stay parallel-safe and never touch the
    /// developer's real data directory.
    fn scratch(tag: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("mooshie-secrets-{tag}-{nanos}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_key_survives_a_save_and_load() {
        let root = scratch("save-load");
        save_nai_key_in(&root, &KEY, "alice", Some("pst-secret-token")).unwrap();
        assert_eq!(
            load_nai_key_in(&root, &KEY, "alice").as_deref(),
            Some("pst-secret-token")
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn saving_none_clears_a_stored_key() {
        let root = scratch("clear");
        save_nai_key_in(&root, &KEY, "alice", Some("pst-secret-token")).unwrap();
        save_nai_key_in(&root, &KEY, "alice", None).unwrap();
        assert!(load_nai_key_in(&root, &KEY, "alice").is_none());
        // The file itself remains, so a later credential type can be added
        // without recreating it; only the NovelAI entry is gone.
        assert!(secrets_path_in(&root, "alice").unwrap().exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_all_whitespace_key_clears_rather_than_stores() {
        let root = scratch("blank");
        save_nai_key_in(&root, &KEY, "alice", Some("pst-secret-token")).unwrap();
        save_nai_key_in(&root, &KEY, "alice", Some("   ")).unwrap();
        assert!(load_nai_key_in(&root, &KEY, "alice").is_none());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_user_who_never_saved_anything_has_no_key() {
        let root = scratch("missing");
        assert!(load_nai_key_in(&root, &KEY, "nobody").is_none());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn deleting_removes_the_file_and_a_second_delete_succeeds() {
        let root = scratch("delete");
        save_nai_key_in(&root, &KEY, "alice", Some("pst-secret-token")).unwrap();
        let path = secrets_path_in(&root, "alice").unwrap();
        assert!(path.exists());
        delete_all_in(&root, "alice").unwrap();
        assert!(!path.exists());
        // Deleting an account that never had a key is not an error.
        delete_all_in(&root, "alice").unwrap();
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_stored_file_round_trips_through_serde() {
        let file = SecretsFile {
            version: 1,
            novelai_api_key: Some(SealedValue {
                nonce: "AAAA".into(),
                ct: "BBBB".into(),
            }),
        };
        let json = serde_json::to_string(&file).unwrap();
        let back: SecretsFile = serde_json::from_str(&json).unwrap();
        assert_eq!(back.version, 1);
        assert_eq!(back.novelai_api_key.unwrap().nonce, "AAAA");
    }

    #[test]
    fn two_usernames_that_filter_alike_get_different_paths() {
        let root = scratch("collide-paths");
        let bob = secrets_path_in(&root, "bob").unwrap();
        let b_o_b = secrets_path_in(&root, "b.o.b").unwrap();
        assert_ne!(bob, b_o_b);
        assert_eq!(secrets_path_in(&root, "bob").unwrap(), bob);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_key_stored_for_one_username_is_invisible_to_a_lookalike() {
        let root = scratch("collide-keys");
        save_nai_key_in(&root, &KEY, "bob", Some("bobs-secret-token")).unwrap();
        // A lookalike account cannot decrypt bob's stored key...
        assert!(load_nai_key_in(&root, &KEY, "b.o.b").is_none());
        // ...and, crucially, saving its own key must not clobber bob's: if
        // both usernames sanitized to the same path, this save would
        // overwrite the file bob's key lives in.
        save_nai_key_in(&root, &KEY, "b.o.b", Some("lookalikes-own-token")).unwrap();
        assert_eq!(
            load_nai_key_in(&root, &KEY, "bob").as_deref(),
            Some("bobs-secret-token")
        );
        assert_eq!(
            load_nai_key_in(&root, &KEY, "b.o.b").as_deref(),
            Some("lookalikes-own-token")
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
