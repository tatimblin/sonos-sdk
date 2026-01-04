//! Error types for the sonos-stream crate
//!
//! This module defines all error types used throughout the crate, providing
//! clear error messages and proper error chaining.

use std::net::IpAddr;

/// Main error type for the EventBroker
#[derive(Debug, thiserror::Error)]
pub enum BrokerError {
    #[error("Registry error: {0}")]
    Registry(#[from] RegistryError),

    #[error("Subscription error: {0}")]
    Subscription(#[from] SubscriptionError),

    #[error("Polling error: {0}")]
    Polling(#[from] PollingError),

    #[error("Event processing error: {0}")]
    EventProcessing(String),

    #[error("Callback server error: {0}")]
    CallbackServer(String),

    #[error("Configuration error: {0}")]
    Configuration(String),

    #[error("Firewall detection error: {0}")]
    FirewallDetection(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

/// Errors related to speaker/service registry operations
#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error("Speaker/service pair already registered: {speaker_ip} {service:?}")]
    DuplicateRegistration {
        speaker_ip: IpAddr,
        service: sonos_api::Service,
    },

    #[error("Registration not found: {0}")]
    NotFound(crate::RegistrationId),

    #[error("Invalid speaker IP address: {0}")]
    InvalidIpAddress(String),

    #[error("Registry is full (max registrations: {max_registrations})")]
    RegistryFull { max_registrations: usize },
}

/// Errors related to subscription management
#[derive(Debug, thiserror::Error)]
pub enum SubscriptionError {
    #[error("Subscription expired")]
    Expired,

    #[error("Subscription failed to create: {0}")]
    CreationFailed(String),

    #[error("Subscription renewal failed: {0}")]
    RenewalFailed(String),

    #[error("Network error: {0}")]
    NetworkError(String),

    #[error("UPnP service error: {0}")]
    ServiceError(String),

    #[error("Callback registration failed: {0}")]
    CallbackRegistration(String),

    #[error("Invalid subscription state")]
    InvalidState,
}

/// Errors related to polling operations
#[derive(Debug, thiserror::Error)]
pub enum PollingError {
    #[error("Network error during polling: {0}")]
    Network(String),

    #[error("State parsing error: {0}")]
    StateParsing(String),

    #[error("Service not supported for polling: {service:?}")]
    UnsupportedService { service: sonos_api::Service },

    #[error("Device unreachable: {device_ip}")]
    DeviceUnreachable { device_ip: IpAddr },

    #[error("Polling task spawn failed: {0}")]
    TaskSpawn(String),

    #[error("Too many consecutive errors: {error_count}")]
    TooManyErrors { error_count: u32 },

    #[error("SOAP client error: {0}")]
    SoapClient(String),
}

/// Errors related to event processing and iteration
#[derive(Debug, thiserror::Error)]
pub enum EventProcessingError {
    #[error("Event parsing failed: {0}")]
    Parsing(String),

    #[error("Event enrichment failed: {0}")]
    Enrichment(String),

    #[error("Channel closed")]
    ChannelClosed,

    #[error("Timeout waiting for event")]
    Timeout,

    #[error("Iterator already consumed")]
    IteratorConsumed,
}

/// Result type alias for BrokerError
pub type BrokerResult<T> = Result<T, BrokerError>;

/// Result type alias for RegistryError
pub type RegistryResult<T> = Result<T, RegistryError>;

/// Result type alias for SubscriptionError
pub type SubscriptionResult<T> = Result<T, SubscriptionError>;

/// Result type alias for PollingError
pub type PollingResult<T> = Result<T, PollingError>;

/// Result type alias for EventProcessingError
pub type EventProcessingResult<T> = Result<T, EventProcessingError>;

#[cfg(test)]
mod tests {
    use std::error::Error;

    use super::*;

    #[test]
    fn test_error_display() {
        let err = BrokerError::Configuration("test error".to_string());
        assert!(err.to_string().contains("Configuration error"));

        let registry_err = RegistryError::DuplicateRegistration {
            speaker_ip: "192.168.1.100".parse().unwrap(),
            service: sonos_api::Service::AVTransport,
        };
        assert!(registry_err.to_string().contains("already registered"));
    }

    #[test]
    fn test_error_chaining() {
        let registry_err = RegistryError::NotFound(crate::RegistrationId::new(1));
        let broker_err = BrokerError::Registry(registry_err);

        assert!(broker_err.to_string().contains("Registry error"));
        assert!(broker_err.source().is_some());
    }
}