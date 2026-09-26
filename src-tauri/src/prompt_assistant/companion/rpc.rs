//! Bounded stdio JSON-RPC. A dropped operation always terminates its owned child.
use super::error;
use crate::error::AppError;
use serde_json::{json, Value};
use std::{collections::VecDeque, process::Stdio};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout, Command},
};

const MESSAGE_LIMIT: usize = 4 * 1024 * 1024;
const OUTPUT_LIMIT: usize = 256 * 1024;

pub(super) struct Rpc {
    child: Option<Child>,
    process: crate::comfyui::ManagedProcess,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    stderr: tokio::task::JoinHandle<()>,
    next_id: u64,
    pending: VecDeque<Value>,
    pending_size: usize,
    cleanup: Option<(Vec<std::path::PathBuf>, tokio::sync::OwnedMutexGuard<()>)>,
}

impl Drop for Rpc {
    fn drop(&mut self) {
        self.stderr.abort();
        if let Some(mut child) = self.child.take() {
            let process = self.process.clone();
            let cleanup = self.cleanup.take();
            // The official Antigravity launcher owns another process and a
            // runtime. Stop the verified process tree before releasing the lock.
            tokio::spawn(async move {
                let stopped = tokio::task::spawn_blocking(move || process.stop()).await;
                let _ = child.start_kill();
                let _ = child.wait().await;
                if let Some((paths, guard)) = cleanup {
                    if matches!(stopped, Ok(Ok(()))) {
                        for path in paths {
                            // Windows can retain a runtime's file handles briefly
                            // after TerminateProcess returns. Keep the provider
                            // locked while retrying deletion of its transient data.
                            for attempt in 0..10 {
                                match tokio::fs::remove_dir_all(&path).await {
                                    Ok(()) => break,
                                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => break,
                                    Err(_) if attempt < 9 => {
                                        tokio::time::sleep(std::time::Duration::from_millis(50))
                                            .await
                                    }
                                    Err(_) => break,
                                }
                            }
                        }
                    }
                    drop(guard);
                }
            });
        }
    }
}

impl Rpc {
    pub(super) fn spawn(mut command: Command) -> Result<Self, AppError> {
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(windows)]
        command.creation_flags(0x08000000);
        let mut child = command.spawn().map_err(|_| {
            error("Could not start the companion tool. Try signing in again to repair setup.")
        })?;
        let process = crate::comfyui::ManagedProcess::from_child(
            child
                .id()
                .ok_or_else(|| error("Companion process unavailable."))?,
        )?;
        let input = child
            .stdin
            .take()
            .ok_or_else(|| error("Companion input unavailable."))?;
        let output = BufReader::new(
            child
                .stdout
                .take()
                .ok_or_else(|| error("Companion output unavailable."))?,
        );
        let mut stderr = child
            .stderr
            .take()
            .ok_or_else(|| error("Companion diagnostics unavailable."))?;
        // Never log provider diagnostics: they can contain URLs, tokens, audio,
        // or the user's prompt. Drain continuously without retaining them.
        let stderr = tokio::spawn(async move {
            let _ = tokio::io::copy(&mut stderr, &mut tokio::io::sink()).await;
        });
        Ok(Self {
            child: Some(child),
            process,
            input,
            output,
            stderr,
            next_id: 0,
            pending: VecDeque::new(),
            pending_size: 0,
            cleanup: None,
        })
    }

    pub(super) fn clean_on_exit(
        &mut self,
        paths: Vec<std::path::PathBuf>,
        guard: tokio::sync::OwnedMutexGuard<()>,
    ) {
        self.cleanup = Some((paths, guard));
    }

    pub(super) async fn send(&mut self, value: &Value) -> Result<(), AppError> {
        let mut bytes = serde_json::to_vec(value)?;
        if bytes.len() > 16 * 1024 * 1024 {
            return Err(error("Companion request is too large."));
        }
        bytes.push(b'\n');
        self.input
            .write_all(&bytes)
            .await
            .map_err(|_| error("Companion connection closed."))?;
        self.input.flush().await?;
        Ok(())
    }

    async fn read(&mut self) -> Result<Value, AppError> {
        loop {
            let mut line = Vec::new();
            loop {
                let available = self.output.fill_buf().await?;
                if available.is_empty() {
                    return Err(error(
                        "Companion stopped unexpectedly. Check sign-in and retry.",
                    ));
                }
                let end = available.iter().position(|b| *b == b'\n').map(|i| i + 1);
                let count = end.unwrap_or(available.len());
                if line.len() + count > MESSAGE_LIMIT {
                    return Err(error("Companion response exceeds the size limit."));
                }
                line.extend_from_slice(&available[..count]);
                self.output.consume(count);
                if end.is_some() {
                    break;
                }
            }
            let message: Value = serde_json::from_slice(&line)
                .map_err(|_| error("Invalid companion protocol response."))?;
            if message.get("id").is_some() && message.get("method").is_some() {
                // These integrations only generate replies. They never grant
                // agent tools permission to read files, run commands or edit.
                let reply = denied_request(&message);
                self.send(&reply).await?;
                continue;
            }
            return Ok(message);
        }
    }

    pub(super) async fn call(&mut self, method: &str, params: Value) -> Result<Value, AppError> {
        self.next_id += 1;
        let id = self.next_id;
        self.send(&json!({"jsonrpc":"2.0", "id":id, "method":method, "params":params}))
            .await?;
        loop {
            let message = self.read().await?;
            if message["id"] == id {
                if let Some(failure) = message.get("error") {
                    return Err(request_error(method, failure));
                }
                return message
                    .get("result")
                    .cloned()
                    .ok_or_else(|| error("Companion returned no result."));
            }
            self.pending_size += serde_json::to_vec(&message)?.len();
            if self.pending.len() >= 1024 || self.pending_size > MESSAGE_LIMIT {
                return Err(error("Too many companion notifications."));
            }
            self.pending.push_back(message);
        }
    }

    pub(super) async fn next(&mut self) -> Result<Value, AppError> {
        if let Some(message) = self.pending.pop_front() {
            self.pending_size = self
                .pending_size
                .saturating_sub(serde_json::to_vec(&message)?.len());
            return Ok(message);
        }
        self.read().await
    }

    /// ACP streams the answer before it answers the session/prompt request.
    pub(super) async fn gemini_prompt(
        &mut self,
        session: &str,
        prompt: Value,
    ) -> Result<String, AppError> {
        self.next_id += 1;
        let id = self.next_id;
        self.send(&json!({"jsonrpc":"2.0", "id":id, "method":"session/prompt", "params":{"sessionId":session,"prompt":prompt}})).await?;
        let mut answer = String::new();
        loop {
            let message = self.next().await?;
            if message["id"] == id {
                if let Some(failure) = message.get("error") {
                    return Err(request_error("session/prompt", failure));
                }
                if message["result"]["stopReason"] != "end_turn" {
                    return Err(error("Gemini could not finish the reply. Check your account quota and model access. No paid API fallback was used."));
                }
                return finished(answer);
            }
            if message["method"] == "session/update" && message["params"]["sessionId"] == session {
                let update = &message["params"]["update"];
                if update["sessionUpdate"] == "agent_message_chunk"
                    && update["content"]["type"] == "text"
                {
                    append(
                        &mut answer,
                        update["content"]["text"].as_str().unwrap_or(""),
                    )?;
                }
            }
        }
    }

    pub(super) async fn codex_answer(
        &mut self,
        thread: &str,
        turn: &str,
    ) -> Result<String, AppError> {
        let mut answer = String::new();
        loop {
            let message = self.next().await?;
            let params = &message["params"];
            if params["threadId"] != thread {
                continue;
            }
            if message["method"] == "item/completed"
                && params["turnId"] == turn
                && params["item"]["type"] == "agentMessage"
            {
                if params["item"]["phase"].is_null() || params["item"]["phase"] == "final_answer" {
                    append(&mut answer, params["item"]["text"].as_str().unwrap_or(""))?;
                }
            }
            if message["method"] == "turn/completed" && params["turn"]["id"] == turn {
                if params["turn"]["status"] != "completed" {
                    return Err(error("ChatGPT could not finish the reply. Check your subscription quota and model access. No API fallback was used."));
                }
                return finished(answer);
            }
        }
    }
}

/// Classify known failures without exposing provider text, which may echo
/// credentials, account details, URLs or the submitted prompt.
fn request_error(method: &str, failure: &Value) -> AppError {
    let message = failure["message"]
        .as_str()
        .unwrap_or("")
        .to_ascii_lowercase();
    let reason = if message.contains("no longer supported for gemini code assist for individuals") {
        "Google has retired Gemini CLI access for personal accounts, including AI Pro and Ultra. This connection requires Google's replacement Antigravity companion; retrying Google sign-in will not restore Gemini CLI access."
    } else if message.contains("timed out waiting for the authentication flow") {
        "The browser sign-in did not finish in time. Retry sign-in and complete the provider's browser page."
    } else if message.contains("resource_exhausted")
        || message.contains("quota exceeded")
        || message.contains("rate limit")
    {
        "The provider's account quota or rate limit was reached. Wait for the limit to reset before retrying."
    } else if message.contains("invalid_grant") || message.contains("unauthenticated") {
        "The provider could not validate this sign-in. Sign out in Prompt Assistant settings, then sign in again."
    } else if message.contains("permission_denied") || message.contains("access denied") {
        "The provider denied access for this account. Check the account's eligibility and model access."
    } else if method == "authenticate" || method == "account/login/start" {
        "The provider could not complete account sign-in. Check account eligibility and try again."
    } else {
        "The companion rejected the request. Check sign-in, model access and account quota in Prompt Assistant settings."
    };
    let stage = match method {
        "initialize" => "setup",
        "authenticate" | "account/login/start" | "account/read" => "sign-in",
        "session/new" | "thread/start" => "session setup",
        "session/set_model" | "session/set_config_option" | "model/list" => "model selection",
        "session/prompt" | "turn/start" => "generation",
        _ => "request",
    };
    let code = failure["code"]
        .as_i64()
        .map(|n| format!(", code {n}"))
        .unwrap_or_default();
    error(&format!(
        "{reason} ({stage}{code}) No paid API fallback was used."
    ))
}

fn denied_request(message: &Value) -> Value {
    let id = &message["id"];
    match message["method"].as_str().unwrap_or("") {
        "session/request_permission" => {
            json!({"jsonrpc":"2.0","id":id,"result":{"outcome":{"outcome":"cancelled"}}})
        }
        "item/commandExecution/requestApproval" | "item/fileChange/requestApproval" => {
            json!({"id":id,"result":{"decision":"decline"}})
        }
        _ => {
            json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"This client does not provide agent tools."}})
        }
    }
}

fn append(answer: &mut String, text: &str) -> Result<(), AppError> {
    if answer.len() + text.len() > OUTPUT_LIMIT {
        return Err(error("Companion answer exceeds the size limit."));
    }
    answer.push_str(text);
    Ok(())
}
fn finished(answer: String) -> Result<String, AppError> {
    if answer.trim().is_empty() {
        return Err(error("The companion returned no answer."));
    }
    Ok(answer)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retired_gemini_login_explains_the_actual_cause_without_echoing_details() {
        let failure = json!({"code":-32000,"message":"This client is no longer supported for Gemini Code Assist for individuals. To continue using Gemini, please migrate to the Antigravity suite of products: https://example.test/?token=SECRET user@example.test"});
        let message = request_error("authenticate", &failure).to_string();
        assert!(message.contains("Google has retired Gemini CLI"));
        assert!(message.contains("Antigravity"));
        assert!(message.contains("sign-in, code -32000"));
        assert!(!message.contains("SECRET"));
        assert!(!message.contains("example.test"));
    }

    #[test]
    fn companion_failures_keep_stage_and_code_but_never_raw_provider_text() {
        for (method, text, expected) in [
            (
                "authenticate",
                "SENSITIVE_PROVIDER_DETAIL",
                "could not complete account sign-in",
            ),
            (
                "session/prompt",
                "RESOURCE_EXHAUSTED: SENSITIVE_PROVIDER_DETAIL",
                "quota or rate limit",
            ),
            (
                "session/new",
                "invalid_grant: SENSITIVE_PROVIDER_DETAIL",
                "could not validate this sign-in",
            ),
            (
                "session/set_model",
                "PERMISSION_DENIED: SENSITIVE_PROVIDER_DETAIL",
                "denied access",
            ),
        ] {
            let message = request_error(method, &json!({"code":-32000,"message":text})).to_string();
            assert!(message.contains(expected), "{message}");
            assert!(message.contains("code -32000"));
            assert!(!message.contains("SENSITIVE_PROVIDER_DETAIL"));
            assert!(message.contains("No paid API fallback"));
        }
    }
    #[test]
    fn tools_are_never_approved() {
        assert_eq!(
            denied_request(&json!({"id":4,"method":"session/request_permission"}))["result"]
                ["outcome"]["outcome"],
            "cancelled"
        );
        assert_eq!(
            denied_request(&json!({"id":9,"method":"item/commandExecution/requestApproval"}))
                ["result"]["decision"],
            "decline"
        );
        assert!(
            denied_request(&json!({"id":8,"method":"fs/read_text_file"}))
                .get("error")
                .is_some()
        );
    }
    #[test]
    fn every_other_approval_request_is_refused() {
        // The Codex app server (0.155.1) treats an error reply to these as
        // "decline" / "grant nothing", which is what the `untrusted` approval
        // policy relies on for tools the feature flags do not cover.
        for method in [
            "item/fileChange/requestApproval",
            "mcpServer/elicitation/request",
            "item/permissions/requestApproval",
            "item/tool/requestUserInput",
            "item/tool/call",
        ] {
            let reply = denied_request(&json!({"id":1,"method":method}));
            assert_eq!(reply["id"], 1, "{method}");
            let approved = reply["result"]["decision"]
                .as_str()
                .is_some_and(|d| d.starts_with("accept"));
            assert!(!approved, "{method}");
            assert!(
                reply.get("error").is_some() || reply["result"]["decision"] == "decline",
                "{method}"
            );
        }
    }

    #[test]
    fn empty_and_unbounded_answers_fail() {
        assert!(finished("   ".into()).is_err());
        assert!(append(&mut String::new(), &"x".repeat(OUTPUT_LIMIT + 1)).is_err());
    }

    #[tokio::test]
    #[ignore = "Uses MOOSHIE_TEST_NODE for a local mock protocol process; no network or account"]
    async fn protocol_streaming_denials_redaction_and_cleanup() {
        let scratch =
            super::super::Scratch::new(&std::env::temp_dir(), "companion-protocol-test").unwrap();
        let history = scratch.0.join("history");
        std::fs::create_dir(&history).unwrap();
        std::fs::write(history.join("audio.json"), b"synthetic audio").unwrap();
        let mut command =
            Command::new(std::env::var("MOOSHIE_TEST_NODE").expect("MOOSHIE_TEST_NODE"));
        command.args(["-e", r#"
const send = v => process.stdout.write(JSON.stringify(v)+'\n');
let promptId;
require('node:readline').createInterface({input:process.stdin}).on('line', line => {
  const v=JSON.parse(line);
  if(v.method==='setup') { send({method:'irrelevant',params:{}}); send({id:v.id,result:{ok:true}}); }
  if(v.method==='failure') send({id:v.id,error:{code:-32000,message:'SENSITIVE_PROVIDER_DETAIL'}});
  if(v.method==='session/prompt') { promptId=v.id; send({id:'permission',method:'session/request_permission',params:{}}); }
  if(v.id==='permission') {
    if(v.result.outcome.outcome!=='cancelled') process.exit(5);
    const chunk=(session,text)=>send({method:'session/update',params:{sessionId:session,update:{sessionUpdate:'agent_message_chunk',content:{type:'text',text}}}});
    chunk('other-session','IGNORE'); chunk('expected','Warm '); chunk('expected','piano 🎵');
    send({id:promptId,result:{stopReason:'end_turn'}});
  }
});
"#]);
        let lock = std::sync::Arc::new(tokio::sync::Mutex::new(()));
        let mut rpc = Rpc::spawn(command).unwrap();
        rpc.clean_on_exit(vec![history.clone()], lock.clone().lock_owned().await);
        assert_eq!(rpc.call("setup", json!({})).await.unwrap()["ok"], true);
        assert!(!rpc
            .call("failure", json!({}))
            .await
            .unwrap_err()
            .to_string()
            .contains("SENSITIVE_PROVIDER_DETAIL"));
        let answer = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            rpc.gemini_prompt("expected", json!([])),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(answer, "Warm piano 🎵");
        drop(rpc);
        let _cleaned = tokio::time::timeout(std::time::Duration::from_secs(5), lock.lock())
            .await
            .unwrap();
        assert!(
            !history.exists(),
            "Session audio is removed after the child exits"
        );
    }
}
