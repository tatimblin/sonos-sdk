//! Canonical GroupRenderingControl service state type.
//!
//! Used by both UPnP event streaming (via `into_state()`) and polling (via `poll()`).

use serde::{Deserialize, Serialize};

use crate::SonosClient;

/// Complete GroupRenderingControl service state.
///
/// Canonical type used by both UPnP event streaming and polling.
/// Fields match the UPnP GroupRenderingControl event data 1:1.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GroupRenderingControlState {
    /// Current group volume level (0-100)
    pub group_volume: Option<u16>,

    /// Whether the group is muted
    pub group_mute: Option<bool>,

    /// Whether the group volume is changeable
    pub group_volume_changeable: Option<bool>,
}

/// Poll a speaker for complete GroupRenderingControl state.
///
/// Calls GetGroupVolume (required), GetGroupMute (optional).
/// GroupVolumeChangeable has no Get operation — always None when polled.
pub fn poll(client: &SonosClient, ip: &str) -> crate::Result<GroupRenderingControlState> {
    let volume = client.execute_enhanced(
        ip,
        super::get_group_volume_operation()
            .build()
            .map_err(|e| crate::ApiError::ParseError(e.to_string()))?,
    )?;

    let mute = super::get_group_mute_operation()
        .build()
        .ok()
        .and_then(|op| client.execute_enhanced(ip, op).ok());

    Ok(GroupRenderingControlState {
        group_volume: Some(volume.current_volume),
        group_mute: mute.map(|m| m.current_mute),
        group_volume_changeable: None,
    })
}
