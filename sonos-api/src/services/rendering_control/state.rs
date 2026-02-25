//! Canonical RenderingControl service state type.
//!
//! Used by both UPnP event streaming (via `into_state()`) and polling (via `poll()`).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::SonosClient;

/// Complete RenderingControl service state.
///
/// Canonical type used by both UPnP event streaming and polling.
/// Fields match the UPnP RenderingControl event data 1:1.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RenderingControlState {
    /// Current volume level (0-100) for Master channel
    pub master_volume: Option<String>,

    /// Current mute state for Master channel
    pub master_mute: Option<String>,

    /// Current volume level (0-100) for Left Front channel
    pub lf_volume: Option<String>,

    /// Current volume level (0-100) for Right Front channel
    pub rf_volume: Option<String>,

    /// Current mute state for Left Front channel
    pub lf_mute: Option<String>,

    /// Current mute state for Right Front channel
    pub rf_mute: Option<String>,

    /// Current bass level
    pub bass: Option<String>,

    /// Current treble level
    pub treble: Option<String>,

    /// Current loudness setting
    pub loudness: Option<String>,

    /// Balance setting (-100 to +100)
    pub balance: Option<String>,

    /// Additional channel configurations (can be extended)
    pub other_channels: HashMap<String, String>,
}

/// Poll a speaker for complete RenderingControl state.
///
/// Calls GetVolume (required), GetMute, GetBass, GetTreble, GetLoudness
/// (optional — fall back to None on failure).
pub fn poll(client: &SonosClient, ip: &str) -> crate::Result<RenderingControlState> {
    let volume = client.execute_enhanced(
        ip,
        super::get_volume_operation("Master".to_string())
            .build()
            .map_err(|e| crate::ApiError::ParseError(e.to_string()))?,
    )?;

    let mute = super::get_mute_operation("Master".to_string())
        .build()
        .ok()
        .and_then(|op| client.execute_enhanced(ip, op).ok());
    let bass = super::get_bass_operation()
        .build()
        .ok()
        .and_then(|op| client.execute_enhanced(ip, op).ok());
    let treble = super::get_treble_operation()
        .build()
        .ok()
        .and_then(|op| client.execute_enhanced(ip, op).ok());
    let loudness = super::get_loudness_operation("Master".to_string())
        .build()
        .ok()
        .and_then(|op| client.execute_enhanced(ip, op).ok());

    Ok(RenderingControlState {
        master_volume: Some(volume.current_volume.to_string()),
        master_mute: mute.map(|m| if m.current_mute { "1" } else { "0" }.to_string()),
        bass: bass.map(|b| b.current_bass.to_string()),
        treble: treble.map(|t| t.current_treble.to_string()),
        loudness: loudness.map(|l| if l.current_loudness { "1" } else { "0" }.to_string()),
        lf_volume: None,
        rf_volume: None,
        lf_mute: None,
        rf_mute: None,
        balance: None,
        other_channels: HashMap::new(),
    })
}
