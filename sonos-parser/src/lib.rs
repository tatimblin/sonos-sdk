//! # sonos-parser
//!
//! A modular XML parsing library for Sonos UPnP service responses and events.
//! This crate provides parsing capabilities for various Sonos UPnP services,
//! with a focus on modularity and reusability across the Sonos SDK workspace.
//!
//! ## Usage
//!
//! ### Top-level convenience access
//! ```rust
//! use sonos_parser::{AVTransportParser, ParseResult};
//! 
//! let parser = AVTransportParser::from_xml(xml_content)?;
//! ```
//!
//! ### Service-specific access
//! ```rust
//! use sonos_parser::services::av_transport::AVTransportParser;
//! ```
//!
//! ### Common utilities access
//! ```rust
//! use sonos_parser::common::{ValueAttribute, NestedAttribute, DidlLite};
//! ```

pub mod error;
pub mod common;
pub mod services;

// Re-export error types for convenient top-level access
pub use error::{ParseError, ParseResult};

// Re-export common utilities for convenient top-level access
pub use common::{ValueAttribute, NestedAttribute, DidlLite, DidlItem, DidlResource};

// Re-export AVTransportParser for convenient top-level access
pub use services::av_transport::AVTransportParser;