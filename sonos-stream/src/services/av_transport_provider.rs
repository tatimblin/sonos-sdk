//! AVTransport service provider implementation.
//!
//! This module provides the `AVTransportProvider` struct that implements the `ServiceStrategy`
//! trait for the AVTransport UPnP service. It encapsulates all AVTransport-specific logic
//! including endpoint configuration, subscription scope, and event parsing.

use async_trait::async_trait;

use crate::error::StrategyError;
use crate::event::TypedEvent;
use crate::subscription::{Subscription, UPnPSubscription};
use crate::types::{ServiceType, SpeakerId, Speaker, SubscriptionConfig, SubscriptionScope};

use super::ServiceStrategy;

/// Provider for AVTransport UPnP service.
///
/// This provider handles all AVTransport-specific logic including:
/// - Service endpoint configuration (`/MediaRenderer/AVTransport/Event`)
/// - Per-speaker subscription scope
/// - XML event parsing using `AVTransportParser`
/// - Subscription creation and management
///
/// The provider is self-contained and requires no external dependencies
/// beyond the standard UPnP subscription infrastructure.
///
/// # Example
///
/// ```rust,ignore
/// use sonos_stream::AVTransportProvider;
///
/// let provider = AVTransportProvider::new();
/// 
/// // Register with event processor
/// processor.register_strategy(Box::new(provider));
/// ```
#[derive(Debug, Clone)]
pub struct AVTransportProvider;

impl AVTransportProvider {
    /// Create a new AVTransport provider instance.
    pub fn new() -> Self {
        Self
    }
}

impl Default for AVTransportProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ServiceStrategy for AVTransportProvider {
    fn service_type(&self) -> ServiceType {
        ServiceType::AVTransport
    }

    fn subscription_scope(&self) -> SubscriptionScope {
        SubscriptionScope::PerSpeaker
    }

    fn service_endpoint_path(&self) -> &'static str {
        "/MediaRenderer/AVTransport/Event"
    }

    fn parse_event(
        &self,
        _speaker_id: &SpeakerId,
        event_xml: &str,
    ) -> Result<TypedEvent, StrategyError> {
        // Parse XML using AVTransportParser
        let parser = sonos_parser::services::av_transport::AVTransportParser::from_xml(event_xml)
            .map_err(|e| StrategyError::EventParseFailed(format!("Failed to parse AVTransport event: {}", e)))?;
        
        // Create TypedEvent with the parsed data
        Ok(TypedEvent::new(Box::new(parser)))
    }

    async fn create_subscription(
        &self,
        speaker: &Speaker,
        callback_url: String,
        config: &SubscriptionConfig,
    ) -> Result<Box<dyn Subscription>, StrategyError> {
        // Construct the full service endpoint URL
        let endpoint_url = format!("http://{}:1400{}", speaker.ip, self.service_endpoint_path());
        
        // Create UPnP subscription using the unified callback server
        let subscription = UPnPSubscription::create_subscription(
            speaker.id.clone(),
            self.service_type(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Speaker, SpeakerId};
    use std::net::IpAddr;

    #[test]
    fn test_av_transport_provider_new() {
        let provider = AVTransportProvider::new();
        assert_eq!(provider.service_type(), ServiceType::AVTransport);
    }

    #[test]
    fn test_av_transport_provider_default() {
        let provider = AVTransportProvider::default();
        assert_eq!(provider.service_type(), ServiceType::AVTransport);
    }

    #[test]
    fn test_service_configuration() {
        let provider = AVTransportProvider::new();
        
        assert_eq!(provider.service_type(), ServiceType::AVTransport);
        assert_eq!(provider.subscription_scope(), SubscriptionScope::PerSpeaker);
        assert_eq!(provider.service_endpoint_path(), "/MediaRenderer/AVTransport/Event");
    }

    #[test]
    fn test_endpoint_url_construction() {
        let provider = AVTransportProvider::new();
        let speaker = Speaker::new(
            SpeakerId::new("RINCON_123"),
            "192.168.1.100".parse::<IpAddr>().unwrap(),
            "Living Room".to_string(),
            "Living Room".to_string(),
        );
        
        let expected_path = "/MediaRenderer/AVTransport/Event";
        assert_eq!(provider.service_endpoint_path(), expected_path);
        
        // Verify the full URL construction logic
        let expected_url = format!("http://{}:1400{}", speaker.ip, expected_path);
        assert_eq!(expected_url, "http://192.168.1.100:1400/MediaRenderer/AVTransport/Event");
    }

    #[test]
    fn test_parse_event_valid_xml() {
        let provider = AVTransportProvider::new();
        let speaker_id = SpeakerId::new("test_speaker");
        
        let event_xml = r#"<e:propertyset xmlns:e="urn:schemas-upnp-org:event-1-0"><e:property><LastChange>&lt;Event xmlns="urn:schemas-upnp-org:metadata-1-0/AVT/"&gt;&lt;InstanceID val="0"&gt;&lt;TransportState val="PLAYING"/&gt;&lt;/InstanceID&gt;&lt;/Event&gt;</LastChange></e:property></e:propertyset>"#;
        
        let result = provider.parse_event(&speaker_id, event_xml);
        assert!(result.is_ok(), "Should successfully parse valid XML");
        
        let typed_event = result.unwrap();
        assert_eq!(typed_event.event_type(), "av_transport_event");
        assert_eq!(typed_event.service_type(), ServiceType::AVTransport);
    }

    #[test]
    fn test_parse_event_invalid_xml() {
        let provider = AVTransportProvider::new();
        let speaker_id = SpeakerId::new("test_speaker");
        
        let result = provider.parse_event(&speaker_id, "<invalid>xml</invalid>");
        assert!(result.is_err(), "Should fail on invalid XML");
        
        match result {
            Err(StrategyError::EventParseFailed(msg)) => {
                assert!(msg.contains("Failed to parse AVTransport event"));
            }
            _ => panic!("Expected EventParseFailed error"),
        }
    }

    #[test]
    fn test_provider_thread_safety() {
        let provider = AVTransportProvider::new();
        
        // Ensure AVTransportProvider implements Send + Sync
        fn assert_send_sync<T: Send + Sync>(_: T) {}
        assert_send_sync(provider);
    }

    #[test]
    fn test_provider_clone() {
        let provider = AVTransportProvider::new();
        let cloned = provider.clone();
        
        assert_eq!(provider.service_type(), cloned.service_type());
        assert_eq!(provider.subscription_scope(), cloned.subscription_scope());
        assert_eq!(provider.service_endpoint_path(), cloned.service_endpoint_path());
    }

    #[test]
    fn test_provider_debug() {
        let provider = AVTransportProvider::new();
        let debug_str = format!("{:?}", provider);
        assert!(debug_str.contains("AVTransportProvider"));
    }
}