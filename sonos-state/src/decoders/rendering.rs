//! RenderingControl event decoder
//!
//! Handles volume, mute, and EQ property updates from RenderingControl events.

use sonos_api::Service;

use crate::decoder::{EventData, EventDecoder, PropertyUpdate, RawEvent};
use crate::property::{Bass, Loudness, Mute, Treble, Volume};
use crate::store::StateStore;

/// Decoder for RenderingControl events
///
/// Extracts volume, mute, and EQ settings from events and creates
/// property updates for affected speakers.
pub struct RenderingControlDecoder;

impl EventDecoder for RenderingControlDecoder {
    fn services(&self) -> &[Service] {
        &[Service::RenderingControl]
    }

    fn decode(&self, event: &RawEvent, store: &StateStore) -> Vec<PropertyUpdate> {
        let EventData::RenderingControl(data) = &event.data else {
            return vec![];
        };

        // Look up speaker ID from IP
        let Some(speaker_id) = store.speaker_id_for_ip(event.speaker_ip) else {
            // Unknown speaker - may need topology update first
            return vec![];
        };

        let mut updates = Vec::new();

        // Volume
        if let Some(volume) = data.master_volume {
            let id = speaker_id.clone();
            updates.push(PropertyUpdate::new(
                format!("Set {} volume to {}", id, volume),
                Service::RenderingControl,
                move |store| {
                    store.set(&id, Volume::new(volume));
                },
            ));
        }

        // Mute
        if let Some(muted) = data.master_mute {
            let id = speaker_id.clone();
            updates.push(PropertyUpdate::new(
                format!("Set {} mute to {}", id, muted),
                Service::RenderingControl,
                move |store| {
                    store.set(&id, Mute::new(muted));
                },
            ));
        }

        // Bass
        if let Some(bass) = data.bass {
            let id = speaker_id.clone();
            updates.push(PropertyUpdate::new(
                format!("Set {} bass to {}", id, bass),
                Service::RenderingControl,
                move |store| {
                    store.set(&id, Bass::new(bass));
                },
            ));
        }

        // Treble
        if let Some(treble) = data.treble {
            let id = speaker_id.clone();
            updates.push(PropertyUpdate::new(
                format!("Set {} treble to {}", id, treble),
                Service::RenderingControl,
                move |store| {
                    store.set(&id, Treble::new(treble));
                },
            ));
        }

        // Loudness
        if let Some(loudness) = data.loudness {
            let id = speaker_id.clone();
            updates.push(PropertyUpdate::new(
                format!("Set {} loudness to {}", id, loudness),
                Service::RenderingControl,
                move |store| {
                    store.set(&id, Loudness::new(loudness));
                },
            ));
        }

        updates
    }

    fn name(&self) -> &'static str {
        "RenderingControlDecoder"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decoder::RenderingControlData;
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
    fn test_decode_volume() {
        let decoder = RenderingControlDecoder;
        let store = create_test_store();

        let event = RawEvent::new(
            "192.168.1.100".parse().unwrap(),
            Service::RenderingControl,
            EventData::RenderingControl(RenderingControlData::new().with_volume(75)),
        );

        let updates = decoder.decode(&event, &store);
        assert_eq!(updates.len(), 1);

        // Apply update
        for update in updates {
            update.apply(&store);
        }

        // Check result
        let id = SpeakerId::new("RINCON_123");
        assert_eq!(store.get::<Volume>(&id), Some(Volume::new(75)));
    }

    #[test]
    fn test_decode_multiple_properties() {
        let decoder = RenderingControlDecoder;
        let store = create_test_store();

        let mut data = RenderingControlData::new();
        data.master_volume = Some(50);
        data.master_mute = Some(true);
        data.bass = Some(5);

        let event = RawEvent::new(
            "192.168.1.100".parse().unwrap(),
            Service::RenderingControl,
            EventData::RenderingControl(data),
        );

        let updates = decoder.decode(&event, &store);
        assert_eq!(updates.len(), 3);

        for update in updates {
            update.apply(&store);
        }

        let id = SpeakerId::new("RINCON_123");
        assert_eq!(store.get::<Volume>(&id), Some(Volume::new(50)));
        assert_eq!(store.get::<Mute>(&id), Some(Mute::new(true)));
        assert_eq!(store.get::<Bass>(&id), Some(Bass::new(5)));
    }

    #[test]
    fn test_unknown_speaker() {
        let decoder = RenderingControlDecoder;
        let store = StateStore::new(); // Empty store

        let event = RawEvent::new(
            "192.168.1.100".parse().unwrap(),
            Service::RenderingControl,
            EventData::RenderingControl(RenderingControlData::new().with_volume(50)),
        );

        let updates = decoder.decode(&event, &store);
        assert!(updates.is_empty()); // No speaker with this IP
    }
}
