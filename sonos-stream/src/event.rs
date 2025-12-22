//! Event types for the sonos-stream crate.

use crate::types::{ServiceType, SpeakerId};

/// Trait for strategy-specific event data.
///
/// Allows strategies to define their own event data types while providing
/// a common interface for the broker and applications.
pub trait EventData: Send + Sync + std::fmt::Debug {
    /// Get the event type identifier (e.g., "transport_state_changed").
    fn event_type(&self) -> &str;
    
    /// Get the service type that produced this event.
    fn service_type(&self) -> ServiceType;
    
    /// Convert to Any for type-safe downcasting.
    fn as_any(&self) -> &dyn std::any::Any;
    
    /// Clone the event data (enables Clone for trait objects).
    fn clone_box(&self) -> Box<dyn EventData>;
}

/// Container for strategy-specific typed event data.
///
/// Holds trait objects that can be downcast to specific strategy event types.
#[derive(Debug)]
pub struct TypedEvent {
    data: Box<dyn EventData>,
}

impl TypedEvent {
    /// Create a new typed event.
    pub fn new(data: Box<dyn EventData>) -> Self {
        Self { data }
    }
    
    /// Get the event type identifier.
    pub fn event_type(&self) -> &str {
        self.data.event_type()
    }
    
    /// Get the service type that produced this event.
    pub fn service_type(&self) -> ServiceType {
        self.data.service_type()
    }
    
    /// Downcast to a specific event data type.
    pub fn downcast_ref<T: EventData + 'static>(&self) -> Option<&T> {
        self.data.as_any().downcast_ref::<T>()
    }
    
    /// Get debug representation of the underlying event data.
    pub fn debug(&self) -> &dyn std::fmt::Debug {
        &*self.data
    }
}

impl Clone for TypedEvent {
    fn clone(&self) -> Self {
        Self {
            data: self.data.clone_box(),
        }
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

    // Mock event data for testing
    #[derive(Debug, Clone)]
    struct MockEventData {
        event_type: String,
        service_type: ServiceType,
        data: String,
    }

    impl EventData for MockEventData {
        fn event_type(&self) -> &str {
            &self.event_type
        }

        fn service_type(&self) -> ServiceType {
            self.service_type
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }

        fn clone_box(&self) -> Box<dyn EventData> {
            Box::new(self.clone())
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
        let mock_data = MockEventData {
            event_type: "test_event".to_string(),
            service_type: ServiceType::AVTransport,
            data: "test_data".to_string(),
        };
        let typed_event = TypedEvent::new(Box::new(mock_data));

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



    // Additional mock for testing downcasting
    #[derive(Debug, Clone)]
    struct AnotherMockEventData {
        value: i32,
    }

    impl EventData for AnotherMockEventData {
        fn event_type(&self) -> &str {
            "another_event"
        }

        fn service_type(&self) -> ServiceType {
            ServiceType::RenderingControl
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }

        fn clone_box(&self) -> Box<dyn EventData> {
            Box::new(self.clone())
        }
    }

    #[test]
    fn typed_event_creation_and_access() {
        let mock_data = MockEventData {
            event_type: "test_event".to_string(),
            service_type: ServiceType::AVTransport,
            data: "test_data".to_string(),
        };

        let typed_event = TypedEvent::new(Box::new(mock_data));

        assert_eq!(typed_event.event_type(), "test_event");
        assert_eq!(typed_event.service_type(), ServiceType::AVTransport);
    }

    #[test]
    fn typed_event_downcast_success_and_failure() {
        let mock_data = MockEventData {
            event_type: "test_event".to_string(),
            service_type: ServiceType::RenderingControl,
            data: "test_data".to_string(),
        };

        let typed_event = TypedEvent::new(Box::new(mock_data));

        // Successful downcast
        let downcast_result = typed_event.downcast_ref::<MockEventData>();
        assert!(downcast_result.is_some());
        
        let downcast_data = downcast_result.unwrap();
        assert_eq!(downcast_data.event_type, "test_event");
        assert_eq!(downcast_data.data, "test_data");

        // Failed downcast to wrong type
        let wrong_downcast = typed_event.downcast_ref::<AnotherMockEventData>();
        assert!(wrong_downcast.is_none());
    }

    #[test]
    fn typed_event_clone_preserves_data() {
        let mock_data = MockEventData {
            event_type: "clone_test".to_string(),
            service_type: ServiceType::AVTransport,
            data: "clone_data".to_string(),
        };

        let typed_event = TypedEvent::new(Box::new(mock_data));
        let cloned_event = typed_event.clone();

        assert_eq!(typed_event.event_type(), cloned_event.event_type());
        assert_eq!(typed_event.service_type(), cloned_event.service_type());

        // Both should downcast successfully
        assert!(typed_event.downcast_ref::<MockEventData>().is_some());
        assert!(cloned_event.downcast_ref::<MockEventData>().is_some());
    }

    #[test]
    fn typed_event_debug_output() {
        let mock_data = MockEventData {
            event_type: "debug_test".to_string(),
            service_type: ServiceType::RenderingControl,
            data: "debug_data".to_string(),
        };

        let typed_event = TypedEvent::new(Box::new(mock_data));

        let debug_str = format!("{:?}", typed_event);
        assert!(debug_str.contains("TypedEvent"));

        let debug_ref = typed_event.debug();
        let debug_str2 = format!("{:?}", debug_ref);
        assert!(debug_str2.contains("MockEventData"));
        assert!(debug_str2.contains("debug_test"));
    }



    #[test]
    fn multiple_event_types_work_uniformly() {
        // Test that different event data types can be processed uniformly
        let events = vec![
            TypedEvent::new(Box::new(MockEventData {
                event_type: "av_event".to_string(),
                service_type: ServiceType::AVTransport,
                data: "av_data".to_string(),
            })),
            TypedEvent::new(Box::new(AnotherMockEventData { value: 42 })),
        ];

        // All events should be processable through uniform interface
        for event in &events {
            assert!(!event.event_type().is_empty());
            assert!(matches!(
                event.service_type(),
                ServiceType::AVTransport | ServiceType::RenderingControl | ServiceType::ZoneGroupTopology
            ));
            
            // Debug formatting should work
            let debug_output = format!("{:?}", event.debug());
            assert!(!debug_output.is_empty());
            
            // Cloning should work
            let cloned = event.clone();
            assert_eq!(event.event_type(), cloned.event_type());
            assert_eq!(event.service_type(), cloned.service_type());
        }
    }
}
