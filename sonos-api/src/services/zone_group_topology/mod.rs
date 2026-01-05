//! ZoneGroupTopology service for topology operations and events
//!
//! This service handles zone group topology operations and related events
//! (zone group changes, speaker joining/leaving groups, etc.).
//!
//! # Control Operations
//! ```rust,ignore
//! use sonos_api::services::zone_group_topology;
//!
//! let topology_op = zone_group_topology::get_zone_group_state().build()?;
//! let response = client.execute("192.168.1.100", topology_op)?;
//! ```
//!
//! # Event Subscriptions
//! ```rust,ignore
//! let subscription = zone_group_topology::subscribe(&client, "192.168.1.100", "http://callback")?;
//! ```
//!
//! # Event Handling
//! ```rust,ignore
//! use sonos_api::services::zone_group_topology::events::{ZoneGroupTopologyEventParser, create_enriched_event};
//! use sonos_api::events::EventSource;
//!
//! let parser = ZoneGroupTopologyEventParser;
//! let event_data = parser.parse_upnp_event(xml_content)?;
//! let enriched = create_enriched_event(speaker_ip, event_source, event_data);
//! ```

pub mod operations;
pub mod events;

// Re-export operations for convenience
pub use operations::*;

// Re-export event types and parsers
pub use events::{
    ZoneGroupTopologyEvent, ZoneGroupInfo, ZoneGroupMemberInfo, NetworkInfo, SatelliteInfo,
    ZoneGroupTopologyEventParser, create_enriched_event, create_enriched_event_with_registration_id
};