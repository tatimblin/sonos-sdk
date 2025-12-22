//! Event types for the sonos-stream crate.
//!
//! This module defines the event types emitted by the broker and the parsed event
//! types returned by strategies.

use crate::types::{ServiceType, SpeakerId};

/// Trait for strategy-specific event data.
///
/// This trait allows strategies to define their own event data types while
/// providing a common interface for the broker and applications. Each strategy
/// can implement this trait for their specific event data structures, enabling
/// type-safe event handling without requiring changes to core event types.
///
/// # Examples
///
/// ```rust,ignore
/// use sonos_stream::{EventData, ServiceType};
/// use std::any::Any;
///
/// #[derive(Debug, Clone)]
/// struct MyEventData {
///     pub value: String,
/// }
///
/// impl EventData for MyEventData {
///     fn event_type(&self) -> &str {
///         "my_event"
///     }
///     
///     fn service_type(&self) -> ServiceType {
///         ServiceType::AVTransport
///     }
///     
///     fn as_any(&self) -> &dyn Any {
///         self
///     }
///     
///     fn clone_box(&self) -> Box<dyn EventData> {
///         Box::new(self.clone())
///     }
/// }
/// ```
pub trait EventData: Send + Sync + std::fmt::Debug {
    /// Get the event type identifier.
    ///
    /// Returns a string that identifies the type of event. This should be
    /// consistent for all instances of the same event type and unique within
    /// the strategy that produces it.
    ///
    /// # Examples
    ///
    /// - "transport_state_changed"
    /// - "volume_changed"
    /// - "track_metadata_updated"
    fn event_type(&self) -> &str;
    
    /// Get the service type that produced this event.
    ///
    /// Returns the UPnP service type that generated this event data. This
    /// allows applications to understand which service the event originated
    /// from without needing to downcast the event data.
    fn service_type(&self) -> ServiceType;
    
    /// Convert to Any for downcasting.
    ///
    /// This method enables type-safe downcasting of the trait object back to
    /// the concrete event data type. Applications can use this to access
    /// strategy-specific event data in a type-safe manner.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// if let Some(av_event) = event_data.as_any().downcast_ref::<AVTransportEvent>() {
    ///     println!("Transport state: {}", av_event.transport_state);
    /// }
    /// ```
    fn as_any(&self) -> &dyn std::any::Any;
    
    /// Clone the event data (for Clone support).
    ///
    /// This method enables cloning of trait objects by returning a boxed clone
    /// of the concrete event data. This is necessary because trait objects
    /// cannot implement Clone directly.
    fn clone_box(&self) -> Box<dyn EventData>;
}

/// Container for strategy-specific typed event data.
///
/// This struct holds trait objects that can be downcast to specific strategy 
/// event types. It provides a uniform interface for handling events from 
/// different strategies while preserving type safety through downcasting.
///
/// # Examples
///
/// ```rust,ignore
/// use sonos_stream::{TypedEvent, AVTransportEvent};
///
/// // Create a typed event
/// let av_event = AVTransportEvent::new("PLAYING".to_string());
/// let typed_event = TypedEvent::new(Box::new(av_event));
///
/// // Access event metadata
/// println!("Event type: {}", typed_event.event_type());
/// println!("Service: {:?}", typed_event.service_type());
///
/// // Downcast to specific type
/// if let Some(av_data) = typed_event.downcast_ref::<AVTransportEvent>() {
///     println!("Transport state: {}", av_data.transport_state);
/// }
/// ```
#[derive(Debug)]
pub struct TypedEvent {
    /// The event data as a trait object
    data: Box<dyn EventData>,
}

impl TypedEvent {
    /// Create a new typed event.
    ///
    /// # Arguments
    ///
    /// * `data` - The event data implementing the EventData trait
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let event_data = MyEventData { value: "test".to_string() };
    /// let typed_event = TypedEvent::new(Box::new(event_data));
    /// ```
    pub fn new(data: Box<dyn EventData>) -> Self {
        Self { data }
    }
    
    /// Get the event type.
    ///
    /// Returns the event type identifier from the underlying event data.
    /// This provides a uniform way to access event type information without
    /// needing to downcast.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let event_type = typed_event.event_type();
    /// match event_type {
    ///     "transport_state_changed" => handle_transport_event(&typed_event),
    ///     "volume_changed" => handle_volume_event(&typed_event),
    ///     _ => println!("Unknown event type: {}", event_type),
    /// }
    /// ```
    pub fn event_type(&self) -> &str {
        self.data.event_type()
    }
    
    /// Get the service type.
    ///
    /// Returns the service type that produced this event. This allows
    /// applications to route events based on service type without needing
    /// to downcast the event data.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// match typed_event.service_type() {
    ///     ServiceType::AVTransport => handle_av_transport_event(&typed_event),
    ///     ServiceType::RenderingControl => handle_rendering_event(&typed_event),
    ///     ServiceType::ZoneGroupTopology => handle_topology_event(&typed_event),
    /// }
    /// ```
    pub fn service_type(&self) -> ServiceType {
        self.data.service_type()
    }
    
    /// Downcast to a specific event data type.
    ///
    /// Attempts to downcast the event data to the specified concrete type.
    /// Returns `Some(&T)` if the downcast succeeds, or `None` if the event
    /// data is not of the requested type.
    ///
    /// # Type Parameters
    ///
    /// * `T` - The concrete event data type to downcast to. Must implement
    ///         EventData and have a 'static lifetime.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Safe downcasting with pattern matching
    /// if let Some(av_event) = typed_event.downcast_ref::<AVTransportEvent>() {
    ///     println!("Transport state: {}", av_event.transport_state);
    ///     if let Some(track_uri) = &av_event.track_uri {
    ///         println!("Playing: {}", track_uri);
    ///     }
    /// }
    ///
    /// // Handle multiple event types
    /// match typed_event.service_type() {
    ///     ServiceType::AVTransport => {
    ///         if let Some(av_event) = typed_event.downcast_ref::<AVTransportEvent>() {
    ///             // Handle AVTransport event
    ///         }
    ///     }
    ///     ServiceType::RenderingControl => {
    ///         if let Some(rc_event) = typed_event.downcast_ref::<RenderingControlEvent>() {
    ///             // Handle RenderingControl event
    ///         }
    ///     }
    ///     _ => {}
    /// }
    /// ```
    pub fn downcast_ref<T: EventData + 'static>(&self) -> Option<&T> {
        self.data.as_any().downcast_ref::<T>()
    }
    
    /// Get debug representation.
    ///
    /// Returns a reference to the Debug trait implementation of the underlying
    /// event data. This allows for uniform debug formatting of all event types
    /// without needing to downcast.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// println!("Event debug: {:?}", typed_event.debug());
    /// ```
    pub fn debug(&self) -> &dyn std::fmt::Debug {
        &*self.data
    }
}

impl Clone for TypedEvent {
    /// Clone the typed event.
    ///
    /// This implementation uses the `clone_box` method from the EventData trait
    /// to create a deep copy of the underlying event data. This enables cloning
    /// of trait objects while preserving the concrete type information.
    fn clone(&self) -> Self {
        Self {
            data: self.data.clone_box(),
        }
    }
}



/// Events emitted by the broker.
///
/// These events represent the lifecycle of subscriptions and the parsed events
/// from services. Applications should handle these events to track subscription
/// state and process service events.
///
/// # Examples
///
/// ```rust,ignore
/// use sonos_stream::{Event, EventBroker, AVTransportEvent};
///
/// let mut event_rx = broker.event_stream();
/// while let Some(event) = event_rx.recv().await {
///     match event {
///         Event::SubscriptionEstablished { speaker_id, service_type, .. } => {
///             println!("Subscribed to {:?} on {}", service_type, speaker_id.as_str());
///         }
///         Event::ServiceEvent { speaker_id, event, .. } => {
///             println!("Event from {}: {:?}", speaker_id.as_str(), event);
///             
///             // Downcast to specific event type for type-safe access
///             if let Some(av_event) = event.downcast_ref::<AVTransportEvent>() {
///                 println!("Transport state: {}", av_event.transport_state);
///             }
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
        event: TypedEvent,
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



    // Tests for EventData trait and TypedEvent

    /// Mock event data for testing EventData trait
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
    fn test_typed_event_creation() {
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
    fn test_typed_event_downcast_success() {
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
        assert_eq!(downcast_data.service_type, ServiceType::RenderingControl);
        assert_eq!(downcast_data.data, "test_data");
    }

    /// Another mock event data type for testing failed downcasts
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
    fn test_typed_event_downcast_failure() {
        let mock_data = MockEventData {
            event_type: "test_event".to_string(),
            service_type: ServiceType::ZoneGroupTopology,
            data: "test_data".to_string(),
        };

        let typed_event = TypedEvent::new(Box::new(mock_data));

        // Failed downcast to wrong type
        let downcast_result = typed_event.downcast_ref::<AnotherMockEventData>();
        assert!(downcast_result.is_none());
    }

    #[test]
    fn test_typed_event_clone() {
        let mock_data = MockEventData {
            event_type: "clone_test".to_string(),
            service_type: ServiceType::AVTransport,
            data: "clone_data".to_string(),
        };

        let typed_event = TypedEvent::new(Box::new(mock_data));
        let cloned_event = typed_event.clone();

        // Verify both events have the same data
        assert_eq!(typed_event.event_type(), cloned_event.event_type());
        assert_eq!(typed_event.service_type(), cloned_event.service_type());

        // Verify they can both be downcast successfully
        let original_data = typed_event.downcast_ref::<MockEventData>().unwrap();
        let cloned_data = cloned_event.downcast_ref::<MockEventData>().unwrap();

        assert_eq!(original_data.event_type, cloned_data.event_type);
        assert_eq!(original_data.service_type, cloned_data.service_type);
        assert_eq!(original_data.data, cloned_data.data);
    }

    #[test]
    fn test_typed_event_debug() {
        let mock_data = MockEventData {
            event_type: "debug_test".to_string(),
            service_type: ServiceType::RenderingControl,
            data: "debug_data".to_string(),
        };

        let typed_event = TypedEvent::new(Box::new(mock_data));

        // Test debug output
        let debug_str = format!("{:?}", typed_event);
        assert!(debug_str.contains("TypedEvent"));

        // Test debug method
        let debug_ref = typed_event.debug();
        let debug_str2 = format!("{:?}", debug_ref);
        assert!(debug_str2.contains("MockEventData"));
        assert!(debug_str2.contains("debug_test"));
        assert!(debug_str2.contains("debug_data"));
    }

    #[test]
    fn test_event_data_trait_methods() {
        let mock_data = MockEventData {
            event_type: "trait_test".to_string(),
            service_type: ServiceType::ZoneGroupTopology,
            data: "trait_data".to_string(),
        };

        // Test trait methods directly
        assert_eq!(mock_data.event_type(), "trait_test");
        assert_eq!(mock_data.service_type(), ServiceType::ZoneGroupTopology);

        // Test as_any method
        let any_ref = mock_data.as_any();
        let downcast_result = any_ref.downcast_ref::<MockEventData>();
        assert!(downcast_result.is_some());

        // Test clone_box method
        let cloned_box = mock_data.clone_box();
        assert_eq!(cloned_box.event_type(), "trait_test");
        assert_eq!(cloned_box.service_type(), ServiceType::ZoneGroupTopology);
    }



    // Property-based tests
    mod property_tests {
        use super::*;
        use proptest::prelude::*;

        /// Additional mock event data types for testing uniform interface handling
        #[derive(Debug, Clone)]
        struct MockRenderingControlEvent {
            event_type: String,
            volume: u8,
        }

        impl EventData for MockRenderingControlEvent {
            fn event_type(&self) -> &str {
                &self.event_type
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

        #[derive(Debug, Clone)]
        struct MockZoneGroupEvent {
            event_type: String,
            zone_count: u32,
        }

        impl EventData for MockZoneGroupEvent {
            fn event_type(&self) -> &str {
                &self.event_type
            }

            fn service_type(&self) -> ServiceType {
                ServiceType::ZoneGroupTopology
            }

            fn as_any(&self) -> &dyn std::any::Any {
                self
            }

            fn clone_box(&self) -> Box<dyn EventData> {
                Box::new(self.clone())
            }
        }

        /// Generate arbitrary MockEventData instances
        fn arb_mock_event_data() -> impl Strategy<Value = MockEventData> {
            (
                "[a-z_]{1,20}",
                prop_oneof![
                    Just(ServiceType::AVTransport),
                    Just(ServiceType::RenderingControl),
                    Just(ServiceType::ZoneGroupTopology),
                ],
                "[a-zA-Z0-9 ]{1,50}",
            ).prop_map(|(event_type, service_type, data)| MockEventData {
                event_type,
                service_type,
                data,
            })
        }

        /// Generate arbitrary MockRenderingControlEvent instances
        fn arb_rendering_control_event() -> impl Strategy<Value = MockRenderingControlEvent> {
            (
                "[a-z_]{1,20}",
                0u8..=100u8,
            ).prop_map(|(event_type, volume)| MockRenderingControlEvent {
                event_type,
                volume,
            })
        }

        /// Generate arbitrary MockZoneGroupEvent instances
        fn arb_zone_group_event() -> impl Strategy<Value = MockZoneGroupEvent> {
            (
                "[a-z_]{1,20}",
                1u32..=50u32,
            ).prop_map(|(event_type, zone_count)| MockZoneGroupEvent {
                event_type,
                zone_count,
            })
        }

        /// Generate a collection of mixed event types
        fn arb_mixed_events() -> impl Strategy<Value = Vec<TypedEvent>> {
            prop::collection::vec(
                prop_oneof![
                    arb_mock_event_data().prop_map(|e| TypedEvent::new(Box::new(e))),
                    arb_rendering_control_event().prop_map(|e| TypedEvent::new(Box::new(e))),
                    arb_zone_group_event().prop_map(|e| TypedEvent::new(Box::new(e))),
                ],
                1..=10
            )
        }

        proptest! {
            /// **Feature: strategy-driven-events, Property 2: Safe downcasting behavior**
            /// 
            /// Property: For any TypedEvent containing data of type T, downcasting to type T 
            /// should succeed and downcasting to any incompatible type U should return None.
            /// 
            /// This test verifies that:
            /// 1. Downcasting to the correct type always succeeds
            /// 2. Downcasting to incorrect types always returns None
            /// 3. Successful downcasts preserve all original data
            /// 4. Failed downcasts don't cause panics or undefined behavior
            #[test]
            fn prop_safe_downcasting_behavior(
                mock_events in prop::collection::vec(arb_mock_event_data(), 1..=5),
                rendering_events in prop::collection::vec(arb_rendering_control_event(), 1..=5),
                zone_events in prop::collection::vec(arb_zone_group_event(), 1..=5)
            ) {
                // Test MockEventData downcasting
                for mock_data in mock_events {
                    let original_event_type = mock_data.event_type.clone();
                    let original_service_type = mock_data.service_type;
                    let original_data = mock_data.data.clone();
                    
                    let typed_event = TypedEvent::new(Box::new(mock_data));
                    
                    // Downcasting to correct type should succeed
                    let downcast_result = typed_event.downcast_ref::<MockEventData>();
                    prop_assert!(downcast_result.is_some(), "Downcasting to correct type should succeed");
                    
                    let downcast_data = downcast_result.unwrap();
                    prop_assert_eq!(&downcast_data.event_type, &original_event_type, "Downcast should preserve event_type");
                    prop_assert_eq!(downcast_data.service_type, original_service_type, "Downcast should preserve service_type");
                    prop_assert_eq!(&downcast_data.data, &original_data, "Downcast should preserve data");
                    
                    // Downcasting to incorrect types should return None
                    let wrong_downcast1 = typed_event.downcast_ref::<MockRenderingControlEvent>();
                    prop_assert!(wrong_downcast1.is_none(), "Downcasting to wrong type should return None");
                    
                    let wrong_downcast2 = typed_event.downcast_ref::<MockZoneGroupEvent>();
                    prop_assert!(wrong_downcast2.is_none(), "Downcasting to wrong type should return None");
                }
                
                // Test MockRenderingControlEvent downcasting
                for rendering_data in rendering_events {
                    let original_event_type = rendering_data.event_type.clone();
                    let original_volume = rendering_data.volume;
                    
                    let typed_event = TypedEvent::new(Box::new(rendering_data));
                    
                    // Downcasting to correct type should succeed
                    let downcast_result = typed_event.downcast_ref::<MockRenderingControlEvent>();
                    prop_assert!(downcast_result.is_some(), "Downcasting to correct type should succeed");
                    
                    let downcast_data = downcast_result.unwrap();
                    prop_assert_eq!(&downcast_data.event_type, &original_event_type, "Downcast should preserve event_type");
                    prop_assert_eq!(downcast_data.volume, original_volume, "Downcast should preserve volume");
                    
                    // Downcasting to incorrect types should return None
                    let wrong_downcast1 = typed_event.downcast_ref::<MockEventData>();
                    prop_assert!(wrong_downcast1.is_none(), "Downcasting to wrong type should return None");
                    
                    let wrong_downcast2 = typed_event.downcast_ref::<MockZoneGroupEvent>();
                    prop_assert!(wrong_downcast2.is_none(), "Downcasting to wrong type should return None");
                }
                
                // Test MockZoneGroupEvent downcasting
                for zone_data in zone_events {
                    let original_event_type = zone_data.event_type.clone();
                    let original_zone_count = zone_data.zone_count;
                    
                    let typed_event = TypedEvent::new(Box::new(zone_data));
                    
                    // Downcasting to correct type should succeed
                    let downcast_result = typed_event.downcast_ref::<MockZoneGroupEvent>();
                    prop_assert!(downcast_result.is_some(), "Downcasting to correct type should succeed");
                    
                    let downcast_data = downcast_result.unwrap();
                    prop_assert_eq!(&downcast_data.event_type, &original_event_type, "Downcast should preserve event_type");
                    prop_assert_eq!(downcast_data.zone_count, original_zone_count, "Downcast should preserve zone_count");
                    
                    // Downcasting to incorrect types should return None
                    let wrong_downcast1 = typed_event.downcast_ref::<MockEventData>();
                    prop_assert!(wrong_downcast1.is_none(), "Downcasting to wrong type should return None");
                    
                    let wrong_downcast2 = typed_event.downcast_ref::<MockRenderingControlEvent>();
                    prop_assert!(wrong_downcast2.is_none(), "Downcasting to wrong type should return None");
                }
            }

            /// **Feature: strategy-driven-events, Property 3: Uniform interface handling**
            /// 
            /// Property: For any collection of events from different strategies, all events 
            /// should be processable through the same EventData trait methods regardless 
            /// of their concrete types.
            /// 
            /// This test verifies that:
            /// 1. All events can be processed uniformly through EventData trait methods
            /// 2. Event type and service type are accessible without downcasting
            /// 3. Debug formatting works consistently across all event types
            /// 4. Cloning works uniformly for all event types
            #[test]
            fn prop_uniform_interface_handling(events in arb_mixed_events()) {
                // Verify all events can be processed through uniform interface
                for event in &events {
                    // All events should provide event_type through uniform interface
                    let event_type = event.event_type();
                    prop_assert!(!event_type.is_empty(), "Event type should not be empty");
                    
                    // All events should provide service_type through uniform interface
                    let service_type = event.service_type();
                    prop_assert!(
                        matches!(service_type, ServiceType::AVTransport | ServiceType::RenderingControl | ServiceType::ZoneGroupTopology),
                        "Service type should be valid: {:?}", service_type
                    );
                    
                    // All events should support debug formatting
                    let debug_output = format!("{:?}", event.debug());
                    prop_assert!(!debug_output.is_empty(), "Debug output should not be empty");
                    
                    // All events should be cloneable
                    let cloned_event = event.clone();
                    prop_assert_eq!(event.event_type(), cloned_event.event_type(), "Cloned event should have same event_type");
                    prop_assert_eq!(event.service_type(), cloned_event.service_type(), "Cloned event should have same service_type");
                }
                
                // Verify we can process collections of mixed event types uniformly
                let event_types: Vec<&str> = events.iter().map(|e| e.event_type()).collect();
                let service_types: Vec<ServiceType> = events.iter().map(|e| e.service_type()).collect();
                
                // All operations should succeed without knowing concrete types
                prop_assert_eq!(event_types.len(), events.len(), "Should be able to extract event types from all events");
                prop_assert_eq!(service_types.len(), events.len(), "Should be able to extract service types from all events");
                
                // Verify uniform processing doesn't depend on event order or type mix
                let mut events_copy = events.clone();
                events_copy.reverse();
                
                let reversed_event_types: Vec<&str> = events_copy.iter().map(|e| e.event_type()).collect();
                let reversed_service_types: Vec<ServiceType> = events_copy.iter().map(|e| e.service_type()).collect();
                
                // Processing should work the same regardless of order
                prop_assert_eq!(reversed_event_types.len(), events_copy.len(), "Uniform processing should work regardless of order");
                prop_assert_eq!(reversed_service_types.len(), events_copy.len(), "Uniform processing should work regardless of order");
            }
        }
    }
}
