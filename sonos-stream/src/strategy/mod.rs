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
//! - [`Strategy::AVTransport`]: Handles playback state changes and track information events
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
//! # Using Strategy::AVTransport
//!
//! ```rust,no_run
//! use sonos_stream::{EventBrokerBuilder, Strategy, ServiceType, Speaker, SpeakerId};
//! use std::net::IpAddr;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Create a broker with the AVTransport strategy
//! let mut broker = EventBrokerBuilder::new()
//!     .with_strategy(Box::new(Strategy::AVTransport))
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
//! Strategies can use the `parse_event_generic` helper function to reduce boilerplate.
//! Simply implement the `EventParser` trait for your parser type, then call the helper:
//!
//! ```rust,ignore
//! use sonos_stream::{
//!     SubscriptionStrategy, EventParser, parse_event_generic,
//!     Speaker, ServiceType, SubscriptionScope, StrategyError, TypedEvent,
//! };
//! use sonos_parser::services::rendering_control::RenderingControlParser;
//!
//! // Implement EventParser for your parser type
//! impl EventParser for RenderingControlParser {
//!     type EventType = RenderingControlEvent;
//!
//!     fn from_xml(xml: &str) -> Result<Self, String> {
//!         RenderingControlParser::from_xml(xml).map_err(|e| e.to_string())
//!     }
//!
//!     fn into_event(self) -> Self::EventType {
//!         RenderingControlEvent::from_parser(self)
//!     }
//! }
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
//!         _speaker_id: &SpeakerId,
//!         event_xml: &str,
//!     ) -> Result<TypedEvent, StrategyError> {
//!         // Use the generic helper - just specify the parser type!
//!         parse_event_generic::<RenderingControlParser>(event_xml)
//!     }
//! }
//! ```

mod base;

pub use base::{AVTransportEvent, BaseStrategy, EventParser, Strategy};
