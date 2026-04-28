use thiserror::Error;

#[derive(Debug, Error)]
pub enum AemError {
    #[error("invalid config: {0}")]
    InvalidConfig(String),
    #[error("invalid data: {0}")]
    InvalidData(String),
    #[error("io error: {0}")]
    Io(String),
    #[error("serde error: {0}")]
    Serde(String),
    #[error("position not found: {0}")]
    PositionNotFound(String),
    #[error("ledger degraded: {0}")]
    LedgerDegraded(String),
    #[error("other: {0}")]
    Other(String),
}

impl From<std::io::Error> for AemError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value.to_string())
    }
}

impl From<serde_json::Error> for AemError {
    fn from(value: serde_json::Error) -> Self {
        Self::Serde(value.to_string())
    }
}
