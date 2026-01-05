//! RenderingControl service for audio rendering operations and events
//!
//! This service handles audio rendering operations (volume, mute, bass, treble) and
//! related events (volume changes, mute state changes, etc.).
//!
//! # Control Operations
//! ```rust,ignore
//! use sonos_api::services::rendering_control;
//!
//! let vol_op = rendering_control::set_volume("Master".to_string(), 75).build()?;
//! client.execute("192.168.1.100", vol_op)?;
//! ```
//!
//! # Event Subscriptions
//! ```rust,ignore
//! let subscription = rendering_control::subscribe(&client, "192.168.1.100", "http://callback")?;
//! ```
//!
//! # Event Handling
//! ```rust,ignore
//! use sonos_api::services::rendering_control::events::{RenderingControlEventParser, create_enriched_event};
//! use sonos_api::events::EventSource;
//!
//! let parser = RenderingControlEventParser;
//! let event_data = parser.parse_upnp_event(xml_content)?;
//! let enriched = create_enriched_event(speaker_ip, event_source, event_data);
//! ```

pub mod operations;
pub mod events;

// Re-export operations for convenience
pub use operations::*;

// Re-export event types and parsers
pub use events::{RenderingControlEvent, RenderingControlEventParser, create_enriched_event, create_enriched_event_with_registration_id};