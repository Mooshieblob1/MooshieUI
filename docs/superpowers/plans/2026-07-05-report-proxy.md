# Report Proxy (Sub-project B) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stand up a self-hosted Rust+axum service on the NUC that receives in-app error reports and creates GitHub issues server-side (so the GitHub credential never ships in the app), with optional LLM summaries from the already-running `snowywood-llm`, fronted by a Cloudflare Tunnel.

**Architecture:** A standalone Rust crate `report-proxy/` (NOT part of the `src-tauri` workspace) exposes `POST /report` and `GET /health`. The handler validates the request, computes a dedup signature, optionally enriches the issue body with a best-effort LLM summary (hard timeout, never blocks), then creates or comments on a GitHub issue. It is packaged as a Docker container that joins the existing `blob_default` network (to reach `snowywood-llm`) and sits behind a `cloudflared` sidecar. Two small changes in the main app activate the proxy and add a fallback.

**Tech Stack:** Rust (edition 2021), axum 0.7, tokio 1, reqwest (rustls-tls), serde/serde_json, sha2. Docker Compose. Node (build-time catalog extraction script). App-side: Rust (`src-tauri`) + TypeScript (Svelte 5 app).

## Global Constraints

- The proxy crate lives at repo root `report-proxy/` and is a **standalone** Cargo project, not added to any workspace.
- All authored text in issues/comments/docs posted in the user's voice: **plain ASCII, no em dashes, no curly quotes, no emojis**.
- Git commits in this repo on Windows MUST be prefixed with `git -c core.hooksPath=/dev/null` (the bash pre-commit hook hangs in PowerShell). **Never** add `Co-Authored-By` trailers.
- The app -> proxy contract is FIXED (defined by Sub-project A). Request headers: `Content-Type: application/json`, `X-Mooshie-App: 1`. Body is `ReportPayload` (camelCase JSON). Success response: `200` with JSON `{ "issueUrl": "..." }`.
- `ReportPayload` fields (camelCase on the wire): `errorCode`, `rawMessage`, `appVersion`, `os`, `arch`, `mode`, `timestamp`, `userNote?`, `logsTail?`.
- LLM is best-effort only. Issue creation must NEVER be blocked or failed by an LLM error/timeout.
- GitHub repo: `Mooshieblob1/MooshieUI`. Issue labels: `bug,in-app-report`. Issue title format: `[in-app] <errorCode>: <first 80 chars of rawMessage>`.
- LLM endpoint default: `http://snowywood-llm:8080` (OpenAI-compatible `/v1/chat/completions`). Model is Qwen3 (a thinking model) with `n_ctx=3072`, CPU-only, shared with a Discord bot. Disable thinking with a `/no_think` prompt prefix; read `message.content`, never `reasoning_content`.
- Secrets (`GITHUB_TOKEN`, `CLOUDFLARE_TUNNEL_TOKEN`) live only in `/home/blob/report-proxy/.env` on the NUC (already created, perms 600). Never commit them; `report-proxy/.env` must be git-ignored.
- The app-side change is on a shared branch `report-proxy` (already created off `main`); land the whole sub-project via one PR.

---

## File Structure

```
report-proxy/
  Cargo.toml                    # standalone crate manifest
  .gitignore                    # ignores /target and .env
  .env.example                  # documents required env vars (no secrets)
  src/
    main.rs                     # axum router, config from env, server bootstrap
    types.rs                    # ReportPayload + AppState
    dedup.rs                    # signature/normalize/marker helpers (pure, tested)
    catalog.rs                  # CatalogEntry struct + include!(catalog_data.rs)
    catalog_data.rs             # @generated errorCode -> copy (checked in)
    github.rs                   # GitHub REST client + pure body/title builders (builders tested)
    llm.rs                      # snowywood-llm client + pure prompt/parse (tested)
    ratelimit.rs                # fixed-window per-IP limiter (pure check, tested)
    report.rs                   # POST /report handler orchestration
  scripts/
    extract-catalog.mjs         # reads src/lib/locales/en.ts -> writes src/catalog_data.rs
  Dockerfile                    # multi-stage build -> debian-slim runtime
  docker-compose.yml            # report-proxy + cloudflared sidecar
  RUNBOOK.md                    # deploy + Cloudflare + PAT instructions

src-tauri/src/config.rs         # MODIFY: add report_endpoint field
src/lib/errors/reportError.ts   # MODIFY: ProxySink -> PrefilledIssueSink fallback
```

Task dependency order: 1 (scaffold) -> 2 (dedup) -> 3 (catalog) -> 4 (github) -> 5 (llm) -> 6 (handler wiring) -> 7 (packaging) -> 8 (app-side activation). Tasks 2-5 are independent of each other and could be done in any order after 1; the sequence below is the recommended path.

---

### Task 1: Crate scaffold + health endpoint + env config

**Files:**
- Create: `report-proxy/Cargo.toml`
- Create: `report-proxy/src/main.rs`
- Create: `report-proxy/src/types.rs`
- Create: `report-proxy/.gitignore`

**Interfaces:**
- Consumes: nothing.
- Produces: `struct Config { github_token, github_repo, llm_base_url, llm_model, llm_timeout_secs, rate_limit_per_min, max_body_bytes, bind_addr }` with `Config::from_env() -> Config`; an axum app with `GET /health` returning `200 "ok"`. `struct AppState` (in `types.rs`) holding shared state, initially just `Config` and a `reqwest::Client`.

- [ ] **Step 1: Create the crate manifest**

`report-proxy/Cargo.toml`:

```toml
[package]
name = "report-proxy"
version = "0.1.0"
edition = "2021"

[dependencies]
axum = "0.7"
tower-http = { version = "0.5", features = ["cors"] }
tokio = { version = "1", features = ["rt-multi-thread", "macros", "signal"] }
reqwest = { version = "0.12", default-features = false, features = ["json", "rustls-tls"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
sha2 = "0.10"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }

[dev-dependencies]
```

- [ ] **Step 2: Create `.gitignore`**

`report-proxy/.gitignore`:

```
/target
.env
```

- [ ] **Step 3: Create `types.rs` with Config and AppState**

`report-proxy/src/types.rs`:

```rust
use serde::Deserialize;

/// The report payload sent by the app. Field names match the app's camelCase wire format.
#[derive(Debug, Clone, Deserialize)]
pub struct ReportPayload {
    #[serde(rename = "errorCode")]
    pub error_code: String,
    #[serde(rename = "rawMessage")]
    pub raw_message: String,
    #[serde(rename = "appVersion")]
    pub app_version: String,
    pub os: String,
    pub arch: String,
    pub mode: String,
    pub timestamp: String,
    #[serde(rename = "userNote")]
    pub user_note: Option<String>,
    #[serde(rename = "logsTail")]
    pub logs_tail: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub github_token: String,
    pub github_repo: String,
    pub llm_base_url: String,
    pub llm_model: String,
    pub llm_timeout_secs: u64,
    pub rate_limit_per_min: u32,
    pub max_body_bytes: usize,
    pub bind_addr: String,
}

impl Config {
    pub fn from_env() -> Config {
        fn var(key: &str, default: &str) -> String {
            std::env::var(key).unwrap_or_else(|_| default.to_string())
        }
        Config {
            github_token: var("GITHUB_TOKEN", ""),
            github_repo: var("GITHUB_REPO", "Mooshieblob1/MooshieUI"),
            llm_base_url: var("LLM_BASE_URL", "http://snowywood-llm:8080"),
            llm_model: var("LLM_MODEL", "local"),
            llm_timeout_secs: var("LLM_TIMEOUT_SECS", "20").parse().unwrap_or(20),
            rate_limit_per_min: var("RATE_LIMIT_PER_MIN", "10").parse().unwrap_or(10),
            max_body_bytes: var("MAX_BODY_BYTES", "524288").parse().unwrap_or(524288),
            bind_addr: var("BIND_ADDR", "0.0.0.0:8091"),
        }
    }
}
```

- [ ] **Step 4: Create `main.rs` with health route**

`report-proxy/src/main.rs`:

```rust
mod types;

use axum::{routing::get, Router};
use types::Config;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let config = Config::from_env();
    let bind_addr = config.bind_addr.clone();

    let app = Router::new().route("/health", get(|| async { "ok" }));

    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .expect("failed to bind");
    tracing::info!("report-proxy listening on {bind_addr}");
    axum::serve(listener, app).await.expect("server error");
}
```

- [ ] **Step 5: Verify it compiles**

Run: `cargo check --manifest-path report-proxy/Cargo.toml`
Expected: compiles (warnings about unused `ReportPayload` fields are fine).

- [ ] **Step 6: Verify health endpoint at runtime**

Run (in one shell): `cargo run --manifest-path report-proxy/Cargo.toml`
Then: `curl -s http://127.0.0.1:8091/health`
Expected: `ok`. Stop the server (Ctrl+C).

- [ ] **Step 7: Commit**

```bash
git -c core.hooksPath=/dev/null add report-proxy/Cargo.toml report-proxy/.gitignore report-proxy/src/main.rs report-proxy/src/types.rs
git -c core.hooksPath=/dev/null commit -m "feat(report-proxy): scaffold crate with health endpoint and env config"
```

---

### Task 2: Dedup signature helpers

**Files:**
- Create: `report-proxy/src/dedup.rs`
- Modify: `report-proxy/src/main.rs` (add `mod dedup;`)

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `pub fn normalize(raw: &str) -> String` — lowercases, replaces digit/hex runs and path-like tokens with a placeholder, collapses whitespace.
  - `pub fn signature(error_code: &str, raw_message: &str) -> String` — returns a 16-char lowercase hex string, stable across near-identical messages.
  - `pub fn marker(sig: &str) -> String` — returns `"<!-- mooshie-sig: {sig} -->"`.
  - `pub fn body_has_marker(body: &str, sig: &str) -> bool`.

- [ ] **Step 1: Write the failing tests**

Create `report-proxy/src/dedup.rs` with only a test module first:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signature_is_stable_across_varying_numbers() {
        let a = signature("out_of_memory", "CUDA OOM: tried to allocate 2048 MB at 0x7ff");
        let b = signature("out_of_memory", "CUDA OOM: tried to allocate 512 MB at 0x1ab");
        assert_eq!(a, b, "digit/hex differences must not change the signature");
    }

    #[test]
    fn signature_differs_by_error_code() {
        let a = signature("out_of_memory", "same text");
        let b = signature("disk_full", "same text");
        assert_ne!(a, b);
    }

    #[test]
    fn signature_is_16_hex_chars() {
        let s = signature("generic", "anything");
        assert_eq!(s.len(), 16);
        assert!(s.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn marker_roundtrips() {
        let sig = signature("generic", "x");
        let body = format!("some body\n{}", marker(&sig));
        assert!(body_has_marker(&body, &sig));
        assert!(!body_has_marker("no marker here", &sig));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --manifest-path report-proxy/Cargo.toml dedup`
Expected: FAIL to compile (`normalize`/`signature`/`marker`/`body_has_marker` not found).

- [ ] **Step 3: Implement the helpers**

Prepend to `report-proxy/src/dedup.rs` (above the test module):

```rust
use sha2::{Digest, Sha256};

/// Collapse volatile parts of a message so near-identical errors share a signature.
pub fn normalize(raw: &str) -> String {
    let lower = raw.to_lowercase();
    let mut out = String::with_capacity(lower.len());
    let mut prev_space = false;
    for ch in lower.chars() {
        let mapped = if ch.is_ascii_hexdigit() || ch == '/' || ch == '\\' || ch == ':' {
            '#'
        } else if ch.is_whitespace() {
            ' '
        } else {
            ch
        };
        if mapped == ' ' {
            if !prev_space {
                out.push(' ');
            }
            prev_space = true;
        } else {
            out.push(mapped);
            prev_space = false;
        }
    }
    out.trim().to_string()
}

/// Stable short hex signature for an error, used for dedup markers.
pub fn signature(error_code: &str, raw_message: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(error_code.as_bytes());
    hasher.update(b"\n");
    hasher.update(normalize(raw_message).as_bytes());
    let digest = hasher.finalize();
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    hex[..16].to_string()
}

pub fn marker(sig: &str) -> String {
    format!("<!-- mooshie-sig: {sig} -->")
}

pub fn body_has_marker(body: &str, sig: &str) -> bool {
    body.contains(&marker(sig))
}
```

- [ ] **Step 4: Wire the module**

Add to `report-proxy/src/main.rs` after `mod types;`:

```rust
mod dedup;
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --manifest-path report-proxy/Cargo.toml dedup`
Expected: 4 tests pass.

- [ ] **Step 6: Commit**

```bash
git -c core.hooksPath=/dev/null add report-proxy/src/dedup.rs report-proxy/src/main.rs
git -c core.hooksPath=/dev/null commit -m "feat(report-proxy): dedup signature and marker helpers"
```

---

### Task 3: Error-copy catalog + extraction script

**Files:**
- Create: `report-proxy/scripts/extract-catalog.mjs`
- Create: `report-proxy/src/catalog_data.rs` (generated, then checked in)
- Create: `report-proxy/src/catalog.rs`
- Modify: `report-proxy/src/main.rs` (add `mod catalog;`)

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `pub struct CatalogEntry { pub title: &'static str, pub what: &'static str, pub why: &'static str, pub fixes: &'static str }`.
  - `pub fn lookup(error_code: &str) -> Option<CatalogEntry>`.

- [ ] **Step 1: Write the extraction script**

`report-proxy/scripts/extract-catalog.mjs`:

```javascript
// Reads src/lib/locales/en.ts and emits report-proxy/src/catalog_data.rs.
// Run from the repo root:  node report-proxy/scripts/extract-catalog.mjs
import { readFileSync, writeFileSync } from "node:fs";

const EN = "src/lib/locales/en.ts";
const OUT = "report-proxy/src/catalog_data.rs";

const text = readFileSync(EN, "utf8");
// Match lines like:  "errors.out_of_memory.what": "The GPU ...",
const re = /"errors\.([a-z0-9_]+)\.(title|what|why|fixes)":\s*"((?:[^"\\]|\\.)*)"/g;
const codes = {};
let m;
while ((m = re.exec(text)) !== null) {
  const [, code, field, value] = m;
  if (code === "card" || code === "report") continue; // UI strings, not error codes
  (codes[code] ??= {})[field] = value;
}

const rustEscape = (s) => JSON.stringify(s ?? ""); // JSON string literals are valid Rust string literals for this ASCII copy

const arms = Object.keys(codes)
  .sort()
  .map((code) => {
    const e = codes[code];
    return `        ${JSON.stringify(code)} => CatalogEntry {
            title: ${rustEscape(e.title)},
            what: ${rustEscape(e.what)},
            why: ${rustEscape(e.why)},
            fixes: ${rustEscape(e.fixes)},
        },`;
  })
  .join("\n");

const out = `// @generated by report-proxy/scripts/extract-catalog.mjs -- do not edit by hand.
// Regenerate after changing error copy in src/lib/locales/en.ts.
use crate::catalog::CatalogEntry;

pub fn catalog_lookup(code: &str) -> Option<CatalogEntry> {
    Some(match code {
${arms}
        _ => return None,
    })
}
`;

writeFileSync(OUT, out);
console.log(`Wrote ${OUT} with ${Object.keys(codes).length} entries.`);
```

- [ ] **Step 2: Run the extraction script**

Run: `node report-proxy/scripts/extract-catalog.mjs`
Expected: `Wrote report-proxy/src/catalog_data.rs with 22 entries.`

- [ ] **Step 3: Create `catalog.rs`**

`report-proxy/src/catalog.rs`:

```rust
include!("catalog_data.rs");

#[derive(Debug, Clone, Copy)]
pub struct CatalogEntry {
    pub title: &'static str,
    pub what: &'static str,
    pub why: &'static str,
    pub fixes: &'static str,
}

/// Look up the human-readable copy for an error code, if known.
pub fn lookup(error_code: &str) -> Option<CatalogEntry> {
    catalog_lookup(error_code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_code_is_found() {
        let e = lookup("out_of_memory").expect("out_of_memory should exist");
        assert_eq!(e.title, "Ran out of memory");
        assert!(e.fixes.contains("||"), "fixes are || separated");
    }

    #[test]
    fn unknown_code_is_none() {
        assert!(lookup("no_such_code_xyz").is_none());
    }
}
```

- [ ] **Step 4: Wire the module**

Add to `report-proxy/src/main.rs` after `mod dedup;`:

```rust
mod catalog;
```

- [ ] **Step 5: Run tests**

Run: `cargo test --manifest-path report-proxy/Cargo.toml catalog`
Expected: 2 tests pass. (If `known_code_is_found` fails on the title string, the en.ts copy changed; update the assertion to match current copy.)

- [ ] **Step 6: Commit**

```bash
git -c core.hooksPath=/dev/null add report-proxy/scripts/extract-catalog.mjs report-proxy/src/catalog_data.rs report-proxy/src/catalog.rs report-proxy/src/main.rs
git -c core.hooksPath=/dev/null commit -m "feat(report-proxy): embedded error-copy catalog with extraction script"
```

---

### Task 4: GitHub client + issue body builders

**Files:**
- Create: `report-proxy/src/github.rs`
- Modify: `report-proxy/src/main.rs` (add `mod github;`)

**Interfaces:**
- Consumes: `types::ReportPayload`, `dedup::marker`, `catalog::CatalogEntry`.
- Produces:
  - `pub fn issue_title(error_code: &str, raw_message: &str) -> String` — `"[in-app] {code}: {first 80 chars}"`.
  - `pub fn issue_body(payload: &ReportPayload, sig: &str, summary: Option<&str>) -> String` — Markdown body ending with the hidden marker.
  - `pub struct ExistingIssue { pub number: u64, pub html_url: String }`.
  - `pub struct GithubClient { client: reqwest::Client, token: String, repo: String }` with `new`, `find_open_by_sig`, `create_issue`, `comment_on`.

- [ ] **Step 1: Write the failing tests for the pure builders**

Create `report-proxy/src/github.rs` with the test module first:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ReportPayload;

    fn sample() -> ReportPayload {
        ReportPayload {
            error_code: "out_of_memory".into(),
            raw_message: "CUDA out of memory".into(),
            app_version: "1.4.35".into(),
            os: "windows".into(),
            arch: "x86_64".into(),
            mode: "desktop".into(),
            timestamp: "2026-07-05T00:00:00Z".into(),
            user_note: Some("was generating a batch".into()),
            logs_tail: Some("line1\nline2".into()),
        }
    }

    #[test]
    fn title_is_prefixed_and_truncated() {
        let long = "x".repeat(200);
        let t = issue_title("disk_full", &long);
        assert!(t.starts_with("[in-app] disk_full: "));
        assert_eq!(t.chars().filter(|c| *c == 'x').count(), 80);
    }

    #[test]
    fn body_contains_env_note_and_marker() {
        let body = issue_body(&sample(), "abc123def456aaaa", Some("A summary."));
        assert!(body.contains("### Summary"));
        assert!(body.contains("A summary."));
        assert!(body.contains("was generating a batch"));
        assert!(body.contains("CUDA out of memory"));
        assert!(body.contains("- App version: 1.4.35"));
        assert!(body.contains("<!-- mooshie-sig: abc123def456aaaa -->"));
    }

    #[test]
    fn body_without_summary_omits_summary_header() {
        let body = issue_body(&sample(), "sig0000000000000", None);
        assert!(!body.contains("### Summary"));
        assert!(body.contains("<!-- mooshie-sig: sig0000000000000 -->"));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --manifest-path report-proxy/Cargo.toml github`
Expected: FAIL to compile (`issue_title` / `issue_body` not found).

- [ ] **Step 3: Implement the builders and client**

Prepend to `report-proxy/src/github.rs` (above the test module):

```rust
use crate::dedup::marker;
use crate::types::ReportPayload;

const MAX_LOG_IN_ISSUE: usize = 60_000; // keep issue bodies well under GitHub's limit

fn truncate_chars(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

pub fn issue_title(error_code: &str, raw_message: &str) -> String {
    format!("[in-app] {}: {}", error_code, truncate_chars(raw_message, 80))
}

pub fn issue_body(payload: &ReportPayload, sig: &str, summary: Option<&str>) -> String {
    let mut lines: Vec<String> = Vec::new();
    if let Some(s) = summary {
        lines.push("### Summary".to_string());
        lines.push(s.to_string());
        lines.push(String::new());
    }
    lines.push("### What happened".to_string());
    lines.push(
        payload
            .user_note
            .clone()
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| "(no description provided)".to_string()),
    );
    lines.push(String::new());
    lines.push("### Error".to_string());
    lines.push("```".to_string());
    lines.push(if payload.raw_message.is_empty() {
        "(empty)".to_string()
    } else {
        payload.raw_message.clone()
    });
    lines.push("```".to_string());
    lines.push(String::new());
    lines.push("### Environment".to_string());
    lines.push(format!("- App version: {}", payload.app_version));
    lines.push(format!("- OS: {}", payload.os));
    lines.push(format!("- Arch: {}", payload.arch));
    lines.push(format!("- Mode: {}", payload.mode));
    lines.push(format!("- Error code: {}", payload.error_code));
    lines.push(format!("- When: {}", payload.timestamp));
    lines.push(String::new());
    if let Some(logs) = payload.logs_tail.as_ref().filter(|l| !l.trim().is_empty()) {
        lines.push("### Diagnostics".to_string());
        lines.push("```".to_string());
        lines.push(truncate_chars(logs, MAX_LOG_IN_ISSUE));
        lines.push("```".to_string());
        lines.push(String::new());
    }
    lines.push(marker(sig));
    lines.join("\n")
}

#[derive(Debug, Clone)]
pub struct ExistingIssue {
    pub number: u64,
    pub html_url: String,
}

#[derive(Clone)]
pub struct GithubClient {
    client: reqwest::Client,
    token: String,
    repo: String,
}

impl GithubClient {
    pub fn new(client: reqwest::Client, token: String, repo: String) -> Self {
        Self { client, token, repo }
    }

    fn ua() -> &'static str {
        "mooshie-report-proxy"
    }

    /// Find an open `in-app-report` issue whose body carries this signature marker.
    pub async fn find_open_by_sig(&self, sig: &str) -> Result<Option<ExistingIssue>, String> {
        let url = format!(
            "https://api.github.com/repos/{}/issues?state=open&labels=in-app-report&per_page=100",
            self.repo
        );
        let resp = self
            .client
            .get(&url)
            .header("User-Agent", Self::ua())
            .header("Accept", "application/vnd.github+json")
            .bearer_auth(&self.token)
            .send()
            .await
            .map_err(|e| format!("github list request failed: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("github list returned {}", resp.status()));
        }
        let issues: Vec<serde_json::Value> = resp
            .json()
            .await
            .map_err(|e| format!("github list decode failed: {e}"))?;
        let needle = marker(sig);
        for issue in issues {
            let body = issue.get("body").and_then(|b| b.as_str()).unwrap_or("");
            if body.contains(&needle) {
                let number = issue.get("number").and_then(|n| n.as_u64()).unwrap_or(0);
                let html_url = issue
                    .get("html_url")
                    .and_then(|u| u.as_str())
                    .unwrap_or("")
                    .to_string();
                return Ok(Some(ExistingIssue { number, html_url }));
            }
        }
        Ok(None)
    }

    /// Create an issue; returns its html_url.
    pub async fn create_issue(
        &self,
        title: &str,
        body: &str,
        labels: &[&str],
    ) -> Result<String, String> {
        let url = format!("https://api.github.com/repos/{}/issues", self.repo);
        let payload = serde_json::json!({ "title": title, "body": body, "labels": labels });
        let resp = self
            .client
            .post(&url)
            .header("User-Agent", Self::ua())
            .header("Accept", "application/vnd.github+json")
            .bearer_auth(&self.token)
            .json(&payload)
            .send()
            .await
            .map_err(|e| format!("github create request failed: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("github create returned {}", resp.status()));
        }
        let created: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| format!("github create decode failed: {e}"))?;
        created
            .get("html_url")
            .and_then(|u| u.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| "github create response missing html_url".to_string())
    }

    /// Add a comment to an existing issue.
    pub async fn comment_on(&self, number: u64, text: &str) -> Result<(), String> {
        let url = format!(
            "https://api.github.com/repos/{}/issues/{}/comments",
            self.repo, number
        );
        let payload = serde_json::json!({ "body": text });
        let resp = self
            .client
            .post(&url)
            .header("User-Agent", Self::ua())
            .header("Accept", "application/vnd.github+json")
            .bearer_auth(&self.token)
            .json(&payload)
            .send()
            .await
            .map_err(|e| format!("github comment request failed: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("github comment returned {}", resp.status()));
        }
        Ok(())
    }
}
```

- [ ] **Step 4: Wire the module**

Add to `report-proxy/src/main.rs` after `mod catalog;`:

```rust
mod github;
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --manifest-path report-proxy/Cargo.toml github`
Expected: 3 tests pass.

- [ ] **Step 6: Commit**

```bash
git -c core.hooksPath=/dev/null add report-proxy/src/github.rs report-proxy/src/main.rs
git -c core.hooksPath=/dev/null commit -m "feat(report-proxy): github client and issue body builders"
```

---

### Task 5: LLM summarizer client

**Files:**
- Create: `report-proxy/src/llm.rs`
- Modify: `report-proxy/src/main.rs` (add `mod llm;`)

**Interfaces:**
- Consumes: `catalog::CatalogEntry`, `types::Config`.
- Produces:
  - `pub fn build_prompt(entry: Option<CatalogEntry>, error_code: &str, raw_message: &str, logs_tail: Option<&str>) -> String`.
  - `pub fn parse_content(json: &serde_json::Value) -> Option<String>` — reads `choices[0].message.content`, trims, `None` if empty.
  - `pub async fn summarize(client: &reqwest::Client, cfg: &Config, prompt: &str) -> Option<String>` — best-effort; `None` on any error/timeout.

- [ ] **Step 1: Write the failing tests**

Create `report-proxy/src/llm.rs` with the test module first:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog;

    #[test]
    fn prompt_disables_thinking_and_includes_context() {
        let entry = catalog::lookup("out_of_memory");
        let prompt = build_prompt(entry, "out_of_memory", "CUDA OOM", Some("log line here"));
        assert!(prompt.contains("/no_think"));
        assert!(prompt.contains("out_of_memory"));
        assert!(prompt.contains("CUDA OOM"));
        assert!(prompt.contains("log line here"));
    }

    #[test]
    fn prompt_truncates_long_logs() {
        let big = "z".repeat(20_000);
        let prompt = build_prompt(None, "generic", "err", Some(&big));
        assert!(prompt.chars().filter(|c| *c == 'z').count() <= 4000);
    }

    #[test]
    fn parse_reads_message_content() {
        let json = serde_json::json!({
            "choices": [ { "message": { "content": "  Hello.  " } } ]
        });
        assert_eq!(parse_content(&json), Some("Hello.".to_string()));
    }

    #[test]
    fn parse_none_when_content_empty_or_thinking_only() {
        let json = serde_json::json!({
            "choices": [ { "message": { "content": "", "reasoning_content": "thinking..." } } ]
        });
        assert_eq!(parse_content(&json), None);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --manifest-path report-proxy/Cargo.toml llm`
Expected: FAIL to compile (`build_prompt` / `parse_content` not found).

- [ ] **Step 3: Implement the client**

Prepend to `report-proxy/src/llm.rs` (above the test module):

```rust
use std::time::Duration;

use crate::catalog::CatalogEntry;
use crate::types::Config;

const MAX_LOG_CHARS: usize = 4000; // ~1-1.3k tokens; the model has only 3072 ctx

/// Build the summarization prompt. `/no_think` disables Qwen3 reasoning so the
/// CPU-bound model spends its budget on the answer, not the thinking pass.
pub fn build_prompt(
    entry: Option<CatalogEntry>,
    error_code: &str,
    raw_message: &str,
    logs_tail: Option<&str>,
) -> String {
    let mut ctx = String::new();
    ctx.push_str("/no_think\n");
    ctx.push_str(
        "You are triaging a bug report for an image-generation desktop app. \
Write ONE short paragraph (2-4 sentences), plain English, no lists, describing the \
likely problem and the most probable cause. Do not restate the logs verbatim.\n\n",
    );
    ctx.push_str(&format!("Error code: {error_code}\n"));
    if let Some(e) = entry {
        ctx.push_str(&format!("Known meaning: {} - {}\n", e.title, e.what));
        ctx.push_str(&format!("Typical cause: {}\n", e.why));
    }
    ctx.push_str(&format!("Raw error: {raw_message}\n"));
    if let Some(logs) = logs_tail {
        let tail: String = if logs.chars().count() > MAX_LOG_CHARS {
            logs.chars().skip(logs.chars().count() - MAX_LOG_CHARS).collect()
        } else {
            logs.to_string()
        };
        ctx.push_str("\nRecent log tail:\n");
        ctx.push_str(&tail);
    }
    ctx
}

/// Extract the assistant's final text, ignoring empty/thinking-only responses.
pub fn parse_content(json: &serde_json::Value) -> Option<String> {
    let content = json
        .get("choices")?
        .get(0)?
        .get("message")?
        .get("content")?
        .as_str()?
        .trim();
    if content.is_empty() {
        None
    } else {
        Some(content.to_string())
    }
}

/// Best-effort one-paragraph summary. Returns None on any failure or timeout.
pub async fn summarize(client: &reqwest::Client, cfg: &Config, prompt: &str) -> Option<String> {
    if cfg.llm_base_url.trim().is_empty() {
        return None;
    }
    let url = format!(
        "{}/v1/chat/completions",
        cfg.llm_base_url.trim_end_matches('/')
    );
    let body = serde_json::json!({
        "model": cfg.llm_model,
        "messages": [ { "role": "user", "content": prompt } ],
        "max_tokens": 300,
        "temperature": 0.3,
        "stream": false
    });
    let resp = client
        .post(&url)
        .json(&body)
        .timeout(Duration::from_secs(cfg.llm_timeout_secs))
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        tracing::warn!("llm returned {}", resp.status());
        return None;
    }
    let json: serde_json::Value = resp.json().await.ok()?;
    parse_content(&json)
}
```

- [ ] **Step 4: Wire the module**

Add to `report-proxy/src/main.rs` after `mod github;`:

```rust
mod llm;
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --manifest-path report-proxy/Cargo.toml llm`
Expected: 4 tests pass.

- [ ] **Step 6: Commit**

```bash
git -c core.hooksPath=/dev/null add report-proxy/src/llm.rs report-proxy/src/main.rs
git -c core.hooksPath=/dev/null commit -m "feat(report-proxy): best-effort llm summarizer client"
```

---

### Task 6: Rate limiter + /report handler wiring

**Files:**
- Create: `report-proxy/src/ratelimit.rs`
- Create: `report-proxy/src/report.rs`
- Modify: `report-proxy/src/types.rs` (add `AppState`)
- Modify: `report-proxy/src/main.rs` (build state, add `POST /report`, body limit)

**Interfaces:**
- Consumes: everything from Tasks 1-5.
- Produces:
  - `ratelimit::RateLimiter` with `pub fn new(limit: u32) -> Self` and `pub fn check(&self, ip: &str, now_secs: u64) -> bool` (fixed 60s window; `true` = allowed).
  - `types::AppState { cfg: Arc<Config>, http: reqwest::Client, github: GithubClient, limiter: Arc<RateLimiter> }`.
  - `report::report_handler` axum handler for `POST /report`.

- [ ] **Step 1: Write the failing rate-limiter test**

Create `report-proxy/src/ratelimit.rs`:

```rust
use std::collections::HashMap;
use std::sync::Mutex;

/// Fixed-window per-key limiter. Window is 60 seconds.
pub struct RateLimiter {
    limit: u32,
    windows: Mutex<HashMap<String, (u64, u32)>>, // key -> (window_start_secs, count)
}

impl RateLimiter {
    pub fn new(limit: u32) -> Self {
        Self { limit, windows: Mutex::new(HashMap::new()) }
    }

    /// Returns true if the request is allowed, false if the key is over its limit.
    pub fn check(&self, key: &str, now_secs: u64) -> bool {
        let window = now_secs / 60;
        let mut map = self.windows.lock().unwrap();
        let entry = map.entry(key.to_string()).or_insert((window, 0));
        if entry.0 != window {
            *entry = (window, 0);
        }
        if entry.1 >= self.limit {
            return false;
        }
        entry.1 += 1;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_up_to_limit_then_blocks() {
        let rl = RateLimiter::new(3);
        assert!(rl.check("1.2.3.4", 100));
        assert!(rl.check("1.2.3.4", 101));
        assert!(rl.check("1.2.3.4", 102));
        assert!(!rl.check("1.2.3.4", 103), "4th in same window is blocked");
    }

    #[test]
    fn resets_in_next_window() {
        let rl = RateLimiter::new(1);
        assert!(rl.check("ip", 0));
        assert!(!rl.check("ip", 30));
        assert!(rl.check("ip", 60), "new 60s window resets the count");
    }

    #[test]
    fn keys_are_independent() {
        let rl = RateLimiter::new(1);
        assert!(rl.check("a", 0));
        assert!(rl.check("b", 0));
    }
}
```

- [ ] **Step 2: Run the rate-limiter test to verify it fails, then passes**

Run: `cargo test --manifest-path report-proxy/Cargo.toml ratelimit`
Expected: FAIL to compile first (module not wired). Add `mod ratelimit;` to `main.rs` after `mod llm;`, then re-run.
Expected: 3 tests pass.

- [ ] **Step 3: Add `AppState` to `types.rs`**

Append to `report-proxy/src/types.rs`:

```rust
use std::sync::Arc;

use crate::github::GithubClient;
use crate::ratelimit::RateLimiter;

#[derive(Clone)]
pub struct AppState {
    pub cfg: Arc<Config>,
    pub http: reqwest::Client,
    pub github: GithubClient,
    pub limiter: Arc<RateLimiter>,
}
```

- [ ] **Step 4: Write the `/report` handler**

`report-proxy/src/report.rs`:

```rust
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde_json::json;

use crate::types::{AppState, ReportPayload};
use crate::{catalog, dedup, github, llm};

fn client_ip(headers: &HeaderMap) -> String {
    headers
        .get("cf-connecting-ip")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown")
        .to_string()
}

/// Seconds since the Unix epoch (used only for rate-limit windows).
fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub async fn report_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> (StatusCode, Json<serde_json::Value>) {
    // 1. App header gate.
    if headers.get("x-mooshie-app").and_then(|v| v.to_str().ok()) != Some("1") {
        return (StatusCode::FORBIDDEN, Json(json!({ "error": "forbidden" })));
    }

    // 2. Rate limit by Cloudflare-provided client IP.
    let ip = client_ip(&headers);
    if !state.limiter.check(&ip, now_secs()) {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({ "error": "rate limited" })),
        );
    }

    // 3. Parse and minimally validate the payload.
    let payload: ReportPayload = match serde_json::from_slice(&body) {
        Ok(p) => p,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": format!("invalid payload: {e}") })),
            );
        }
    };
    if payload.error_code.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "errorCode is required" })),
        );
    }

    // 4. Dedup: comment on an existing open issue if we have seen this signature.
    let sig = dedup::signature(&payload.error_code, &payload.raw_message);
    match state.github.find_open_by_sig(&sig).await {
        Ok(Some(existing)) => {
            let note = format!(
                "Seen again from another user. App version {}, OS {}, arch {}.",
                payload.app_version, payload.os, payload.arch
            );
            let _ = state.github.comment_on(existing.number, &note).await;
            return (
                StatusCode::OK,
                Json(json!({ "issueUrl": existing.html_url })),
            );
        }
        Ok(None) => {}
        Err(e) => {
            // Non-fatal: fall through and create a fresh issue.
            tracing::warn!("dedup lookup failed: {e}");
        }
    }

    // 5. Best-effort LLM summary (never blocks issue creation).
    let entry = catalog::lookup(&payload.error_code);
    let prompt = llm::build_prompt(
        entry,
        &payload.error_code,
        &payload.raw_message,
        payload.logs_tail.as_deref(),
    );
    let summary = llm::summarize(&state.http, &state.cfg, &prompt).await;

    // 6. Create the issue.
    let title = github::issue_title(&payload.error_code, &payload.raw_message);
    let issue_body = github::issue_body(&payload, &sig, summary.as_deref());
    match state
        .github
        .create_issue(&title, &issue_body, &["bug", "in-app-report"])
        .await
    {
        Ok(url) => (StatusCode::OK, Json(json!({ "issueUrl": url }))),
        Err(e) => {
            tracing::error!("issue creation failed: {e}");
            (
                StatusCode::BAD_GATEWAY,
                Json(json!({ "error": "failed to create issue" })),
            )
        }
    }
}
```

- [ ] **Step 5: Wire state, route, and body limit in `main.rs`**

Replace the body of `main()` and the module list in `report-proxy/src/main.rs` so the file reads:

```rust
mod catalog;
mod dedup;
mod github;
mod llm;
mod ratelimit;
mod report;
mod types;

use std::sync::Arc;

use axum::extract::DefaultBodyLimit;
use axum::http::{HeaderName, Method};
use axum::routing::{get, post};
use axum::Router;
use tower_http::cors::{Any, CorsLayer};

use github::GithubClient;
use ratelimit::RateLimiter;
use types::{AppState, Config};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let config = Config::from_env();
    let bind_addr = config.bind_addr.clone();
    let max_body = config.max_body_bytes;

    let http = reqwest::Client::new();
    let github = GithubClient::new(
        http.clone(),
        config.github_token.clone(),
        config.github_repo.clone(),
    );
    let limiter = Arc::new(RateLimiter::new(config.rate_limit_per_min));

    let state = AppState {
        cfg: Arc::new(config),
        http,
        github,
        limiter,
    };

    // Permissive-but-header-gated CORS: browser mode posts cross-origin, and the
    // custom X-Mooshie-App header forces a preflight. Abuse control is the header
    // gate + rate limit, not the origin.
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::POST, Method::OPTIONS])
        .allow_headers([
            HeaderName::from_static("content-type"),
            HeaderName::from_static("x-mooshie-app"),
        ]);

    let app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/report", post(report::report_handler))
        .layer(cors)
        .layer(DefaultBodyLimit::max(max_body))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .expect("failed to bind");
    tracing::info!("report-proxy listening on {bind_addr}");
    axum::serve(listener, app).await.expect("server error");
}
```

- [ ] **Step 6: Full compile + test + clippy**

Run: `cargo test --manifest-path report-proxy/Cargo.toml`
Expected: all tests pass (dedup 4, catalog 2, github 3, llm 4, ratelimit 3).

Run: `cargo clippy --manifest-path report-proxy/Cargo.toml -- -D warnings`
Expected: no warnings. (`.get(0)` on a `serde_json::Value` is the inherent index accessor, not the slice method, so `clippy::get_first` does not fire.)

- [ ] **Step 7: Manual smoke test (header gate + bad body)**

Run (in one shell): `GITHUB_TOKEN=dummy cargo run --manifest-path report-proxy/Cargo.toml`
Then:

```bash
# Missing header -> 403
curl -s -o /dev/null -w "%{http_code}\n" -X POST http://127.0.0.1:8091/report -d '{}'
# With header but invalid body -> 400
curl -s -o /dev/null -w "%{http_code}\n" -X POST http://127.0.0.1:8091/report \
  -H "X-Mooshie-App: 1" -H "Content-Type: application/json" -d 'not json'
```

Expected: `403` then `400`. Stop the server.

- [ ] **Step 8: Commit**

```bash
git -c core.hooksPath=/dev/null add report-proxy/src/ratelimit.rs report-proxy/src/report.rs report-proxy/src/types.rs report-proxy/src/main.rs
git -c core.hooksPath=/dev/null commit -m "feat(report-proxy): rate limiter and /report handler"
```

---

### Task 7: Packaging (Dockerfile, compose, runbook)

**Files:**
- Create: `report-proxy/Dockerfile`
- Create: `report-proxy/docker-compose.yml`
- Create: `report-proxy/.env.example`
- Create: `report-proxy/RUNBOOK.md`

**Interfaces:**
- Consumes: the built crate.
- Produces: a runnable container image and compose stack. No automated tests (deployment artifacts); validated by a local Docker build.

- [ ] **Step 1: Write the Dockerfile**

`report-proxy/Dockerfile`:

```dockerfile
# Build stage
FROM rust:1-slim AS build
WORKDIR /app
COPY Cargo.toml ./
COPY src ./src
RUN cargo build --release

# Runtime stage
FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /app/target/release/report-proxy /usr/local/bin/report-proxy
EXPOSE 8091
ENV BIND_ADDR=0.0.0.0:8091
CMD ["report-proxy"]
```

- [ ] **Step 2: Write the compose file**

`report-proxy/docker-compose.yml`:

```yaml
services:
  report-proxy:
    build: .
    container_name: report-proxy
    restart: unless-stopped
    env_file: .env
    environment:
      RUST_LOG: info
      LLM_BASE_URL: http://snowywood-llm:8080
      GITHUB_REPO: Mooshieblob1/MooshieUI
    networks:
      - blob_default
    # No host port published; only cloudflared reaches it.

  cloudflared:
    image: cloudflare/cloudflared:latest
    container_name: report-cloudflared
    restart: unless-stopped
    command: tunnel --no-autoupdate run --token ${CLOUDFLARE_TUNNEL_TOKEN}
    network_mode: "service:report-proxy"
    depends_on:
      - report-proxy

networks:
  blob_default:
    external: true
```

- [ ] **Step 3: Write `.env.example`**

`report-proxy/.env.example`:

```
# Copy to .env on the NUC and fill in. Never commit .env.
# Fine-grained GitHub PAT: Mooshieblob1/MooshieUI only, Issues read+write.
GITHUB_TOKEN=
# Cloudflare Tunnel token (from the dashboard tunnel install command).
CLOUDFLARE_TUNNEL_TOKEN=
# Optional overrides (defaults shown):
# LLM_BASE_URL=http://snowywood-llm:8080
# LLM_MODEL=local
# LLM_TIMEOUT_SECS=20
# RATE_LIMIT_PER_MIN=10
# MAX_BODY_BYTES=524288
```

- [ ] **Step 4: Write the runbook**

`report-proxy/RUNBOOK.md`:

```markdown
# Report Proxy Runbook

Self-hosted service on the NUC (192.168.4.80) that turns in-app error reports into
GitHub issues on Mooshieblob1/MooshieUI. The GitHub credential lives only here.

## One-time setup

### 1. GitHub token
Create a fine-grained PAT: GitHub Settings -> Developer settings -> Fine-grained tokens.
- Repository access: only Mooshieblob1/MooshieUI.
- Permissions: Issues -> Read and write. Nothing else.

### 2. Cloudflare Tunnel (done in the dashboard)
- Zero Trust -> Networks -> Tunnels -> Create a tunnel -> Cloudflared.
- Name it (e.g. mooshie-report), save. Copy the token from the install command
  (the long string after `service install`). Do NOT install cloudflared by hand; the
  compose runs it as a container.
- Public Hostname tab -> Add a public hostname:
  - Subdomain: report
  - Domain: your domain
  - Type: HTTP
  - URL: localhost:8091

### 3. Secrets on the NUC
The file /home/blob/report-proxy/.env (perms 600) holds:
    GITHUB_TOKEN=...
    CLOUDFLARE_TUNNEL_TOKEN=...

## Deploy

    cd /home/blob/report-proxy
    docker compose up -d --build

The report-proxy container joins the existing blob_default network to reach
snowywood-llm at http://snowywood-llm:8080. cloudflared shares its network namespace
and forwards report.<domain> to localhost:8091.

## Update the error-copy catalog
When error copy in src/lib/locales/en.ts changes, regenerate the embedded catalog:

    node report-proxy/scripts/extract-catalog.mjs

Then rebuild: `docker compose up -d --build`.

## Smoke test
    curl -s -X POST https://report.<domain>/report \
      -H "X-Mooshie-App: 1" -H "Content-Type: application/json" \
      -d '{"errorCode":"generic","rawMessage":"smoke test","appVersion":"0","os":"x","arch":"x","mode":"desktop","timestamp":"2026-07-05T00:00:00Z"}'
Expect JSON: {"issueUrl":"https://github.com/Mooshieblob1/MooshieUI/issues/N"}.
Send the same payload again; expect the same issueUrl (deduped via a comment).

## Logs
    docker compose logs -f report-proxy
    docker compose logs -f cloudflared
```

- [ ] **Step 5: Verify the Docker image builds**

Run: `docker build -t report-proxy report-proxy/`
Expected: build succeeds and produces the `report-proxy` image. (Requires Docker locally; if unavailable, this is verified on the NUC at deploy time instead. Note that in the plan checkbox.)

- [ ] **Step 6: Commit**

```bash
git -c core.hooksPath=/dev/null add report-proxy/Dockerfile report-proxy/docker-compose.yml report-proxy/.env.example report-proxy/RUNBOOK.md
git -c core.hooksPath=/dev/null commit -m "feat(report-proxy): docker packaging, compose, and runbook"
```

---

### Task 8: App-side activation (config field + ProxySink fallback)

**Files:**
- Modify: `src-tauri/src/config.rs` (add `report_endpoint` field + default)
- Modify: `src/lib/errors/reportError.ts` (fallback when ProxySink fails)

**Interfaces:**
- Consumes: existing `AppConfig` (Rust) and `reportError()` (TS).
- Produces: `AppConfig.report_endpoint: Option<String>` serialized as `report_endpoint`; `reportError()` that falls back to `PrefilledIssueSink` when `ProxySink` throws.

- [ ] **Step 1: Add the Rust config field**

In `src-tauri/src/config.rs`, inside `pub struct AppConfig` (after the `llm_external_model` field near line 161), add:

```rust
    /// Optional report proxy endpoint (Cloudflare Tunnel URL). When set, in-app
    /// error reports POST here instead of opening a prefilled GitHub issue.
    #[serde(default)]
    pub report_endpoint: Option<String>,
```

- [ ] **Step 2: Add the default**

In the `impl Default for AppConfig` block, add alongside the other defaults (matching the existing style, e.g. near the `llm_external_model` default):

```rust
            report_endpoint: None,
```

- [ ] **Step 3: Verify Rust compiles**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: compiles. The TS type `src/lib/types/index.ts:243` already declares `report_endpoint?: string | null`, so no TS type change is needed.

- [ ] **Step 4: Add the ProxySink fallback in `reportError.ts`**

In `src/lib/errors/reportError.ts`, replace the `reportError` function (lines 116-124) with a version that falls back to the prefilled sink when the proxy sink fails:

```typescript
/** Report a resolved error via the active sink, falling back to a prefilled issue. */
export async function reportError(
  error: FriendlyError,
  userNote?: string,
): Promise<{ issueUrl?: string }> {
  const { sink, usesLogs } = await activeSink();
  const payload = await buildReportPayload(error, userNote, usesLogs);
  try {
    return await sink.submit(payload);
  } catch (err) {
    if (sink instanceof ProxySink) {
      // Proxy unreachable or failed; fall back to a prefilled GitHub issue so the
      // report is never lost. The prefilled sink does not use logsTail.
      const fallback = new PrefilledIssueSink();
      const fallbackPayload = await buildReportPayload(error, userNote, false);
      return fallback.submit(fallbackPayload);
    }
    throw err;
  }
}
```

- [ ] **Step 5: Verify the frontend builds**

Run: `npm run build`
Expected: build succeeds (this is the repo's TS/Svelte gate).

- [ ] **Step 6: Commit**

```bash
git -c core.hooksPath=/dev/null add src-tauri/src/config.rs src/lib/errors/reportError.ts
git -c core.hooksPath=/dev/null commit -m "feat(errors): activate report proxy via config and add ProxySink fallback"
```

---

## After implementation

Once all tasks are committed on the `report-proxy` branch:
1. Push the branch and open a PR into `main` (the whole sub-project lands as one PR; CI gate is the "GlassWorm Infection Audit" check).
2. On the NUC: fill `/home/blob/report-proxy/.env` (already created) with the two tokens, copy the `report-proxy/` directory to `/home/blob/report-proxy/`, and run `docker compose up -d --build`.
3. Create the Cloudflare Tunnel + public hostname route per `RUNBOOK.md`.
4. Set `report_endpoint` in the app config to `https://report.<domain>/report` to activate the proxy. Leaving it empty keeps the existing prefilled-issue behavior.
