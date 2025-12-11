//! Core types for the sonos-stream crate.

use std::net::IpAddr;
use std::time::Duration;

/// Unique identifier for a Sonos speaker.
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct SpeakerId(pub String);

impl SpeakerId {
    /// Create a new speaker ID from a string.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Get the speaker ID as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for SpeakerId {
    fn from(id: String) -> Self {
        Self(id)
    }
}

impl From<&str> for SpeakerId {
    fn from(id: &str) -> Self {
        Self(id.to_string())
    }
}

impl std::fmt::Display for SpeakerId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Represents a Sonos speaker on the network.
#[derive(Debug, Clone)]
pub struct Speaker {
    /// Unique identifier for the speaker
    pub id: SpeakerId,
    /// IP address of the speaker
    pub ip: IpAddr,
    /// Human-readable name of the speaker
    pub name: String,
    /// Room name where the speaker is located
    pub room: String,
}

impl Speaker {
    /// Create a new speaker instance.
    pub fn new(id: SpeakerId, ip: IpAddr, name: String, room: String) -> Self {
        Self { id, ip, name, room }
    }
}

/// UPnP service types that can be subscribed to.
#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq)]
pub enum ServiceType {
    /// AVTransport service for playback control
    AVTransport,
    /// RenderingControl service for volume and EQ
    RenderingControl,
    /// ZoneGroupTopology service for speaker grouping
    ZoneGroupTopology,
}

/// Classification of subscription scope for a service.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum SubscriptionScope {
    /// Service requires a subscription per speaker
    PerSpeaker,
    /// Service requires only one subscription for the entire network
    NetworkWide,
}

/// Unique key for tracking subscriptions.
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct SubscriptionKey {
    /// The speaker this subscription is for
    pub speaker_id: SpeakerId,
    /// The service type being subscribed to
    pub service_type: ServiceType,
}

impl SubscriptionKey {
    /// Create a new subscription key.
    pub fn new(speaker_id: SpeakerId, service_type: ServiceType) -> Self {
        Self {
            speaker_id,
            service_type,
        }
    }
}

/// Configuration for the event broker.
#[derive(Debug, Clone)]
pub struct BrokerConfig {
    /// Port range for the callback server (start, end)
    pub callback_port_range: (u16, u16),
    /// Timeout for subscription operations
    pub subscription_timeout: Duration,
    /// Threshold before expiration to trigger renewal
    pub renewal_threshold: Duration,
    /// Maximum number of retry attempts for failed operations
    pub max_retry_attempts: u32,
    /// Base duration for exponential backoff between retries
    pub retry_backoff_base: Duration,
    /// Size of the event buffer channel
    pub event_buffer_size: usize,
}

impl Default for BrokerConfig {
    fn default() -> Self {
        Self {
            callback_port_range: (3400, 3500),
            subscription_timeout: Duration::from_secs(1800), // 30 minutes
            renewal_threshold: Duration::from_secs(300),     // 5 minutes before expiration
            max_retry_attempts: 3,
            retry_backoff_base: Duration::from_secs(2),
            event_buffer_size: 100,
        }
    }
}

/// Configuration for individual subscriptions.
#[derive(Debug, Clone)]
pub struct SubscriptionConfig {
    /// Timeout in seconds for the subscription
    pub timeout_seconds: u32,
    /// Callback URL for receiving events
    pub callback_url: String,
}

impl SubscriptionConfig {
    /// Create a new subscription configuration.
    pub fn new(timeout_seconds: u32, callback_url: String) -> Self {
        Self {
            timeout_seconds,
            callback_url,
        }
    }
}
