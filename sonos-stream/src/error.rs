//! Error types for the sonos-stream crate.

use crate::types::{ServiceType, SpeakerId};

/// Errors that can occur in the event broker.
#[derive(Debug, thiserror::Error)]
pub enum BrokerError {
    /// No strategy is registered for the requested service type
    #[error("No strategy registered for service type: {0:?}")]
    NoStrategyForService(ServiceType),

    /// A subscription already exists for this speaker-service combination
    #[error("Subscription already exists: {speaker_id:?} / {service_type:?}")]
    SubscriptionAlreadyExists {
        /// The speaker ID
        speaker_id: SpeakerId,
        /// The service type
        service_type: ServiceType,
    },

    /// The requested subscription was not found
    #[error("Subscription not found: {speaker_id:?} / {service_type:?}")]
    SubscriptionNotFound {
        /// The speaker ID
        speaker_id: SpeakerId,
        /// The service type
        service_type: ServiceType,
    },

    /// An error occurred in the callback server
    #[error("Callback server error: {0}")]
    CallbackServerError(String),

    /// An error occurred in a strategy implementation
    #[error("Strategy error: {0}")]
    StrategyError(#[from] StrategyError),

    /// Invalid configuration provided
    #[error("Configuration error: {0}")]
    ConfigurationError(String),

    /// An error occurred during shutdown
    #[error("Shutdown error: {0}")]
    ShutdownError(String),
}

/// Errors from strategy implementations.
#[derive(Debug, thiserror::Error)]
pub enum StrategyError {
    /// Failed to create a subscription
    #[error("Failed to create subscription: {0}")]
    SubscriptionCreationFailed(String),

    /// Failed to parse an event
    #[error("Failed to parse event: {0}")]
    EventParseFailed(String),

    /// A network error occurred
    #[error("Network error: {0}")]
    NetworkError(String),

    /// Invalid configuration provided to the strategy
    #[error("Invalid configuration: {0}")]
    InvalidConfiguration(String),

    /// Invalid input data provided to the strategy
    #[error("Invalid input: {0}")]
    InvalidInput(String),

    /// Service temporarily unavailable
    #[error("Service unavailable: {0}")]
    ServiceUnavailable(String),
}

/// Errors from subscription operations.
#[derive(Debug, thiserror::Error)]
pub enum SubscriptionError {
    /// Failed to renew the subscription
    #[error("Renewal failed: {0}")]
    RenewalFailed(String),

    /// Failed to unsubscribe
    #[error("Unsubscribe failed: {0}")]
    UnsubscribeFailed(String),

    /// A network error occurred
    #[error("Network error: {0}")]
    NetworkError(String),

    /// The subscription has expired
    #[error("Subscription expired")]
    Expired,
}

/// Convenience type alias for Results using BrokerError.
pub type Result<T> = std::result::Result<T, BrokerError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_broker_error_display() {
        let error = BrokerError::NoStrategyForService(ServiceType::AVTransport);
        assert_eq!(
            error.to_string(),
            "No strategy registered for service type: AVTransport"
        );

        let error = BrokerError::SubscriptionAlreadyExists {
            speaker_id: SpeakerId::new("speaker1"),
            service_type: ServiceType::RenderingControl,
        };
        assert!(error.to_string().contains("Subscription already exists"));
        assert!(error.to_string().contains("speaker1"));
        assert!(error.to_string().contains("RenderingControl"));

        let error = BrokerError::CallbackServerError("port in use".to_string());
        assert_eq!(error.to_string(), "Callback server error: port in use");

        let error = BrokerError::ConfigurationError("invalid port range".to_string());
        assert_eq!(error.to_string(), "Configuration error: invalid port range");

        let error = BrokerError::ShutdownError("timeout".to_string());
        assert_eq!(error.to_string(), "Shutdown error: timeout");
    }

    #[test]
    fn test_strategy_error_display() {
        let error = StrategyError::SubscriptionCreationFailed("connection refused".to_string());
        assert_eq!(
            error.to_string(),
            "Failed to create subscription: connection refused"
        );

        let error = StrategyError::EventParseFailed("invalid XML".to_string());
        assert_eq!(error.to_string(), "Failed to parse event: invalid XML");

        let error = StrategyError::NetworkError("timeout".to_string());
        assert_eq!(error.to_string(), "Network error: timeout");

        let error = StrategyError::InvalidConfiguration("missing field".to_string());
        assert_eq!(error.to_string(), "Invalid configuration: missing field");

        let error = StrategyError::InvalidInput("empty data".to_string());
        assert_eq!(error.to_string(), "Invalid input: empty data");

        let error = StrategyError::ServiceUnavailable("device offline".to_string());
        assert_eq!(error.to_string(), "Service unavailable: device offline");
    }

    #[test]
    fn test_subscription_error_display() {
        let error = SubscriptionError::RenewalFailed("network timeout".to_string());
        assert_eq!(error.to_string(), "Renewal failed: network timeout");

        let error = SubscriptionError::UnsubscribeFailed("not found".to_string());
        assert_eq!(error.to_string(), "Unsubscribe failed: not found");

        let error = SubscriptionError::NetworkError("connection lost".to_string());
        assert_eq!(error.to_string(), "Network error: connection lost");

        let error = SubscriptionError::Expired;
        assert_eq!(error.to_string(), "Subscription expired");
    }

    #[test]
    fn test_error_conversion_from_strategy_error() {
        let strategy_error = StrategyError::NetworkError("timeout".to_string());
        let broker_error: BrokerError = strategy_error.into();

        match broker_error {
            BrokerError::StrategyError(e) => {
                assert_eq!(e.to_string(), "Network error: timeout");
            }
            _ => panic!("Expected StrategyError variant"),
        }
    }

    #[test]
    fn test_result_type_alias() {
        fn returns_result() -> Result<i32> {
            Ok(42)
        }

        fn returns_error() -> Result<i32> {
            Err(BrokerError::ConfigurationError("test".to_string()))
        }

        assert_eq!(returns_result().unwrap(), 42);
        assert!(returns_error().is_err());
    }
}
