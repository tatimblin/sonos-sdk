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
//! - [`AVTransportProvider`]: Handles playback state changes and track information events
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
//! # Using AVTransportProvider
//!
//! ```rust,no_run
//! use sonos_stream::{EventBrokerBuilder, AVTransportProvider, ServiceType, Speaker, SpeakerId};
//! use std::net::IpAddr;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Create a broker with the AVTransport provider
//! let mut broker = EventBrokerBuilder::new()
//!     .with_strategy(Box::new(AVTransportProvider::new()))
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
//! To add support for a new UPnP service, implement the `ServiceStrategy` trait:
//!
//! ```rust,ignore
//! use sonos_stream::{
//!     ServiceStrategy, Subscription, Speaker, ServiceType, SubscriptionScope,
//!     SubscriptionConfig, StrategyError, TypedEvent, SpeakerId, SubscriptionError,
//! };
//! use std::time::Duration;
//!
//! #[derive(Debug, Clone)]
//! struct MyServiceProvider;
//!
//! #[async_trait::async_trait]
//! impl ServiceStrategy for MyServiceProvider {
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
//!     ) -> Result<TypedEvent, StrategyError> {
//!         // Parse the XML and extract state variables
//!         // Convert to TypedEvent instance
//!         unimplemented!()
//!     }
//! }
//! ```

mod service_strategy;
mod av_transport_provider;

pub use service_strategy::ServiceStrategy;
pub use av_transport_provider::AVTransportProvider;
