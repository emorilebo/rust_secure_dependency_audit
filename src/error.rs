//! Error types for the audit system

use thiserror::Error;

/// Result type alias for audit operations
pub type Result<T> = std::result::Result<T, AuditError>;

/// Main error type for audit operations
#[derive(Error, Debug)]
pub enum AuditError {
    #[error("Failed to parse project metadata: {0}")]
    ParseError(String),

    #[error("Network error: {0}")]
    NetworkError(#[source] Box<dyn std::error::Error + Send + Sync>),

    #[error("API error from {service}: {message}")]
    ApiError { service: String, message: String },

    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("JSON serialization error: {0}")]
    JsonError(#[from] serde_json::Error),

    #[error("TOML parsing error: {0}")]
    TomlError(#[from] toml::de::Error),

    #[error("Cargo metadata error: {0}")]
    CargoMetadataError(#[from] cargo_metadata::Error),

    #[error("HTTP request error: {0}")]
    ReqwestError(#[from] reqwest::Error),

    #[error("Invalid dependency: {0}")]
    InvalidDependency(String),

    #[error("Rate limit exceeded for {service}. Retry after: {retry_after:?}")]
    RateLimitExceeded {
        service: String,
        retry_after: Option<std::time::Duration>,
    },

    #[error("Dependency not found: {0}")]
    DependencyNotFound(String),
}

#[derive(Debug)]
struct StringError(String);

impl std::fmt::Display for StringError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for StringError {}

impl AuditError {
    /// Create a parse error
    pub fn parse(msg: impl Into<String>) -> Self {
        Self::ParseError(msg.into())
    }

    /// Create a network error
    pub fn network(msg: impl Into<String>) -> Self {
        Self::NetworkError(Box::new(StringError(msg.into())))
    }

    /// Create an API error
    pub fn api(service: impl Into<String>, message: impl Into<String>) -> Self {
        Self::ApiError {
            service: service.into(),
            message: message.into(),
        }
    }

    /// Create a configuration error
    pub fn config(msg: impl Into<String>) -> Self {
        Self::ConfigError(msg.into())
    }
}
