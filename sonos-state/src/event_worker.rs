//! Background event worker for consuming events from SonosEventManager
//!
//! This module provides a background thread that consumes events from the
//! SonosEventManager and applies them to the StateStore.

use std::collections::HashSet;
use std::net::IpAddr;
use std::sync::{mpsc, Arc};
use std::thread::{self, JoinHandle};

use parking_lot::RwLock;

use sonos_api::Service;
use sonos_event_manager::SonosEventManager;
use sonos_stream::events::EventData;

use sonos_api::ServiceScope;

use crate::decoder::{decode_event, decode_topology_event, PropertyChange, TopologyChanges};
use crate::model::SpeakerId;
use crate::property::{GroupMembership, Property, Scope};
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
            if let EventData::ZoneGroupTopology(ref zgt_event) = event.event_data {
                tracing::debug!("Processing ZoneGroupTopology event");
                let topology_changes = decode_topology_event(zgt_event);
                apply_topology_changes(&store, &watched, &event_tx, &ip_to_speaker, topology_changes);
                continue;
            }

            // Look up speaker_id from IP for non-topology events
            let speaker_id = {
                let ip_map = ip_to_speaker.read();

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

            // For PerCoordinator services (e.g. AVTransport), skip events from
            // non-coordinator speakers. Their events carry empty/default values
            // because the coordinator owns playback state for the whole group.
            // The coordinator's events will be propagated to members below.
            if event.service.scope() == ServiceScope::PerCoordinator {
                let is_coordinator = {
                    let s = store.read();
                    // If no group info exists yet, treat as coordinator (safe default)
                    s.speaker_to_group
                        .get(&speaker_id)
                        .and_then(|gid| s.groups.get(gid))
                        .map(|group| group.coordinator_id == speaker_id)
                        .unwrap_or(true)
                };

                if !is_coordinator {
                    tracing::debug!(
                        "Skipping PerCoordinator {:?} event from non-coordinator {}",
                        event.service,
                        speaker_id.as_str()
                    );
                    continue;
                }
            }

            // Decode event
            let decoded = decode_event(&event, speaker_id.clone());
            tracing::debug!(
                "Decoded {} property changes from event",
                decoded.changes.len()
            );

            // Apply changes to the originating speaker (coordinator)
            for change in &decoded.changes {
                tracing::debug!("Applying change: {:?}", change);
                apply_property_change(&store, &watched, &event_tx, &speaker_id, change);
            }

            // For PerCoordinator services, notify group members who are watching
            // these properties. No data is copied — members read the coordinator's
            // value at read time via get_resolved().
            if event.service.scope() == ServiceScope::PerCoordinator {
                let members = {
                    let s = store.read();
                    resolve_group_members(&s, &speaker_id)
                };
                if !members.is_empty() {
                    notify_group_members(&watched, &event_tx, &members, &decoded.changes);
                }
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
/// 4. Updates boot_seq, speaker IPs, and satellite IDs
/// 5. Emits change events for watched GroupMembership properties
fn apply_topology_changes(
    store: &Arc<RwLock<StateStore>>,
    watched: &Arc<RwLock<HashSet<(SpeakerId, &'static str)>>>,
    event_tx: &mpsc::Sender<ChangeEvent>,
    ip_to_speaker: &Arc<RwLock<std::collections::HashMap<IpAddr, SpeakerId>>>,
    changes: TopologyChanges,
) {
    tracing::debug!(
        "Applying topology changes: {} groups, {} memberships",
        changes.groups.len(),
        changes.memberships.len()
    );

    // Apply all changes within a single write lock
    let (membership_changes, ip_updates): (Vec<(SpeakerId, bool)>, Vec<(IpAddr, IpAddr, SpeakerId)>) = {
        let mut store = store.write();

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

        // 4. Update boot_seq for each speaker
        for (speaker_id, boot_seq) in changes.boot_seqs {
            if let Some(speaker) = store.speakers.get_mut(&speaker_id) {
                speaker.boot_seq = boot_seq;
            }
        }

        // 5. Apply IP updates from topology location URLs
        let mut changed_ips = Vec::new();
        for (speaker_id, new_ip) in &changes.speaker_ips {
            if let Some(old_ip) = store.update_speaker_ip_address(speaker_id, *new_ip) {
                tracing::info!(
                    "Speaker {} IP changed: {} -> {}",
                    speaker_id.as_str(),
                    old_ip,
                    new_ip
                );
                changed_ips.push((old_ip, *new_ip, speaker_id.clone()));
            }
        }

        // 6. Store satellite IDs
        store.satellite_ids = changes.satellite_ids.into_iter().collect();

        (changed_memberships, changed_ips)
    };

    // Update ip_to_speaker reverse map (outside store lock)
    if !ip_updates.is_empty() {
        let mut map = ip_to_speaker.write();
        for (old_ip, new_ip, speaker_id) in ip_updates {
            map.remove(&old_ip);
            map.insert(new_ip, speaker_id);
        }
    }

    // Emit change events for watched properties (outside write locks)
    let watched_set = watched.read();

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

/// Resolve the non-coordinator group members for the given coordinator speaker.
///
/// Returns an empty Vec if:
/// - The speaker is not in any group
/// - The speaker is not the coordinator of its group
/// - The group has only one member (standalone speaker)
fn resolve_group_members(store: &StateStore, speaker_id: &SpeakerId) -> Vec<SpeakerId> {
    store
        .speaker_to_group
        .get(speaker_id)
        .and_then(|gid| store.groups.get(gid))
        .filter(|group| group.coordinator_id == *speaker_id && group.member_ids.len() > 1)
        .map(|group| {
            group
                .member_ids
                .iter()
                .filter(|id| *id != speaker_id)
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

/// Notify group members who are watching speaker-scoped properties that changed
/// on the coordinator. Only emits ChangeEvents — no data is copied. Members
/// read the coordinator's value at read time via `StateStore::get_resolved()`.
fn notify_group_members(
    watched: &Arc<RwLock<HashSet<(SpeakerId, &'static str)>>>,
    event_tx: &mpsc::Sender<ChangeEvent>,
    members: &[SpeakerId],
    changes: &[PropertyChange],
) {
    let watched_set = watched.read();
    for member_id in members {
        for change in changes {
            if change.scope() == Scope::Speaker {
                let key = change.key();
                if watched_set.contains(&(member_id.clone(), key)) {
                    tracing::debug!(
                        "Notifying member {} of coordinator change for {}",
                        member_id.as_str(),
                        key
                    );
                    let _ =
                        event_tx.send(ChangeEvent::new(member_id.clone(), key, change.service()));
                }
            }
        }
    }
}

/// Apply a single property change to the store
fn apply_property_change(
    store: &Arc<RwLock<StateStore>>,
    watched: &Arc<RwLock<HashSet<(SpeakerId, &'static str)>>>,
    event_tx: &mpsc::Sender<ChangeEvent>,
    speaker_id: &SpeakerId,
    change: &PropertyChange,
) {
    let key = change.key();
    let service = change.service();

    let changed = {
        let mut store = store.write();
        change.apply(&mut store, speaker_id)
    };

    if changed {
        let is_watched = watched.read().contains(&(speaker_id.clone(), key));

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
            let mut s = store.write();
            s.add_speaker(crate::model::SpeakerInfo {
                id: speaker_id.clone(),
                name: "Test".to_string(),
                room_name: "Test".to_string(),
                ip_address: "192.168.1.100".parse().unwrap(),
                port: 1400,
                model_name: "Test".to_string(),
                software_version: "1.0".to_string(),
                boot_seq: 0,
                satellites: vec![],
            });
        }

        // Apply change without watch
        apply_property_change(
            &store,
            &watched,
            &tx,
            &speaker_id,
            &PropertyChange::Volume(Volume(50)),
        );

        // No event should be emitted (not watched)
        assert!(rx.try_recv().is_err());

        // Verify value was stored
        let stored: Option<Volume> = store.read().get(&speaker_id);
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
            let mut s = store.write();
            s.add_speaker(crate::model::SpeakerInfo {
                id: speaker_id.clone(),
                name: "Test".to_string(),
                room_name: "Test".to_string(),
                ip_address: "192.168.1.100".parse().unwrap(),
                port: 1400,
                model_name: "Test".to_string(),
                software_version: "1.0".to_string(),
                boot_seq: 0,
                satellites: vec![],
            });
        }

        // Register watch
        {
            let mut w = watched.write();
            w.insert((speaker_id.clone(), Volume::KEY));
        }

        // Apply change
        apply_property_change(
            &store,
            &watched,
            &tx,
            &speaker_id,
            &PropertyChange::Volume(Volume(75)),
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
            boot_seq: 0,
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
            let mut s = store.write();
            s.add_speaker(make_speaker_info(
                "RINCON_111",
                "Living Room",
                "192.168.1.101",
            ));
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
            &PropertyChange::GroupVolume(crate::property::GroupVolume(75)),
        );

        // Verify value was stored in group_props
        let s = store.read();
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
            let mut s = store.write();
            s.add_speaker(make_speaker_info(
                "RINCON_111",
                "Living Room",
                "192.168.1.101",
            ));
        }

        // Apply GroupVolume change - should be silently dropped
        apply_property_change(
            &store,
            &watched,
            &tx,
            &speaker_id,
            &PropertyChange::GroupVolume(crate::property::GroupVolume(50)),
        );

        // No crash, no stored value
        let s = store.read();
        assert!(s.group_props.is_empty());
    }

    #[test]
    fn test_apply_topology_changes_updates_groups() {
        let store = Arc::new(RwLock::new(StateStore::new()));
        let watched = Arc::new(RwLock::new(HashSet::new()));
        let (tx, _rx) = mpsc::channel();

        // Add speakers to store
        {
            let mut s = store.write();
            s.add_speaker(make_speaker_info(
                "RINCON_111",
                "Living Room",
                "192.168.1.101",
            ));
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
                (
                    speaker1.clone(),
                    GroupMembership::new(group_id.clone(), true),
                ),
                (
                    speaker2.clone(),
                    GroupMembership::new(group_id.clone(), false),
                ),
            ],
            boot_seqs: vec![],
            speaker_ips: vec![],
            satellite_ids: vec![],
        };

        let ip_to_speaker = Arc::new(RwLock::new(std::collections::HashMap::new()));
        apply_topology_changes(&store, &watched, &tx, &ip_to_speaker, changes);

        // Verify groups are updated
        let s = store.read();
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
            let mut s = store.write();
            s.add_speaker(make_speaker_info(
                "RINCON_111",
                "Living Room",
                "192.168.1.101",
            ));
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
                (
                    speaker1.clone(),
                    GroupMembership::new(group_id.clone(), true),
                ),
                (
                    speaker2.clone(),
                    GroupMembership::new(group_id.clone(), false),
                ),
            ],
            boot_seqs: vec![],
            speaker_ips: vec![],
            satellite_ids: vec![],
        };

        let ip_to_speaker = Arc::new(RwLock::new(std::collections::HashMap::new()));
        apply_topology_changes(&store, &watched, &tx, &ip_to_speaker, changes);

        // Verify GroupMembership is updated for each speaker
        let s = store.read();

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
            let mut s = store.write();
            s.add_speaker(make_speaker_info(
                "RINCON_111",
                "Living Room",
                "192.168.1.101",
            ));
            s.add_speaker(make_speaker_info("RINCON_222", "Kitchen", "192.168.1.102"));
        }

        // Watch GroupMembership for speaker1 only
        {
            let mut w = watched.write();
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
                (
                    speaker1.clone(),
                    GroupMembership::new(group_id.clone(), true),
                ),
                (
                    speaker2.clone(),
                    GroupMembership::new(group_id.clone(), false),
                ),
            ],
            boot_seqs: vec![],
            speaker_ips: vec![],
            satellite_ids: vec![],
        };

        let ip_to_speaker = Arc::new(RwLock::new(std::collections::HashMap::new()));
        apply_topology_changes(&store, &watched, &tx, &ip_to_speaker, changes);

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
            let mut s = store.write();
            s.add_speaker(make_speaker_info(
                "RINCON_111",
                "Living Room",
                "192.168.1.101",
            ));
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
            let s = store.read();
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
                (
                    speaker1.clone(),
                    GroupMembership::new(new_group_id.clone(), false),
                ),
                (
                    speaker2.clone(),
                    GroupMembership::new(new_group_id.clone(), true),
                ),
            ],
            boot_seqs: vec![],
            speaker_ips: vec![],
            satellite_ids: vec![],
        };

        let ip_to_speaker = Arc::new(RwLock::new(std::collections::HashMap::new()));
        apply_topology_changes(&store, &watched, &tx, &ip_to_speaker, changes);

        // Verify old group is gone, new group exists
        let s = store.read();
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
            let mut s = store.write();
            s.add_speaker(make_speaker_info(
                "RINCON_111",
                "Living Room",
                "192.168.1.101",
            ));
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
                (
                    speaker1.clone(),
                    GroupMembership::new(group_id.clone(), true),
                ),
                (
                    speaker2.clone(),
                    GroupMembership::new(group_id.clone(), false),
                ),
            ],
            boot_seqs: vec![],
            speaker_ips: vec![],
            satellite_ids: vec![],
        };

        let ip_to_speaker = Arc::new(RwLock::new(std::collections::HashMap::new()));
        apply_topology_changes(&store, &watched, &tx, &ip_to_speaker, changes);

        // Verify speaker_to_group mapping is updated
        let s = store.read();
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
            let mut s = store.write();
            s.add_speaker(make_speaker_info(
                "RINCON_111",
                "Living Room",
                "192.168.1.101",
            ));
            s.set(&speaker1, GroupMembership::new(group_id.clone(), true));
        }

        // Watch the property
        {
            let mut w = watched.write();
            w.insert((speaker1.clone(), GroupMembership::KEY));
        }

        // Apply same topology (no change)
        let changes = TopologyChanges {
            groups: vec![GroupInfo::new(
                group_id.clone(),
                speaker1.clone(),
                vec![speaker1.clone()],
            )],
            memberships: vec![(
                speaker1.clone(),
                GroupMembership::new(group_id.clone(), true),
            )],
            boot_seqs: vec![],
            speaker_ips: vec![],
            satellite_ids: vec![],
        };

        let ip_to_speaker = Arc::new(RwLock::new(std::collections::HashMap::new()));
        apply_topology_changes(&store, &watched, &tx, &ip_to_speaker, changes);

        // No event should be emitted since membership didn't change
        assert!(rx.try_recv().is_err());
    }

    // ========================================================================
    // PerCoordinator Read-Time Resolution Tests
    // ========================================================================

    #[test]
    fn test_per_coordinator_notifies_members_without_data_copy() {
        use crate::property::PlaybackState;

        let store = Arc::new(RwLock::new(StateStore::new()));
        let watched = Arc::new(RwLock::new(HashSet::new()));
        let (tx, rx) = mpsc::channel();

        let coordinator = SpeakerId::new("RINCON_COORD");
        let member = SpeakerId::new("RINCON_MEMBER");
        let group_id = GroupId::new("RINCON_COORD:1");

        // Add speakers and group
        {
            let mut s = store.write();
            s.add_speaker(make_speaker_info(
                "RINCON_COORD",
                "Bedroom",
                "192.168.1.101",
            ));
            s.add_speaker(make_speaker_info(
                "RINCON_MEMBER",
                "Kitchen",
                "192.168.1.102",
            ));
            s.add_group(GroupInfo::new(
                group_id.clone(),
                coordinator.clone(),
                vec![coordinator.clone(), member.clone()],
            ));
        }

        // Watch PlaybackState on both speakers
        {
            let mut w = watched.write();
            w.insert((coordinator.clone(), PlaybackState::KEY));
            w.insert((member.clone(), PlaybackState::KEY));
        }

        // Simulate what event_worker does: apply changes to coordinator, then notify members
        let changes = vec![PropertyChange::PlaybackState(PlaybackState::Playing)];

        // Apply to coordinator only
        for change in &changes {
            apply_property_change(&store, &watched, &tx, &coordinator, change);
        }

        // Notify group members (notification only, no data copy)
        let members = {
            let s = store.read();
            resolve_group_members(&s, &coordinator)
        };
        notify_group_members(&watched, &tx, &members, &changes);

        // Both coordinator and member should have received ChangeEvents
        let event1 = rx.try_recv().unwrap();
        assert_eq!(event1.speaker_id, coordinator);
        assert_eq!(event1.property_key, PlaybackState::KEY);

        let event2 = rx.try_recv().unwrap();
        assert_eq!(event2.speaker_id, member);
        assert_eq!(event2.property_key, PlaybackState::KEY);

        // No more events
        assert!(rx.try_recv().is_err());

        // Coordinator has the value in its own props
        let s = store.read();
        let coord_state: Option<PlaybackState> = s.get(&coordinator);
        assert_eq!(coord_state, Some(PlaybackState::Playing));

        // Member does NOT have the value in its own props (no data copy)
        let member_state: Option<PlaybackState> = s.get(&member);
        assert_eq!(member_state, None);

        // But get_resolved on member returns the coordinator's value
        let resolved_state: Option<PlaybackState> = s.get_resolved(&member);
        assert_eq!(resolved_state, Some(PlaybackState::Playing));
    }

    #[test]
    fn test_per_coordinator_no_notification_for_standalone() {
        use crate::property::PlaybackState;

        let store = Arc::new(RwLock::new(StateStore::new()));
        let watched = Arc::new(RwLock::new(HashSet::new()));
        let (tx, rx) = mpsc::channel();

        let speaker = SpeakerId::new("RINCON_STANDALONE");
        let group_id = GroupId::new("RINCON_STANDALONE:1");

        // Add standalone speaker (single-member group)
        {
            let mut s = store.write();
            s.add_speaker(make_speaker_info(
                "RINCON_STANDALONE",
                "Bedroom",
                "192.168.1.101",
            ));
            s.add_group(GroupInfo::new(
                group_id.clone(),
                speaker.clone(),
                vec![speaker.clone()],
            ));
        }

        // Watch PlaybackState
        {
            let mut w = watched.write();
            w.insert((speaker.clone(), PlaybackState::KEY));
        }

        // Apply change to the standalone speaker
        let changes = vec![PropertyChange::PlaybackState(PlaybackState::Playing)];
        for change in &changes {
            apply_property_change(&store, &watched, &tx, &speaker, change);
        }

        // resolve_group_members should return empty for standalone
        let members = {
            let s = store.read();
            resolve_group_members(&s, &speaker)
        };
        assert!(members.is_empty());

        // Only one event (from the coordinator itself), no extra fan-out
        let event = rx.try_recv().unwrap();
        assert_eq!(event.speaker_id, speaker);
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn test_per_speaker_service_not_notified() {
        // RenderingControl is PerSpeaker — changes on the coordinator should NOT
        // notify group members even when a group exists.
        let store = Arc::new(RwLock::new(StateStore::new()));
        let watched = Arc::new(RwLock::new(HashSet::new()));
        let (tx, rx) = mpsc::channel();

        let coordinator = SpeakerId::new("RINCON_COORD");
        let member = SpeakerId::new("RINCON_MEMBER");
        let group_id = GroupId::new("RINCON_COORD:1");

        // Add speakers and group
        {
            let mut s = store.write();
            s.add_speaker(make_speaker_info(
                "RINCON_COORD",
                "Bedroom",
                "192.168.1.101",
            ));
            s.add_speaker(make_speaker_info(
                "RINCON_MEMBER",
                "Kitchen",
                "192.168.1.102",
            ));
            s.add_group(GroupInfo::new(
                group_id.clone(),
                coordinator.clone(),
                vec![coordinator.clone(), member.clone()],
            ));
        }

        // Watch Volume on both speakers
        {
            let mut w = watched.write();
            w.insert((coordinator.clone(), Volume::KEY));
            w.insert((member.clone(), Volume::KEY));
        }

        // Apply Volume change only to coordinator (PerSpeaker service — no notification)
        apply_property_change(
            &store,
            &watched,
            &tx,
            &coordinator,
            &PropertyChange::Volume(Volume(80)),
        );

        // RenderingControl is PerSpeaker, so we do NOT notify members.
        // Only the coordinator gets the event.
        let event = rx.try_recv().unwrap();
        assert_eq!(event.speaker_id, coordinator);
        assert_eq!(event.property_key, Volume::KEY);

        // No event for the member
        assert!(rx.try_recv().is_err());

        // Verify member does NOT have the volume value
        let s = store.read();
        let coord_vol: Option<Volume> = s.get(&coordinator);
        let member_vol: Option<Volume> = s.get(&member);
        assert_eq!(coord_vol, Some(Volume(80)));
        assert_eq!(member_vol, None);
    }

    #[test]
    fn test_resolve_group_members_empty_for_non_coordinator() {
        // resolve_group_members should return empty when called with
        // a non-coordinator speaker.
        let mut store = StateStore::new();

        let coordinator = SpeakerId::new("RINCON_COORD");
        let member = SpeakerId::new("RINCON_MEMBER");
        let group_id = GroupId::new("RINCON_COORD:1");

        store.add_speaker(make_speaker_info(
            "RINCON_COORD",
            "Bedroom",
            "192.168.1.101",
        ));
        store.add_speaker(make_speaker_info(
            "RINCON_MEMBER",
            "Kitchen",
            "192.168.1.102",
        ));
        store.add_group(GroupInfo::new(
            group_id,
            coordinator,
            vec![SpeakerId::new("RINCON_COORD"), member.clone()],
        ));

        // Non-coordinator should never resolve group members
        let members = resolve_group_members(&store, &member);
        assert!(members.is_empty());
    }

    #[test]
    fn test_notify_group_members_only_notifies_watched() {
        use crate::property::PlaybackState;

        let watched = Arc::new(RwLock::new(HashSet::new()));
        let (tx, rx) = mpsc::channel();

        let member_watched = SpeakerId::new("RINCON_WATCHED");
        let member_unwatched = SpeakerId::new("RINCON_UNWATCHED");

        // Only watch PlaybackState on one member
        {
            let mut w = watched.write();
            w.insert((member_watched.clone(), PlaybackState::KEY));
            // member_unwatched is NOT in the watched set
        }

        let changes = vec![PropertyChange::PlaybackState(PlaybackState::Playing)];
        let members = vec![member_watched.clone(), member_unwatched.clone()];

        notify_group_members(&watched, &tx, &members, &changes);

        // Only the watched member should get a notification
        let event = rx.try_recv().unwrap();
        assert_eq!(event.speaker_id, member_watched);
        assert_eq!(event.property_key, PlaybackState::KEY);

        // No event for the unwatched member
        assert!(rx.try_recv().is_err());
    }
}
