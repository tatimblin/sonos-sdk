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
//! ## Example Implementation
//!
//! ```rust,ignore
//! use sonos_stream::{
//!     SubscriptionStrategy, Subscription, Speaker, ServiceType, SubscriptionScope,
//!     SubscriptionConfig, StrategyError, ParsedEvent,
//! };
//!
//! struct AVTransportStrategy;
//!
//! impl SubscriptionStrategy for AVTransportStrategy {
//!     fn service_type(&self) -> ServiceType {
//!         ServiceType::AVTransport
//!     }
//!
//!     fn subscription_scope(&self) -> SubscriptionScope {
//!         SubscriptionScope::PerSpeaker
//!     }
//!
//!     fn create_subscription(
//!         &self,
//!         speaker: &Speaker,
//!         callback_url: String,
//!         config: &SubscriptionConfig,
//!     ) -> Result<Box<dyn Subscription>, StrategyError> {
//!         // Create HTTP client
//!         // Send SUBSCRIBE request to speaker's AVTransport endpoint
//!         // Parse response to get subscription ID and timeout
//!         // Return subscription instance
//!         todo!()
//!     }
//!
//!     fn parse_event(
//!         &self,
//!         speaker_id: &SpeakerId,
//!         event_xml: &str,
//!     ) -> Result<Vec<ParsedEvent>, StrategyError> {
//!         // Parse XML using quick-xml
//!         // Extract AVTransport-specific state variables
//!         // Return structured events
//!         todo!()
//!     }
//! }
//! ```

use crate::error::StrategyError;
use crate::subscription::Subscription;
use crate::types::{ServiceType, SpeakerId, Speaker, SubscriptionConfig, SubscriptionScope};
use std::collections::HashMap;

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

    /// Create a new subscription for a speaker.
    ///
    /// This method should:
    /// 1. Construct the service endpoint URL using the speaker's IP address
    /// 2. Send a SUBSCRIBE request with the callback URL and timeout
    /// 3. Parse the response to extract the subscription ID and actual timeout
    /// 4. Create and return a `Subscription` instance that tracks the subscription state
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
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// fn create_subscription(
    ///     &self,
    ///     speaker: &Speaker,
    ///     callback_url: String,
    ///     config: &SubscriptionConfig,
    /// ) -> Result<Box<dyn Subscription>, StrategyError> {
    ///     let endpoint = format!("http://{}:1400/MediaRenderer/AVTransport/Event", speaker.ip);
    ///     let full_callback = format!("{}/notify/{}", callback_url, uuid::Uuid::new_v4());
    ///     
    ///     // Send SUBSCRIBE request...
    ///     // Parse response...
    ///     // Create subscription instance...
    ///     
    ///     Ok(Box::new(subscription))
    /// }
    /// ```
    fn create_subscription(
        &self,
        speaker: &Speaker,
        callback_url: String,
        config: &SubscriptionConfig,
    ) -> Result<Box<dyn Subscription>, StrategyError>;

    /// Parse a raw UPnP event into structured event data.
    ///
    /// This method receives the raw XML body of a UPnP NOTIFY request and should:
    /// 1. Parse the XML to extract state variables
    /// 2. Convert the state variables into service-specific event types
    /// 3. Return a vector of parsed events (one event may contain multiple state changes)
    ///
    /// # Parameters
    ///
    /// - `speaker_id`: The ID of the speaker that sent the event
    /// - `event_xml`: The raw XML body from the NOTIFY request
    ///
    /// # Returns
    ///
    /// A vector of `ParsedEvent` instances. Multiple events may be returned if the XML
    /// contains multiple state variable changes. An empty vector is valid if the event
    /// contains no actionable state changes.
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
    /// ) -> Result<Vec<ParsedEvent>, StrategyError> {
    ///     use quick_xml::Reader;
    ///     
    ///     let mut reader = Reader::from_str(event_xml);
    ///     // Parse XML...
    ///     // Extract state variables...
    ///     // Create ParsedEvent instances...
    ///     
    ///     Ok(events)
    /// }
    /// ```
    fn parse_event(
        &self,
        speaker_id: &SpeakerId,
        event_xml: &str,
    ) -> Result<Vec<ParsedEvent>, StrategyError>;
}

/// Parsed event data from a service.
///
/// This enum represents structured event data after parsing the raw UPnP XML.
/// Service-specific strategies can extend this enum with their own event types,
/// or use the `Custom` variant for flexibility.
///
/// # Design Note
///
/// This is intentionally kept simple in the core crate. Service-specific crates
/// (like `sonos-av-transport`) will define their own rich event types and convert
/// them to/from this representation.
#[derive(Debug, Clone)]
pub enum ParsedEvent {
    /// A custom event with arbitrary key-value data.
    ///
    /// This variant provides maximum flexibility for service implementations.
    /// The `event_type` identifies the kind of event, and `data` contains
    /// the event-specific fields.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// ParsedEvent::Custom {
    ///     event_type: "transport_state_changed".to_string(),
    ///     data: HashMap::from([
    ///         ("state".to_string(), "PLAYING".to_string()),
    ///         ("track_uri".to_string(), "x-sonos-spotify:...".to_string()),
    ///     ]),
    /// }
    /// ```
    Custom {
        /// The type of event (e.g., "transport_state_changed", "volume_changed")
        event_type: String,
        /// Event-specific data as key-value pairs
        data: HashMap<String, String>,
    },
}

impl ParsedEvent {
    /// Create a new custom event.
    ///
    /// # Example
    ///
    /// ```rust
    /// use sonos_stream::ParsedEvent;
    /// use std::collections::HashMap;
    ///
    /// let event = ParsedEvent::custom(
    ///     "volume_changed",
    ///     HashMap::from([("volume".to_string(), "50".to_string())]),
    /// );
    /// ```
    pub fn custom(event_type: impl Into<String>, data: HashMap<String, String>) -> Self {
        Self::Custom {
            event_type: event_type.into(),
            data,
        }
    }

    /// Get the event type as a string slice.
    ///
    /// # Example
    ///
    /// ```rust
    /// use sonos_stream::ParsedEvent;
    /// use std::collections::HashMap;
    ///
    /// let event = ParsedEvent::custom("test_event", HashMap::new());
    /// assert_eq!(event.event_type(), "test_event");
    /// ```
    pub fn event_type(&self) -> &str {
        match self {
            Self::Custom { event_type, .. } => event_type,
        }
    }

    /// Get the event data.
    ///
    /// # Example
    ///
    /// ```rust
    /// use sonos_stream::ParsedEvent;
    /// use std::collections::HashMap;
    ///
    /// let data = HashMap::from([("key".to_string(), "value".to_string())]);
    /// let event = ParsedEvent::custom("test", data.clone());
    /// assert_eq!(event.data().get("key").map(|s| s.as_str()), Some("value"));
    /// ```
    pub fn data(&self) -> &HashMap<String, String> {
        match self {
            Self::Custom { data, .. } => data,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parsed_event_custom_creation() {
        let data = HashMap::from([
            ("state".to_string(), "PLAYING".to_string()),
            ("volume".to_string(), "50".to_string()),
        ]);

        let event = ParsedEvent::custom("state_changed", data.clone());

        assert_eq!(event.event_type(), "state_changed");
        assert_eq!(event.data().get("state").map(|s| s.as_str()), Some("PLAYING"));
        assert_eq!(event.data().get("volume").map(|s| s.as_str()), Some("50"));
    }

    #[test]
    fn test_parsed_event_empty_data() {
        let event = ParsedEvent::custom("empty_event", HashMap::new());

        assert_eq!(event.event_type(), "empty_event");
        assert!(event.data().is_empty());
    }

    #[test]
    fn test_parsed_event_clone() {
        let data = HashMap::from([("key".to_string(), "value".to_string())]);
        let event = ParsedEvent::custom("test", data);

        let cloned = event.clone();
        assert_eq!(cloned.event_type(), event.event_type());
        assert_eq!(cloned.data(), event.data());
    }

    #[test]
    fn test_parsed_event_debug() {
        let event = ParsedEvent::custom("debug_test", HashMap::new());
        let debug_str = format!("{:?}", event);
        assert!(debug_str.contains("Custom"));
        assert!(debug_str.contains("debug_test"));
    }
}
