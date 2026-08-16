//! Background event worker for consuming events from SonosEventManager
//!
//! This module provides a background thread that consumes events from the
//! SonosEventManager and applies them to the StateStore.

use std::net::IpAddr;
use std::sync::{mpsc, Arc};
use std::thread::{self, JoinHandle};

use parking_lot::RwLock;

use sonos_event_manager::SonosEventManager;
use sonos_stream::events::{EnrichedEvent, EventData};

use sonos_api::ServiceScope;

use crate::decoder::{decode_event, decode_topology_event, PropertyChange, TopologyChanges};
use crate::model::SpeakerId;
use crate::property::{GroupMembership, Property, Scope};
use crate::state::{
    is_pair_watched, ChangeEvent, ChangeSource, StateStore, WatchCounts, WriteStamp,
};

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
    watched: Arc<RwLock<WatchCounts>>,
    event_tx: mpsc::Sender<ChangeEvent>,
    ip_to_speaker: Arc<RwLock<std::collections::HashMap<IpAddr, SpeakerId>>>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        tracing::info!("State event worker started, waiting for events...");

        // Consume events from event manager (blocking)
        run_event_loop(
            event_manager.iter(),
            &store,
            &watched,
            &event_tx,
            &ip_to_speaker,
        );

        tracing::info!("State event worker stopped");
    })
}

/// Escalate the panic log every N panics so a repeatedly-failing decode path is
/// impossible to miss in a long-running log.
const PANIC_ESCALATION_INTERVAL: u64 = 10;

/// Drain `events`, applying each one to the store.
///
/// Each event is processed inside `catch_unwind` so that a panic anywhere in
/// decoding or applying a single event terminates that event, not the worker.
/// Before this guard existed, one panic silently killed the thread and *every*
/// subsequent state update — with no log, no `Err`, and no panic surfacing to
/// the user, because the `JoinHandle` is never joined.
///
/// Panics are counted and logged at `error!` every single time; they are never
/// swallowed. `catch_unwind` here is a containment boundary, not a way to
/// tolerate bugs.
fn run_event_loop<I>(
    events: I,
    store: &Arc<RwLock<StateStore>>,
    watched: &Arc<RwLock<WatchCounts>>,
    event_tx: &mpsc::Sender<ChangeEvent>,
    ip_to_speaker: &Arc<RwLock<std::collections::HashMap<IpAddr, SpeakerId>>>,
) where
    I: Iterator<Item = EnrichedEvent>,
{
    let mut panic_count: u64 = 0;

    for event in events {
        // `AssertUnwindSafe` is sound here for two reasons:
        //
        // 1. Every piece of shared state is behind a `parking_lot::RwLock`,
        //    which — unlike `std::sync::RwLock` — does not poison on panic. A
        //    guard held at unwind time is simply released, so the store and the
        //    watched set stay usable afterwards.
        // 2. Events are independent: an aborted event leaves the store in a
        //    partially-updated but internally consistent shape (each
        //    `PropertyChange` is applied under its own lock acquisition), and
        //    the next topology or service event overwrites it wholesale.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            handle_event(&event, store, watched, event_tx, ip_to_speaker);
        }));

        if result.is_err() {
            panic_count += 1;
            tracing::error!(
                "Panic while processing event from {} for service {:?}; \
                 skipping this event and continuing (panic #{} for this worker)",
                event.speaker_ip,
                event.service,
                panic_count
            );

            if panic_count % PANIC_ESCALATION_INTERVAL == 0 {
                tracing::error!(
                    "State event worker has now panicked {} times — state updates \
                     are being dropped and this is a bug that needs fixing",
                    panic_count
                );
            }
        }
    }
}

/// Process a single event: resolve identity, gate on coordinator, decode, apply.
fn handle_event(
    event: &EnrichedEvent,
    store: &Arc<RwLock<StateStore>>,
    watched: &Arc<RwLock<WatchCounts>>,
    event_tx: &mpsc::Sender<ChangeEvent>,
    ip_to_speaker: &Arc<RwLock<std::collections::HashMap<IpAddr, SpeakerId>>>,
) {
    tracing::debug!(
        "Received event from {} for service {:?}",
        event.speaker_ip,
        event.service
    );

    // Test-only injection point for `test_worker_survives_decoder_panic`. Kept
    // behind `cfg(test)` so no permanent production API exists for it.
    #[cfg(test)]
    if event.speaker_ip == tests::PANIC_TRIGGER_IP {
        panic!("injected test panic while decoding event");
    }

    // One stamp for the whole event, taken from when the event was *observed*
    // upstream — not from `Instant::now()`. Everything between observation and
    // this line (HTTP receive, parse, two channel hops, worker scheduling) is
    // latency, not new information, and stamping here would make a stale NOTIFY
    // outrank a local write or a `fetch()` that genuinely saw the device later.
    let stamp = WriteStamp::observed_at(ChangeSource::Event, event.observed_at);

    // Handle ZoneGroupTopology events specially - they affect all speakers
    if let EventData::ZoneGroupTopology(ref zgt_event) = event.event_data {
        tracing::debug!("Processing ZoneGroupTopology event");
        let topology_changes = decode_topology_event(zgt_event);
        apply_topology_changes(
            store,
            watched,
            event_tx,
            ip_to_speaker,
            topology_changes,
            stamp,
        );
        return;
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
                return;
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
        let coordinator_lookup = {
            let s = store.read();
            s.speaker_to_group
                .get(&speaker_id)
                .and_then(|gid| s.groups.get(gid))
                .map(|group| group.coordinator_id == speaker_id)
        };

        // A lookup miss means we have no topology yet (or the speaker is absent
        // from it), so the speaker is treated as its own coordinator. That is
        // the right default for a standalone speaker and for the pre-topology
        // window, but it also means a stale/incomplete topology silently makes
        // every group member act as a coordinator — so say so out loud.
        let is_coordinator = coordinator_lookup.unwrap_or_else(|| {
            tracing::debug!(
                "No group data for {} while handling PerCoordinator {:?} event; \
                 treating it as its own coordinator",
                speaker_id.as_str(),
                event.service
            );
            true
        });

        if !is_coordinator {
            tracing::debug!(
                "Skipping PerCoordinator {:?} event from non-coordinator {}",
                event.service,
                speaker_id.as_str()
            );
            return;
        }
    }

    // Decode event
    let decoded = decode_event(event, speaker_id.clone());
    tracing::debug!(
        "Decoded {} property changes from event",
        decoded.changes.len()
    );

    // Apply changes to the originating speaker (coordinator)
    for change in &decoded.changes {
        tracing::debug!("Applying change: {:?}", change);
        apply_property_change(store, watched, event_tx, &speaker_id, change, stamp);
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
            // A notification, not a write, so nothing is ordered against this
            // stamp — but consumers read `ChangeEvent::timestamp` from it, and a
            // member must be told the same observation time as the coordinator,
            // so it reuses the event's stamp rather than taking a fresh one.
            notify_group_members(watched, event_tx, &members, &decoded.changes, stamp);
        }
    }
}

/// Apply topology changes from a ZoneGroupTopology event
///
/// This function:
/// 1. Clears existing groups from the store
/// 2. Adds new groups from the TopologyChanges
/// 3. Updates GroupMembership for each speaker
/// 4. Updates boot_seq, speaker IPs, and satellite IDs
/// 5. Emits change events for watched GroupMembership properties
///
/// A topology event with no groups is treated as a *partial* event and ignored,
/// not as "the household has no groups" — see the early return below.
///
/// `stamp` is the observation stamp of the originating event and covers the whole
/// snapshot: every membership in a single topology event was observed at the same
/// moment, so they must not be able to order against each other.
fn apply_topology_changes(
    store: &Arc<RwLock<StateStore>>,
    watched: &Arc<RwLock<WatchCounts>>,
    event_tx: &mpsc::Sender<ChangeEvent>,
    ip_to_speaker: &Arc<RwLock<std::collections::HashMap<IpAddr, SpeakerId>>>,
    changes: TopologyChanges,
    stamp: WriteStamp,
) {
    tracing::debug!(
        "Applying topology changes: {} groups, {} memberships",
        changes.groups.len(),
        changes.memberships.len()
    );

    // A ZoneGroupTopology NOTIFY does not have to carry ZoneGroupState. Sonos
    // sends topology events for other variables too (AlarmRunSequence,
    // ThirdPartyMediaServersX, an empty <VanishedDevices></VanishedDevices>,
    // ...), and `ZoneGroupTopologyEvent::zone_groups()` returns an empty Vec
    // when the ZoneGroupState variable is absent. Clearing on such an event
    // would drop every group, every group property, and every speaker→group
    // mapping in response to an unrelated update, leaving `groups()` empty and
    // coordinator resolution wrong until the next full snapshot arrived.
    if changes.groups.is_empty() {
        tracing::warn!(
            "Ignoring ZoneGroupTopology event with no zone groups \
             ({} memberships, {} boot_seqs, {} IPs, {} satellites): treating it as a \
             partial event rather than clearing cached group state",
            changes.memberships.len(),
            changes.boot_seqs.len(),
            changes.speaker_ips.len(),
            changes.satellite_ids.len()
        );
        return;
    }

    // Apply all changes within a single write lock
    let (membership_changes, ip_updates) = {
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

        // 3. Update GroupMembership for each speaker and track which ones
        //    changed. The membership value is kept alongside the flag because
        //    the change event now carries it, and re-reading it afterwards would
        //    race a subsequent topology event.
        let mut changed_memberships = Vec::new();
        for (speaker_id, membership) in changes.memberships {
            let outcome = store.set(&speaker_id, membership.clone(), stamp);
            changed_memberships.push((speaker_id, outcome, membership));
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

    for (speaker_id, outcome, membership) in membership_changes {
        if outcome.changed() && is_pair_watched(&watched_set, &speaker_id, GroupMembership::KEY) {
            tracing::debug!(
                "GroupMembership changed for {}, emitting event",
                speaker_id.as_str()
            );
            let _ = event_tx.send(ChangeEvent::new(
                speaker_id,
                PropertyChange::GroupMembership(membership),
                stamp,
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
/// on the coordinator.
///
/// Nothing is written to the members' bags — the coordinator's single stored
/// value stays the only copy, and a member's `get_property` still resolves to it
/// via `StateStore::get_resolved()`. What the member now receives is the
/// coordinator's value *on the event*, which is the same value it would resolve
/// to, delivered without a second lookup.
fn notify_group_members(
    watched: &Arc<RwLock<WatchCounts>>,
    event_tx: &mpsc::Sender<ChangeEvent>,
    members: &[SpeakerId],
    changes: &[PropertyChange],
    stamp: WriteStamp,
) {
    let watched_set = watched.read();
    for member_id in members {
        for change in changes {
            if change.scope() == Scope::Speaker {
                let key = change.key();
                if is_pair_watched(&watched_set, member_id, key) {
                    tracing::debug!(
                        "Notifying member {} of coordinator change for {}",
                        member_id.as_str(),
                        key
                    );
                    let _ =
                        event_tx.send(ChangeEvent::new(member_id.clone(), change.clone(), stamp));
                }
            }
        }
    }
}

/// Apply a single property change to the store.
///
/// `stamp` comes from the originating event's observation instant and is reused
/// for the notification, so the stored entry and the emitted `ChangeEvent` agree
/// on when this value was observed. It is a parameter rather than taken here
/// because this function runs arbitrarily long after the observation.
fn apply_property_change(
    store: &Arc<RwLock<StateStore>>,
    watched: &Arc<RwLock<WatchCounts>>,
    event_tx: &mpsc::Sender<ChangeEvent>,
    speaker_id: &SpeakerId,
    change: &PropertyChange,
    stamp: WriteStamp,
) {
    let key = change.key();

    let outcome = {
        let mut store = store.write();
        change.apply(&mut store, speaker_id, stamp)
    };

    if outcome.changed() {
        let is_watched = is_pair_watched(&watched.read(), speaker_id, key);

        if is_watched {
            tracing::debug!(
                "Property {} changed for {}, emitting event",
                key,
                speaker_id.as_str()
            );
            let _ = event_tx.send(ChangeEvent::new(speaker_id.clone(), change.clone(), stamp));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::GroupId;
    use crate::property::{GroupInfo, Property, Volume};
    use crate::state::retain_direct_watch;
    use sonos_api::Service;

    /// Events from this IP make `handle_event` panic, so the worker loop's
    /// `catch_unwind` guard can be tested without a permanent production hook.
    pub(super) const PANIC_TRIGGER_IP: IpAddr =
        IpAddr::V4(std::net::Ipv4Addr::new(203, 0, 113, 255));

    /// An event-sourced stamp for "now", for tests that only need a valid one.
    fn test_stamp() -> WriteStamp {
        WriteStamp::now(ChangeSource::Event)
    }

    #[test]
    fn test_apply_property_change_volume() {
        let store = Arc::new(RwLock::new(StateStore::new()));
        let watched = Arc::new(RwLock::new(WatchCounts::new()));
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
            test_stamp(),
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
        let watched = Arc::new(RwLock::new(WatchCounts::new()));
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
        retain_direct_watch(&watched, &speaker_id, Volume::KEY);

        // Apply change
        apply_property_change(
            &store,
            &watched,
            &tx,
            &speaker_id,
            &PropertyChange::Volume(Volume(75)),
            test_stamp(),
        );

        // Event should be emitted
        let event = rx.try_recv().unwrap();
        assert_eq!(event.speaker_id, speaker_id);
        assert_eq!(event.property_key(), Volume::KEY);
        assert_eq!(event.service(), Service::RenderingControl);
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
        let watched = Arc::new(RwLock::new(WatchCounts::new()));
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
            test_stamp(),
        );

        // Verify value was stored in group_props
        let s = store.read();
        let stored: Option<crate::property::GroupVolume> = s.get_group(&group_id);
        assert_eq!(stored, Some(crate::property::GroupVolume(75)));
    }

    #[test]
    fn test_apply_property_change_group_volume_no_group() {
        let store = Arc::new(RwLock::new(StateStore::new()));
        let watched = Arc::new(RwLock::new(WatchCounts::new()));
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
            test_stamp(),
        );

        // No crash, no stored value
        let s = store.read();
        assert!(s.group_props.is_empty());
    }

    #[test]
    fn test_apply_topology_changes_updates_groups() {
        let store = Arc::new(RwLock::new(StateStore::new()));
        let watched = Arc::new(RwLock::new(WatchCounts::new()));
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
        apply_topology_changes(&store, &watched, &tx, &ip_to_speaker, changes, test_stamp());

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
        let watched = Arc::new(RwLock::new(WatchCounts::new()));
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
        apply_topology_changes(&store, &watched, &tx, &ip_to_speaker, changes, test_stamp());

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
        let watched = Arc::new(RwLock::new(WatchCounts::new()));
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
        retain_direct_watch(&watched, &speaker1, GroupMembership::KEY);

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
        apply_topology_changes(&store, &watched, &tx, &ip_to_speaker, changes, test_stamp());

        // Should receive event for speaker1 (watched) but not speaker2 (not watched)
        let event = rx.try_recv().unwrap();
        assert_eq!(event.speaker_id, speaker1);
        assert_eq!(event.property_key(), GroupMembership::KEY);
        assert_eq!(event.service(), Service::ZoneGroupTopology);

        // No more events (speaker2 is not watched)
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn test_apply_topology_changes_clears_old_groups() {
        let store = Arc::new(RwLock::new(StateStore::new()));
        let watched = Arc::new(RwLock::new(WatchCounts::new()));
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
        apply_topology_changes(&store, &watched, &tx, &ip_to_speaker, changes, test_stamp());

        // Verify old group is gone, new group exists
        let s = store.read();
        assert_eq!(s.groups.len(), 1);
        assert!(!s.groups.contains_key(&GroupId::new("OLD_GROUP:1")));
        assert!(s.groups.contains_key(&new_group_id));
    }

    #[test]
    fn test_apply_topology_changes_updates_speaker_to_group_mapping() {
        let store = Arc::new(RwLock::new(StateStore::new()));
        let watched = Arc::new(RwLock::new(WatchCounts::new()));
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
        apply_topology_changes(&store, &watched, &tx, &ip_to_speaker, changes, test_stamp());

        // Verify speaker_to_group mapping is updated
        let s = store.read();
        assert_eq!(s.speaker_to_group.get(&speaker1), Some(&group_id));
        assert_eq!(s.speaker_to_group.get(&speaker2), Some(&group_id));
    }

    #[test]
    fn test_partial_topology_event_does_not_clear_groups() {
        // A ZoneGroupTopology NOTIFY that carries no ZoneGroupState (e.g. an
        // AlarmRunSequence update, or an empty <VanishedDevices></VanishedDevices>)
        // decodes to zero groups. It must not wipe cached group state.
        let store = Arc::new(RwLock::new(StateStore::new()));
        let watched = Arc::new(RwLock::new(WatchCounts::new()));
        let (tx, rx) = mpsc::channel();

        let speaker1 = SpeakerId::new("RINCON_111");
        let group_id = GroupId::new("RINCON_111:1");

        // Seed one group plus a group-scoped property
        {
            let mut s = store.write();
            s.add_speaker(make_speaker_info(
                "RINCON_111",
                "Living Room",
                "192.168.1.101",
            ));
            s.add_group(GroupInfo::new(
                group_id.clone(),
                speaker1.clone(),
                vec![speaker1.clone()],
            ));
            s.set_group(&group_id, crate::property::GroupVolume(42), test_stamp());
        }

        // Watch GroupMembership so we can also assert nothing is emitted
        retain_direct_watch(&watched, &speaker1, GroupMembership::KEY);

        let partial = TopologyChanges {
            groups: vec![],
            memberships: vec![],
            boot_seqs: vec![],
            speaker_ips: vec![],
            satellite_ids: vec![],
        };

        let ip_to_speaker = Arc::new(RwLock::new(std::collections::HashMap::new()));
        apply_topology_changes(&store, &watched, &tx, &ip_to_speaker, partial, test_stamp());

        // The seeded group, its properties, and the speaker→group mapping survive
        let s = store.read();
        assert_eq!(s.groups.len(), 1);
        assert!(s.groups.contains_key(&group_id));
        assert_eq!(s.speaker_to_group.get(&speaker1), Some(&group_id));
        assert_eq!(
            s.get_group::<crate::property::GroupVolume>(&group_id),
            Some(crate::property::GroupVolume(42))
        );

        // And no spurious notification was emitted
        assert!(rx.try_recv().is_err());
    }

    // ========================================================================
    // Observation-time stamping of events
    //
    // These drive `handle_event` rather than `apply_property_change`, because
    // the thing under test is precisely *where the stamp comes from* — and only
    // `handle_event` makes that choice. Calling `apply_property_change` with a
    // hand-made stamp would test the store's ordering, which is already covered,
    // and would pass regardless of the bug.
    // ========================================================================

    /// A RenderingControl volume NOTIFY observed at `observed_at`.
    fn volume_event(ip: IpAddr, volume: u8, observed_at: std::time::Instant) -> EnrichedEvent {
        use sonos_stream::events::RenderingControlState;
        use sonos_stream::{EventSource, RegistrationId};

        EnrichedEvent::observed_at(
            RegistrationId::new(1),
            ip,
            Service::RenderingControl,
            EventSource::UPnPNotification {
                subscription_id: "uuid:test".to_string(),
            },
            EventData::RenderingControl(RenderingControlState {
                master_volume: Some(volume.to_string()),
                master_mute: None,
                bass: None,
                treble: None,
                loudness: None,
                lf_volume: None,
                rf_volume: None,
                lf_mute: None,
                rf_mute: None,
                balance: None,
                other_channels: std::collections::HashMap::new(),
            }),
            observed_at,
        )
    }

    /// A store, watch set, channel and IP map wired up for one speaker.
    #[allow(clippy::type_complexity)]
    fn one_speaker_worker_fixture() -> (
        Arc<RwLock<StateStore>>,
        Arc<RwLock<WatchCounts>>,
        mpsc::Sender<ChangeEvent>,
        mpsc::Receiver<ChangeEvent>,
        Arc<RwLock<std::collections::HashMap<IpAddr, SpeakerId>>>,
        SpeakerId,
        IpAddr,
    ) {
        let store = Arc::new(RwLock::new(StateStore::new()));
        let watched = Arc::new(RwLock::new(WatchCounts::new()));
        let (tx, rx) = mpsc::channel();

        let speaker_id = SpeakerId::new("RINCON_111");
        // RFC 5737 TEST-NET-1: documentation-only, never routed.
        let speaker_ip: IpAddr = "192.0.2.11".parse().unwrap();

        store
            .write()
            .add_speaker(make_speaker_info("RINCON_111", "Living Room", "192.0.2.11"));

        let ip_to_speaker = Arc::new(RwLock::new(std::collections::HashMap::from([(
            speaker_ip,
            speaker_id.clone(),
        )])));

        (
            store,
            watched,
            tx,
            rx,
            ip_to_speaker,
            speaker_id,
            speaker_ip,
        )
    }

    /// Advance the monotonic clock far enough that two `Instant`s taken either
    /// side of this call are unambiguously ordered.
    ///
    /// These tests deliberately use *real* instants in real program order rather
    /// than synthetic future ones. A future-dated stamp would make the buggy
    /// `WriteStamp::now()` look older than the fixture and the test would pass
    /// against the bug it exists to catch.
    fn tick() {
        thread::sleep(std::time::Duration::from_millis(20));
    }

    #[test]
    fn test_late_notify_does_not_clobber_fresher_local_action() {
        // The "volume snaps back" regression. A NOTIFY describing the *old*
        // volume 10 is observed on the wire first; the user then sets volume 40;
        // only afterwards does the NOTIFY reach the worker. Stamping the event at
        // apply time made it look newer than the local write, and the volume
        // visibly reverted to 10.
        let (store, watched, tx, _rx, ip_to_speaker, speaker_id, speaker_ip) =
            one_speaker_worker_fixture();

        // 1. The NOTIFY is observed at the callback server.
        let event = volume_event(speaker_ip, 10, std::time::Instant::now());
        tick();

        // 2. The user's local action lands, strictly later.
        store.write().set(
            &speaker_id,
            Volume(40),
            WriteStamp::now(ChangeSource::LocalAction),
        );
        tick();

        // 3. Only now does the already-stale NOTIFY reach the worker.
        handle_event(&event, &store, &watched, &tx, &ip_to_speaker);

        assert_eq!(
            store.read().get::<Volume>(&speaker_id),
            Some(Volume(40)),
            "a NOTIFY observed before a local write must not overwrite it — \
             this is the volume-snaps-back symptom"
        );
    }

    #[test]
    fn test_current_fetch_not_rejected_by_older_notify() {
        // A NOTIFY is observed, then a `fetch()` request goes out — so the fetch
        // holds strictly newer truth — but the NOTIFY reaches the worker before
        // the fetch response lands. Stamping the event at apply time made it look
        // newer than the fetch, so the fetch was dropped as `Stale` and `get()`
        // disagreed with `fetch()` indefinitely.
        let (store, watched, tx, _rx, ip_to_speaker, speaker_id, speaker_ip) =
            one_speaker_worker_fixture();

        // 1. The NOTIFY is observed at the callback server.
        let event = volume_event(speaker_ip, 10, std::time::Instant::now());
        tick();

        // 2. The fetch request goes out, observing the device strictly later.
        let fetch_observed = std::time::Instant::now();
        tick();

        // 3. The NOTIFY reaches the worker first and is applied.
        handle_event(&event, &store, &watched, &tx, &ip_to_speaker);
        assert_eq!(
            store.read().get::<Volume>(&speaker_id),
            Some(Volume(10)),
            "precondition: the event should have been applied"
        );

        // 4. Then the slower fetch response finally lands.
        let outcome = store.write().set(
            &speaker_id,
            Volume(40),
            WriteStamp::observed_at(ChangeSource::Fetch, fetch_observed),
        );

        assert_eq!(
            outcome,
            crate::state::WriteOutcome::Changed,
            "a fetch that observed the device after the event must be accepted, \
             not rejected as stale"
        );
        assert_eq!(store.read().get::<Volume>(&speaker_id), Some(Volume(40)));
    }

    #[test]
    fn test_genuinely_newer_event_still_wins() {
        // The inverse-failure guard: correcting the stamp must not start
        // discarding events that really are newer than the stored value. Here the
        // local write happens first and the NOTIFY is observed after it, so the
        // event must win — on the instant, not merely on the `Event` >
        // `LocalAction` tie-break.
        let (store, watched, tx, rx, ip_to_speaker, speaker_id, speaker_ip) =
            one_speaker_worker_fixture();
        retain_direct_watch(&watched, &speaker_id, Volume::KEY);

        store.write().set(
            &speaker_id,
            Volume(40),
            WriteStamp::now(ChangeSource::LocalAction),
        );
        tick();

        let event_observed = std::time::Instant::now();
        handle_event(
            &volume_event(speaker_ip, 10, event_observed),
            &store,
            &watched,
            &tx,
            &ip_to_speaker,
        );

        assert_eq!(
            store.read().get::<Volume>(&speaker_id),
            Some(Volume(10)),
            "an event observed after the stored write must still be applied"
        );

        // And it must still notify — a dropped notification is as bad as a
        // dropped write.
        let notified = rx.try_recv().expect("newer event must still notify");
        assert_eq!(notified.property_key(), Volume::KEY);
        assert_eq!(
            notified.timestamp, event_observed,
            "the emitted event must carry the observation instant, not the apply instant"
        );
    }

    #[test]
    fn test_worker_survives_decoder_panic() {
        use sonos_stream::events::RenderingControlState;
        use sonos_stream::{EnrichedEvent, EventSource, RegistrationId};

        let store = Arc::new(RwLock::new(StateStore::new()));
        let watched = Arc::new(RwLock::new(WatchCounts::new()));
        let (tx, rx) = mpsc::channel();

        let speaker_id = SpeakerId::new("RINCON_111");
        let speaker_ip: IpAddr = "192.168.1.101".parse().unwrap();

        {
            let mut s = store.write();
            s.add_speaker(make_speaker_info(
                "RINCON_111",
                "Living Room",
                "192.168.1.101",
            ));
        }
        retain_direct_watch(&watched, &speaker_id, Volume::KEY);

        let ip_to_speaker = Arc::new(RwLock::new(std::collections::HashMap::from([(
            speaker_ip,
            speaker_id.clone(),
        )])));

        let make_event = |ip: IpAddr, volume: &str| {
            EnrichedEvent::new(
                RegistrationId::new(1),
                ip,
                Service::RenderingControl,
                EventSource::UPnPNotification {
                    subscription_id: "uuid:test".to_string(),
                },
                EventData::RenderingControl(RenderingControlState {
                    master_volume: Some(volume.to_string()),
                    master_mute: None,
                    bass: None,
                    treble: None,
                    loudness: None,
                    lf_volume: None,
                    rf_volume: None,
                    lf_mute: None,
                    rf_mute: None,
                    balance: None,
                    other_channels: std::collections::HashMap::new(),
                }),
            )
        };

        // First event panics (see PANIC_TRIGGER_IP), second is valid.
        // A panic backtrace on stderr during this test is expected.
        let events = vec![
            make_event(PANIC_TRIGGER_IP, "10"),
            make_event(speaker_ip, "37"),
        ];

        run_event_loop(events.into_iter(), &store, &watched, &tx, &ip_to_speaker);

        // The valid event was still processed and its notification delivered
        let event = rx.try_recv().unwrap();
        assert_eq!(event.speaker_id, speaker_id);
        assert_eq!(event.property_key(), Volume::KEY);
        assert_eq!(store.read().get::<Volume>(&speaker_id), Some(Volume(37)));
    }

    #[test]
    fn test_apply_topology_changes_no_event_when_membership_unchanged() {
        let store = Arc::new(RwLock::new(StateStore::new()));
        let watched = Arc::new(RwLock::new(WatchCounts::new()));
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
            s.set(
                &speaker1,
                GroupMembership::new(group_id.clone(), true),
                test_stamp(),
            );
        }

        // Watch the property
        retain_direct_watch(&watched, &speaker1, GroupMembership::KEY);

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
        apply_topology_changes(&store, &watched, &tx, &ip_to_speaker, changes, test_stamp());

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
        let watched = Arc::new(RwLock::new(WatchCounts::new()));
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
        retain_direct_watch(&watched, &coordinator, PlaybackState::KEY);
        retain_direct_watch(&watched, &member, PlaybackState::KEY);

        // Simulate what event_worker does: apply changes to coordinator, then notify members
        let changes = vec![PropertyChange::PlaybackState(PlaybackState::Playing)];

        // Apply to coordinator only
        for change in &changes {
            apply_property_change(&store, &watched, &tx, &coordinator, change, test_stamp());
        }

        // Notify group members (notification only, no data copy)
        let members = {
            let s = store.read();
            resolve_group_members(&s, &coordinator)
        };
        notify_group_members(&watched, &tx, &members, &changes, test_stamp());

        // Both coordinator and member should have received ChangeEvents
        let event1 = rx.try_recv().unwrap();
        assert_eq!(event1.speaker_id, coordinator);
        assert_eq!(event1.property_key(), PlaybackState::KEY);

        let event2 = rx.try_recv().unwrap();
        assert_eq!(event2.speaker_id, member);
        assert_eq!(event2.property_key(), PlaybackState::KEY);

        // The member's event carries the coordinator's value, so a member-side
        // consumer needs no store lookup and no coordinator resolution to react.
        assert!(
            matches!(
                event2.change,
                PropertyChange::PlaybackState(PlaybackState::Playing)
            ),
            "member notification must carry the coordinator's value, got {:?}",
            event2.change
        );

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
        let watched = Arc::new(RwLock::new(WatchCounts::new()));
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
        retain_direct_watch(&watched, &speaker, PlaybackState::KEY);

        // Apply change to the standalone speaker
        let changes = vec![PropertyChange::PlaybackState(PlaybackState::Playing)];
        for change in &changes {
            apply_property_change(&store, &watched, &tx, &speaker, change, test_stamp());
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
        let watched = Arc::new(RwLock::new(WatchCounts::new()));
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
        retain_direct_watch(&watched, &coordinator, Volume::KEY);
        retain_direct_watch(&watched, &member, Volume::KEY);

        // Apply Volume change only to coordinator (PerSpeaker service — no notification)
        apply_property_change(
            &store,
            &watched,
            &tx,
            &coordinator,
            &PropertyChange::Volume(Volume(80)),
            test_stamp(),
        );

        // RenderingControl is PerSpeaker, so we do NOT notify members.
        // Only the coordinator gets the event.
        let event = rx.try_recv().unwrap();
        assert_eq!(event.speaker_id, coordinator);
        assert_eq!(event.property_key(), Volume::KEY);

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

        let watched = Arc::new(RwLock::new(WatchCounts::new()));
        let (tx, rx) = mpsc::channel();

        let member_watched = SpeakerId::new("RINCON_WATCHED");
        let member_unwatched = SpeakerId::new("RINCON_UNWATCHED");

        // Only watch PlaybackState on one member
        retain_direct_watch(&watched, &member_watched, PlaybackState::KEY);
        // member_unwatched is NOT in the watched set

        let changes = vec![PropertyChange::PlaybackState(PlaybackState::Playing)];
        let members = vec![member_watched.clone(), member_unwatched.clone()];

        notify_group_members(&watched, &tx, &members, &changes, test_stamp());

        // Only the watched member should get a notification
        let event = rx.try_recv().unwrap();
        assert_eq!(event.speaker_id, member_watched);
        assert_eq!(event.property_key(), PlaybackState::KEY);

        // No event for the unwatched member
        assert!(rx.try_recv().is_err());
    }
}
