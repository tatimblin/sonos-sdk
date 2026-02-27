//! Canonical ZoneGroupTopology service state type.
//!
//! Used by both UPnP event streaming (via `into_state()`) and polling (via `poll()`).

use serde::{Deserialize, Serialize};

use super::events::ZoneGroupInfo;
use crate::SonosClient;

/// Complete ZoneGroupTopology service state.
///
/// Canonical type used by both UPnP event streaming and polling.
/// Reuses existing public types from events.rs for zone group data.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ZoneGroupTopologyState {
    /// Complete zone group topology data
    pub zone_groups: Vec<ZoneGroupInfo>,

    /// Devices that have vanished from the network
    pub vanished_devices: Vec<String>,
}

/// Poll a speaker for complete ZoneGroupTopology state.
///
/// Calls GetZoneGroupState and parses the raw XML into structured topology data.
pub fn poll(client: &SonosClient, ip: &str) -> crate::Result<ZoneGroupTopologyState> {
    let response = client.execute_enhanced(
        ip,
        super::get_zone_group_state_operation()
            .build()
            .map_err(|e| crate::ApiError::ParseError(e.to_string()))?,
    )?;

    let zone_groups = super::events::parse_zone_group_state_xml(&response.zone_group_state)?;

    Ok(ZoneGroupTopologyState {
        zone_groups,
        vanished_devices: vec![],
    })
}
