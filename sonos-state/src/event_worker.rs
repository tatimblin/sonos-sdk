//! Background event worker for consuming events from SonosEventManager
//!
//! This module provides a background thread that consumes events from the
//! SonosEventManager and applies them to the StateStore.

use std::collections::HashSet;
use std::net::IpAddr;
use std::sync::{mpsc, Arc, RwLock};
use std::thread::{self, JoinHandle};

use sonos_api::Service;
use sonos_event_manager::SonosEventManager;
use sonos_stream::events::EventData;

use crate::decoder::{decode_event, decode_topology_event, PropertyChange, TopologyChanges};
use crate::model::SpeakerId;
use crate::property::{GroupMembership, Property};
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

            // Handle ZoneGroupTopology events specially - they affect all speakers
            if let EventData::ZoneGroupTopologyEvent(ref zgt_event) = event.event_data {
                tracing::debug!("Processing ZoneGroupTopology event");
                let topology_changes = decode_topology_event(zgt_event);
                apply_topology_changes(&store, &watched, &event_tx, topology_changes);
                continue;
            }

            // Look up speaker_id from IP for non-topology events
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

/// Apply topology changes from a ZoneGroupTopology event
///
/// This function:
/// 1. Clears existing groups from the store
/// 2. Adds new groups from the TopologyChanges
/// 3. Updates GroupMembership for each speaker
/// 4. Emits change events for watched GroupMembership properties
fn apply_topology_changes(
    store: &Arc<RwLock<StateStore>>,
    watched: &Arc<RwLock<HashSet<(SpeakerId, &'static str)>>>,
    event_tx: &mpsc::Sender<ChangeEvent>,
    changes: TopologyChanges,
) {
    tracing::debug!(
        "Applying topology changes: {} groups, {} memberships",
        changes.groups.len(),
        changes.memberships.len()
    );

    // Apply all changes within a single write lock
    let membership_changes: Vec<(SpeakerId, bool)> = {
        let mut store = match store.write() {
            Ok(s) => s,
            Err(_) => {
                tracing::warn!("Failed to acquire store write lock for topology changes");
                return;
            }
        };

        // 1. Clear existing groups
        store.clear_groups();

        // 2. Add new groups
        for group in changes.groups {
            tracing::debug!(
                "Adding group {} with {} members",
                group.id.as_str(),
                group.member_ids.len()
            );
            store.add_group(group);
        }

        // 3. Update GroupMembership for each speaker and track which ones changed
        let mut changed_memberships = Vec::new();
        for (speaker_id, membership) in changes.memberships {
            let changed = store.set(&speaker_id, membership);
            changed_memberships.push((speaker_id, changed));
        }

        changed_memberships
    };

    // 4. Emit change events for watched properties (outside the write lock)
    let watched_set = match watched.read() {
        Ok(w) => w,
        Err(_) => {
            tracing::warn!("Failed to acquire watched lock");
            return;
        }
    };

    for (speaker_id, changed) in membership_changes {
        if changed && watched_set.contains(&(speaker_id.clone(), GroupMembership::KEY)) {
            tracing::debug!(
                "GroupMembership changed for {}, emitting event",
                speaker_id.as_str()
            );
            let _ = event_tx.send(ChangeEvent::new(
                speaker_id,
                GroupMembership::KEY,
                Service::ZoneGroupTopology,
            ));
        }
    }
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

        change.apply(&mut store, speaker_id)
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
    use crate::model::GroupId;
    use crate::property::{GroupInfo, Property, Volume};
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

    // ========================================================================
    // Unit Tests for apply_topology_changes
    // ========================================================================

    /// Helper to create a SpeakerInfo for testing
    fn make_speaker_info(id: &str, name: &str, ip: &str) -> crate::model::SpeakerInfo {
        crate::model::SpeakerInfo {
            id: SpeakerId::new(id),
            name: name.to_string(),
            room_name: name.to_string(),
            ip_address: ip.parse().unwrap(),
            port: 1400,
            model_name: "Test".to_string(),
            software_version: "1.0".to_string(),
            satellites: vec![],
        }
    }

    #[test]
    fn test_apply_property_change_group_volume() {
        let store = Arc::new(RwLock::new(StateStore::new()));
        let watched = Arc::new(RwLock::new(HashSet::new()));
        let (tx, _rx) = mpsc::channel();

        let speaker_id = SpeakerId::new("RINCON_111");
        let group_id = GroupId::new("RINCON_111:1");

        // Add speaker and group to store
        {
            let mut s = store.write().unwrap();
            s.add_speaker(make_speaker_info("RINCON_111", "Living Room", "192.168.1.101"));
            s.add_group(GroupInfo::new(
                group_id.clone(),
                speaker_id.clone(),
                vec![speaker_id.clone()],
            ));
        }

        // Apply GroupVolume change via the coordinator speaker
        apply_property_change(
            &store,
            &watched,
            &tx,
            &speaker_id,
            PropertyChange::GroupVolume(crate::property::GroupVolume(75)),
        );

        // Verify value was stored in group_props
        let s = store.read().unwrap();
        let stored: Option<crate::property::GroupVolume> = s.get_group(&group_id);
        assert_eq!(stored, Some(crate::property::GroupVolume(75)));
    }

    #[test]
    fn test_apply_property_change_group_volume_no_group() {
        let store = Arc::new(RwLock::new(StateStore::new()));
        let watched = Arc::new(RwLock::new(HashSet::new()));
        let (tx, _rx) = mpsc::channel();

        let speaker_id = SpeakerId::new("RINCON_111");

        // Add speaker but no group
        {
            let mut s = store.write().unwrap();
            s.add_speaker(make_speaker_info("RINCON_111", "Living Room", "192.168.1.101"));
        }

        // Apply GroupVolume change - should be silently dropped
        apply_property_change(
            &store,
            &watched,
            &tx,
            &speaker_id,
            PropertyChange::GroupVolume(crate::property::GroupVolume(50)),
        );

        // No crash, no stored value
        let s = store.read().unwrap();
        assert!(s.group_props.is_empty());
    }

    #[test]
    fn test_apply_topology_changes_updates_groups() {
        let store = Arc::new(RwLock::new(StateStore::new()));
        let watched = Arc::new(RwLock::new(HashSet::new()));
        let (tx, _rx) = mpsc::channel();

        // Add speakers to store
        {
            let mut s = store.write().unwrap();
            s.add_speaker(make_speaker_info("RINCON_111", "Living Room", "192.168.1.101"));
            s.add_speaker(make_speaker_info("RINCON_222", "Kitchen", "192.168.1.102"));
        }

        // Create topology changes with one group containing both speakers
        let group_id = GroupId::new("RINCON_111:1");
        let speaker1 = SpeakerId::new("RINCON_111");
        let speaker2 = SpeakerId::new("RINCON_222");

        let changes = TopologyChanges {
            groups: vec![GroupInfo::new(
                group_id.clone(),
                speaker1.clone(),
                vec![speaker1.clone(), speaker2.clone()],
            )],
            memberships: vec![
                (speaker1.clone(), GroupMembership::new(group_id.clone(), true)),
                (speaker2.clone(), GroupMembership::new(group_id.clone(), false)),
            ],
        };

        // Apply topology changes
        apply_topology_changes(&store, &watched, &tx, changes);

        // Verify groups are updated
        let s = store.read().unwrap();
        assert_eq!(s.groups.len(), 1);
        
        let group = s.groups.get(&group_id).unwrap();
        assert_eq!(group.coordinator_id, speaker1);
        assert_eq!(group.member_ids.len(), 2);
        assert!(group.member_ids.contains(&speaker1));
        assert!(group.member_ids.contains(&speaker2));
    }

    #[test]
    fn test_apply_topology_changes_updates_group_membership() {
        let store = Arc::new(RwLock::new(StateStore::new()));
        let watched = Arc::new(RwLock::new(HashSet::new()));
        let (tx, _rx) = mpsc::channel();

        // Add speakers to store
        {
            let mut s = store.write().unwrap();
            s.add_speaker(make_speaker_info("RINCON_111", "Living Room", "192.168.1.101"));
            s.add_speaker(make_speaker_info("RINCON_222", "Kitchen", "192.168.1.102"));
        }

        let group_id = GroupId::new("RINCON_111:1");
        let speaker1 = SpeakerId::new("RINCON_111");
        let speaker2 = SpeakerId::new("RINCON_222");

        let changes = TopologyChanges {
            groups: vec![GroupInfo::new(
                group_id.clone(),
                speaker1.clone(),
                vec![speaker1.clone(), speaker2.clone()],
            )],
            memberships: vec![
                (speaker1.clone(), GroupMembership::new(group_id.clone(), true)),
                (speaker2.clone(), GroupMembership::new(group_id.clone(), false)),
            ],
        };

        apply_topology_changes(&store, &watched, &tx, changes);

        // Verify GroupMembership is updated for each speaker
        let s = store.read().unwrap();
        
        let membership1: Option<GroupMembership> = s.get(&speaker1);
        assert!(membership1.is_some());
        let m1 = membership1.unwrap();
        assert_eq!(m1.group_id, group_id);
        assert!(m1.is_coordinator);

        let membership2: Option<GroupMembership> = s.get(&speaker2);
        assert!(membership2.is_some());
        let m2 = membership2.unwrap();
        assert_eq!(m2.group_id, group_id);
        assert!(!m2.is_coordinator);
    }

    #[test]
    fn test_apply_topology_changes_emits_events_for_watched_properties() {
        let store = Arc::new(RwLock::new(StateStore::new()));
        let watched = Arc::new(RwLock::new(HashSet::new()));
        let (tx, rx) = mpsc::channel();

        let speaker1 = SpeakerId::new("RINCON_111");
        let speaker2 = SpeakerId::new("RINCON_222");

        // Add speakers to store
        {
            let mut s = store.write().unwrap();
            s.add_speaker(make_speaker_info("RINCON_111", "Living Room", "192.168.1.101"));
            s.add_speaker(make_speaker_info("RINCON_222", "Kitchen", "192.168.1.102"));
        }

        // Watch GroupMembership for speaker1 only
        {
            let mut w = watched.write().unwrap();
            w.insert((speaker1.clone(), GroupMembership::KEY));
        }

        let group_id = GroupId::new("RINCON_111:1");

        let changes = TopologyChanges {
            groups: vec![GroupInfo::new(
                group_id.clone(),
                speaker1.clone(),
                vec![speaker1.clone(), speaker2.clone()],
            )],
            memberships: vec![
                (speaker1.clone(), GroupMembership::new(group_id.clone(), true)),
                (speaker2.clone(), GroupMembership::new(group_id.clone(), false)),
            ],
        };

        apply_topology_changes(&store, &watched, &tx, changes);

        // Should receive event for speaker1 (watched) but not speaker2 (not watched)
        let event = rx.try_recv().unwrap();
        assert_eq!(event.speaker_id, speaker1);
        assert_eq!(event.property_key, GroupMembership::KEY);
        assert_eq!(event.service, Service::ZoneGroupTopology);

        // No more events (speaker2 is not watched)
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn test_apply_topology_changes_clears_old_groups() {
        let store = Arc::new(RwLock::new(StateStore::new()));
        let watched = Arc::new(RwLock::new(HashSet::new()));
        let (tx, _rx) = mpsc::channel();

        let speaker1 = SpeakerId::new("RINCON_111");
        let speaker2 = SpeakerId::new("RINCON_222");

        // Add speakers and an initial group
        {
            let mut s = store.write().unwrap();
            s.add_speaker(make_speaker_info("RINCON_111", "Living Room", "192.168.1.101"));
            s.add_speaker(make_speaker_info("RINCON_222", "Kitchen", "192.168.1.102"));
            
            // Add initial group
            let old_group_id = GroupId::new("OLD_GROUP:1");
            s.add_group(GroupInfo::new(
                old_group_id.clone(),
                speaker1.clone(),
                vec![speaker1.clone()],
            ));
        }

        // Verify old group exists
        {
            let s = store.read().unwrap();
            assert_eq!(s.groups.len(), 1);
            assert!(s.groups.contains_key(&GroupId::new("OLD_GROUP:1")));
        }

        // Apply new topology changes with different group
        let new_group_id = GroupId::new("NEW_GROUP:1");
        let changes = TopologyChanges {
            groups: vec![GroupInfo::new(
                new_group_id.clone(),
                speaker2.clone(),
                vec![speaker1.clone(), speaker2.clone()],
            )],
            memberships: vec![
                (speaker1.clone(), GroupMembership::new(new_group_id.clone(), false)),
                (speaker2.clone(), GroupMembership::new(new_group_id.clone(), true)),
            ],
        };

        apply_topology_changes(&store, &watched, &tx, changes);

        // Verify old group is gone, new group exists
        let s = store.read().unwrap();
        assert_eq!(s.groups.len(), 1);
        assert!(!s.groups.contains_key(&GroupId::new("OLD_GROUP:1")));
        assert!(s.groups.contains_key(&new_group_id));
    }

    #[test]
    fn test_apply_topology_changes_updates_speaker_to_group_mapping() {
        let store = Arc::new(RwLock::new(StateStore::new()));
        let watched = Arc::new(RwLock::new(HashSet::new()));
        let (tx, _rx) = mpsc::channel();

        let speaker1 = SpeakerId::new("RINCON_111");
        let speaker2 = SpeakerId::new("RINCON_222");

        // Add speakers
        {
            let mut s = store.write().unwrap();
            s.add_speaker(make_speaker_info("RINCON_111", "Living Room", "192.168.1.101"));
            s.add_speaker(make_speaker_info("RINCON_222", "Kitchen", "192.168.1.102"));
        }

        let group_id = GroupId::new("RINCON_111:1");

        let changes = TopologyChanges {
            groups: vec![GroupInfo::new(
                group_id.clone(),
                speaker1.clone(),
                vec![speaker1.clone(), speaker2.clone()],
            )],
            memberships: vec![
                (speaker1.clone(), GroupMembership::new(group_id.clone(), true)),
                (speaker2.clone(), GroupMembership::new(group_id.clone(), false)),
            ],
        };

        apply_topology_changes(&store, &watched, &tx, changes);

        // Verify speaker_to_group mapping is updated
        let s = store.read().unwrap();
        assert_eq!(s.speaker_to_group.get(&speaker1), Some(&group_id));
        assert_eq!(s.speaker_to_group.get(&speaker2), Some(&group_id));
    }

    #[test]
    fn test_apply_topology_changes_no_event_when_membership_unchanged() {
        let store = Arc::new(RwLock::new(StateStore::new()));
        let watched = Arc::new(RwLock::new(HashSet::new()));
        let (tx, rx) = mpsc::channel();

        let speaker1 = SpeakerId::new("RINCON_111");
        let group_id = GroupId::new("RINCON_111:1");

        // Add speaker and set initial membership
        {
            let mut s = store.write().unwrap();
            s.add_speaker(make_speaker_info("RINCON_111", "Living Room", "192.168.1.101"));
            s.set(&speaker1, GroupMembership::new(group_id.clone(), true));
        }

        // Watch the property
        {
            let mut w = watched.write().unwrap();
            w.insert((speaker1.clone(), GroupMembership::KEY));
        }

        // Apply same topology (no change)
        let changes = TopologyChanges {
            groups: vec![GroupInfo::new(
                group_id.clone(),
                speaker1.clone(),
                vec![speaker1.clone()],
            )],
            memberships: vec![
                (speaker1.clone(), GroupMembership::new(group_id.clone(), true)),
            ],
        };

        apply_topology_changes(&store, &watched, &tx, changes);

        // No event should be emitted since membership didn't change
        assert!(rx.try_recv().is_err());
    }
}

