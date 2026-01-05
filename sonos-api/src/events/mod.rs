//! Event handling framework for Sonos UPnP services
//!
//! This module provides a comprehensive event handling system that consolidates
//! event processing logic from across the Sonos SDK. Each service handles its
//! own event types and parsing logic, while this framework provides common
//! infrastructure for event processing, routing, and management.
//!
//! # Architecture
//!
//! - **Service-specific events**: Each service module (av_transport, rendering_control, etc.)
//!   defines its own event types and parsing logic in an `events` submodule
//! - **Common framework**: This module provides generic event processing infrastructure
//! - **Type safety**: Events are strongly typed per service while maintaining flexibility
//!
//! # Usage
//!
//! ## Service-specific event handling
//! ```rust,ignore
//! use sonos_api::services::av_transport;
//! use sonos_api::events::EventSource;
//!
//! // Parse an AVTransport event
//! let parser = av_transport::events::AVTransportEventParser;
//! let event_data = parser.parse_upnp_event(xml_content)?;
//!
//! // Create enriched event
//! let enriched = av_transport::events::create_enriched_event(
//!     speaker_ip,
//!     EventSource::UPnPNotification { subscription_id: "uuid:123".to_string() },
//!     event_data
//! );
//! ```
//!
//! ## Generic event processing
//! ```rust,ignore
//! use sonos_api::events::{EventParserRegistry, EventProcessor};
//!
//! // Register parsers for all services
//! let mut registry = EventParserRegistry::new();
//! registry.register(av_transport::events::AVTransportEventParser);
//! registry.register(rendering_control::events::RenderingControlEventParser);
//!
//! // Process events generically
//! let processor = EventProcessor::new(registry);
//! processor.process_event(service, xml_content, event_source)?;
//! ```

pub mod types;
pub mod processor;
pub mod xml_utils;

// Re-export common types for convenience
pub use types::{
    EnrichedEvent, EventSource, EventParser, EventParserRegistry, EventParserDyn,
    extract_xml_value,
};
pub use processor::EventProcessor;
pub use xml_utils::{
    DidlLite, DidlItem, DidlResource, ValueAttribute, NestedAttribute,
    strip_namespaces, deserialize_nested, parse,
};