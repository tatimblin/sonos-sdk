//! ZoneGroupTopology service parser module
//!
//! This module provides parsing capabilities for Sonos ZoneGroupTopology UPnP service
//! events and responses. The ZoneGroupTopology service handles speaker grouping,
//! coordinator relationships, and provides comprehensive information about all speakers
//! in the household including their network configuration and grouping state.
//!
//! ## Usage
//!
//! ```rust,ignore
//! use sonos_parser::services::zone_group_topology::ZoneGroupTopologyParser;
//!
//! let parser = ZoneGroupTopologyParser::from_xml(xml_content)?;
//! if let Some(zone_groups) = parser.zone_groups() {
//!     let coordinator = &zone_groups[0].coordinator;
//! }
//! ```

pub mod parser;

// Re-export the main parser type for clean public interface
pub use parser::ZoneGroupTopologyParser;