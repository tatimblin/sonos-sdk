//! Error types for sonos-state

/// Result type for sonos-state operations
pub type Result<T> = std::result::Result<T, StateError>;

/// Errors that can occur during state management
#[derive(Debug, thiserror::Error)]
pub enum StateError {
    /// Error during initialization
    #[error("Initialization error: {0}")]
    Init(String),

    /// Error parsing data
    #[error("Parse error: {0}")]
    Parse(String),

    /// Error from sonos-api
    #[error("API error: {0}")]
    Api(#[from] sonos_api::ApiError),

    /// State manager is already running
    #[error("State manager is already running")]
    AlreadyRunning,

    /// Shutdown failed
    #[error("Shutdown failed")]
    ShutdownFailed,

    /// Lock acquisition failed
    #[error("Lock error: {0}")]
    LockError(String),

    /// Speaker not found
    #[error("Speaker not found: {0:?}")]
    SpeakerNotFound(crate::model::SpeakerId),

    /// Invalid URL
    #[error("Invalid URL: {0}")]
    InvalidUrl(String),

    /// Initialization failed
    #[error("Initialization failed: {0}")]
    InitializationFailed(String),

    /// Device registration failed
    #[error("Device registration failed: {0}")]
    DeviceRegistrationFailed(String),

    /// Subscription failed
    #[error("Subscription failed: {0}")]
    SubscriptionFailed(String),

    /// Invalid IP address
    #[error("Invalid IP address: {0}")]
    InvalidIpAddress(String),

    /// Lock poisoned (internal mutex error)
    #[error("Internal lock poisoned")]
    LockPoisoned,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error as _;

    #[test]
    fn api_variant_keeps_message_and_source() {
        let err = StateError::from(sonos_api::ApiError::NetworkError("boom".to_string()));
        assert_eq!(err.to_string(), "API error: Network error: boom");
        assert!(err.source().is_some());
    }
}
