//! XML parsers for UPnP service events.
//!
//! This module contains serde-based parsers for various UPnP service events.
//! Each parser handles the specific XML structure of its service type.

pub mod av_transport;
pub mod xml_decode;

pub use av_transport::{AVTransportParser, DidlLite, DidlItem, LastChangeEvent};
