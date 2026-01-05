//! Error types for sonos-state

use std::fmt;

/// Result type for sonos-state operations
pub type Result<T> = std::result::Result<T, StateError>;

/// Errors that can occur during state management
#[derive(Debug)]
pub enum StateError {
    /// Error during initialization
    Init(String),

    /// Error parsing data
    Parse(String),

    /// Error from sonos-api
    Api(sonos_api::ApiError),

    /// State manager is already running
    AlreadyRunning,

    /// Shutdown failed
    ShutdownFailed,

    /// Lock acquisition failed
    LockError(String),

    /// Speaker not found
    SpeakerNotFound(String),

    /// Invalid URL
    InvalidUrl(String),
}

impl fmt::Display for StateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StateError::Init(msg) => write!(f, "Initialization error: {}", msg),
            StateError::Parse(msg) => write!(f, "Parse error: {}", msg),
            StateError::Api(err) => write!(f, "API error: {}", err),
            StateError::AlreadyRunning => write!(f, "State manager is already running"),
            StateError::ShutdownFailed => write!(f, "Shutdown failed"),
            StateError::LockError(msg) => write!(f, "Lock error: {}", msg),
            StateError::SpeakerNotFound(id) => write!(f, "Speaker not found: {}", id),
            StateError::InvalidUrl(url) => write!(f, "Invalid URL: {}", url),
        }
    }
}

impl std::error::Error for StateError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            StateError::Api(err) => Some(err),
            _ => None,
        }
    }
}

impl From<sonos_api::ApiError> for StateError {
    fn from(err: sonos_api::ApiError) -> Self {
        StateError::Api(err)
    }
}

impl From<url::ParseError> for StateError {
    fn from(err: url::ParseError) -> Self {
        StateError::InvalidUrl(err.to_string())
    }
}
