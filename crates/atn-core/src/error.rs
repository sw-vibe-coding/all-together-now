use crate::agent::AgentId;

/// ATN error type.
#[derive(Debug, thiserror::Error)]
pub enum AtnError {
    #[error("PTY error: {0}")]
    Pty(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("TOML deserialization error: {0}")]
    TomlDeser(#[from] toml::de::Error),

    #[error("agent not found: {0}")]
    AgentNotFound(AgentId),

    #[error("agent not ready for input: {0}")]
    AgentNotReady(AgentId),

    #[error("wiki error: {0}")]
    Wiki(String),

    #[error("trail error: {0}")]
    Trail(String),

    #[error("inbox error: {0}")]
    Inbox(String),

    #[error("channel send error: {0}")]
    Channel(String),
}

/// ATN result type.
pub type Result<T> = std::result::Result<T, AtnError>;
