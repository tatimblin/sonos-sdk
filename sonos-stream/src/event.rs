//! Event types for the sonos-stream crate.

use crate::types::{ServiceType, SpeakerId};
use std::time::SystemTime;

/// Container for strategy-specific typed event data.
///
/// Holds parser instances directly that can be downcast to specific parser types.
#[derive(Debug)]
pub struct TypedEvent {
    /// The actual parsed data directly (parser instances)
    data: Box<dyn std::any::Any + Send + Sync>,
    /// The event type identifier
    event_type: &'static str,
    /// The service type that produced this event
    service_type: ServiceType,
}

impl TypedEvent {
    /// Create a new typed event from a parser instance.
    ///
    /// # Parameters
    /// 
    /// * `parser` - The parser instance containing the parsed event data
    /// * `event_type` - The event type identifier (e.g., "av_transport_event")
    /// * `service_type` - The service type that produced this event
    pub fn new_parser<T: 'static + Send + Sync>(
        parser: T,
        event_type: &'static str,
        service_type: ServiceType,
    ) -> Self {
        Self {
            data: Box::new(parser),
            event_type,
            service_type,
        }
    }
    
    /// Get the event type identifier.
    pub fn event_type(&self) -> &str {
        self.event_type
    }
    
    /// Get the service type that produced this event.
    pub fn service_type(&self) -> ServiceType {
        self.service_type
    }
    
    /// Downcast to a specific parser type.
    pub fn downcast_ref<T: 'static>(&self) -> Option<&T> {
        self.data.downcast_ref::<T>()
    }
}

impl Clone for TypedEvent {
    fn clone(&self) -> Self {
        // Note: This implementation requires that the contained parser types implement Clone.
        // Since we can't clone arbitrary Any types, this will panic if the parser doesn't implement Clone.
        // This is a limitation of the simplified design, but matches the existing behavior expectation.
        panic!("TypedEvent cloning is not supported in the simplified design. Parser types should be accessed via downcasting instead of cloning the entire TypedEvent.")
    }
}



/// Events emitted by the broker.
#[derive(Debug, Clone)]
pub enum Event {
    /// Subscription successfully established.
    SubscriptionEstablished {
        speaker_id: SpeakerId,
        service_type: ServiceType,
        subscription_id: String,
        /// Timestamp when the subscription was established
        timestamp: SystemTime,
    },

    /// Subscription failed to establish.
    SubscriptionFailed {
        speaker_id: SpeakerId,
        service_type: ServiceType,
        error: String,
        /// Timestamp when the subscription failure occurred
        timestamp: SystemTime,
    },

    /// Subscription successfully renewed.
    SubscriptionRenewed {
        speaker_id: SpeakerId,
        service_type: ServiceType,
        /// Timestamp when the subscription was renewed
        timestamp: SystemTime,
    },

    /// Subscription expired after renewal attempts failed.
    SubscriptionExpired {
        speaker_id: SpeakerId,
        service_type: ServiceType,
        /// Timestamp when the subscription expired
        timestamp: SystemTime,
    },

    /// Subscription removed (unsubscribed).
    SubscriptionRemoved {
        speaker_id: SpeakerId,
        service_type: ServiceType,
        /// Timestamp when the subscription was removed
        timestamp: SystemTime,
    },

    /// Parsed event from a service.
    ServiceEvent {
        speaker_id: SpeakerId,
        service_type: ServiceType,
        event: TypedEvent,
        /// Timestamp when the event was received and processed
        timestamp: SystemTime,
    },

    /// Error parsing an event.
    ParseError {
        speaker_id: SpeakerId,
        service_type: ServiceType,
        error: String,
        /// Timestamp when the parse error occurred
        timestamp: SystemTime,
    },
}

impl Event {
    /// Get the timestamp for this event.
    /// 
    /// All events now include timestamps for unified stream compatibility.
    pub fn timestamp(&self) -> SystemTime {
        match self {
            Event::SubscriptionEstablished { timestamp, .. } => *timestamp,
            Event::SubscriptionFailed { timestamp, .. } => *timestamp,
            Event::SubscriptionRenewed { timestamp, .. } => *timestamp,
            Event::SubscriptionExpired { timestamp, .. } => *timestamp,
            Event::SubscriptionRemoved { timestamp, .. } => *timestamp,
            Event::ServiceEvent { timestamp, .. } => *timestamp,
            Event::ParseError { timestamp, .. } => *timestamp,
        }
    }

    /// Get the speaker ID for this event.
    /// 
    /// All events are associated with a specific speaker.
    pub fn speaker_id(&self) -> &SpeakerId {
        match self {
            Event::SubscriptionEstablished { speaker_id, .. } => speaker_id,
            Event::SubscriptionFailed { speaker_id, .. } => speaker_id,
            Event::SubscriptionRenewed { speaker_id, .. } => speaker_id,
            Event::SubscriptionExpired { speaker_id, .. } => speaker_id,
            Event::SubscriptionRemoved { speaker_id, .. } => speaker_id,
            Event::ServiceEvent { speaker_id, .. } => speaker_id,
            Event::ParseError { speaker_id, .. } => speaker_id,
        }
    }

    /// Get the service type for this event.
    /// 
    /// All events are associated with a specific UPnP service type.
    pub fn service_type(&self) -> ServiceType {
        match self {
            Event::SubscriptionEstablished { service_type, .. } => *service_type,
            Event::SubscriptionFailed { service_type, .. } => *service_type,
            Event::SubscriptionRenewed { service_type, .. } => *service_type,
            Event::SubscriptionExpired { service_type, .. } => *service_type,
            Event::SubscriptionRemoved { service_type, .. } => *service_type,
            Event::ServiceEvent { service_type, .. } => *service_type,
            Event::ParseError { service_type, .. } => *service_type,
        }
    }

    /// Check if this is a service event containing parsed data.
    /// 
    /// Returns true for ServiceEvent variants, false for lifecycle and error events.
    pub fn is_service_event(&self) -> bool {
        matches!(self, Event::ServiceEvent { .. })
    }

    /// Check if this is an error event.
    /// 
    /// Returns true for ParseError and SubscriptionFailed variants.
    pub fn is_error_event(&self) -> bool {
        matches!(self, Event::ParseError { .. } | Event::SubscriptionFailed { .. })
    }

    /// Check if this is a subscription lifecycle event.
    /// 
    /// Returns true for subscription establishment, renewal, expiration, and removal events.
    pub fn is_subscription_lifecycle_event(&self) -> bool {
        matches!(
            self,
            Event::SubscriptionEstablished { .. }
                | Event::SubscriptionRenewed { .. }
                | Event::SubscriptionExpired { .. }
                | Event::SubscriptionRemoved { .. }
        )
    }
}





#[cfg(test)]
mod tests {
    use super::*;
    use std::time::SystemTime;

    // Mock parser for testing
    #[derive(Debug, Clone)]
    struct MockParser {
        data: String,
    }

    impl MockParser {
        fn new(data: String) -> Self {
            Self { data }
        }
        
        fn get_data(&self) -> &str {
            &self.data
        }
    }

    #[test]
    fn event_debug_contains_expected_fields() {
        let timestamp = SystemTime::now();
        let event = Event::SubscriptionEstablished {
            speaker_id: SpeakerId::new("speaker1"),
            service_type: ServiceType::AVTransport,
            subscription_id: "test-sub-123".to_string(),
            timestamp,
        };

        let debug_str = format!("{event:?}");
        assert!(debug_str.contains("SubscriptionEstablished"));
        assert!(debug_str.contains("speaker1"));
        assert!(debug_str.contains("AVTransport"));
        assert!(debug_str.contains("test-sub-123"));
    }

    #[test]
    fn event_clone_preserves_data() {
        let timestamp = SystemTime::now();
        let event = Event::SubscriptionFailed {
            speaker_id: SpeakerId::new("speaker1"),
            service_type: ServiceType::RenderingControl,
            error: "connection failed".to_string(),
            timestamp,
        };

        let cloned = event.clone();
        
        if let (
            Event::SubscriptionFailed { speaker_id: s1, service_type: st1, error: e1, timestamp: t1 },
            Event::SubscriptionFailed { speaker_id: s2, service_type: st2, error: e2, timestamp: t2 },
        ) = (event, cloned) {
            assert_eq!(s1, s2);
            assert_eq!(st1, st2);
            assert_eq!(e1, e2);
            assert_eq!(t1, t2);
        } else {
            panic!("Event type mismatch after clone");
        }
    }

    #[test]
    fn subscription_events_contain_correct_data() {
        let timestamp = SystemTime::now();
        let cases = [
            Event::SubscriptionEstablished {
                speaker_id: SpeakerId::new("RINCON_123"),
                service_type: ServiceType::AVTransport,
                subscription_id: "uuid:sub-123".to_string(),
                timestamp,
            },
            Event::SubscriptionFailed {
                speaker_id: SpeakerId::new("RINCON_456"),
                service_type: ServiceType::RenderingControl,
                error: "Network timeout".to_string(),
                timestamp,
            },
            Event::SubscriptionRenewed {
                speaker_id: SpeakerId::new("RINCON_789"),
                service_type: ServiceType::ZoneGroupTopology,
                timestamp,
            },
            Event::SubscriptionExpired {
                speaker_id: SpeakerId::new("RINCON_ABC"),
                service_type: ServiceType::AVTransport,
                timestamp,
            },
            Event::SubscriptionRemoved {
                speaker_id: SpeakerId::new("RINCON_DEF"),
                service_type: ServiceType::RenderingControl,
                timestamp,
            },
        ];

        for event in cases {
            // All subscription events should be debuggable
            let debug_str = format!("{event:?}");
            assert!(!debug_str.is_empty());
            
            // All events should have timestamps
            assert_eq!(event.timestamp(), timestamp);
        }
    }

    #[test]
    fn service_event_contains_typed_event() {
        let timestamp = SystemTime::now();
        let mock_parser = MockParser::new("test_data".to_string());
        let typed_event = TypedEvent::new_parser(
            mock_parser,
            "test_event",
            ServiceType::AVTransport,
        );

        let event = Event::ServiceEvent {
            speaker_id: SpeakerId::new("RINCON_GHI"),
            service_type: ServiceType::AVTransport,
            event: typed_event,
            timestamp,
        };

        if let Event::ServiceEvent { speaker_id, service_type, event, timestamp: event_timestamp } = event {
            assert_eq!(speaker_id.as_str(), "RINCON_GHI");
            assert_eq!(service_type, ServiceType::AVTransport);
            assert_eq!(event.event_type(), "test_event");
            assert_eq!(event_timestamp, timestamp);
        } else {
            panic!("Expected ServiceEvent");
        }
    }

    #[test]
    fn parse_error_contains_error_message() {
        let timestamp = SystemTime::now();
        let event = Event::ParseError {
            speaker_id: SpeakerId::new("RINCON_JKL"),
            service_type: ServiceType::ZoneGroupTopology,
            error: "Invalid XML".to_string(),
            timestamp,
        };

        if let Event::ParseError { speaker_id, service_type, error, timestamp: event_timestamp } = event {
            assert_eq!(speaker_id.as_str(), "RINCON_JKL");
            assert_eq!(service_type, ServiceType::ZoneGroupTopology);
            assert_eq!(error, "Invalid XML");
            assert_eq!(event_timestamp, timestamp);
        } else {
            panic!("Expected ParseError");
        }
    }

    #[test]
    fn event_accessor_methods_work_correctly() {
        let timestamp = SystemTime::now();
        let speaker_id = SpeakerId::new("RINCON_TEST");
        let service_type = ServiceType::AVTransport;

        let events = vec![
            Event::SubscriptionEstablished {
                speaker_id: speaker_id.clone(),
                service_type,
                subscription_id: "sub-123".to_string(),
                timestamp,
            },
            Event::SubscriptionFailed {
                speaker_id: speaker_id.clone(),
                service_type,
                error: "failed".to_string(),
                timestamp,
            },
            Event::SubscriptionRenewed {
                speaker_id: speaker_id.clone(),
                service_type,
                timestamp,
            },
            Event::SubscriptionExpired {
                speaker_id: speaker_id.clone(),
                service_type,
                timestamp,
            },
            Event::SubscriptionRemoved {
                speaker_id: speaker_id.clone(),
                service_type,
                timestamp,
            },
            Event::ParseError {
                speaker_id: speaker_id.clone(),
                service_type,
                error: "parse failed".to_string(),
                timestamp,
            },
        ];

        for event in &events {
            assert_eq!(event.timestamp(), timestamp);
            assert_eq!(event.speaker_id(), &speaker_id);
            assert_eq!(event.service_type(), service_type);
        }
    }

    #[test]
    fn event_classification_methods_work_correctly() {
        let timestamp = SystemTime::now();
        let speaker_id = SpeakerId::new("RINCON_TEST");
        let service_type = ServiceType::AVTransport;

        // Service event
        let mock_parser = MockParser::new("test_data".to_string());
        let typed_event = TypedEvent::new_parser(mock_parser, "test_event", service_type);
        let service_event = Event::ServiceEvent {
            speaker_id: speaker_id.clone(),
            service_type,
            event: typed_event,
            timestamp,
        };
        assert!(service_event.is_service_event());
        assert!(!service_event.is_error_event());
        assert!(!service_event.is_subscription_lifecycle_event());

        // Error events
        let parse_error = Event::ParseError {
            speaker_id: speaker_id.clone(),
            service_type,
            error: "parse failed".to_string(),
            timestamp,
        };
        assert!(!parse_error.is_service_event());
        assert!(parse_error.is_error_event());
        assert!(!parse_error.is_subscription_lifecycle_event());

        let subscription_failed = Event::SubscriptionFailed {
            speaker_id: speaker_id.clone(),
            service_type,
            error: "failed".to_string(),
            timestamp,
        };
        assert!(!subscription_failed.is_service_event());
        assert!(subscription_failed.is_error_event());
        assert!(!subscription_failed.is_subscription_lifecycle_event());

        // Lifecycle events
        let subscription_established = Event::SubscriptionEstablished {
            speaker_id: speaker_id.clone(),
            service_type,
            subscription_id: "sub-123".to_string(),
            timestamp,
        };
        assert!(!subscription_established.is_service_event());
        assert!(!subscription_established.is_error_event());
        assert!(subscription_established.is_subscription_lifecycle_event());

        let subscription_renewed = Event::SubscriptionRenewed {
            speaker_id: speaker_id.clone(),
            service_type,
            timestamp,
        };
        assert!(!subscription_renewed.is_service_event());
        assert!(!subscription_renewed.is_error_event());
        assert!(subscription_renewed.is_subscription_lifecycle_event());

        let subscription_expired = Event::SubscriptionExpired {
            speaker_id: speaker_id.clone(),
            service_type,
            timestamp,
        };
        assert!(!subscription_expired.is_service_event());
        assert!(!subscription_expired.is_error_event());
        assert!(subscription_expired.is_subscription_lifecycle_event());

        let subscription_removed = Event::SubscriptionRemoved {
            speaker_id: speaker_id.clone(),
            service_type,
            timestamp,
        };
        assert!(!subscription_removed.is_service_event());
        assert!(!subscription_removed.is_error_event());
        assert!(subscription_removed.is_subscription_lifecycle_event());
    }

    // Additional mock parser for testing downcasting
    #[derive(Debug, Clone)]
    struct AnotherMockParser {
        _value: i32,
    }

    impl AnotherMockParser {
        fn new(value: i32) -> Self {
            Self { _value: value }
        }
    }

    #[test]
    fn typed_event_creation_and_access() {
        let mock_parser = MockParser::new("test_data".to_string());
        let typed_event = TypedEvent::new_parser(
            mock_parser,
            "test_event",
            ServiceType::AVTransport,
        );

        assert_eq!(typed_event.event_type(), "test_event");
        assert_eq!(typed_event.service_type(), ServiceType::AVTransport);
    }

    #[test]
    fn typed_event_downcast_success_and_failure() {
        let mock_parser = MockParser::new("test_data".to_string());
        let typed_event = TypedEvent::new_parser(
            mock_parser,
            "test_event",
            ServiceType::RenderingControl,
        );

        // Successful downcast
        let downcast_result = typed_event.downcast_ref::<MockParser>();
        assert!(downcast_result.is_some());
        
        let downcast_parser = downcast_result.unwrap();
        assert_eq!(downcast_parser.get_data(), "test_data");

        // Failed downcast to wrong type
        let wrong_downcast = typed_event.downcast_ref::<AnotherMockParser>();
        assert!(wrong_downcast.is_none());
    }

    #[test]
    fn typed_event_debug_output() {
        let mock_parser = MockParser::new("debug_data".to_string());
        let typed_event = TypedEvent::new_parser(
            mock_parser,
            "debug_test",
            ServiceType::RenderingControl,
        );

        let debug_str = format!("{:?}", typed_event);
        assert!(debug_str.contains("TypedEvent"));
    }

    #[test]
    fn multiple_parser_types_work_uniformly() {
        // Test that different parser types can be processed uniformly
        let events = vec![
            TypedEvent::new_parser(
                MockParser::new("av_data".to_string()),
                "av_event",
                ServiceType::AVTransport,
            ),
            TypedEvent::new_parser(
                AnotherMockParser::new(42),
                "another_event",
                ServiceType::RenderingControl,
            ),
        ];

        // All events should be processable through uniform interface
        for event in &events {
            assert!(!event.event_type().is_empty());
            assert!(matches!(
                event.service_type(),
                ServiceType::AVTransport | ServiceType::RenderingControl | ServiceType::ZoneGroupTopology
            ));
            
            // Debug formatting should work
            let debug_output = format!("{:?}", event);
            assert!(!debug_output.is_empty());
        }
    }
}
