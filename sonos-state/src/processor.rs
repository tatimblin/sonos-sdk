//! State processor for converting events to state changes

use std::collections::HashMap;
use std::net::IpAddr;

use crate::{
    Group, GroupId, PlaybackState, SpeakerId, SpeakerRef, StateCache, StateChange, StateEvent,
    StateEventPayload, TrackInfo,
};

/// Processes events and updates state, emitting state changes
pub struct StateProcessor {
    cache: StateCache,
    /// Map speaker IP to speaker ID for event routing
    ip_to_speaker: HashMap<IpAddr, SpeakerId>,
}

impl StateProcessor {
    /// Create a new StateProcessor with the given cache
    pub fn new(cache: StateCache) -> Self {
        Self {
            cache,
            ip_to_speaker: HashMap::new(),
        }
    }

    /// Rebuild IP-to-speaker mapping from current state
    pub fn rebuild_ip_mapping(&mut self) {
        self.ip_to_speaker.clear();
        for speaker_state in self.cache.get_all_speakers() {
            self.ip_to_speaker.insert(
                speaker_state.speaker.ip_address,
                speaker_state.speaker.id.clone(),
            );
        }
    }

    /// Process an event and return any state changes
    pub fn process_event(&mut self, event: StateEvent) -> Vec<StateChange> {
        let mut changes = Vec::new();

        match event.payload {
            StateEventPayload::Transport {
                transport_state,
                current_track_uri,
                track_duration,
                rel_time,
                track_metadata,
            } => {
                if let Some(speaker_id) = self.ip_to_speaker.get(&event.speaker_ip).cloned() {
                    // Process transport state
                    if let Some(state_str) = transport_state {
                        let playback_state = PlaybackState::from_transport_state(&state_str);
                        if let Some(change) =
                            self.cache.update_playback_state(&speaker_id, playback_state)
                        {
                            changes.push(change);
                        }
                    }

                    // Process position
                    if let Some(rel_time_str) = rel_time {
                        if let Some(position_ms) = parse_time_to_ms(&rel_time_str) {
                            let duration_ms = track_duration
                                .as_ref()
                                .and_then(|d| parse_time_to_ms(d))
                                .unwrap_or(0);

                            if let Some(change) =
                                self.cache.update_position(&speaker_id, position_ms, duration_ms)
                            {
                                changes.push(change);
                            }
                        }
                    }

                    // Process track info
                    if track_metadata.is_some() || current_track_uri.is_some() {
                        let track_info = TrackInfo {
                            title: track_metadata.as_ref().and_then(|m| parse_didl_title(m)),
                            artist: track_metadata.as_ref().and_then(|m| parse_didl_artist(m)),
                            album: track_metadata.as_ref().and_then(|m| parse_didl_album(m)),
                            duration_ms: track_duration.as_ref().and_then(|d| parse_time_to_ms(d)),
                            uri: current_track_uri,
                            album_art_uri: track_metadata
                                .as_ref()
                                .and_then(|m| parse_didl_album_art(m)),
                        };

                        // Only update if track has meaningful content
                        let track_option = if track_info.is_empty() {
                            None
                        } else {
                            Some(track_info)
                        };

                        if let Some(change) = self.cache.update_track(&speaker_id, track_option) {
                            changes.push(change);
                        }
                    }
                }
            }

            StateEventPayload::Rendering {
                master_volume,
                master_mute,
            } => {
                if let Some(speaker_id) = self.ip_to_speaker.get(&event.speaker_ip).cloned() {
                    if let Some(volume) = master_volume {
                        if let Some(change) = self.cache.update_volume(&speaker_id, volume) {
                            changes.push(change);
                        }
                    }

                    if let Some(muted) = master_mute {
                        if let Some(change) = self.cache.update_mute(&speaker_id, muted) {
                            changes.push(change);
                        }
                    }
                }
            }

            StateEventPayload::Topology { zone_groups } => {
                // Convert to Group structs
                let groups: Vec<Group> = zone_groups
                    .into_iter()
                    .map(|zg| {
                        let members = zg
                            .members
                            .into_iter()
                            .map(|m| {
                                SpeakerRef::new(
                                    SpeakerId::new(&m.uuid),
                                    m.satellites.into_iter().map(SpeakerId::new).collect(),
                                )
                            })
                            .collect();

                        Group::new(
                            GroupId::new(&zg.id),
                            SpeakerId::new(&zg.coordinator),
                            members,
                        )
                    })
                    .collect();

                if let Some(change) = self.cache.set_groups(groups) {
                    changes.push(change);
                }
            }
        }

        changes
    }

    /// Get a reference to the underlying cache
    pub fn cache(&self) -> &StateCache {
        &self.cache
    }
}

/// Parse a time string (HH:MM:SS or HH:MM:SS.mmm) to milliseconds
fn parse_time_to_ms(time_str: &str) -> Option<u64> {
    // Handle "NOT_IMPLEMENTED" or other invalid values
    if !time_str.contains(':') {
        return None;
    }

    let parts: Vec<&str> = time_str.split(':').collect();
    if parts.len() != 3 {
        return None;
    }

    let hours: u64 = parts[0].parse().ok()?;
    let minutes: u64 = parts[1].parse().ok()?;

    // Handle optional milliseconds
    let seconds_parts: Vec<&str> = parts[2].split('.').collect();
    let seconds: u64 = seconds_parts[0].parse().ok()?;
    let millis: u64 = seconds_parts.get(1).and_then(|m| m.parse().ok()).unwrap_or(0);

    Some((hours * 3600 + minutes * 60 + seconds) * 1000 + millis)
}

/// Parse title from DIDL-Lite metadata
fn parse_didl_title(metadata: &str) -> Option<String> {
    extract_xml_value(metadata, "dc:title")
}

/// Parse artist from DIDL-Lite metadata
fn parse_didl_artist(metadata: &str) -> Option<String> {
    extract_xml_value(metadata, "dc:creator")
        .or_else(|| extract_xml_value(metadata, "upnp:artist"))
}

/// Parse album from DIDL-Lite metadata
fn parse_didl_album(metadata: &str) -> Option<String> {
    extract_xml_value(metadata, "upnp:album")
}

/// Parse album art URI from DIDL-Lite metadata
fn parse_didl_album_art(metadata: &str) -> Option<String> {
    extract_xml_value(metadata, "upnp:albumArtURI")
}

/// Extract value from XML element
fn extract_xml_value(xml: &str, tag: &str) -> Option<String> {
    // Try standard format: <tag>value</tag>
    let start_tag = format!("<{}>", tag);
    let end_tag = format!("</{}>", tag);

    if let Some(start) = xml.find(&start_tag) {
        let start = start + start_tag.len();
        if let Some(end) = xml[start..].find(&end_tag) {
            let value = &xml[start..start + end];
            // Unescape XML entities
            return Some(unescape_xml(value));
        }
    }

    // Try with namespace prefix variations
    // e.g., <r:title> instead of <dc:title>
    let tag_name = tag.split(':').last().unwrap_or(tag);
    for prefix in &["dc:", "r:", "upnp:", ""] {
        let alt_start = format!("<{}{}>", prefix, tag_name);
        let alt_end = format!("</{}{}>", prefix, tag_name);

        if let Some(start) = xml.find(&alt_start) {
            let start = start + alt_start.len();
            if let Some(end) = xml[start..].find(&alt_end) {
                let value = &xml[start..start + end];
                return Some(unescape_xml(value));
            }
        }
    }

    None
}

/// Unescape XML entities
fn unescape_xml(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_time_to_ms() {
        assert_eq!(parse_time_to_ms("0:00:00"), Some(0));
        assert_eq!(parse_time_to_ms("0:01:00"), Some(60_000));
        assert_eq!(parse_time_to_ms("1:00:00"), Some(3_600_000));
        assert_eq!(parse_time_to_ms("0:03:45"), Some(225_000));
        assert_eq!(parse_time_to_ms("0:03:45.500"), Some(225_500));
        assert_eq!(parse_time_to_ms("NOT_IMPLEMENTED"), None);
        assert_eq!(parse_time_to_ms("invalid"), None);
    }

    #[test]
    fn test_extract_xml_value() {
        let xml = r#"<DIDL-Lite><item><dc:title>Test Song</dc:title></item></DIDL-Lite>"#;
        assert_eq!(extract_xml_value(xml, "dc:title"), Some("Test Song".to_string()));

        let xml2 = r#"<item><dc:creator>Test Artist</dc:creator></item>"#;
        assert_eq!(extract_xml_value(xml2, "dc:creator"), Some("Test Artist".to_string()));
    }

    #[test]
    fn test_extract_xml_value_with_entities() {
        let xml = r#"<dc:title>Rock &amp; Roll</dc:title>"#;
        assert_eq!(extract_xml_value(xml, "dc:title"), Some("Rock & Roll".to_string()));
    }

    #[test]
    fn test_unescape_xml() {
        assert_eq!(unescape_xml("a &amp; b"), "a & b");
        assert_eq!(unescape_xml("&lt;tag&gt;"), "<tag>");
    }
}
