use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    #[error("Failed to start ComfyUI: {0}")]
    ProcessSpawnFailed(String),

    #[error("API error ({status}): {message}")]
    ApiError { status: u16, message: String },

    #[error("WebSocket error: {0}")]
    WebSocketError(String),

    #[error("Invalid workflow: {0}")]
    InvalidWorkflow(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),

    #[error("HTTP error: {0}")]
    HttpError(#[from] reqwest::Error),

    #[error("Interrogator error: {0}")]
    InterrogatorError(String),

    #[error("Prompt assistant error: {0}")]
    LlmError(String),

    /// An external LLM that could not answer right now: no response, a
    /// timeout, a rate limit or a server-side failure. Worded exactly like
    /// `LlmError` for the user; kept apart so the fallback chain can tell a
    /// provider that is down or busy from one that refused the request.
    #[error("Prompt assistant error: {0}")]
    LlmUnavailable(String),

    #[error("{0}")]
    Other(String),
}

impl AppError {
    /// Whether the next model in the LLM fallback chain should be tried.
    pub fn is_llm_unavailable(&self) -> bool {
        matches!(self, AppError::LlmUnavailable(_))
    }
}

impl From<String> for AppError {
    fn from(s: String) -> Self {
        AppError::Other(s)
    }
}

impl From<&str> for AppError {
    fn from(s: &str) -> Self {
        AppError::Other(s.to_string())
    }
}

impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}
