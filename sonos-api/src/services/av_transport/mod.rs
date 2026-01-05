//! AVTransport service for playback control and transport events
//!
//! This service handles transport operations (play, pause, stop) and
//! related events (transport state changes, track changes, etc.).
//!
//! # Control Operations
//! ```rust,ignore
//! use sonos_api::services::av_transport;
//!
//! let play_op = av_transport::play("1".to_string()).build()?;
//! client.execute("192.168.1.100", play_op)?;
//! ```
//!
//! # Event Subscriptions
//! ```rust,ignore
//! let subscription = av_transport::subscribe(&client, "192.168.1.100", "http://callback")?;
//! ```
//!
//! # Event Handling
//! ```rust,ignore
//! use sonos_api::services::av_transport::events::{AVTransportEventParser, create_enriched_event};
//! use sonos_api::events::EventSource;
//!
//! let parser = AVTransportEventParser;
//! let event_data = parser.parse_upnp_event(xml_content)?;
//! let enriched = create_enriched_event(speaker_ip, event_source, event_data);
//! ```

pub mod operations;
pub mod events;

// Re-export operations for convenience
pub use operations::*;

// Re-export event types and parsers
pub use events::{AVTransportEvent, AVTransportEventParser, create_enriched_event, create_enriched_event_with_registration_id};