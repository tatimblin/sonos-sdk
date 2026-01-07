//! AVTransport event decoder
//!
//! Handles playback state, track info, and position updates from AVTransport events.

use sonos_api::Service;

use crate::decoder::{
    parse_didl_album, parse_didl_album_art, parse_didl_artist, parse_didl_title,
    EventData, EventDecoder, PropertyUpdate, RawEvent,
};
use crate::property::{CurrentTrack, PlaybackState, Position};
use crate::store::StateStore;

/// Decoder for AVTransport events
///
/// Extracts playback state, current track info, and position from events.
pub struct AVTransportDecoder;

impl EventDecoder for AVTransportDecoder {
    fn services(&self) -> &[Service] {
        &[Service::AVTransport]
    }

    fn decode(&self, event: &RawEvent, store: &StateStore) -> Vec<PropertyUpdate> {
        let EventData::AVTransport(data) = &event.data else {
            return vec![];
        };

        // Look up speaker ID from IP
        let Some(speaker_id) = store.speaker_id_for_ip(event.speaker_ip) else {
            return vec![];
        };

        let mut updates = Vec::new();

        // Playback state
        if let Some(state_str) = &data.transport_state {
            let state = PlaybackState::from_transport_state(state_str);
            let id = speaker_id.clone();
            updates.push(PropertyUpdate::new(
                format!("Set {} playback state to {:?}", id, state),
                Service::AVTransport,
                move |store| {
                    store.set(&id, state);
                },
            ));
        }

        // Position
        if let Some(rel_time) = &data.rel_time {
            if let Some(position_ms) = Position::parse_time_to_ms(rel_time) {
                let duration_ms = data
                    .track_duration
                    .as_ref()
                    .and_then(|d| Position::parse_time_to_ms(d))
                    .unwrap_or(0);

                let id = speaker_id.clone();
                updates.push(PropertyUpdate::new(
                    format!("Set {} position to {}ms / {}ms", id, position_ms, duration_ms),
                    Service::AVTransport,
                    move |store| {
                        store.set(&id, Position::new(position_ms, duration_ms));
                    },
                ));
            }
        }

        // Track info
        if data.has_track_info() {
            let track = CurrentTrack {
                title: data.track_metadata.as_ref().and_then(|m| parse_didl_title(m)),
                artist: data.track_metadata.as_ref().and_then(|m| parse_didl_artist(m)),
                album: data.track_metadata.as_ref().and_then(|m| parse_didl_album(m)),
                album_art_uri: data
                    .track_metadata
                    .as_ref()
                    .and_then(|m| parse_didl_album_art(m)),
                uri: data.current_track_uri.clone(),
            };

            // Only update if track has meaningful content
            if !track.is_empty() {
                let id = speaker_id.clone();
                updates.push(PropertyUpdate::new(
                    format!("Set {} track to {}", id, track.display()),
                    Service::AVTransport,
                    move |store| {
                        store.set(&id, track);
                    },
                ));
            }
        }

        updates
    }

    fn name(&self) -> &'static str {
        "AVTransportDecoder"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decoder::AVTransportData;
    use crate::model::{SpeakerId, SpeakerInfo};

    fn create_test_store() -> StateStore {
        let store = StateStore::new();
        store.add_speaker(SpeakerInfo {
            id: SpeakerId::new("RINCON_123"),
            name: "Test".to_string(),
            room_name: "Test".to_string(),
            ip_address: "192.168.1.100".parse().unwrap(),
            port: 1400,
            model_name: "Test".to_string(),
            software_version: "1.0".to_string(),
            satellites: vec![],
        });
        store
    }

    #[test]
    fn test_decode_playback_state() {
        let decoder = AVTransportDecoder;
        let store = create_test_store();

        let event = RawEvent::new(
            "192.168.1.100".parse().unwrap(),
            Service::AVTransport,
            EventData::AVTransport(AVTransportData::new().with_transport_state("PLAYING")),
        );

        let updates = decoder.decode(&event, &store);
        assert_eq!(updates.len(), 1);

        for update in updates {
            update.apply(&store);
        }

        let id = SpeakerId::new("RINCON_123");
        assert_eq!(store.get::<PlaybackState>(&id), Some(PlaybackState::Playing));
    }

    #[test]
    fn test_decode_position() {
        let decoder = AVTransportDecoder;
        let store = create_test_store();

        let mut data = AVTransportData::new();
        data.rel_time = Some("0:01:30".to_string());
        data.track_duration = Some("0:03:45".to_string());

        let event = RawEvent::new(
            "192.168.1.100".parse().unwrap(),
            Service::AVTransport,
            EventData::AVTransport(data),
        );

        let updates = decoder.decode(&event, &store);
        assert_eq!(updates.len(), 1);

        for update in updates {
            update.apply(&store);
        }

        let id = SpeakerId::new("RINCON_123");
        let pos = store.get::<Position>(&id).unwrap();
        assert_eq!(pos.position_ms, 90_000); // 1:30 = 90 seconds
        assert_eq!(pos.duration_ms, 225_000); // 3:45 = 225 seconds
    }

    #[test]
    fn test_decode_track_info() {
        let decoder = AVTransportDecoder;
        let store = create_test_store();

        let mut data = AVTransportData::new();
        data.track_metadata = Some(
            r#"<DIDL-Lite><item>
                <dc:title>Test Song</dc:title>
                <dc:creator>Test Artist</dc:creator>
                <upnp:album>Test Album</upnp:album>
            </item></DIDL-Lite>"#
                .to_string(),
        );
        data.current_track_uri = Some("x-sonos-spotify:track123".to_string());

        let event = RawEvent::new(
            "192.168.1.100".parse().unwrap(),
            Service::AVTransport,
            EventData::AVTransport(data),
        );

        let updates = decoder.decode(&event, &store);
        // Should have track update
        assert!(!updates.is_empty());

        for update in updates {
            update.apply(&store);
        }

        let id = SpeakerId::new("RINCON_123");
        let track = store.get::<CurrentTrack>(&id).unwrap();
        assert_eq!(track.title, Some("Test Song".to_string()));
        assert_eq!(track.artist, Some("Test Artist".to_string()));
        assert_eq!(track.album, Some("Test Album".to_string()));
    }

    #[test]
    fn test_decode_full_event() {
        let decoder = AVTransportDecoder;
        let store = create_test_store();

        let mut data = AVTransportData::new();
        data.transport_state = Some("PLAYING".to_string());
        data.rel_time = Some("0:01:30".to_string());
        data.track_duration = Some("0:03:45".to_string());
        data.track_metadata = Some("<DIDL-Lite><dc:title>Song</dc:title></DIDL-Lite>".to_string());
        data.current_track_uri = Some("track:123".to_string());

        let event = RawEvent::new(
            "192.168.1.100".parse().unwrap(),
            Service::AVTransport,
            EventData::AVTransport(data),
        );

        let updates = decoder.decode(&event, &store);
        // Should have playback state, position, and track
        assert_eq!(updates.len(), 3);
    }
}
