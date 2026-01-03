//! Core types for the sonos-stream crate.

use std::net::IpAddr;
use std::time::{Duration, SystemTime};
use sonos_api::Service;

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

impl ServiceType {
    /// Convert to sonos-api Service enum
    pub fn to_sonos_api_service(self) -> Service {
        match self {
            ServiceType::AVTransport => Service::AVTransport,
            ServiceType::RenderingControl => Service::RenderingControl,
            ServiceType::ZoneGroupTopology => Service::ZoneGroupTopology,
        }
    }
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

/// Raw event received from the callback server with Sonos-specific context.
///
/// This represents an unparsed UPnP event notification that has been received
/// via HTTP callback and enriched with Sonos-specific information (speaker ID
/// and service type). It needs to be processed by the event processor.
#[derive(Debug, Clone)]
pub struct RawEvent {
    /// The subscription ID this event is for
    pub subscription_id: String,
    /// The speaker ID
    pub speaker_id: SpeakerId,
    /// The service type
    pub service_type: ServiceType,
    /// The raw XML event body
    pub event_xml: String,
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

/// Active subscription state tracked by the broker.
///
/// This struct contains metadata about subscriptions for event processing
/// and renewal management. The actual subscription management is now handled
/// by the `sonos-api` crate's `ManagedSubscription` types.
#[derive(Debug, Clone)]
pub struct ActiveSubscription {
    /// The unique key identifying this subscription
    pub key: SubscriptionKey,
    /// The subscription ID returned by the device
    pub subscription_id: String,
    /// When this subscription was created
    pub created_at: SystemTime,
    /// When the last event was received (None if no events yet)
    pub last_event: Option<SystemTime>,
    /// When this subscription expires
    pub expires_at: SystemTime,
}

impl ActiveSubscription {
    /// Create a new active subscription.
    pub fn new(
        key: SubscriptionKey,
        subscription_id: String,
        expires_at: SystemTime,
    ) -> Self {
        Self {
            key,
            subscription_id,
            created_at: SystemTime::now(),
            last_event: None,
            expires_at,
        }
    }

    /// Update the last event timestamp to now.
    pub fn mark_event_received(&mut self) {
        self.last_event = Some(SystemTime::now());
    }

    /// Check if the subscription needs renewal.
    pub fn needs_renewal(&self, threshold: Duration) -> bool {
        let now = SystemTime::now();
        if let Ok(time_until_expiry) = self.expires_at.duration_since(now) {
            time_until_expiry <= threshold
        } else {
            // Already expired
            true
        }
    }

    /// Check if the subscription has expired.
    pub fn is_expired(&self) -> bool {
        SystemTime::now() >= self.expires_at
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_service_type_to_sonos_api_service_conversion() {
        // Test AVTransport conversion
        assert_eq!(
            ServiceType::AVTransport.to_sonos_api_service(),
            Service::AVTransport
        );

        // Test RenderingControl conversion
        assert_eq!(
            ServiceType::RenderingControl.to_sonos_api_service(),
            Service::RenderingControl
        );

        // Test ZoneGroupTopology conversion
        assert_eq!(
            ServiceType::ZoneGroupTopology.to_sonos_api_service(),
            Service::ZoneGroupTopology
        );
    }
}
