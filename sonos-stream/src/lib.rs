//! # sonos-stream
//!
//! A micro-crate for managing UPnP event subscriptions for Sonos speakers using a broker pattern.
//!
//! This crate provides a clean abstraction for subscribing to speaker events without needing to
//! understand the underlying UPnP protocol details. It uses the strategy pattern to delegate
//! service-specific event parsing to separate implementations, keeping the core broker logic
//! service-agnostic.
//!
//! ## Architecture Overview
//!
//! The crate is built around three main concepts:
//!
//! - **EventBroker**: The central component that manages subscription lifecycle and routes events
//! - **SubscriptionStrategy**: A trait for implementing service-specific subscription and parsing logic
//! - **Subscription**: A trait representing an active UPnP subscription instance
//!
//! The broker remains service-agnostic, delegating all service-specific operations to pluggable
//! strategy implementations. This design allows you to add support for new UPnP services without
//! modifying the broker code.
//!
//! ## Basic Usage
//!
//! ```rust,no_run
//! use sonos_stream::{EventBrokerBuilder, Event, ServiceType, Speaker, SpeakerId};
//! use std::net::IpAddr;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Create a broker with a custom strategy (implementation not shown)
//! # struct MyStrategy;
//! # #[async_trait::async_trait]
//! # impl sonos_stream::SubscriptionStrategy for MyStrategy {
//! #     fn service_type(&self) -> ServiceType { ServiceType::AVTransport }
//! #     fn subscription_scope(&self) -> sonos_stream::SubscriptionScope {
//! #         sonos_stream::SubscriptionScope::PerSpeaker
//! #     }
//! #     fn service_endpoint_path(&self) -> &'static str {
//! #         "/MediaRenderer/AVTransport/Event"
//! #     }
//! #     fn parse_event(
//! #         &self,
//! #         _speaker_id: &SpeakerId,
//! #         _event_xml: &str,
//! #     ) -> Result<sonos_stream::TypedEvent, sonos_stream::StrategyError> {
//! #         unimplemented!()
//! #     }
//! # }
//! let mut broker = EventBrokerBuilder::new()
//!     .with_strategy(Box::new(MyStrategy))
//!     .with_port_range(3400, 3500)
//!     .build().await?;
//!
//! // Create a speaker instance
//! let speaker = Speaker::new(
//!     SpeakerId::new("RINCON_000XXX"),
//!     "192.168.1.100".parse::<IpAddr>()?,
//!     "Living Room".to_string(),
//!     "Living Room".to_string(),
//! );
//!
//! // Subscribe to a service
//! broker.subscribe(&speaker, ServiceType::AVTransport).await?;
//!
//! // Get the event stream
//! let mut event_stream = broker.event_stream();
//!
//! // Process events
//! while let Some(event) = event_stream.recv().await {
//!     match event {
//!         Event::SubscriptionEstablished { speaker_id, service_type, .. } => {
//!             println!("Subscribed to {:?} on {:?}", service_type, speaker_id);
//!         }
//!         Event::ServiceEvent { speaker_id, event, .. } => {
//!             println!("Event from {:?}: {:?}", speaker_id, event);
//!         }
//!         Event::SubscriptionFailed { speaker_id, service_type, error } => {
//!             eprintln!("Failed to subscribe to {:?} on {:?}: {}", service_type, speaker_id, error);
//!         }
//!         _ => {}
//!     }
//! }
//!
//! // Unsubscribe when done
//! broker.unsubscribe(&speaker, ServiceType::AVTransport).await?;
//!
//! // Shutdown the broker
//! broker.shutdown().await?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Implementing a Custom Strategy
//!
//! To add support for a new UPnP service, implement the `SubscriptionStrategy` trait:
//!
//! ```rust,no_run
//! use sonos_stream::{
//!     SubscriptionStrategy, Subscription, Speaker, ServiceType, SubscriptionScope,
//!     SubscriptionConfig, StrategyError, TypedEvent, SpeakerId, SubscriptionError,
//! };
//! use std::time::Duration;
//!
//! struct MyServiceStrategy;
//!
//! #[async_trait::async_trait]
//! impl SubscriptionStrategy for MyServiceStrategy {
//!     fn service_type(&self) -> ServiceType {
//!         ServiceType::AVTransport
//!     }
//!
//!     fn subscription_scope(&self) -> SubscriptionScope {
//!         SubscriptionScope::PerSpeaker
//!     }
//!
//!     fn service_endpoint_path(&self) -> &'static str {
//!         "/MediaRenderer/AVTransport/Event"
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
//!
//! ## Error Handling
//!
//! The crate provides comprehensive error types for different failure scenarios:
//!
//! - `BrokerError`: Errors from the broker (missing strategy, duplicate subscription, etc.)
//! - `StrategyError`: Errors from strategy implementations (subscription creation, parsing, etc.)
//! - `SubscriptionError`: Errors from subscription operations (renewal, unsubscribe, etc.)
//!
//! All errors implement `std::error::Error` and provide detailed context about what went wrong.
//!
//! ## Event Types
//!
//! The broker emits various lifecycle and data events through the event stream:
//!
//! - `SubscriptionEstablished`: A subscription was successfully created
//! - `SubscriptionFailed`: A subscription failed to establish
//! - `SubscriptionRenewed`: A subscription was automatically renewed
//! - `SubscriptionExpired`: A subscription expired after all renewal attempts failed
//! - `SubscriptionRemoved`: A subscription was explicitly unsubscribed
//! - `ServiceEvent`: A parsed event from a service
//! - `ParseError`: An error occurred parsing an event
//!
//! ## Configuration
//!
//! The broker can be configured through the builder:
//!
//! ```rust,no_run
//! use sonos_stream::EventBrokerBuilder;
//! use std::time::Duration;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let broker = EventBrokerBuilder::new()
//!     .with_port_range(3400, 3500)                    // Callback server port range
//!     .with_subscription_timeout(Duration::from_secs(1800))  // 30 minutes
//!     .with_retry_config(3, Duration::from_secs(2))   // 3 retries, 2s base backoff
//!     .build().await?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Thread Safety
//!
//! All public types are thread-safe and can be shared across async tasks. The broker uses
//! internal locking to ensure safe concurrent access to subscription state.

// Module declarations
mod broker;
mod builder;
mod error;
mod event;
mod strategy;
mod subscription;
mod types;

// Re-export main broker types
pub use broker::{ActiveSubscription, EventBroker};
pub use builder::EventBrokerBuilder;

// Re-export callback types from callback-server crate
pub use callback_server::{CallbackServer, EventRouter};

// Re-export event types
pub use event::{AVTransportEvent, Event, EventData, ParsedEvent, TypedEvent};

// Re-export trait definitions
pub use strategy::{SubscriptionStrategy, AVTransportStrategy};
pub use subscription::{Subscription, UPnPSubscription};

// Re-export error types
pub use error::{BrokerError, StrategyError, SubscriptionError};

// Re-export core types
pub use types::{
    BrokerConfig, RawEvent, ServiceType, Speaker, SpeakerId, SubscriptionConfig, SubscriptionKey,
    SubscriptionScope,
};

// Re-export parser types from sonos-parser crate
pub use sonos_parser;
