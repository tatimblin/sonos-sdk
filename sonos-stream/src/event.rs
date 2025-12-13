//! Event types for the sonos-stream crate.
//!
//! This module defines the event types emitted by the broker and the parsed event
//! types returned by strategies.

use std::collections::HashMap;

use crate::types::{ServiceType, SpeakerId};
use sonos_parser::services::av_transport::AVTransportParser;

/// Events emitted by the broker.
///
/// These events represent the lifecycle of subscriptions and the parsed events
/// from services. Applications should handle these events to track subscription
/// state and process service events.
///
/// # Examples
///
/// ```rust,ignore
/// use sonos_stream::{Event, EventBroker};
///
/// let mut event_rx = broker.event_stream();
/// while let Some(event) = event_rx.recv().await {
///     match event {
///         Event::SubscriptionEstablished { speaker_id, service_type, .. } => {
///             println!("Subscribed to {:?} on {}", service_type, speaker_id.as_str());
///         }
///         Event::ServiceEvent { speaker_id, event, .. } => {
///             println!("Event from {}: {:?}", speaker_id.as_str(), event);
///         }
///         Event::ParseError { error, .. } => {
///             eprintln!("Parse error: {}", error);
///         }
///         _ => {}
///     }
/// }
/// ```
#[derive(Debug, Clone)]
pub enum Event {
    /// A subscription was successfully established.
    ///
    /// This event is emitted when a subscription is created successfully and is ready
    /// to receive events from the speaker.
    SubscriptionEstablished {
        /// The speaker ID
        speaker_id: SpeakerId,
        /// The service type
        service_type: ServiceType,
        /// The UPnP subscription ID
        subscription_id: String,
    },

    /// A subscription failed to establish.
    ///
    /// This event is emitted when a subscription creation fails. The error message
    /// provides details about why the subscription failed.
    SubscriptionFailed {
        /// The speaker ID
        speaker_id: SpeakerId,
        /// The service type
        service_type: ServiceType,
        /// Error message describing the failure
        error: String,
    },

    /// A subscription was successfully renewed.
    ///
    /// This event is emitted when a subscription is automatically renewed before
    /// it expires. Renewals happen in the background without user intervention.
    SubscriptionRenewed {
        /// The speaker ID
        speaker_id: SpeakerId,
        /// The service type
        service_type: ServiceType,
    },

    /// A subscription expired after all renewal attempts failed.
    ///
    /// This event is emitted when a subscription fails to renew after all retry
    /// attempts have been exhausted. The subscription is removed from the broker.
    SubscriptionExpired {
        /// The speaker ID
        speaker_id: SpeakerId,
        /// The service type
        service_type: ServiceType,
    },

    /// A subscription was removed (unsubscribed).
    ///
    /// This event is emitted when a subscription is explicitly unsubscribed by
    /// the application.
    SubscriptionRemoved {
        /// The speaker ID
        speaker_id: SpeakerId,
        /// The service type
        service_type: ServiceType,
    },

    /// A parsed event from a service.
    ///
    /// This event is emitted when a UPnP event notification is received and
    /// successfully parsed by the strategy. The parsed event contains
    /// service-specific data.
    ServiceEvent {
        /// The speaker ID
        speaker_id: SpeakerId,
        /// The service type
        service_type: ServiceType,
        /// The parsed event data
        event: ParsedEvent,
    },

    /// An error occurred parsing an event.
    ///
    /// This event is emitted when a UPnP event notification is received but
    /// the strategy fails to parse it. The error message provides details
    /// about the parse failure. Event processing continues for other events.
    ParseError {
        /// The speaker ID
        speaker_id: SpeakerId,
        /// The service type
        service_type: ServiceType,
        /// Error message describing the parse failure
        error: String,
    },
}

/// Parsed event data from a service.
///
/// This enum represents the structured data extracted from UPnP event notifications.
/// Strategies parse raw XML events into this format for consumption by applications.
///
/// # Extensibility
///
/// The `Custom` variant allows strategies to return arbitrary key-value data without
/// requiring changes to this enum. Service-specific crates can define their own
/// strongly-typed event structures and convert them to/from this representation.
///
/// # Examples
///
/// ```rust
/// use sonos_stream::ParsedEvent;
/// use std::collections::HashMap;
///
/// // Create a custom event
/// let event = ParsedEvent::custom(
///     "volume_changed",
///     HashMap::from([
///         ("volume".to_string(), "50".to_string()),
///         ("channel".to_string(), "Master".to_string()),
///     ]),
/// );
///
/// assert_eq!(event.event_type(), "volume_changed");
/// assert_eq!(event.data().unwrap().get("volume").map(|s| s.as_str()), Some("50"));
/// ```
#[derive(Debug, Clone)]
pub enum ParsedEvent {
    /// A custom event with arbitrary key-value data.
    ///
    /// This variant allows strategies to return any structured data without
    /// requiring changes to the core event types. The `event_type` field
    /// identifies the type of event, and the `data` field contains the
    /// event-specific data as key-value pairs.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// ParsedEvent::Custom {
    ///     event_type: "transport_state_changed".to_string(),
    ///     data: HashMap::from([
    ///         ("state".to_string(), "PLAYING".to_string()),
    ///         ("track".to_string(), "1".to_string()),
    ///     ]),
    /// }
    /// ```
    Custom {
        /// The type of event (e.g., "volume_changed", "transport_state_changed")
        event_type: String,
        /// Event-specific data as key-value pairs
        data: HashMap<String, String>,
    },

    /// A typed AVTransport event with strongly-typed data.
    ///
    /// This variant provides direct access to parsed AVTransport data without
    /// the overhead of JSON serialization and HashMap conversion. The parsed
    /// data maintains all type information and provides convenient accessor
    /// methods for common fields.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// ParsedEvent::AVTransport {
    ///     event_type: "av_transport_event".to_string(),
    ///     data: parsed_av_transport_data,
    /// }
    /// ```
    AVTransport {
        /// The type of event (e.g., "av_transport_event")
        event_type: String,
        /// Strongly-typed AVTransport data
        data: AVTransportParser,
    },
}

impl ParsedEvent {
    /// Create a new custom event.
    ///
    /// # Arguments
    ///
    /// * `event_type` - The type of event (e.g., "volume_changed")
    /// * `data` - Event-specific data as key-value pairs
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

    /// Create a new AVTransport event with typed data.
    ///
    /// # Arguments
    ///
    /// * `event_type` - The type of event (e.g., "av_transport_event")
    /// * `data` - Parsed AVTransport data
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use sonos_stream::ParsedEvent;
    /// use sonos_parser::services::av_transport::AVTransportParser;
    ///
    /// let parsed_data = AVTransportParser::from_xml(xml_string)?;
    /// let event = ParsedEvent::av_transport("av_transport_event", parsed_data);
    /// ```
    pub fn av_transport(event_type: impl Into<String>, data: AVTransportParser) -> Self {
        Self::AVTransport {
            event_type: event_type.into(),
            data,
        }
    }

    /// Get the event type.
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
            Self::AVTransport { event_type, .. } => event_type,
        }
    }

    /// Get the event data as HashMap (for Custom events only).
    ///
    /// Returns `None` for AVTransport events since they use typed data.
    /// Use `av_transport_data()` to access typed AVTransport data.
    ///
    /// # Example
    ///
    /// ```rust
    /// use sonos_stream::ParsedEvent;
    /// use std::collections::HashMap;
    ///
    /// let data = HashMap::from([("key".to_string(), "value".to_string())]);
    /// let event = ParsedEvent::custom("test", data.clone());
    /// assert_eq!(event.data().unwrap().get("key").map(|s| s.as_str()), Some("value"));
    /// ```
    pub fn data(&self) -> Option<&HashMap<String, String>> {
        match self {
            Self::Custom { data, .. } => Some(data),
            Self::AVTransport { .. } => None,
        }
    }

    /// Get the typed AVTransport data (for AVTransport events only).
    ///
    /// Returns `None` for Custom events. Use `data()` to access HashMap data
    /// for Custom events.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use sonos_stream::ParsedEvent;
    ///
    /// if let Some(av_data) = event.av_transport_data() {
    ///     println!("Transport state: {}", av_data.transport_state());
    ///     if let Some(title) = av_data.track_title() {
    ///         println!("Track title: {}", title);
    ///     }
    /// }
    /// ```
    pub fn av_transport_data(&self) -> Option<&AVTransportParser> {
        match self {
            Self::Custom { .. } => None,
            Self::AVTransport { data, .. } => Some(data),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_debug() {
        let event = Event::SubscriptionEstablished {
            speaker_id: SpeakerId::new("speaker1"),
            service_type: ServiceType::AVTransport,
            subscription_id: "test-sub-123".to_string(),
        };

        let debug_str = format!("{event:?}");
        assert!(debug_str.contains("SubscriptionEstablished"));
        assert!(debug_str.contains("speaker1"));
        assert!(debug_str.contains("AVTransport"));
        assert!(debug_str.contains("test-sub-123"));
    }

    #[test]
    fn test_event_clone() {
        let event = Event::SubscriptionFailed {
            speaker_id: SpeakerId::new("speaker1"),
            service_type: ServiceType::RenderingControl,
            error: "connection failed".to_string(),
        };

        let cloned = event.clone();

        match (event, cloned) {
            (
                Event::SubscriptionFailed {
                    speaker_id: s1,
                    service_type: st1,
                    error: e1,
                },
                Event::SubscriptionFailed {
                    speaker_id: s2,
                    service_type: st2,
                    error: e2,
                },
            ) => {
                assert_eq!(s1, s2);
                assert_eq!(st1, st2);
                assert_eq!(e1, e2);
            }
            _ => panic!("Event type mismatch after clone"),
        }
    }

    #[test]
    fn test_event_subscription_established() {
        let event = Event::SubscriptionEstablished {
            speaker_id: SpeakerId::new("RINCON_123"),
            service_type: ServiceType::AVTransport,
            subscription_id: "uuid:sub-123".to_string(),
        };

        match event {
            Event::SubscriptionEstablished {
                speaker_id,
                service_type,
                subscription_id,
            } => {
                assert_eq!(speaker_id.as_str(), "RINCON_123");
                assert_eq!(service_type, ServiceType::AVTransport);
                assert_eq!(subscription_id, "uuid:sub-123");
            }
            _ => panic!("Expected SubscriptionEstablished"),
        }
    }

    #[test]
    fn test_event_subscription_failed() {
        let event = Event::SubscriptionFailed {
            speaker_id: SpeakerId::new("RINCON_456"),
            service_type: ServiceType::RenderingControl,
            error: "Network timeout".to_string(),
        };

        match event {
            Event::SubscriptionFailed {
                speaker_id,
                service_type,
                error,
            } => {
                assert_eq!(speaker_id.as_str(), "RINCON_456");
                assert_eq!(service_type, ServiceType::RenderingControl);
                assert_eq!(error, "Network timeout");
            }
            _ => panic!("Expected SubscriptionFailed"),
        }
    }

    #[test]
    fn test_event_subscription_renewed() {
        let event = Event::SubscriptionRenewed {
            speaker_id: SpeakerId::new("RINCON_789"),
            service_type: ServiceType::ZoneGroupTopology,
        };

        match event {
            Event::SubscriptionRenewed {
                speaker_id,
                service_type,
            } => {
                assert_eq!(speaker_id.as_str(), "RINCON_789");
                assert_eq!(service_type, ServiceType::ZoneGroupTopology);
            }
            _ => panic!("Expected SubscriptionRenewed"),
        }
    }

    #[test]
    fn test_event_subscription_expired() {
        let event = Event::SubscriptionExpired {
            speaker_id: SpeakerId::new("RINCON_ABC"),
            service_type: ServiceType::AVTransport,
        };

        match event {
            Event::SubscriptionExpired {
                speaker_id,
                service_type,
            } => {
                assert_eq!(speaker_id.as_str(), "RINCON_ABC");
                assert_eq!(service_type, ServiceType::AVTransport);
            }
            _ => panic!("Expected SubscriptionExpired"),
        }
    }

    #[test]
    fn test_event_subscription_removed() {
        let event = Event::SubscriptionRemoved {
            speaker_id: SpeakerId::new("RINCON_DEF"),
            service_type: ServiceType::RenderingControl,
        };

        match event {
            Event::SubscriptionRemoved {
                speaker_id,
                service_type,
            } => {
                assert_eq!(speaker_id.as_str(), "RINCON_DEF");
                assert_eq!(service_type, ServiceType::RenderingControl);
            }
            _ => panic!("Expected SubscriptionRemoved"),
        }
    }

    #[test]
    fn test_event_service_event() {
        let parsed_event = ParsedEvent::custom(
            "test_event",
            HashMap::from([("key".to_string(), "value".to_string())]),
        );

        let event = Event::ServiceEvent {
            speaker_id: SpeakerId::new("RINCON_GHI"),
            service_type: ServiceType::AVTransport,
            event: parsed_event,
        };

        match event {
            Event::ServiceEvent {
                speaker_id,
                service_type,
                event,
            } => {
                assert_eq!(speaker_id.as_str(), "RINCON_GHI");
                assert_eq!(service_type, ServiceType::AVTransport);
                assert_eq!(event.event_type(), "test_event");
            }
            _ => panic!("Expected ServiceEvent"),
        }
    }

    #[test]
    fn test_event_parse_error() {
        let event = Event::ParseError {
            speaker_id: SpeakerId::new("RINCON_JKL"),
            service_type: ServiceType::ZoneGroupTopology,
            error: "Invalid XML".to_string(),
        };

        match event {
            Event::ParseError {
                speaker_id,
                service_type,
                error,
            } => {
                assert_eq!(speaker_id.as_str(), "RINCON_JKL");
                assert_eq!(service_type, ServiceType::ZoneGroupTopology);
                assert_eq!(error, "Invalid XML");
            }
            _ => panic!("Expected ParseError"),
        }
    }

    #[test]
    fn test_parsed_event_custom() {
        let data = HashMap::from([
            ("state".to_string(), "PLAYING".to_string()),
            ("track".to_string(), "1".to_string()),
        ]);

        let event = ParsedEvent::custom("state_changed", data.clone());

        assert_eq!(event.event_type(), "state_changed");
        assert_eq!(event.data().unwrap().get("state").map(|s| s.as_str()), Some("PLAYING"));
        assert_eq!(event.data().unwrap().get("track").map(|s| s.as_str()), Some("1"));
        assert!(event.av_transport_data().is_none());
    }

    #[test]
    fn test_parsed_event_empty_data() {
        let event = ParsedEvent::custom("empty_event", HashMap::new());

        assert_eq!(event.event_type(), "empty_event");
        assert!(event.data().unwrap().is_empty());
        assert!(event.av_transport_data().is_none());
    }

    #[test]
    fn test_parsed_event_clone() {
        let data = HashMap::from([("key".to_string(), "value".to_string())]);
        let event = ParsedEvent::custom("test", data);

        let cloned = event.clone();

        assert_eq!(event.event_type(), cloned.event_type());
        assert_eq!(event.data(), cloned.data());
    }

    #[test]
    fn test_parsed_event_debug() {
        let event = ParsedEvent::custom("debug_test", HashMap::new());
        let debug_str = format!("{event:?}");
        assert!(debug_str.contains("Custom"));
        assert!(debug_str.contains("debug_test"));
    }

    #[test]
    fn test_parsed_event_av_transport() {
        let xml = r#"<e:propertyset xmlns:e="urn:schemas-upnp-org:event-1-0"><e:property><LastChange>&lt;Event xmlns=&quot;urn:schemas-upnp-org:metadata-1-0/AVT/&quot;&gt;&lt;InstanceID val=&quot;0&quot;&gt;&lt;TransportState val=&quot;PLAYING&quot;/&gt;&lt;/InstanceID&gt;&lt;/Event&gt;</LastChange></e:property></e:propertyset>"#;
        
        let parsed_data = AVTransportParser::from_xml(xml).unwrap();
        let event = ParsedEvent::av_transport("av_transport_event", parsed_data);

        assert_eq!(event.event_type(), "av_transport_event");
        assert!(event.data().is_none());
        
        let av_data = event.av_transport_data().unwrap();
        assert_eq!(av_data.transport_state(), "PLAYING");
    }

    #[test]
    fn test_parsed_event_av_transport_clone() {
        let xml = r#"<e:propertyset xmlns:e="urn:schemas-upnp-org:event-1-0"><e:property><LastChange>&lt;Event xmlns=&quot;urn:schemas-upnp-org:metadata-1-0/AVT/&quot;&gt;&lt;InstanceID val=&quot;0&quot;&gt;&lt;TransportState val=&quot;STOPPED&quot;/&gt;&lt;/InstanceID&gt;&lt;/Event&gt;</LastChange></e:property></e:propertyset>"#;
        
        let parsed_data = AVTransportParser::from_xml(xml).unwrap();
        let event = ParsedEvent::av_transport("test_event", parsed_data);

        let cloned = event.clone();

        assert_eq!(event.event_type(), cloned.event_type());
        assert_eq!(
            event.av_transport_data().unwrap().transport_state(),
            cloned.av_transport_data().unwrap().transport_state()
        );
    }

    #[test]
    fn test_parsed_event_av_transport_debug() {
        let xml = r#"<e:propertyset xmlns:e="urn:schemas-upnp-org:event-1-0"><e:property><LastChange>&lt;Event xmlns=&quot;urn:schemas-upnp-org:metadata-1-0/AVT/&quot;&gt;&lt;InstanceID val=&quot;0&quot;&gt;&lt;TransportState val=&quot;PAUSED_PLAYBACK&quot;/&gt;&lt;/InstanceID&gt;&lt;/Event&gt;</LastChange></e:property></e:propertyset>"#;
        
        let parsed_data = AVTransportParser::from_xml(xml).unwrap();
        let event = ParsedEvent::av_transport("debug_test", parsed_data);
        
        let debug_str = format!("{event:?}");
        assert!(debug_str.contains("AVTransport"));
        assert!(debug_str.contains("debug_test"));
    }
}
