use std::net::IpAddr;
use thiserror::Error;

/// Errors that can occur in the Sonos Event Manager
#[derive(Error, Debug)]
pub enum EventManagerError {
    /// Error initializing the event broker
    #[error("Failed to initialize event broker: {0}")]
    BrokerInitialization(#[from] sonos_stream::BrokerError),

    /// Error registering device with broker
    #[error("Failed to register device {device_ip} for service {service:?}: {source}")]
    DeviceRegistration {
        device_ip: IpAddr,
        service: sonos_api::Service,
        #[source]
        source: sonos_stream::BrokerError,
    },

    /// Error unregistering device from broker
    #[error("Failed to unregister device {device_ip} for service {service:?}: {source}")]
    DeviceUnregistration {
        device_ip: IpAddr,
        service: sonos_api::Service,
        #[source]
        source: sonos_stream::BrokerError,
    },

    /// Error creating event consumer
    #[error("Failed to create event consumer for {device_ip} service {service:?}")]
    ConsumerCreation {
        device_ip: IpAddr,
        service: sonos_api::Service,
    },

    /// Device not found
    #[error("Device with IP {0} not found")]
    DeviceNotFound(IpAddr),

    /// Subscription not found
    #[error("Subscription for device {device_ip} service {service:?} not found")]
    SubscriptionNotFound {
        device_ip: IpAddr,
        service: sonos_api::Service,
    },

    /// Event channel closed
    #[error("Event channel has been closed")]
    ChannelClosed,

    /// Device discovery error
    #[error("Device discovery failed: {0}")]
    Discovery(#[from] sonos_discovery::DiscoveryError),

    /// Internal synchronization error
    #[error("Internal synchronization error: {0}")]
    Sync(String),
}

/// Result type for Event Manager operations
pub type Result<T> = std::result::Result<T, EventManagerError>;