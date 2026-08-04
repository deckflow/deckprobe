use thiserror::Error;

#[derive(Debug, Error)]
pub enum DeckProbeError {
    #[error("source I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("budget exceeded: {0}")]
    BudgetExceeded(String),
    #[error("unsupported format: {0}")]
    UnsupportedFormat(String),
    #[error("unsupported target: {0}")]
    UnsupportedTarget(String),
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    #[error("malformed input: {0}")]
    MalformedInput(String),
    #[error("format parser failed: {0}")]
    Parser(String),
}

pub type Result<T> = std::result::Result<T, DeckProbeError>;
