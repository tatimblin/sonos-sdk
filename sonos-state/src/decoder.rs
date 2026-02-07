//! Event decoder - converts EnrichedEvent to typed property changes
//!
//! This module decodes raw events from sonos-stream into typed property
//! changes that can be applied to the StateStore.

use sonos_api::Service;
use sonos_stream::events::{
    AVTransportEvent, EnrichedEvent, EventData, RenderingControlEvent, ZoneGroupTopologyEvent,
};

use crate::model::SpeakerId;
use crate::property::{
    Bass, CurrentTrack, GroupMembership, Loudness, Mute, PlaybackState, Position, Treble, Volume,
};

/// Decoded changes from a single event
#[derive(Debug)]
pub struct DecodedChanges {
    /// Speaker ID the changes apply to
    pub speaker_id: SpeakerId,
    /// List of property changes
    pub changes: Vec<PropertyChange>,
}

/// A single property change
#[derive(Debug, Clone)]
pub enum PropertyChange {
    Volume(Volume),
    Mute(Mute),
    Bass(Bass),
    Treble(Treble),
    Loudness(Loudness),
    PlaybackState(PlaybackState),
    Position(Position),
    CurrentTrack(CurrentTrack),
    GroupMembership(GroupMembership),
}

impl PropertyChange {
    /// Get the property key for this change
    pub fn key(&self) -> &'static str {
        use crate::property::Property;
        match self {
            PropertyChange::Volume(_) => Volume::KEY,
            PropertyChange::Mute(_) => Mute::KEY,
            PropertyChange::Bass(_) => Bass::KEY,
            PropertyChange::Treble(_) => Treble::KEY,
            PropertyChange::Loudness(_) => Loudness::KEY,
            PropertyChange::PlaybackState(_) => PlaybackState::KEY,
            PropertyChange::Position(_) => Position::KEY,
            PropertyChange::CurrentTrack(_) => CurrentTrack::KEY,
            PropertyChange::GroupMembership(_) => GroupMembership::KEY,
        }
    }

    /// Get the service this property belongs to
    pub fn service(&self) -> Service {
        use crate::property::SonosProperty;
        match self {
            PropertyChange::Volume(_) => Volume::SERVICE,
            PropertyChange::Mute(_) => Mute::SERVICE,
            PropertyChange::Bass(_) => Bass::SERVICE,
            PropertyChange::Treble(_) => Treble::SERVICE,
            PropertyChange::Loudness(_) => Loudness::SERVICE,
            PropertyChange::PlaybackState(_) => PlaybackState::SERVICE,
            PropertyChange::Position(_) => Position::SERVICE,
            PropertyChange::CurrentTrack(_) => CurrentTrack::SERVICE,
            PropertyChange::GroupMembership(_) => GroupMembership::SERVICE,
        }
    }
}

/// Decode an enriched event into typed property changes
pub fn decode_event(event: &EnrichedEvent, speaker_id: SpeakerId) -> DecodedChanges {
    let changes = match &event.event_data {
        EventData::RenderingControlEvent(rc) => decode_rendering_control(rc),
        EventData::AVTransportEvent(avt) => decode_av_transport(avt),
        EventData::ZoneGroupTopologyEvent(zgt) => decode_topology(zgt),
        EventData::DevicePropertiesEvent(_) => vec![],
    };

    DecodedChanges { speaker_id, changes }
}

/// Decode RenderingControl event data
fn decode_rendering_control(event: &RenderingControlEvent) -> Vec<PropertyChange> {
    let mut changes = vec![];

    // Volume
    if let Some(vol_str) = &event.master_volume {
        if let Ok(vol) = vol_str.parse::<u8>() {
            changes.push(PropertyChange::Volume(Volume(vol.min(100))));
        }
    }

    // Mute
    if let Some(mute_str) = &event.master_mute {
        let muted = mute_str == "1" || mute_str.eq_ignore_ascii_case("true");
        changes.push(PropertyChange::Mute(Mute(muted)));
    }

    // Bass
    if let Some(bass_str) = &event.bass {
        if let Ok(bass) = bass_str.parse::<i8>() {
            changes.push(PropertyChange::Bass(Bass(bass.clamp(-10, 10))));
        }
    }

    // Treble
    if let Some(treble_str) = &event.treble {
        if let Ok(treble) = treble_str.parse::<i8>() {
            changes.push(PropertyChange::Treble(Treble(treble.clamp(-10, 10))));
        }
    }

    // Loudness
    if let Some(loudness_str) = &event.loudness {
        let loudness = loudness_str == "1" || loudness_str.eq_ignore_ascii_case("true");
        changes.push(PropertyChange::Loudness(Loudness(loudness)));
    }

    changes
}

/// Decode AVTransport event data
fn decode_av_transport(event: &AVTransportEvent) -> Vec<PropertyChange> {
    let mut changes = vec![];

    // Playback state
    if let Some(state) = &event.transport_state {
        let ps = match state.to_uppercase().as_str() {
            "PLAYING" => PlaybackState::Playing,
            "PAUSED_PLAYBACK" | "PAUSED" => PlaybackState::Paused,
            "STOPPED" => PlaybackState::Stopped,
            _ => PlaybackState::Transitioning,
        };
        changes.push(PropertyChange::PlaybackState(ps));
    }

    // Position
    if event.rel_time.is_some() || event.track_duration.is_some() {
        let position_ms = parse_duration_ms(event.rel_time.as_deref()).unwrap_or(0);
        let duration_ms = parse_duration_ms(event.track_duration.as_deref()).unwrap_or(0);

        let position = Position {
            position_ms,
            duration_ms,
        };
        changes.push(PropertyChange::Position(position));
    }

    // CurrentTrack
    if event.current_track_uri.is_some() || event.track_metadata.is_some() {
        // Parse metadata if available (track_metadata is raw XML, need to parse it)
        let (title, artist, album, album_art_uri) =
            parse_track_metadata(event.track_metadata.as_deref());

        let track = CurrentTrack {
            title,
            artist,
            album,
            album_art_uri,
            uri: event.current_track_uri.clone(),
        };
        changes.push(PropertyChange::CurrentTrack(track));
    }

    changes
}

/// Decode ZoneGroupTopology event data
fn decode_topology(_event: &ZoneGroupTopologyEvent) -> Vec<PropertyChange> {
    // Topology events typically update the system-wide topology
    // Individual speaker group membership would need to be extracted from the zone_groups
    // For now, return empty since this requires more complex processing
    vec![]
}

/// Parse duration string (HH:MM:SS or H:MM:SS) to milliseconds
fn parse_duration_ms(duration: Option<&str>) -> Option<u64> {
    let d = duration?;

    // Handle NOT_IMPLEMENTED or empty strings
    if d.is_empty() || d == "NOT_IMPLEMENTED" {
        return None;
    }

    let parts: Vec<&str> = d.split(':').collect();
    if parts.len() != 3 {
        return None;
    }

    let hours: u64 = parts[0].parse().ok()?;
    let minutes: u64 = parts[1].parse().ok()?;

    // Handle potential milliseconds in seconds part (HH:MM:SS.mmm)
    let seconds_parts: Vec<&str> = parts[2].split('.').collect();
    let seconds: u64 = seconds_parts[0].parse().ok()?;
    let millis: u64 = seconds_parts.get(1).and_then(|m| m.parse().ok()).unwrap_or(0);

    Some((hours * 3600 + minutes * 60 + seconds) * 1000 + millis)
}

/// Parse DIDL-Lite track metadata XML
fn parse_track_metadata(
    metadata: Option<&str>,
) -> (Option<String>, Option<String>, Option<String>, Option<String>) {
    let xml = match metadata {
        Some(m) if !m.is_empty() && m != "NOT_IMPLEMENTED" => m,
        _ => return (None, None, None, None),
    };

    // Simple XML extraction (could use quick-xml for more robust parsing)
    let title = extract_xml_element(xml, "dc:title");
    let artist = extract_xml_element(xml, "dc:creator")
        .or_else(|| extract_xml_element(xml, "r:albumArtist"));
    let album = extract_xml_element(xml, "upnp:album");
    let album_art_uri = extract_xml_element(xml, "upnp:albumArtURI");

    (title, artist, album, album_art_uri)
}

/// Extract content from an XML element (simple regex-free implementation)
fn extract_xml_element(xml: &str, element: &str) -> Option<String> {
    let start_tag = format!("<{}>", element);
    let end_tag = format!("</{}>", element);

    let start_idx = xml.find(&start_tag)? + start_tag.len();
    let end_idx = xml[start_idx..].find(&end_tag)? + start_idx;

    let content = &xml[start_idx..end_idx];

    // Unescape basic XML entities
    let unescaped = content
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
        .replace("&apos;", "'")
        .replace("&quot;", "\"");

    if unescaped.is_empty() {
        None
    } else {
        Some(unescaped)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_duration_ms() {
        assert_eq!(parse_duration_ms(Some("0:00:00")), Some(0));
        assert_eq!(parse_duration_ms(Some("0:01:00")), Some(60_000));
        assert_eq!(parse_duration_ms(Some("1:00:00")), Some(3_600_000));
        assert_eq!(parse_duration_ms(Some("0:03:45")), Some(225_000));
        assert_eq!(parse_duration_ms(Some("0:03:45.500")), Some(225_500));
        assert_eq!(parse_duration_ms(Some("NOT_IMPLEMENTED")), None);
        assert_eq!(parse_duration_ms(None), None);
        assert_eq!(parse_duration_ms(Some("")), None);
    }

    #[test]
    fn test_extract_xml_element() {
        let xml = r#"<DIDL-Lite><item><dc:title>Test Song</dc:title><dc:creator>Artist Name</dc:creator></item></DIDL-Lite>"#;

        assert_eq!(
            extract_xml_element(xml, "dc:title"),
            Some("Test Song".to_string())
        );
        assert_eq!(
            extract_xml_element(xml, "dc:creator"),
            Some("Artist Name".to_string())
        );
        assert_eq!(extract_xml_element(xml, "upnp:album"), None);
    }

    #[test]
    fn test_decode_rendering_control() {
        let event = RenderingControlEvent {
            master_volume: Some("50".to_string()),
            master_mute: Some("0".to_string()),
            bass: Some("5".to_string()),
            treble: Some("-3".to_string()),
            loudness: Some("1".to_string()),
            lf_volume: None,
            rf_volume: None,
            lf_mute: None,
            rf_mute: None,
            balance: None,
            other_channels: std::collections::HashMap::new(),
        };

        let changes = decode_rendering_control(&event);

        assert_eq!(changes.len(), 5);

        // Check volume
        if let PropertyChange::Volume(v) = &changes[0] {
            assert_eq!(v.0, 50);
        } else {
            panic!("Expected Volume change");
        }

        // Check mute
        if let PropertyChange::Mute(m) = &changes[1] {
            assert!(!m.0);
        } else {
            panic!("Expected Mute change");
        }
    }

    #[test]
    fn test_decode_av_transport() {
        let event = AVTransportEvent {
            transport_state: Some("PLAYING".to_string()),
            transport_status: None,
            speed: None,
            current_track_uri: Some("x-sonos-spotify:track123".to_string()),
            track_duration: Some("0:03:45".to_string()),
            rel_time: Some("0:01:30".to_string()),
            abs_time: None,
            rel_count: None,
            abs_count: None,
            play_mode: None,
            track_metadata: None,
            next_track_uri: None,
            next_track_metadata: None,
            queue_length: None,
        };

        let changes = decode_av_transport(&event);

        assert!(changes.len() >= 2);

        // Check playback state
        if let PropertyChange::PlaybackState(ps) = &changes[0] {
            assert_eq!(*ps, PlaybackState::Playing);
        } else {
            panic!("Expected PlaybackState change");
        }
    }

    #[test]
    fn test_property_change_key() {
        use crate::property::Property;

        let vol_change = PropertyChange::Volume(Volume(50));
        assert_eq!(vol_change.key(), Volume::KEY);

        let mute_change = PropertyChange::Mute(Mute(false));
        assert_eq!(mute_change.key(), Mute::KEY);

        let ps_change = PropertyChange::PlaybackState(PlaybackState::Playing);
        assert_eq!(ps_change.key(), PlaybackState::KEY);
    }

    #[test]
    fn test_property_change_service() {
        use crate::property::SonosProperty;

        let vol_change = PropertyChange::Volume(Volume(50));
        assert_eq!(vol_change.service(), Volume::SERVICE);

        let ps_change = PropertyChange::PlaybackState(PlaybackState::Playing);
        assert_eq!(ps_change.service(), PlaybackState::SERVICE);

        let gm_change =
            PropertyChange::GroupMembership(GroupMembership::new(None, true));
        assert_eq!(gm_change.service(), GroupMembership::SERVICE);
    }
}
