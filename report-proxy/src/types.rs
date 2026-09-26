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
    pub rate_limit_per_min: u32,
    pub global_writes_per_min: u32,
    pub web_origin_writes_per_min: u32,
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
            rate_limit_per_min: var("RATE_LIMIT_PER_MIN", "10").parse().unwrap_or(10),
            // Issue creations + comments per minute across ALL clients. 8/min stays
            // under GitHub's 500 content-creating requests/hour secondary limit.
            global_writes_per_min: var("GLOBAL_WRITES_PER_MIN", "8").parse().unwrap_or(8),
            // The share of that budget reports from web origins (browser mode, or
            // any other website) may take, so they can never starve the app's.
            web_origin_writes_per_min: var("WEB_ORIGIN_WRITES_PER_MIN", "3").parse().unwrap_or(3),
            // 1 MiB: the app still sends its whole diagnostic log, but the proxy only
            // keeps the last `report::MAX_LOG_TAIL_CHARS` of it, so anything much
            // bigger is wasted parsing.
            max_body_bytes: var("MAX_BODY_BYTES", "1048576").parse().unwrap_or(1048576),
            bind_addr: var("BIND_ADDR", "0.0.0.0:8091"),
        }
    }
}

use std::sync::Arc;

use crate::github::GithubClient;
use crate::ratelimit::{RateLimiter, WriteBudget};

#[derive(Clone)]
pub struct AppState {
    pub github: GithubClient,
    pub limiter: Arc<RateLimiter>,
    pub budget: Arc<WriteBudget>,
    /// Taken (in addition to `budget`) by reports from web origins.
    pub web_budget: Arc<WriteBudget>,
}
