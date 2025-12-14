//! Strategy trait for service-specific subscription and event parsing.
//!
//! The `SubscriptionStrategy` trait defines the interface for implementing service-specific
//! logic for UPnP event subscriptions. Each service type (AVTransport, RenderingControl,
//! ZoneGroupTopology) should have its own strategy implementation that handles:
//!
//! - Creating subscriptions with service-specific endpoints
//! - Parsing service-specific event XML into structured data
//! - Providing metadata about the service's subscription scope
//!
//! # Available Strategies
//!
//! This module provides the following strategy implementations:
//!
//! - [`AVTransportStrategy`]: Handles playback state changes and track information events
//!   from the AVTransport UPnP service. Emits `transport_state_changed` and `track_changed`
//!   events with parsed metadata.
//!
//! # Design Philosophy
//!
//! The strategy pattern keeps the broker service-agnostic. The broker handles subscription
//! lifecycle, event routing, and error handling, while strategies handle service-specific
//! details. This separation allows:
//!
//! - Adding new services without modifying the broker
//! - Testing services independently
//! - Reusing the broker for different UPnP devices
//!
//! # Implementation Guidelines
//!
//! Strategies should be stateless - all subscription state belongs in `Subscription` instances.
//! Strategies are shared across all subscriptions of their service type, so they must be
//! thread-safe (`Send + Sync`).
//!
//! # Using AVTransportStrategy
//!
//! ```rust,no_run
//! use sonos_stream::{EventBrokerBuilder, AVTransportStrategy, ServiceType, Speaker, SpeakerId};
//! use std::net::IpAddr;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Create a broker with the AVTransport strategy
//! let mut broker = EventBrokerBuilder::new()
//!     .with_strategy(Box::new(AVTransportStrategy::new()))
//!     .build().await?;
//!
//! // Subscribe to AVTransport events from a speaker
//! let speaker = Speaker::new(
//!     SpeakerId::new("RINCON_000XXX"),
//!     "192.168.1.100".parse::<IpAddr>()?,
//!     "Living Room".to_string(),
//!     "Living Room".to_string(),
//! );
//! broker.subscribe(&speaker, ServiceType::AVTransport).await?;
//! # Ok(())
//! # }
//! ```
//!
//! # Implementing a Custom Strategy
//!
//! ```rust,ignore
//! use sonos_stream::{
//!     SubscriptionStrategy, Subscription, Speaker, ServiceType, SubscriptionScope,
//!     SubscriptionConfig, StrategyError, ParsedEvent,
//! };
//!
//! struct MyCustomStrategy;
//!
//! impl SubscriptionStrategy for MyCustomStrategy {
//!     fn service_type(&self) -> ServiceType {
//!         ServiceType::RenderingControl
//!     }
//!
//!     fn subscription_scope(&self) -> SubscriptionScope {
//!         SubscriptionScope::PerSpeaker
//!     }
//!
//!     fn service_endpoint_path(&self) -> &'static str {
//!         "/MediaRenderer/RenderingControl/Event"
//!     }
//!
//!     fn parse_event(
//!         &self,
//!         speaker_id: &SpeakerId,
//!         event_xml: &str,
//!     ) -> Result<Vec<ParsedEvent>, StrategyError> {
//!         // Parse XML using quick-xml
//!         // Extract service-specific state variables
//!         // Return structured events
//!         todo!()
//!     }
//! }
//! ```

pub mod av_transport;

pub use av_transport::AVTransportStrategy;

use crate::error::StrategyError;
use crate::event::TypedEvent;
use async_trait::async_trait;
use crate::subscription::{Subscription, UPnPSubscription};
use crate::types::{ServiceType, SpeakerId, Speaker, SubscriptionConfig, SubscriptionScope};

/// Trait for implementing service-specific subscription and event parsing logic.
///
/// Each UPnP service type (AVTransport, RenderingControl, ZoneGroupTopology) requires
/// its own strategy implementation. The strategy is responsible for:
///
/// 1. Creating subscriptions with the correct service endpoint
/// 2. Parsing service-specific event XML into structured data
/// 3. Providing metadata about subscription scope
///
/// # Thread Safety
///
/// Implementations must be `Send + Sync` as they are shared across async tasks.
/// Strategies should be stateless - all subscription state belongs in `Subscription` instances.
///
/// # Service Endpoints
///
/// Each Sonos service has a specific endpoint path:
/// - AVTransport: `/MediaRenderer/AVTransport/Event`
/// - RenderingControl: `/MediaRenderer/RenderingControl/Event`
/// - ZoneGroupTopology: `/ZoneGroupTopology/Event`
///
/// Strategies should construct the full subscription URL using the speaker's IP address
/// and the appropriate endpoint path.
#[async_trait]
pub trait SubscriptionStrategy: Send + Sync {
    /// Get the service type this strategy handles.
    ///
    /// This is used by the broker to route subscription requests and events to the
    /// correct strategy implementation.
    fn service_type(&self) -> ServiceType;

    /// Get metadata about the subscription scope for this service.
    ///
    /// Returns `SubscriptionScope::PerSpeaker` if each speaker needs its own subscription,
    /// or `SubscriptionScope::NetworkWide` if only one subscription is needed for the
    /// entire network.
    ///
    /// # Note
    ///
    /// The broker does not use this information to optimize subscriptions - it treats
    /// all subscriptions uniformly. This metadata is provided for higher-level components
    /// that may want to implement optimization strategies.
    ///
    /// For example, a higher-level component might choose to subscribe to ZoneGroupTopology
    /// on only one speaker if it knows the service is network-wide.
    fn subscription_scope(&self) -> SubscriptionScope;

    /// Get the service endpoint path for this strategy.
    ///
    /// Returns the UPnP service endpoint path that will be appended to the speaker's
    /// base URL (http://IP:1400) to create the full subscription endpoint.
    ///
    /// # Examples
    ///
    /// - AVTransport: `/MediaRenderer/AVTransport/Event`
    /// - RenderingControl: `/MediaRenderer/RenderingControl/Event`
    /// - ZoneGroupTopology: `/ZoneGroupTopology/Event`
    fn service_endpoint_path(&self) -> &'static str;

    /// Create a new subscription for a speaker.
    ///
    /// This method has a default implementation that:
    /// 1. Constructs the service endpoint URL using the speaker's IP address and `service_endpoint_path()`
    /// 2. Calls `create_subscription_with_endpoint()` to handle the UPnP subscription protocol
    ///
    /// Most strategies should not need to override this method. Instead, implement
    /// `service_endpoint_path()` to provide the service-specific endpoint path.
    ///
    /// # Parameters
    ///
    /// - `speaker`: The speaker to subscribe to
    /// - `callback_url`: The base URL of the callback server (e.g., "http://192.168.1.100:3400")
    /// - `config`: Configuration including timeout and full callback URL
    ///
    /// # Errors
    ///
    /// Returns `StrategyError::SubscriptionCreationFailed` if the subscription request fails.
    /// Returns `StrategyError::NetworkError` if a network error occurs.
    /// Returns `StrategyError::InvalidConfiguration` if the configuration is invalid.
    async fn create_subscription(
        &self,
        speaker: &Speaker,
        callback_url: String,
        config: &SubscriptionConfig,
    ) -> Result<Box<dyn Subscription>, StrategyError> {
        // Construct the service endpoint URL using the speaker's IP and service path
        let endpoint_url = format!("http://{}:1400{}", speaker.ip, self.service_endpoint_path());
        
        // Use the helper method to create the UPnP subscription
        self.create_subscription_with_endpoint(speaker, callback_url, config, &endpoint_url).await
    }

    /// Parse a raw UPnP event into a typed event.
    ///
    /// This method receives the raw XML body of a UPnP NOTIFY request and should:
    /// 1. Parse the XML to extract state variables
    /// 2. Convert the state variables into service-specific event types
    /// 3. Return exactly one typed event per UPnP notification
    ///
    /// # Parameters
    ///
    /// - `speaker_id`: The ID of the speaker that sent the event
    /// - `event_xml`: The raw XML body from the NOTIFY request
    ///
    /// # Returns
    ///
    /// A single `TypedEvent` instance containing strategy-specific event data.
    /// Each UPnP notification produces exactly one typed event, even if the XML
    /// contains multiple state variable changes (they are consolidated into a
    /// single event object).
    ///
    /// # Errors
    ///
    /// Returns `StrategyError::EventParseFailed` if the XML is malformed or cannot be parsed.
    /// The error should include diagnostic information to help debug parsing issues.
    ///
    /// # UPnP Event Format
    ///
    /// UPnP events follow this XML structure:
    ///
    /// ```xml
    /// <e:propertyset xmlns:e="urn:schemas-upnp-org:event-1-0">
    ///   <e:property>
    ///     <LastChange>
    ///       <!-- Service-specific XML with state variables -->
    ///     </LastChange>
    ///   </e:property>
    /// </e:propertyset>
    /// ```
    ///
    /// The `LastChange` element contains service-specific XML that varies by service type.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// fn parse_event(
    ///     &self,
    ///     speaker_id: &SpeakerId,
    ///     event_xml: &str,
    /// ) -> Result<TypedEvent, StrategyError> {
    ///     use quick_xml::Reader;
    ///     
    ///     let mut reader = Reader::from_str(event_xml);
    ///     // Parse XML...
    ///     // Extract state variables...
    ///     // Create strategy-specific event data...
    ///     // Wrap in TypedEvent...
    ///     
    ///     Ok(typed_event)
    /// }
    /// ```
    fn parse_event(
        &self,
        speaker_id: &SpeakerId,
        event_xml: &str,
    ) -> Result<TypedEvent, StrategyError>;

    /// Helper method to create a UPnP subscription with a specific endpoint URL.
    ///
    /// This default implementation handles the standard UPnP SUBSCRIBE protocol:
    /// 1. Sends a SUBSCRIBE request with proper headers and timeout
    /// 2. Parses the response to extract subscription ID and timeout
    /// 3. Creates and returns a UPnPSubscription instance
    ///
    /// This method is provided as a default implementation to avoid code duplication
    /// across strategy implementations. Strategies can override this method if they
    /// need custom subscription behavior.
    ///
    /// # Parameters
    ///
    /// - `speaker`: The speaker to subscribe to
    /// - `callback_url`: The callback URL for receiving events
    /// - `config`: Configuration including timeout settings
    /// - `endpoint_url`: The full endpoint URL for the subscription
    ///
    /// # Returns
    ///
    /// A boxed `Subscription` instance (specifically a `UPnPSubscription`) that can be
    /// used to manage the subscription lifecycle.
    ///
    /// # Errors
    ///
    /// Returns `StrategyError::SubscriptionCreationFailed` if the subscription request fails.
    /// Returns `StrategyError::NetworkError` if a network error occurs.
    /// Returns `StrategyError::InvalidConfiguration` if the configuration is invalid.
    ///
    /// # UPnP SUBSCRIBE Protocol
    ///
    /// The method sends a SUBSCRIBE request with these headers:
    /// - `HOST`: Extracted from the endpoint URL
    /// - `CALLBACK`: The callback URL wrapped in angle brackets
    /// - `NT`: Set to "upnp:event" for UPnP event notifications
    /// - `TIMEOUT`: Formatted as "Second-{timeout_seconds}"
    ///
    /// The response should contain:
    /// - `SID`: The subscription ID assigned by the device
    /// - `TIMEOUT`: The actual timeout granted by the device
    async fn create_subscription_with_endpoint(
        &self,
        speaker: &Speaker,
        callback_url: String,
        config: &SubscriptionConfig,
        endpoint_url: &str,
    ) -> Result<Box<dyn Subscription>, StrategyError> {
        // Use UPnPSubscription's create_subscription method to handle the SUBSCRIBE request
        let subscription = UPnPSubscription::create_subscription(
            speaker.id.clone(),
            self.service_type(),
            endpoint_url.to_string(),
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

/// Extract host header from a URL.
///
/// This helper function extracts the host and port from a URL for use in HTTP headers.
/// It handles both URLs with and without explicit ports.
///
/// # Arguments
///
/// * `url` - The URL to extract the host from
///
/// # Returns
///
/// The host and port as a string (e.g., "192.168.1.100:1400"), or None if the URL is invalid.
///
/// # Examples
///
/// ```rust,ignore
/// assert_eq!(
///     extract_host_from_url("http://192.168.1.100:1400/path"),
///     Some("192.168.1.100:1400".to_string())
/// );
/// ```
#[allow(dead_code)]
fn extract_host_from_url(url_str: &str) -> Option<String> {
    let url = url::Url::parse(url_str).ok()?;
    let host = url.host_str()?;
    
    if let Some(port) = url.port() {
        Some(format!("{}:{}", host, port))
    } else {
        Some(host.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mock strategy for testing the helper method
    struct MockStrategy;

    impl SubscriptionStrategy for MockStrategy {
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
            _event_xml: &str,
        ) -> Result<TypedEvent, StrategyError> {
            // Create a mock event data for testing
            use crate::event::{EventData, TypedEvent};
            use std::any::Any;
            
            #[derive(Debug, Clone)]
            struct MockEventData;
            
            impl EventData for MockEventData {
                fn event_type(&self) -> &str {
                    "mock_event"
                }
                
                fn service_type(&self) -> crate::types::ServiceType {
                    crate::types::ServiceType::AVTransport
                }
                
                fn as_any(&self) -> &dyn Any {
                    self
                }
                
                fn clone_box(&self) -> Box<dyn EventData> {
                    Box::new(self.clone())
                }
            }
            
            Ok(TypedEvent::new(Box::new(MockEventData)))
        }
    }





    #[test]
    fn test_mock_strategy_service_type() {
        let strategy = MockStrategy;
        assert_eq!(strategy.service_type(), ServiceType::AVTransport);
        assert_eq!(strategy.subscription_scope(), SubscriptionScope::PerSpeaker);
    }

    #[test]
    fn test_mock_strategy_parse_event() {
        let strategy = MockStrategy;
        let result = strategy.parse_event(&SpeakerId::new("test"), "<xml></xml>");
        assert!(result.is_ok());
        
        let typed_event = result.unwrap();
        assert_eq!(typed_event.event_type(), "mock_event");
        assert_eq!(typed_event.service_type(), ServiceType::AVTransport);
    }

    #[test]
    fn test_extract_host_from_url() {
        // Test with port
        assert_eq!(
            extract_host_from_url("http://192.168.1.100:1400/MediaRenderer/AVTransport/Event"),
            Some("192.168.1.100:1400".to_string())
        );

        // Test without port
        assert_eq!(
            extract_host_from_url("http://example.com/path"),
            Some("example.com".to_string())
        );

        // Test with HTTPS and port
        assert_eq!(
            extract_host_from_url("https://test.local:8080/service"),
            Some("test.local:8080".to_string())
        );

        // Test invalid URL
        assert_eq!(extract_host_from_url("invalid-url"), None);

        // Test URL without host
        assert_eq!(extract_host_from_url("file:///path/to/file"), None);
    }

    // Note: We can't easily test create_subscription_with_endpoint without a real server
    // or extensive mocking, but the structure is validated by compilation and the
    // individual components are tested above.
}
