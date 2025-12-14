//! Common utilities and data structures shared across UPnP services
//!
//! This module contains shared parsing utilities, helper types, and data structures
//! that can be reused across different UPnP service parsers.
//!
//! ## Available Utilities
//!
//! - [`xml_decode`]: XML decoding utilities and custom deserializers
//! - [`attributes`]: Helper types for UPnP XML attribute patterns
//! - [`didl`]: DIDL-Lite data structures for media metadata
//!
//! ## Usage
//!
//! ```rust
//! use sonos_parser::common::{ValueAttribute, NestedAttribute, DidlLite};
//! ```

pub mod xml_decode;
pub mod attributes;
pub mod didl;

// Re-export commonly used types for convenient access
pub use attributes::{ValueAttribute, NestedAttribute};
pub use didl::{DidlLite, DidlItem, DidlResource};