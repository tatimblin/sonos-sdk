//! Background event worker for consuming events from SonosEventManager
//!
//! This module provides a background thread that consumes events from the
//! SonosEventManager and applies them to the StateStore.

use std::collections::HashSet;
use std::net::IpAddr;
use std::sync::{mpsc, Arc, RwLock};
use std::thread::{self, JoinHandle};

use sonos_event_manager::SonosEventManager;

use crate::decoder::{decode_event, PropertyChange};
use crate::model::SpeakerId;
use crate::state::{ChangeEvent, StateStore};

/// Spawns the state event worker thread
///
/// This worker:
/// - Consumes events from SonosEventManager's iterator
/// - Decodes them into typed property changes
/// - Applies changes to the StateStore
/// - Emits ChangeEvents for watched properties
pub(crate) fn spawn_state_event_worker(
    event_manager: Arc<SonosEventManager>,
    store: Arc<RwLock<StateStore>>,
    watched: Arc<RwLock<HashSet<(SpeakerId, &'static str)>>>,
    event_tx: mpsc::Sender<ChangeEvent>,
    ip_to_speaker: Arc<RwLock<std::collections::HashMap<IpAddr, SpeakerId>>>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        tracing::info!("State event worker started, waiting for events...");

        // Consume events from event manager (blocking)
        for event in event_manager.iter() {
            tracing::debug!(
                "Received event from {} for service {:?}",
                event.speaker_ip,
                event.service
            );

            // Look up speaker_id from IP
            let speaker_id = {
                let ip_map = match ip_to_speaker.read() {
                    Ok(m) => m,
                    Err(_) => {
                        tracing::warn!("Failed to acquire ip_to_speaker lock");
                        continue;
                    }
                };

                tracing::debug!(
                    "ip_to_speaker map has {} entries: {:?}",
                    ip_map.len(),
                    ip_map.keys().collect::<Vec<_>>()
                );

                match ip_map.get(&event.speaker_ip) {
                    Some(id) => id.clone(),
                    None => {
                        tracing::warn!(
                            "Received event from unknown speaker IP: {} (not in ip_to_speaker map)",
                            event.speaker_ip
                        );
                        continue;
                    }
                }
            };

            tracing::debug!(
                "Mapped IP {} to speaker_id {}",
                event.speaker_ip,
                speaker_id.as_str()
            );

            // Decode event
            let decoded = decode_event(&event, speaker_id.clone());
            tracing::debug!(
                "Decoded {} property changes from event",
                decoded.changes.len()
            );

            // Apply changes
            for change in decoded.changes {
                tracing::debug!("Applying change: {:?}", change);
                apply_property_change(&store, &watched, &event_tx, &speaker_id, change);
            }
        }

        tracing::info!("State event worker stopped");
    })
}

/// Apply a single property change to the store
fn apply_property_change(
    store: &Arc<RwLock<StateStore>>,
    watched: &Arc<RwLock<HashSet<(SpeakerId, &'static str)>>>,
    event_tx: &mpsc::Sender<ChangeEvent>,
    speaker_id: &SpeakerId,
    change: PropertyChange,
) {
    let key = change.key();
    let service = change.service();

    let changed = {
        let mut store = match store.write() {
            Ok(s) => s,
            Err(_) => {
                tracing::warn!("Failed to acquire store write lock");
                return;
            }
        };

        match change {
            PropertyChange::Volume(v) => store.set(speaker_id, v),
            PropertyChange::Mute(v) => store.set(speaker_id, v),
            PropertyChange::Bass(v) => store.set(speaker_id, v),
            PropertyChange::Treble(v) => store.set(speaker_id, v),
            PropertyChange::Loudness(v) => store.set(speaker_id, v),
            PropertyChange::PlaybackState(v) => store.set(speaker_id, v),
            PropertyChange::Position(v) => store.set(speaker_id, v),
            PropertyChange::CurrentTrack(v) => store.set(speaker_id, v),
            PropertyChange::GroupMembership(v) => store.set(speaker_id, v),
        }
    };

    if changed {
        let is_watched = watched
            .read()
            .map(|w| w.contains(&(speaker_id.clone(), key)))
            .unwrap_or(false);

        if is_watched {
            tracing::debug!(
                "Property {} changed for {}, emitting event",
                key,
                speaker_id.as_str()
            );
            let _ = event_tx.send(ChangeEvent::new(speaker_id.clone(), key, service));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::property::{Property, Volume};
    use sonos_api::Service;

    #[test]
    fn test_apply_property_change_volume() {
        let store = Arc::new(RwLock::new(StateStore::new()));
        let watched = Arc::new(RwLock::new(HashSet::new()));
        let (tx, rx) = mpsc::channel();

        let speaker_id = SpeakerId::new("test-speaker");

        // Add speaker to store first
        {
            let mut s = store.write().unwrap();
            s.add_speaker(crate::model::SpeakerInfo {
                id: speaker_id.clone(),
                name: "Test".to_string(),
                room_name: "Test".to_string(),
                ip_address: "192.168.1.100".parse().unwrap(),
                port: 1400,
                model_name: "Test".to_string(),
                software_version: "1.0".to_string(),
                satellites: vec![],
            });
        }

        // Apply change without watch
        apply_property_change(
            &store,
            &watched,
            &tx,
            &speaker_id,
            PropertyChange::Volume(Volume(50)),
        );

        // No event should be emitted (not watched)
        assert!(rx.try_recv().is_err());

        // Verify value was stored
        let stored: Option<Volume> = store.read().unwrap().get(&speaker_id);
        assert_eq!(stored, Some(Volume(50)));
    }

    #[test]
    fn test_apply_property_change_with_watch() {
        let store = Arc::new(RwLock::new(StateStore::new()));
        let watched = Arc::new(RwLock::new(HashSet::new()));
        let (tx, rx) = mpsc::channel();

        let speaker_id = SpeakerId::new("test-speaker");

        // Add speaker to store
        {
            let mut s = store.write().unwrap();
            s.add_speaker(crate::model::SpeakerInfo {
                id: speaker_id.clone(),
                name: "Test".to_string(),
                room_name: "Test".to_string(),
                ip_address: "192.168.1.100".parse().unwrap(),
                port: 1400,
                model_name: "Test".to_string(),
                software_version: "1.0".to_string(),
                satellites: vec![],
            });
        }

        // Register watch
        {
            let mut w = watched.write().unwrap();
            w.insert((speaker_id.clone(), Volume::KEY));
        }

        // Apply change
        apply_property_change(
            &store,
            &watched,
            &tx,
            &speaker_id,
            PropertyChange::Volume(Volume(75)),
        );

        // Event should be emitted
        let event = rx.try_recv().unwrap();
        assert_eq!(event.speaker_id, speaker_id);
        assert_eq!(event.property_key, Volume::KEY);
        assert_eq!(event.service, Service::RenderingControl);
    }
}

