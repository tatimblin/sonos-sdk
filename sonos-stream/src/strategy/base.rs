//! Core strategy enum and helpers for service-specific subscription handling.

use crate::error::StrategyError;
use crate::event::{TypedEvent, EventData};
use crate::subscription::{Subscription, UPnPSubscription};
use crate::types::{ServiceType, SpeakerId, Speaker, SubscriptionConfig, SubscriptionScope};




/// Trait for parsers that convert XML to event data.
///
/// Implement this for parser types to enable generic event parsing.
pub trait EventParser: Sized {
    type EventType: EventData + 'static;

    fn from_xml(xml: &str) -> Result<Self, String>;
    fn into_event(self) -> Self::EventType;
}

// Implement EventParser for AVTransportParser
impl EventParser for sonos_parser::services::av_transport::AVTransportParser {
    type EventType = Self;

    fn from_xml(xml: &str) -> Result<Self, String> {
        Self::from_xml(xml).map_err(|e| e.to_string())
    }

    fn into_event(self) -> Self::EventType {
        self
    }
}

/// Generic helper to parse events using a specific parser type.
///
/// Reduces boilerplate by handling: XML parsing → event data → TypedEvent wrapping.
fn parse_event_generic<P: EventParser>(event_xml: &str) -> Result<TypedEvent, StrategyError> {
    let parsed = P::from_xml(event_xml)
        .map_err(|e| StrategyError::EventParseFailed(format!("Failed to parse event: {}", e)))?;
    
    let event_data = parsed.into_event();
    Ok(TypedEvent::new(Box::new(event_data)))
}

/// Base strategy trait providing common subscription logic.
///
/// This trait provides default implementations for creating and managing UPnP
/// subscriptions. Implementors only need to provide service-specific configuration
/// methods (service_type, subscription_scope, service_endpoint_path, parse_event).
///
/// # Design
///
/// - Implementors define: Service-specific configuration (what parser? what endpoint?)
/// - Trait provides: Common implementation logic (how to subscribe?)
#[async_trait::async_trait]
pub trait BaseStrategy {
    /// Get the service type this strategy handles.
    fn service_type(&self) -> ServiceType;

    /// Get subscription scope metadata for this service.
    fn subscription_scope(&self) -> SubscriptionScope;

    /// Get the UPnP service endpoint path.
    fn service_endpoint_path(&self) -> &'static str;

    /// Parse raw UPnP event XML into a typed event using the service-specific parser.
    fn parse_event(
        &self,
        speaker_id: &SpeakerId,
        event_xml: &str,
    ) -> Result<TypedEvent, StrategyError>;

    /// Create a new UPnP subscription for a speaker.
    ///
    /// Default implementation constructs the service endpoint URL and creates
    /// a UPnP subscription. Override only if custom subscription logic is needed.
    ///
    /// # Parameters
    ///
    /// - `speaker`: The speaker to subscribe to
    /// - `callback_url`: The base URL of the callback server
    /// - `config`: Configuration including timeout and full callback URL
    ///
    /// # Returns
    ///
    /// A boxed `Subscription` instance that can be used to manage the subscription lifecycle.
    ///
    /// # Errors
    ///
    /// - `StrategyError::SubscriptionCreationFailed` if the subscription request fails
    /// - `StrategyError::NetworkError` if a network error occurs
    async fn create_subscription(
        &self,
        speaker: &Speaker,
        callback_url: String,
        config: &SubscriptionConfig,
    ) -> Result<Box<dyn Subscription>, StrategyError> {
        let endpoint_url = format!("http://{}:1400{}", speaker.ip, self.service_endpoint_path());
        let service_type = self.service_type();
        
        let subscription = UPnPSubscription::create_subscription(
            speaker.id.clone(),
            service_type,
            endpoint_url,
            callback_url,
            config.timeout_seconds,
        )
        .await
        .map_err(|e| match e {
            crate::error::SubscriptionError::NetworkError(msg) => StrategyError::NetworkError(msg),
            crate::error::SubscriptionError::UnsubscribeFailed(msg) => StrategyError::SubscriptionCreationFailed(msg),
            crate::error::SubscriptionError::RenewalFailed(msg) => StrategyError::SubscriptionCreationFailed(msg),
            crate::error::SubscriptionError::Expired => StrategyError::SubscriptionCreationFailed("Subscription expired during creation".to_string()),
        })?;

        Ok(Box::new(subscription))
    }
}

/// Strategy enum for service-specific subscription and event parsing.
///
/// Each variant represents a UPnP service and handles service-specific logic for
/// creating subscriptions and parsing events. The enum uses zero-cost dispatch to
/// route operations to the appropriate service implementation.
///
/// # Available Services
///
/// - [`AVTransport`](Strategy::AVTransport): Handles playback state changes, track metadata,
///   and transport control events (PLAYING, PAUSED, STOPPED, etc.)
///
/// # Thread Safety
///
/// All strategy variants are stateless and thread-safe (`Send + Sync`). Subscription
/// state is managed by `UPnPSubscription` instances.
///
/// # Design
///
/// `Strategy` provides service-specific configuration (endpoints, parsers) while
/// inheriting common implementation methods from `BaseStrategy` via `Deref`.
///
/// # Example
///
/// ```rust,ignore
/// use sonos_stream::{EventBrokerBuilder, Strategy};
///
/// let broker = EventBrokerBuilder::new()
///     .with_strategy(Box::new(Strategy::AVTransport))
///     .build().await?;
/// ```
#[derive(Debug, Clone, Copy)]
pub enum Strategy {
    /// AVTransport service strategy.
    ///
    /// Provides events for:
    /// - Transport state changes (PLAYING, PAUSED, STOPPED, etc.)
    /// - Track metadata changes (title, artist, album, duration)
    /// - Current track URI changes
    ///
    /// Endpoint: `/MediaRenderer/AVTransport/Event`
    /// Scope: Per-speaker (each speaker needs its own subscription)
    AVTransport,
    // Add new service strategies here as they're implemented
}



impl BaseStrategy for Strategy {
    fn service_type(&self) -> ServiceType {
        match self {
            Strategy::AVTransport => ServiceType::AVTransport,
        }
    }

    fn subscription_scope(&self) -> SubscriptionScope {
        match self {
            Strategy::AVTransport => SubscriptionScope::PerSpeaker,
        }
    }

    fn service_endpoint_path(&self) -> &'static str {
        match self {
            Strategy::AVTransport => "/MediaRenderer/AVTransport/Event",
        }
    }

    fn parse_event(
        &self,
        _speaker_id: &SpeakerId,
        event_xml: &str,
    ) -> Result<TypedEvent, StrategyError> {
        match self {
            Strategy::AVTransport => {
                parse_event_generic::<sonos_parser::services::av_transport::AVTransportParser>(event_xml)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Speaker, SpeakerId};
    use std::net::IpAddr;
    
    // Import the trait to use its methods
    use super::BaseStrategy;

    fn create_test_strategy() -> Strategy {
        Strategy::AVTransport
    }

    #[test]
    fn test_strategy_service_type() {
        let strategy = create_test_strategy();
        assert_eq!(strategy.service_type(), ServiceType::AVTransport);
    }

    #[test]
    fn test_strategy_subscription_scope() {
        let strategy = create_test_strategy();
        assert_eq!(strategy.subscription_scope(), SubscriptionScope::PerSpeaker);
    }

    #[test]
    fn test_strategy_endpoint_path() {
        let strategy = create_test_strategy();
        assert_eq!(strategy.service_endpoint_path(), "/MediaRenderer/AVTransport/Event");
    }

    #[test]
    fn test_strategy_thread_safety() {
        let strategy = create_test_strategy();
        
        // Ensure Strategy implements Send + Sync
        fn assert_send_sync<T: Send + Sync>(_: T) {}
        assert_send_sync(strategy);
    }

    #[test]
    fn test_strategy_clone() {
        let strategy = create_test_strategy();
        let cloned = strategy.clone();
        
        assert_eq!(strategy.service_type(), cloned.service_type());
        assert_eq!(strategy.subscription_scope(), cloned.subscription_scope());
        assert_eq!(strategy.service_endpoint_path(), cloned.service_endpoint_path());
    }

    #[test]
    fn test_endpoint_url_construction() {
        let strategy = create_test_strategy();
        let speaker = Speaker::new(
            SpeakerId::new("RINCON_123"),
            "192.168.1.100".parse::<IpAddr>().unwrap(),
            "Living Room".to_string(),
            "Living Room".to_string(),
        );
        
        let expected_path = "/MediaRenderer/AVTransport/Event";
        assert_eq!(strategy.service_endpoint_path(), expected_path);
        
        // Verify the full URL construction logic
        let expected_url = format!("http://{}:1400{}", speaker.ip, expected_path);
        assert_eq!(expected_url, "http://192.168.1.100:1400/MediaRenderer/AVTransport/Event");
    }

    #[test]
    fn test_parse_event_valid_xml() {
        let strategy = create_test_strategy();
        let speaker_id = SpeakerId::new("test_speaker");
        
        let event_xml = r#"<e:propertyset xmlns:e="urn:schemas-upnp-org:event-1-0"><e:property><LastChange>&lt;Event xmlns="urn:schemas-upnp-org:metadata-1-0/AVT/"&gt;&lt;InstanceID val="0"&gt;&lt;TransportState val="PLAYING"/&gt;&lt;/InstanceID&gt;&lt;/Event&gt;</LastChange></e:property></e:propertyset>"#;
        
        let result = strategy.parse_event(&speaker_id, event_xml);
        assert!(result.is_ok(), "Should successfully parse valid XML");
        
        let typed_event = result.unwrap();
        assert_eq!(typed_event.event_type(), "av_transport_event");
        assert_eq!(typed_event.service_type(), ServiceType::AVTransport);
    }

    #[test]
    fn test_parse_event_invalid_xml() {
        let strategy = create_test_strategy();
        let speaker_id = SpeakerId::new("test_speaker");
        
        let result = strategy.parse_event(&speaker_id, "<invalid>xml</invalid>");
        assert!(result.is_err(), "Should fail on invalid XML");
        
        match result {
            Err(StrategyError::EventParseFailed(msg)) => {
                assert!(msg.contains("Failed to parse event"));
            }
            _ => panic!("Expected EventParseFailed error"),
        }
    }
}
