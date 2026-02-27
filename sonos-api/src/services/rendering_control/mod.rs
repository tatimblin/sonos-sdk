//! RenderingControl service for audio rendering operations and events
//!
//! This service handles audio rendering operations on individual Sonos speakers
//! and related events (volume changes, mute state changes, etc.).
//!
//! # Operations
//!
//! | Operation | Description |
//! |-----------|-------------|
//! | `get_volume` / `set_volume` | Get/set volume level (0-100) |
//! | `set_relative_volume` | Adjust volume relatively (-100 to +100) |
//! | `get_mute` / `set_mute` | Get/set mute state |
//! | `get_bass` / `set_bass` | Get/set bass level (-10 to +10) |
//! | `get_treble` / `set_treble` | Get/set treble level (-10 to +10) |
//! | `get_loudness` / `set_loudness` | Get/set loudness compensation |
//!
//! # Examples
//! ```rust,ignore
//! use sonos_api::services::rendering_control;
//!
//! // Volume
//! let op = rendering_control::set_volume("Master".to_string(), 75).build()?;
//! client.execute("192.168.1.100", op)?;
//!
//! // Mute
//! let op = rendering_control::get_mute("Master".to_string()).build()?;
//! let response = client.execute_enhanced("192.168.1.100", op)?;
//! println!("Muted: {}", response.current_mute);
//!
//! // Bass / Treble
//! let op = rendering_control::set_bass(5).build()?;
//! client.execute("192.168.1.100", op)?;
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
pub mod state;

// Re-export operations for convenience
pub use operations::*;

// Re-export event types and parsers
pub use events::{RenderingControlEvent, RenderingControlEventParser, create_enriched_event, create_enriched_event_with_registration_id};
pub use state::RenderingControlState;