//! AVTransport service parser module
//!
//! This module provides parsing capabilities for Sonos AVTransport UPnP service
//! events and responses. The AVTransport service handles media transport operations
//! like play, pause, stop, and provides information about the current track and
//! transport state.
//!
//! ## Usage
//!
//! ```rust
//! use sonos_parser::services::av_transport::AVTransportParser;
//!
//! let parser = AVTransportParser::from_xml(xml_content)?;
//! let state = parser.transport_state();
//! let title = parser.track_title();
//! ```

pub mod parser;

// Re-export the main parser type for clean public interface
pub use parser::AVTransportParser;