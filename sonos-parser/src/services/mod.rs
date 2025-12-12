//! Service-specific parsers organized by UPnP service type
//!
//! This module contains parsers for different UPnP services used by Sonos devices.
//! Each service parser can be imported independently, allowing developers to use
//! only the parsers they need.
//!
//! ## Available Services
//!
//! - [`av_transport`]: Parser for AVTransport service events and responses
//!
//! ## Usage
//!
//! ### Import specific service parser
//! ```rust
//! use sonos_parser::services::av_transport::AVTransportParser;
//! ```
//!
//! ### Import all service parsers
//! ```rust
//! use sonos_parser::services::*;
//! ```

pub mod av_transport;