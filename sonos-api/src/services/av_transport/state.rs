//! Canonical AVTransport service state type.
//!
//! Used by both UPnP event streaming (via `into_state()`) and polling (via `poll()`).

use serde::{Deserialize, Serialize};

use crate::SonosClient;

/// Complete AVTransport service state.
///
/// Canonical type used by both UPnP event streaming and polling.
/// Fields match the UPnP AVTransport event data 1:1.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AVTransportState {
    /// Current transport state (PLAYING, PAUSED_PLAYBACK, STOPPED, etc.)
    pub transport_state: Option<String>,

    /// Current transport status (OK, ERROR_OCCURRED, etc.)
    pub transport_status: Option<String>,

    /// Current playback speed
    pub speed: Option<String>,

    /// Current track URI
    pub current_track_uri: Option<String>,

    /// Track duration
    pub track_duration: Option<String>,

    /// Current track metadata (DIDL-Lite XML)
    pub track_metadata: Option<String>,

    /// Relative time position in current track
    pub rel_time: Option<String>,

    /// Absolute time position
    pub abs_time: Option<String>,

    /// Relative track number in queue.
    /// UPnP returns i32 (-1 means "not implemented"); negative values map to None.
    pub rel_count: Option<u32>,

    /// Absolute track number.
    /// UPnP returns i32 (-1 means "not implemented"); negative values map to None.
    pub abs_count: Option<u32>,

    /// Current play mode (NORMAL, REPEAT_ALL, REPEAT_ONE, SHUFFLE, etc.)
    pub play_mode: Option<String>,

    /// Next track URI
    pub next_track_uri: Option<String>,

    /// Next track metadata
    pub next_track_metadata: Option<String>,

    /// Queue size/length
    pub queue_length: Option<u32>,
}

/// Poll a speaker for complete AVTransport state.
///
/// Calls GetTransportInfo (required), GetPositionInfo, GetTransportSettings,
/// and GetMediaInfo (optional — fall back to None on failure).
pub fn poll(client: &SonosClient, ip: &str) -> crate::Result<AVTransportState> {
    let transport = client.execute_enhanced(
        ip,
        super::get_transport_info_operation().build()
            .map_err(|e| crate::ApiError::ParseError(e.to_string()))?,
    )?;

    let position = super::get_position_info_operation()
        .build()
        .ok()
        .and_then(|op| client.execute_enhanced(ip, op).ok());
    let settings = super::get_transport_settings_operation()
        .build()
        .ok()
        .and_then(|op| client.execute_enhanced(ip, op).ok());
    let media = super::get_media_info_operation()
        .build()
        .ok()
        .and_then(|op| client.execute_enhanced(ip, op).ok());

    Ok(AVTransportState {
        transport_state: Some(transport.current_transport_state),
        transport_status: Some(transport.current_transport_status),
        speed: Some(transport.current_speed),
        current_track_uri: position.as_ref().map(|p| p.track_uri.clone()),
        track_duration: position.as_ref().map(|p| p.track_duration.clone()),
        track_metadata: position.as_ref().map(|p| p.track_meta_data.clone()),
        rel_time: position.as_ref().map(|p| p.rel_time.clone()),
        abs_time: position.as_ref().map(|p| p.abs_time.clone()),
        rel_count: position.as_ref().and_then(|p| u32::try_from(p.rel_count).ok()),
        abs_count: position.as_ref().and_then(|p| u32::try_from(p.abs_count).ok()),
        play_mode: settings.map(|s| s.play_mode),
        next_track_uri: media.as_ref().map(|m| m.next_uri.clone()),
        next_track_metadata: media.as_ref().map(|m| m.next_uri_meta_data.clone()),
        queue_length: media.map(|m| m.nr_tracks),
    })
}
