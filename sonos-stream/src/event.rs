//! Event types for the sonos-stream crate.

use crate::types::{ServiceType, SpeakerId};

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
    },

    /// Subscription failed to establish.
    SubscriptionFailed {
        speaker_id: SpeakerId,
        service_type: ServiceType,
        error: String,
    },

    /// Subscription successfully renewed.
    SubscriptionRenewed {
        speaker_id: SpeakerId,
        service_type: ServiceType,
    },

    /// Subscription expired after renewal attempts failed.
    SubscriptionExpired {
        speaker_id: SpeakerId,
        service_type: ServiceType,
    },

    /// Subscription removed (unsubscribed).
    SubscriptionRemoved {
        speaker_id: SpeakerId,
        service_type: ServiceType,
    },

    /// Parsed event from a service.
    ServiceEvent {
        speaker_id: SpeakerId,
        service_type: ServiceType,
        event: TypedEvent,
    },

    /// Error parsing an event.
    ParseError {
        speaker_id: SpeakerId,
        service_type: ServiceType,
        error: String,
    },
}





#[cfg(test)]
mod tests {
    use super::*;

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
    fn event_clone_preserves_data() {
        let event = Event::SubscriptionFailed {
            speaker_id: SpeakerId::new("speaker1"),
            service_type: ServiceType::RenderingControl,
            error: "connection failed".to_string(),
        };

        let cloned = event.clone();
        
        if let (
            Event::SubscriptionFailed { speaker_id: s1, service_type: st1, error: e1 },
            Event::SubscriptionFailed { speaker_id: s2, service_type: st2, error: e2 },
        ) = (event, cloned) {
            assert_eq!(s1, s2);
            assert_eq!(st1, st2);
            assert_eq!(e1, e2);
        } else {
            panic!("Event type mismatch after clone");
        }
    }

    #[test]
    fn subscription_events_contain_correct_data() {
        let cases = [
            Event::SubscriptionEstablished {
                speaker_id: SpeakerId::new("RINCON_123"),
                service_type: ServiceType::AVTransport,
                subscription_id: "uuid:sub-123".to_string(),
            },
            Event::SubscriptionFailed {
                speaker_id: SpeakerId::new("RINCON_456"),
                service_type: ServiceType::RenderingControl,
                error: "Network timeout".to_string(),
            },
            Event::SubscriptionRenewed {
                speaker_id: SpeakerId::new("RINCON_789"),
                service_type: ServiceType::ZoneGroupTopology,
            },
            Event::SubscriptionExpired {
                speaker_id: SpeakerId::new("RINCON_ABC"),
                service_type: ServiceType::AVTransport,
            },
            Event::SubscriptionRemoved {
                speaker_id: SpeakerId::new("RINCON_DEF"),
                service_type: ServiceType::RenderingControl,
            },
        ];

        for event in cases {
            // All subscription events should be debuggable
            let debug_str = format!("{event:?}");
            assert!(!debug_str.is_empty());
        }
    }

    #[test]
    fn service_event_contains_typed_event() {
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
        };

        if let Event::ServiceEvent { speaker_id, service_type, event } = event {
            assert_eq!(speaker_id.as_str(), "RINCON_GHI");
            assert_eq!(service_type, ServiceType::AVTransport);
            assert_eq!(event.event_type(), "test_event");
        } else {
            panic!("Expected ServiceEvent");
        }
    }

    #[test]
    fn parse_error_contains_error_message() {
        let event = Event::ParseError {
            speaker_id: SpeakerId::new("RINCON_JKL"),
            service_type: ServiceType::ZoneGroupTopology,
            error: "Invalid XML".to_string(),
        };

        if let Event::ParseError { speaker_id, service_type, error } = event {
            assert_eq!(speaker_id.as_str(), "RINCON_JKL");
            assert_eq!(service_type, ServiceType::ZoneGroupTopology);
            assert_eq!(error, "Invalid XML");
        } else {
            panic!("Expected ParseError");
        }
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
