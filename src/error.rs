//! Error types for Yolog Core

use thiserror::Error;

/// Core error type
#[derive(Error, Debug)]
pub enum CoreError {
    /// Database error
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    /// IO error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Configuration error
    #[error("Configuration error: {0}")]
    Config(String),

    /// Parser error
    #[error("Parser error: {0}")]
    Parser(String),

    /// Watcher error
    #[error("Watcher error: {0}")]
    Watcher(String),

    /// API error
    #[error("API error: {0}")]
    Api(String),

    /// Embedding error
    #[error("Embedding error: {0}")]
    Embedding(String),

    /// JSON error
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// TOML parsing error
    #[error("TOML parse error: {0}")]
    TomlParse(#[from] toml::de::Error),

    /// Not found error
    #[error("{0} not found: {1}")]
    NotFound(&'static str, String),

    /// Validation error
    #[error("Validation error: {0}")]
    Validation(String),
}

/// Result type alias for Core operations
pub type Result<T> = std::result::Result<T, CoreError>;

impl From<notify::Error> for CoreError {
    fn from(e: notify::Error) -> Self {
        CoreError::Watcher(e.to_string())
    }
}
