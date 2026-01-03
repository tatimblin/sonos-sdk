//! Error types for the sonos-stream crate.

use crate::types::{ServiceType, SpeakerId};
use sonos_api::ApiError;

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

/// Convert sonos-api ApiError to SubscriptionError
/// 
/// This implementation maps all possible ApiError variants to appropriate
/// SubscriptionError variants while preserving original error messages for debugging.
impl From<ApiError> for SubscriptionError {
    fn from(error: ApiError) -> Self {
        match error {
            // Network-related errors map to NetworkError
            ApiError::NetworkError(msg) => SubscriptionError::NetworkError(msg),

            // Parse errors map to NetworkError (communication issue)
            ApiError::ParseError(msg) => SubscriptionError::NetworkError(format!("Parse error: {}", msg)),

            // SOAP faults map to UnsubscribeFailed (operation failure)
            ApiError::SoapFault(code) => SubscriptionError::UnsubscribeFailed(format!("SOAP fault: error code {}", code)),

            // Subscription-specific errors map appropriately
            ApiError::SubscriptionError(msg) => {
                if msg.contains("expired") {
                    SubscriptionError::Expired
                } else {
                    SubscriptionError::RenewalFailed(msg)
                }
            },

            // Device errors map to NetworkError with descriptive messages
            ApiError::DeviceError(msg) => SubscriptionError::NetworkError(format!("Device error: {}", msg)),

            // Invalid parameters map to NetworkError
            ApiError::InvalidParameter(msg) => SubscriptionError::NetworkError(format!("Invalid parameter: {}", msg)),
        }
    }
}

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

    #[test]
    fn test_api_error_to_subscription_error_conversion() {
        // Test NetworkError mapping
        let api_error = ApiError::NetworkError("connection failed".to_string());
        let sub_error: SubscriptionError = api_error.into();
        match sub_error {
            SubscriptionError::NetworkError(msg) => assert_eq!(msg, "connection failed"),
            _ => panic!("Expected NetworkError variant"),
        }

        // Test DeviceError mapping
        let api_error = ApiError::DeviceError("device offline".to_string());
        let sub_error: SubscriptionError = api_error.into();
        match sub_error {
            SubscriptionError::NetworkError(msg) => assert_eq!(msg, "Device error: device offline"),
            _ => panic!("Expected NetworkError variant"),
        }

        // Test ParseError mapping
        let api_error = ApiError::ParseError("invalid XML".to_string());
        let sub_error: SubscriptionError = api_error.into();
        match sub_error {
            SubscriptionError::NetworkError(msg) => assert_eq!(msg, "Parse error: invalid XML"),
            _ => panic!("Expected NetworkError variant"),
        }

        // Test SoapFault mapping
        let api_error = ApiError::SoapFault(500);
        let sub_error: SubscriptionError = api_error.into();
        match sub_error {
            SubscriptionError::UnsubscribeFailed(msg) => assert_eq!(msg, "SOAP fault: error code 500"),
            _ => panic!("Expected UnsubscribeFailed variant"),
        }

        // Test SubscriptionError mapping
        let api_error = ApiError::SubscriptionError("subscription rejected".to_string());
        let sub_error: SubscriptionError = api_error.into();
        match sub_error {
            SubscriptionError::UnsubscribeFailed(msg) => assert_eq!(msg, "subscription rejected"),
            _ => panic!("Expected UnsubscribeFailed variant"),
        }

        // Test RenewalFailed mapping
        let api_error = ApiError::SubscriptionError("renewal timeout".to_string());
        let sub_error: SubscriptionError = api_error.into();
        match sub_error {
            SubscriptionError::RenewalFailed(msg) => assert_eq!(msg, "renewal timeout"),
            _ => panic!("Expected RenewalFailed variant"),
        }

        // Test SubscriptionExpired mapping
        let api_error = ApiError::SubscriptionError("subscription expired".to_string());
        let sub_error: SubscriptionError = api_error.into();
        match sub_error {
            SubscriptionError::Expired => {},
            _ => panic!("Expected Expired variant"),
        }

        // Test InvalidCallbackUrl mapping
        let api_error = ApiError::InvalidParameter("malformed URL".to_string());
        let sub_error: SubscriptionError = api_error.into();
        match sub_error {
            SubscriptionError::NetworkError(msg) => assert_eq!(msg, "Invalid parameter: malformed URL"),
            _ => panic!("Expected NetworkError variant"),
        }

        // Test EventParsingFailed mapping
        let api_error = ApiError::ParseError("bad event XML".to_string());
        let sub_error: SubscriptionError = api_error.into();
        match sub_error {
            SubscriptionError::NetworkError(msg) => assert_eq!(msg, "Parse error: bad event XML"),
            _ => panic!("Expected NetworkError variant"),
        }

        // Test InvalidParameter mapping
        let api_error = ApiError::InvalidParameter("Volume 150 is out of range".to_string());
        let sub_error: SubscriptionError = api_error.into();
        match sub_error {
            SubscriptionError::NetworkError(msg) => assert_eq!(msg, "Invalid parameter: Volume 150 is out of range"),
            _ => panic!("Expected NetworkError variant"),
        }

        // Test DeviceError mapping
        let api_error = ApiError::DeviceError("Device is not a group coordinator: 192.168.1.100".to_string());
        let sub_error: SubscriptionError = api_error.into();
        match sub_error {
            SubscriptionError::NetworkError(msg) => assert_eq!(msg, "Device error: Device is not a group coordinator: 192.168.1.100"),
            _ => panic!("Expected NetworkError variant"),
        }

        // Test DeviceError mapping for unsupported operation
        let api_error = ApiError::DeviceError("Unsupported operation".to_string());
        let sub_error: SubscriptionError = api_error.into();
        match sub_error {
            SubscriptionError::NetworkError(msg) => assert_eq!(msg, "Device error: Unsupported operation"),
            _ => panic!("Expected NetworkError variant"),
        }

        // Test DeviceError mapping for invalid state
        let api_error = ApiError::DeviceError("Invalid device state: not playing".to_string());
        let sub_error: SubscriptionError = api_error.into();
        match sub_error {
            SubscriptionError::NetworkError(msg) => assert_eq!(msg, "Device error: Invalid device state: not playing"),
            _ => panic!("Expected NetworkError variant"),
        }

        // Test InvalidParameter mapping
        let api_error = ApiError::InvalidParameter("bad instance ID".to_string());
        let sub_error: SubscriptionError = api_error.into();
        match sub_error {
            SubscriptionError::NetworkError(msg) => assert_eq!(msg, "Invalid parameter: bad instance ID"),
            _ => panic!("Expected NetworkError variant"),
        }
    }
}
