//! Subscription sign-in through the providers' unmodified official processes.
//! No borrowed OAuth client, token extraction, API-key fallback or shared CLI home.
mod install;
mod rpc;

use crate::{error::AppError, state::AppState};
use base64::{engine::general_purpose::STANDARD, Engine};
use rpc::Rpc;
use serde_json::{json, Value};
use std::{
    future::Future,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, LazyLock,
    },
    time::Duration,
};
use tokio::{process::Command, sync::Mutex};

static CHATGPT_LOCK: LazyLock<Arc<Mutex<()>>> = LazyLock::new(|| Arc::new(Mutex::new(())));
static GEMINI_LOCK: LazyLock<Arc<Mutex<()>>> = LazyLock::new(|| Arc::new(Mutex::new(())));
static CANCEL_LOGIN: AtomicU64 = AtomicU64::new(0);
static CLOSING: AtomicBool = AtomicBool::new(false);

fn error(message: &str) -> AppError {
    AppError::LlmError(message.into())
}
pub fn is_companion(provider: &str) -> bool {
    matches!(provider, "chatgpt" | "gemini-cli")
}
fn lock(provider: &str) -> &'static Arc<Mutex<()>> {
    if provider == "chatgpt" {
        &CHATGPT_LOCK
    } else {
        &GEMINI_LOCK
    }
}

fn private_directory(path: &Path) -> Result<(), AppError> {
    std::fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn root(provider: &str) -> Result<PathBuf, AppError> {
    if !is_companion(provider) {
        return Err(error("Unknown companion provider."));
    }
    let path = crate::config::app_data_dir()
        .ok_or_else(|| error("Cannot find the app data directory."))?
        .join("prompt-assistant")
        .join("companions")
        .join(provider);
    private_directory(&path)?;
    Ok(path)
}

struct Scratch(PathBuf);
impl Scratch {
    fn new(parent: &Path, prefix: &str) -> Result<Self, AppError> {
        let dir = parent.join(format!("{prefix}-{}", uuid::Uuid::new_v4()));
        private_directory(&dir)?;
        Ok(Self(dir))
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A non-secret completion receipt, not evidence that a revoked login is valid.
/// Every inference still authenticates with the official tool before proceeding.
pub fn connected(provider: &str) -> bool {
    is_companion(provider) && account_path(provider).is_ok_and(|p| p.join("connected").is_file())
}

pub fn session_version(provider: &str) -> String {
    account_path(provider)
        .ok()
        .and_then(|p| std::fs::read_to_string(p.join("connected")).ok())
        .unwrap_or_default()
}

pub fn cancel_login() {
    CANCEL_LOGIN.fetch_add(1, Ordering::SeqCst);
}
pub async fn shutdown() {
    CLOSING.store(true, Ordering::SeqCst);
    cancel_login();
    // Dropped requests kill their children, then clean history while holding
    // the lock. Wait for that cleanup before the runtime exits.
    let _ = tokio::time::timeout(Duration::from_secs(10), async {
        let _chatgpt = CHATGPT_LOCK.lock().await;
        let _gemini = GEMINI_LOCK.lock().await;
    })
    .await;
}

async fn bounded<T>(
    seconds: u64,
    login: Option<u64>,
    work: impl Future<Output = Result<T, AppError>>,
) -> Result<T, AppError> {
    tokio::select! {
        result = tokio::time::timeout(Duration::from_secs(seconds), work) =>
            result.map_err(|_| error("Companion request timed out. Please retry."))?,
        _ = async {
            loop {
                if CLOSING.load(Ordering::SeqCst) || login.is_some_and(|n| n != CANCEL_LOGIN.load(Ordering::SeqCst)) { break; }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        } => Err(error("Companion request cancelled.")),
    }
}

fn command(provider: &str, account: &Path, cwd: &Path) -> Result<Command, AppError> {
    let executable = install::executable(provider)?;
    Ok(launch_command(provider, &executable, account, cwd))
}

fn launch_command(provider: &str, executable: &Path, account: &Path, cwd: &Path) -> Command {
    let mut command = Command::new(executable);
    // Do not inherit API keys, endpoint overrides, debug loggers, Node injection,
    // cloud credentials or the caller's agent session configuration.
    command.env_clear();
    for name in [
        "PATH",
        "SystemRoot",
        "WINDIR",
        "COMSPEC",
        "PATHEXT",
        "TEMP",
        "TMP",
        "HOME",
        "USERPROFILE",
        "APPDATA",
        "LOCALAPPDATA",
        "DISPLAY",
        "WAYLAND_DISPLAY",
        "DBUS_SESSION_BUS_ADDRESS",
        "XDG_RUNTIME_DIR",
        "LANG",
        "LC_ALL",
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "NO_PROXY",
        "SSL_CERT_FILE",
        "SSL_CERT_DIR",
    ] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    command.current_dir(cwd);
    if provider == "chatgpt" {
        command.env("CODEX_HOME", account).args([
            "--listen",
            "stdio://",
            "-c",
            "forced_login_method=\"chatgpt\"",
            "-c",
            "cli_auth_credentials_store=\"file\"",
            "-c",
            "features.shell_tool=false",
            "-c",
            "features.unified_exec=false",
            "-c",
            "features.code_mode=false",
            "-c",
            "features.code_mode_host=false",
            "-c",
            "features.apps=false",
            "-c",
            "features.browser_use=false",
            "-c",
            "features.computer_use=false",
            "-c",
            "features.multi_agent=false",
            "-c",
            "features.hooks=false",
            "-c",
            "features.view_image=false",
            "-c",
            "features.skip_host_skill_discovery=true",
            "-c",
            "features.apply_patch_freeform=false",
            "-c",
            "web_search=\"disabled\"",
            "-c",
            "analytics.enabled=false",
        ]);
    } else {
        // GEMINI_HOME isolates ACP auth, settings, hooks and sessions. Keep the
        // actual OS home variables: its browser launcher needs the user's profile.
        command
            .env("GEMINI_HOME", account.join(".gemini"))
            .env("AGY_ACP_FORCE_FILE_STORAGE", "true");
        // Matches Google's published ACP launch arguments on Linux.
        #[cfg(target_os = "linux")]
        command.arg("--uid=");
    }
    command
}

fn configure_account(provider: &str, account: &Path, node: Option<&Path>) -> Result<(), AppError> {
    private_directory(account)?;
    if provider == "gemini-cli" {
        let home = account.join(".gemini");
        let config = home.join("config");
        let acp = home.join("antigravity-acp");
        private_directory(&home)?;
        private_directory(&config)?;
        private_directory(&acp)?;
        std::fs::write(
            acp.join("settings.json"),
            br#"{"auth":{"type":"oauth-personal"}}"#,
        )?;
        std::fs::write(config.join("mcp_config.json"), br#"{"mcpServers":{}}"#)?;
        let node = node
            .ok_or_else(|| error("Node.js is required for Prompt Assistant's tool isolation."))?;
        let hook = config.join("deny-tools.cjs");
        std::fs::write(&hook, include_str!("deny-tools.cjs"))?;
        // The official server parses this command with shlex.split on every OS,
        // then spawns argv directly. No shell and no user content in the command.
        let command = format!("{} {}", shlex_path(node), shlex_path(&hook));
        let hooks = json!({"mooshie-reply-only":{"enabled":true,"PreToolUse":[{
            "matcher":"*","hooks":[{"type":"command","command":command,"timeout":5}]
        }]}});
        std::fs::write(config.join("hooks.json"), serde_json::to_vec(&hooks)?)?;
    }
    Ok(())
}

fn shlex_path(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\"'\"'"))
}

fn history_paths(account: &Path) -> Vec<PathBuf> {
    vec![
        account.join(".gemini/antigravity-acp/conversations"),
        account.join(".gemini/antigravity-acp/brain"),
        account.join(".gemini/artifacts"),
    ]
}

async fn initialize(
    provider: &str,
    account: &Path,
    cwd: &Path,
    guard: tokio::sync::OwnedMutexGuard<()>,
) -> Result<(Rpc, Value), AppError> {
    let node = if provider == "gemini-cli" {
        Some(install::node_path()?)
    } else {
        None
    };
    configure_account(provider, account, node.as_deref())?;
    let mut rpc = Rpc::spawn(command(provider, account, cwd)?)?;
    let mut cleanup = if provider == "gemini-cli" {
        history_paths(account)
    } else {
        Vec::new()
    };
    cleanup.push(cwd.to_path_buf());
    rpc.clean_on_exit(cleanup, guard);
    let params = if provider == "chatgpt" {
        json!({"clientInfo":{"name":"mooshieui","title":"MooshieUI","version":env!("CARGO_PKG_VERSION")}})
    } else {
        json!({"protocolVersion":1,"clientInfo":{"name":"MooshieUI","version":env!("CARGO_PKG_VERSION")},"clientCapabilities":{"fs":{"readTextFile":true,"writeTextFile":true}}})
    };
    let capabilities = rpc.call("initialize", params).await?;
    if provider == "chatgpt" {
        rpc.send(&json!({"method":"initialized","params":{}}))
            .await?;
    }
    Ok((rpc, capabilities))
}

fn account_path(provider: &str) -> Result<PathBuf, AppError> {
    // Never copy the retired Gemini CLI's OAuth credentials into another client.
    Ok(root(provider)?.join(if provider == "gemini-cli" {
        "account-antigravity"
    } else {
        "account"
    }))
}

fn require_connected(provider: &str) -> Result<(), AppError> {
    if !connected(provider) {
        return Err(error(
            "Sign in to this provider in Settings > Prompt Assistant first.",
        ));
    }
    Ok(())
}

fn validate_chatgpt_account(value: &Value) -> Result<(), AppError> {
    if value["account"]["type"] != "chatgpt" {
        return Err(error(
            "A ChatGPT subscription sign-in is required. API keys are not used by this provider.",
        ));
    }
    Ok(())
}

pub async fn connect(state: &AppState, provider: &str) -> Result<(), AppError> {
    if !is_companion(provider) {
        return Err(error("Unknown companion provider."));
    }
    let generation = CANCEL_LOGIN.load(Ordering::SeqCst);
    bounded(600, Some(generation), async {
        let guard = lock(provider).clone().try_lock_owned().map_err(|_| {
            error("This companion is busy. Wait for the current request to finish.")
        })?;
        install::ensure(state, provider).await?;
        let account = account_path(provider)?;
        let cwd = Scratch::new(&root(provider)?, "signin")?;
        let (mut rpc, capabilities) = initialize(provider, &account, &cwd.0, guard).await?;
        if provider == "chatgpt" {
            let existing = rpc
                .call("account/read", json!({"refreshToken":false}))
                .await?;
            if validate_chatgpt_account(&existing).is_err() {
                let login = rpc
                    .call("account/login/start", json!({"type":"chatgpt"}))
                    .await?;
                let auth_url = login["authUrl"]
                    .as_str()
                    .ok_or_else(|| error("ChatGPT returned no sign-in URL."))?;
                if !valid_openai_login_url(auth_url) {
                    return Err(error("Unexpected ChatGPT sign-in destination."));
                }
                open::that(auth_url)
                    .map_err(|_| error("Could not open your browser for ChatGPT sign-in."))?;
                let id = login["loginId"]
                    .as_str()
                    .ok_or_else(|| error("ChatGPT returned no login ID."))?;
                loop {
                    let message = rpc.next().await?;
                    if message["method"] == "account/login/completed"
                        && message["params"]["loginId"] == id
                    {
                        if message["params"]["success"] != true {
                            return Err(error("ChatGPT sign-in was not completed."));
                        }
                        break;
                    }
                }
            }
            validate_chatgpt_account(
                &rpc.call("account/read", json!({"refreshToken":false}))
                    .await?,
            )?;
        } else {
            if !capabilities["authMethods"]
                .as_array()
                .is_some_and(|methods| methods.iter().any(|m| m["id"] == "oauth-personal"))
            {
                return Err(error(
                    "This Antigravity companion does not offer Google sign-in.",
                ));
            }
            rpc.call("authenticate", json!({"methodId":"oauth-personal"}))
                .await?;
        }
        // Only the official process handles or stores the OAuth credentials.
        tokio::fs::write(account.join("connected"), uuid::Uuid::new_v4().to_string()).await?;
        Ok(())
    })
    .await
}

fn valid_openai_login_url(value: &str) -> bool {
    url::Url::parse(value).is_ok_and(|url| {
        url.scheme() == "https"
            && url.host_str() == Some("auth.openai.com")
            && url.username().is_empty()
            && url.password().is_none()
            && url.port().is_none()
    })
}

pub async fn disconnect(provider: &str) -> Result<(), AppError> {
    if !is_companion(provider) {
        return Err(error("Unknown companion provider."));
    }
    let _guard = lock(provider).try_lock().map_err(|_| {
        error("This companion is busy. Cancel sign-in or wait for the request to finish.")
    })?;
    let path = account_path(provider)?;
    // The only deletion target is this provider's app-owned account directory.
    // Official CLI account stores outside MooshieUI are never touched.
    if path.exists() {
        tokio::fs::remove_dir_all(path).await?;
    }
    if provider == "gemini-cli" {
        let retired = root(provider)?.join("account");
        if retired.exists() {
            tokio::fs::remove_dir_all(retired).await?;
        }
    }
    Ok(())
}

pub async fn models(provider: &str) -> Result<Vec<String>, AppError> {
    bounded(120, None, async {
        require_connected(provider)?;
        let guard = lock(provider).clone().lock_owned().await;
        require_connected(provider)?;
        let cwd = Scratch::new(&root(provider)?, "models")?;
        let (mut rpc, _) = initialize(provider, &account_path(provider)?, &cwd.0, guard).await?;
        let result = if provider == "chatgpt" {
            validate_chatgpt_account(
                &rpc.call("account/read", json!({"refreshToken":false}))
                    .await?,
            )?;
            rpc.call("model/list", json!({})).await?["data"].clone()
        } else {
            rpc.call("session/new", json!({"cwd":cwd.0,"mcpServers":[]}))
                .await?["models"]["availableModels"]
                .clone()
        };
        Ok(result
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(|m| {
                        m[if provider == "chatgpt" {
                            "model"
                        } else {
                            "modelId"
                        }]
                        .as_str()
                        .map(str::to_string)
                    })
                    .collect()
            })
            .unwrap_or_default())
    })
    .await
}

pub async fn check_audio() -> Result<(), AppError> {
    bounded(120, None, async {
        require_connected("gemini-cli")?;
        let guard = GEMINI_LOCK.clone().lock_owned().await;
        require_connected("gemini-cli")?;
        let cwd = Scratch::new(&root("gemini-cli")?, "check")?;
        let (_rpc, capabilities) =
            initialize("gemini-cli", &account_path("gemini-cli")?, &cwd.0, guard).await?;
        if capabilities["agentCapabilities"]["promptCapabilities"]["audio"] != true {
            return Err(error(
                "The installed Gemini companion does not support audio input.",
            ));
        }
        Ok(())
    })
    .await
}

/// `thread/start` parameters for a reply-only ChatGPT request.
///
/// `"untrusted"` is the approval policy that asks for everything: every
/// command, every patch and every MCP tool call is sent to this client as an
/// approval request, which `rpc::denied_request` declines. `"never"` is the
/// opposite of what it sounds like here: it means "never ask", so any tool the
/// disabled feature flags miss would simply run.
fn chatgpt_thread_params(cwd: &Path, system: &str, model: &str) -> Value {
    let mut params = json!({"cwd":cwd,"ephemeral":true,"sandbox":"read-only","approvalPolicy":"untrusted","baseInstructions":system,"developerInstructions":"Return only the requested answer. Do not use tools, inspect files or run commands."});
    if !model.trim().is_empty() {
        params["model"] = json!(model);
    }
    params
}

pub async fn chat(
    provider: &str,
    model: &str,
    system: &str,
    user: &str,
    images: &[super::vision::VisionImage],
    audio: Option<&[u8]>,
) -> Result<String, AppError> {
    bounded(240, None, async {
        require_connected(provider)?;
        let guard = lock(provider).clone().lock_owned().await;
        // Sign-out may have run while this request was waiting for the lock.
        require_connected(provider)?;
        let account = account_path(provider)?;
        let cwd = Scratch::new(&root(provider)?, "request")?;
        let (mut rpc, capabilities) = initialize(provider, &account, &cwd.0, guard).await?;
        if provider == "chatgpt" {
            if audio.is_some() { return Err(error("ChatGPT subscription sign-in does not support music audio analysis. Choose Gemini sign-in or an audio-capable API provider.")); }
            validate_chatgpt_account(&rpc.call("account/read", json!({"refreshToken":false})).await?)?;
            let thread = rpc.call("thread/start", chatgpt_thread_params(&cwd.0, system, model)).await?;
            let id = thread["thread"]["id"].as_str().ok_or_else(|| error("ChatGPT returned no thread."))?;
            let mut input = vec![json!({"type":"text","text":user,"text_elements":[]})];
            for image in images { input.push(json!({"type":"image","url":format!("data:{};base64,{}",image.media_type,image.base64)})); }
            let turn = rpc.call("turn/start", json!({"threadId":id,"input":input})).await?;
            let turn_id = turn["turn"]["id"].as_str().ok_or_else(|| error("ChatGPT returned no turn."))?;
            rpc.codex_answer(id, turn_id).await
        } else {
            if audio.is_some() && capabilities["agentCapabilities"]["promptCapabilities"]["audio"] != true {
                return Err(error("This Gemini companion cannot accept audio."));
            }
            if !images.is_empty() && capabilities["agentCapabilities"]["promptCapabilities"]["image"] != true {
                return Err(error("This Gemini companion cannot accept images."));
            }
            let session = rpc.call("session/new", json!({"cwd":cwd.0,"mcpServers":[]})).await?;
            let id = session["sessionId"].as_str().ok_or_else(|| error("Gemini returned no session."))?;
            if !model.trim().is_empty() { rpc.call("session/set_config_option", json!({"sessionId":id,"configId":"model","value":model})).await?; }
            let mut prompt = vec![json!({"type":"text","text":format!("{system}\n\n{user}\n\nReturn only the requested answer. Do not use tools.")})];
            for image in images { prompt.push(json!({"type":"image","mimeType":image.media_type,"data":image.base64})); }
            if let Some(audio) = audio { prompt.push(json!({"type":"audio","mimeType":"audio/mpeg","data":STANDARD.encode(audio)})); }
            rpc.gemini_prompt(id, json!(prompt)).await
        }
    }).await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn antigravity_uses_private_google_auth_and_a_deny_all_tool_hook() {
        let scratch = Scratch::new(&std::env::temp_dir(), "companion-config-test").unwrap();
        let account = scratch.0.join("account");
        let node = Path::new("/test folder/it's node");
        configure_account("gemini-cli", &account, Some(node)).unwrap();
        let home = account.join(".gemini");
        let settings: Value = serde_json::from_slice(
            &std::fs::read(home.join("antigravity-acp/settings.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(settings, json!({"auth":{"type":"oauth-personal"}}));
        let hooks: Value =
            serde_json::from_slice(&std::fs::read(home.join("config/hooks.json")).unwrap())
                .unwrap();
        let rule = &hooks["mooshie-reply-only"];
        assert_eq!(rule["enabled"], true);
        assert_eq!(rule["PreToolUse"][0]["matcher"], "*");
        assert!(rule["PreToolUse"][0]["hooks"][0]["command"]
            .as_str()
            .unwrap()
            .starts_with("'/test folder/it'\"'\"'s node' "));
        assert!(configure_account("gemini-cli", &account, None).is_err());
        for path in history_paths(&account) {
            assert!(path.starts_with(&home));
            assert!(!path.ends_with("antigravity-acp"));
        }
        let command = launch_command(
            "gemini-cli",
            Path::new("agy_acp_server"),
            &account,
            &scratch.0,
        );
        let vars: std::collections::HashMap<_, _> = command.as_std().get_envs().collect();
        assert_eq!(
            vars[std::ffi::OsStr::new("GEMINI_HOME")],
            Some(home.as_os_str())
        );
        assert_eq!(
            vars[std::ffi::OsStr::new("AGY_ACP_FORCE_FILE_STORAGE")],
            Some(std::ffi::OsStr::new("true"))
        );
        for key in ["HOME", "USERPROFILE"] {
            assert_eq!(
                vars.get(std::ffi::OsStr::new(key)).copied().flatten(),
                std::env::var_os(key).as_deref()
            );
        }
        assert!(!vars.contains_key(std::ffi::OsStr::new("GEMINI_API_KEY")));
    }
    #[test]
    fn only_managed_chatgpt_accounts_are_accepted() {
        assert!(validate_chatgpt_account(&json!({"account":{"type":"apiKey"}})).is_err());
        assert!(validate_chatgpt_account(&json!({"account":null})).is_err());
        assert!(validate_chatgpt_account(&json!({"account":{"type":"chatgpt"}})).is_ok());
    }
    #[test]
    fn chatgpt_threads_ask_for_every_tool_so_the_client_can_decline_it() {
        let params = chatgpt_thread_params(Path::new("/tmp/request"), "sys", "");
        assert_eq!(params["approvalPolicy"], "untrusted");
        assert_eq!(params["sandbox"], "read-only");
        assert!(params.get("model").is_none());
        assert_eq!(
            chatgpt_thread_params(Path::new("/tmp/request"), "sys", "gpt-x")["model"],
            "gpt-x"
        );
    }

    #[test]
    fn login_urls_are_checked_before_opening_browser() {
        assert!(valid_openai_login_url(
            "https://auth.openai.com/oauth/authorize?state=test"
        ));
        for url in [
            "https://auth.openai.com.evil.test/",
            "http://auth.openai.com/",
            "https://user@auth.openai.com/",
            "file:///tmp/test",
            "https://auth.openai.com:9000/",
        ] {
            assert!(!valid_openai_login_url(url));
        }
    }

    #[tokio::test]
    #[ignore = "Requires an explicitly authorized app Google account with no active requests; authenticates and creates a session, but makes no inference request"]
    async fn signed_in_google_session_cleanup() {
        assert_eq!(
            std::env::var("MOOSHIE_TEST_LIVE_GEMINI").as_deref(),
            Ok("1")
        );
        let account = account_path("gemini-cli").unwrap();
        let cwd = Scratch::new(&root("gemini-cli").unwrap(), "test-session").unwrap();
        let process_lock = Arc::new(Mutex::new(()));
        let guard = process_lock.clone().lock_owned().await;
        let (mut rpc, _) = initialize("gemini-cli", &account, &cwd.0, guard)
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(120), async {
            rpc.call("authenticate", json!({"methodId":"oauth-personal"}))
                .await
                .unwrap();
            let session = rpc
                .call("session/new", json!({"cwd":cwd.0,"mcpServers":[]}))
                .await
                .unwrap();
            assert!(!session["models"]["availableModels"]
                .as_array()
                .unwrap()
                .is_empty());
            assert!(history_paths(&account)[0].is_dir());
        })
        .await
        .expect("Google account verification timed out");
        drop(rpc);
        let _finished = tokio::time::timeout(Duration::from_secs(15), process_lock.lock())
            .await
            .unwrap();
        for path in history_paths(&account) {
            assert!(
                !path.exists(),
                "Transient Google session data should be removed"
            );
        }
        assert!(!cwd.0.exists());
    }

    #[tokio::test]
    #[ignore = "Uses official companion binaries in an isolated temporary home; no sign-in or inference"]
    async fn official_protocol_smoke() {
        let parent = std::env::temp_dir();
        let scratch = Scratch::new(&parent, "mooshie-companion-test").unwrap();
        for (provider, variable) in [
            ("chatgpt", "MOOSHIE_TEST_CODEX_SERVER"),
            ("gemini-cli", "MOOSHIE_TEST_ANTIGRAVITY_SERVER"),
        ] {
            let executable = PathBuf::from(std::env::var(variable).expect(variable));
            let node = std::env::var_os("MOOSHIE_TEST_NODE").map(PathBuf::from);
            let account = scratch.0.join(provider);
            configure_account(provider, &account, node.as_deref()).unwrap();
            let cwd = Scratch::new(&scratch.0, "workspace").unwrap();
            let mut command = launch_command(provider, &executable, &account, &cwd.0);
            // Tests must neither inherit real accounts nor open a login page.
            command
                .env("HOME", &account)
                .env("USERPROFILE", &account)
                .env("NO_BROWSER", "true");
            let mut rpc = Rpc::spawn(command).unwrap();
            let process_lock = Arc::new(Mutex::new(()));
            let mut cleanup = history_paths(&account);
            cleanup.push(cwd.0.clone());
            rpc.clean_on_exit(cleanup, process_lock.clone().lock_owned().await);
            tokio::time::timeout(Duration::from_secs(120), async {
                if provider == "chatgpt" {
                    rpc.call("initialize", json!({"clientInfo":{"name":"mooshieui_test","version":"1"}})).await.unwrap();
                    rpc.send(&json!({"method":"initialized","params":{}})).await.unwrap();
                    let account = rpc.call("account/read", json!({"refreshToken":false})).await.unwrap();
                    assert!(account["account"].is_null(), "Test process must not access an existing account");
                } else {
                    let result = rpc.call("initialize", json!({"protocolVersion":1,"clientCapabilities":{"fs":{"readTextFile":true,"writeTextFile":true}}})).await.unwrap();
                    assert_eq!(result["agentCapabilities"]["promptCapabilities"]["audio"], true);
                    assert!(result["authMethods"].as_array().unwrap().iter().any(|m| m["id"] == "oauth-personal"));
                }
            }).await.expect("Companion initialization timed out");
            drop(rpc);
            let _finished = tokio::time::timeout(Duration::from_secs(15), process_lock.lock())
                .await
                .expect("Companion process cleanup timed out");
            assert!(
                !cwd.0.exists(),
                "Cleanup waits for the real companion process tree before removing the workspace"
            );
        }
    }
}
