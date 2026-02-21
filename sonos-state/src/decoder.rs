//! Event decoder - converts EnrichedEvent to typed property changes
//!
//! This module decodes raw events from sonos-stream into typed property
//! changes that can be applied to the StateStore.

use sonos_api::Service;
use sonos_stream::events::{
    AVTransportEvent, EnrichedEvent, EventData, RenderingControlEvent, ZoneGroupTopologyEvent,
};

use crate::model::{GroupId, SpeakerId};
use crate::property::{
    Bass, CurrentTrack, GroupInfo, GroupMembership, Loudness, Mute, PlaybackState, Position,
    Treble, Volume,
};

/// Decoded changes from a single event
#[derive(Debug)]
pub struct DecodedChanges {
    /// Speaker ID the changes apply to
    pub speaker_id: SpeakerId,
    /// List of property changes
    pub changes: Vec<PropertyChange>,
}

/// Changes extracted from a ZoneGroupTopology event
///
/// This struct contains the complete topology update including:
/// - All groups with their coordinator and members
/// - GroupMembership for each speaker in the topology
#[derive(Debug)]
pub struct TopologyChanges {
    /// Updated group information
    pub groups: Vec<GroupInfo>,
    /// Updated speaker memberships: (speaker_id, membership)
    pub memberships: Vec<(SpeakerId, GroupMembership)>,
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
        EventData::GroupManagementEvent(_) => vec![],
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

/// Decode ZoneGroupTopology event data into property changes
///
/// Note: This returns an empty Vec because topology changes are handled
/// specially via `decode_topology_event()` which returns `TopologyChanges`.
fn decode_topology(_event: &ZoneGroupTopologyEvent) -> Vec<PropertyChange> {
    // Topology events are handled specially via decode_topology_event()
    // which returns TopologyChanges instead of PropertyChange
    vec![]
}

/// Decode a ZoneGroupTopology event into TopologyChanges
///
/// This extracts group information and speaker memberships from the topology event.
/// Each zone group becomes a GroupInfo, and each member gets a GroupMembership.
///
/// # Arguments
/// * `event` - The ZoneGroupTopology event to decode
///
/// # Returns
/// TopologyChanges containing all groups and speaker memberships
pub fn decode_topology_event(event: &ZoneGroupTopologyEvent) -> TopologyChanges {
    let mut groups = Vec::new();
    let mut memberships = Vec::new();

    for zone_group in &event.zone_groups {
        let group_id = GroupId::new(&zone_group.id);
        let coordinator_id = SpeakerId::new(&zone_group.coordinator);

        // Collect all member IDs
        let member_ids: Vec<SpeakerId> = zone_group
            .members
            .iter()
            .map(|m| SpeakerId::new(&m.uuid))
            .collect();

        // Create GroupInfo for this zone group
        let group_info = GroupInfo::new(
            group_id.clone(),
            coordinator_id.clone(),
            member_ids.clone(),
        );
        groups.push(group_info);

        // Create GroupMembership for each member
        for member in &zone_group.members {
            let speaker_id = SpeakerId::new(&member.uuid);
            let is_coordinator = speaker_id == coordinator_id;
            let membership = GroupMembership::new(group_id.clone(), is_coordinator);
            memberships.push((speaker_id, membership));
        }
    }

    TopologyChanges { groups, memberships }
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
        use crate::model::GroupId;

        let vol_change = PropertyChange::Volume(Volume(50));
        assert_eq!(vol_change.service(), Volume::SERVICE);

        let ps_change = PropertyChange::PlaybackState(PlaybackState::Playing);
        assert_eq!(ps_change.service(), PlaybackState::SERVICE);

        let gm_change =
            PropertyChange::GroupMembership(GroupMembership::new(GroupId::new("RINCON_test:1"), true));
        assert_eq!(gm_change.service(), GroupMembership::SERVICE);
    }

    // ========================================================================
    // Unit Tests for decode_topology_event
    // ========================================================================

    use sonos_stream::events::types::{NetworkInfo, ZoneGroupInfo, ZoneGroupMemberInfo};

    /// Helper to create a ZoneGroupMemberInfo for testing
    fn make_member(uuid: &str, zone_name: &str) -> ZoneGroupMemberInfo {
        ZoneGroupMemberInfo {
            uuid: uuid.to_string(),
            location: format!("http://192.168.1.100:1400/xml/device_description.xml"),
            zone_name: zone_name.to_string(),
            software_version: "79.1-56030".to_string(),
            network_info: NetworkInfo {
                wireless_mode: "0".to_string(),
                wifi_enabled: "1".to_string(),
                eth_link: "1".to_string(),
                channel_freq: "2412".to_string(),
                behind_wifi_extender: "0".to_string(),
            },
            satellites: vec![],
        }
    }

    #[test]
    fn test_decode_topology_single_group_one_speaker() {
        // Single speaker in a standalone group
        let event = ZoneGroupTopologyEvent {
            zone_groups: vec![ZoneGroupInfo {
                coordinator: "RINCON_111111111111".to_string(),
                id: "RINCON_111111111111:0".to_string(),
                members: vec![make_member("RINCON_111111111111", "Living Room")],
            }],
            vanished_devices: vec![],
        };

        let result = decode_topology_event(&event);

        // Should have 1 group
        assert_eq!(result.groups.len(), 1);
        let group = &result.groups[0];
        assert_eq!(group.id.as_str(), "RINCON_111111111111:0");
        assert_eq!(group.coordinator_id.as_str(), "RINCON_111111111111");
        assert_eq!(group.member_ids.len(), 1);
        assert!(group.is_standalone());

        // Should have 1 membership
        assert_eq!(result.memberships.len(), 1);
        let (speaker_id, membership) = &result.memberships[0];
        assert_eq!(speaker_id.as_str(), "RINCON_111111111111");
        assert_eq!(membership.group_id.as_str(), "RINCON_111111111111:0");
        assert!(membership.is_coordinator);
    }

    #[test]
    fn test_decode_topology_single_group_multiple_speakers() {
        // Group with 3 speakers: coordinator + 2 members
        let event = ZoneGroupTopologyEvent {
            zone_groups: vec![ZoneGroupInfo {
                coordinator: "RINCON_111111111111".to_string(),
                id: "RINCON_111111111111:0".to_string(),
                members: vec![
                    make_member("RINCON_111111111111", "Living Room"),
                    make_member("RINCON_222222222222", "Kitchen"),
                    make_member("RINCON_333333333333", "Bedroom"),
                ],
            }],
            vanished_devices: vec![],
        };

        let result = decode_topology_event(&event);

        // Should have 1 group with 3 members
        assert_eq!(result.groups.len(), 1);
        let group = &result.groups[0];
        assert_eq!(group.member_ids.len(), 3);
        assert!(!group.is_standalone());

        // Should have 3 memberships
        assert_eq!(result.memberships.len(), 3);

        // Check coordinator membership
        let coordinator_membership = result.memberships.iter()
            .find(|(sid, _)| sid.as_str() == "RINCON_111111111111")
            .map(|(_, m)| m);
        assert!(coordinator_membership.is_some());
        assert!(coordinator_membership.unwrap().is_coordinator);

        // Check non-coordinator memberships
        let kitchen_membership = result.memberships.iter()
            .find(|(sid, _)| sid.as_str() == "RINCON_222222222222")
            .map(|(_, m)| m);
        assert!(kitchen_membership.is_some());
        assert!(!kitchen_membership.unwrap().is_coordinator);

        let bedroom_membership = result.memberships.iter()
            .find(|(sid, _)| sid.as_str() == "RINCON_333333333333")
            .map(|(_, m)| m);
        assert!(bedroom_membership.is_some());
        assert!(!bedroom_membership.unwrap().is_coordinator);
    }

    #[test]
    fn test_decode_topology_multiple_groups() {
        // Two separate groups
        let event = ZoneGroupTopologyEvent {
            zone_groups: vec![
                ZoneGroupInfo {
                    coordinator: "RINCON_111111111111".to_string(),
                    id: "RINCON_111111111111:0".to_string(),
                    members: vec![
                        make_member("RINCON_111111111111", "Living Room"),
                        make_member("RINCON_222222222222", "Kitchen"),
                    ],
                },
                ZoneGroupInfo {
                    coordinator: "RINCON_333333333333".to_string(),
                    id: "RINCON_333333333333:0".to_string(),
                    members: vec![make_member("RINCON_333333333333", "Bedroom")],
                },
            ],
            vanished_devices: vec![],
        };

        let result = decode_topology_event(&event);

        // Should have 2 groups
        assert_eq!(result.groups.len(), 2);

        // First group: 2 members
        let group1 = &result.groups[0];
        assert_eq!(group1.id.as_str(), "RINCON_111111111111:0");
        assert_eq!(group1.member_ids.len(), 2);

        // Second group: 1 member (standalone)
        let group2 = &result.groups[1];
        assert_eq!(group2.id.as_str(), "RINCON_333333333333:0");
        assert_eq!(group2.member_ids.len(), 1);
        assert!(group2.is_standalone());

        // Should have 3 total memberships
        assert_eq!(result.memberships.len(), 3);

        // Verify each speaker has correct group_id
        let living_room = result.memberships.iter()
            .find(|(sid, _)| sid.as_str() == "RINCON_111111111111")
            .map(|(_, m)| m).unwrap();
        assert_eq!(living_room.group_id.as_str(), "RINCON_111111111111:0");
        assert!(living_room.is_coordinator);

        let kitchen = result.memberships.iter()
            .find(|(sid, _)| sid.as_str() == "RINCON_222222222222")
            .map(|(_, m)| m).unwrap();
        assert_eq!(kitchen.group_id.as_str(), "RINCON_111111111111:0");
        assert!(!kitchen.is_coordinator);

        let bedroom = result.memberships.iter()
            .find(|(sid, _)| sid.as_str() == "RINCON_333333333333")
            .map(|(_, m)| m).unwrap();
        assert_eq!(bedroom.group_id.as_str(), "RINCON_333333333333:0");
        assert!(bedroom.is_coordinator);
    }

    #[test]
    fn test_decode_topology_empty_event() {
        // Empty topology (no groups)
        let event = ZoneGroupTopologyEvent {
            zone_groups: vec![],
            vanished_devices: vec![],
        };

        let result = decode_topology_event(&event);

        assert!(result.groups.is_empty());
        assert!(result.memberships.is_empty());
    }
}

// ============================================================================
// Property-Based Tests for Topology Decoding
// ============================================================================

#[cfg(test)]
mod property_tests {
    use super::*;
    use proptest::prelude::*;
    use sonos_stream::events::types::{NetworkInfo, ZoneGroupInfo, ZoneGroupMemberInfo};

    /// Strategy for generating valid RINCON-style speaker UUIDs
    fn speaker_uuid_strategy() -> impl Strategy<Value = String> {
        "[A-F0-9]{12}".prop_map(|s| format!("RINCON_{}", s))
    }

    /// Strategy for generating a zone group member
    fn zone_group_member_strategy() -> impl Strategy<Value = ZoneGroupMemberInfo> {
        (
            speaker_uuid_strategy(),
            "[A-Za-z ]{3,15}",
        ).prop_map(|(uuid, zone_name)| {
            ZoneGroupMemberInfo {
                uuid,
                location: "http://192.168.1.100:1400/xml/device_description.xml".to_string(),
                zone_name: zone_name.trim().to_string(),
                software_version: "79.1-56030".to_string(),
                network_info: NetworkInfo {
                    wireless_mode: "0".to_string(),
                    wifi_enabled: "1".to_string(),
                    eth_link: "1".to_string(),
                    channel_freq: "2412".to_string(),
                    behind_wifi_extender: "0".to_string(),
                },
                satellites: vec![],
            }
        })
    }

    /// Strategy for generating a zone group with 1-5 members
    fn zone_group_strategy() -> impl Strategy<Value = ZoneGroupInfo> {
        proptest::collection::vec(zone_group_member_strategy(), 1..=5)
            .prop_flat_map(|members| {
                // First member is always the coordinator
                let coordinator = members[0].uuid.clone();
                let group_id = format!("{}:0", coordinator);
                Just(ZoneGroupInfo {
                    coordinator,
                    id: group_id,
                    members,
                })
            })
    }

    /// Strategy for generating a topology event with 1-3 groups
    fn topology_event_strategy() -> impl Strategy<Value = ZoneGroupTopologyEvent> {
        proptest::collection::vec(zone_group_strategy(), 1..=3)
            .prop_map(|zone_groups| {
                ZoneGroupTopologyEvent {
                    zone_groups,
                    vanished_devices: vec![],
                }
            })
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        /// *For any* valid ZoneGroupTopology event containing zone groups, after processing:
        /// - Each zone group in the event corresponds to a GroupInfo in the result
        /// - Each member in each zone group has a GroupMembership in the result
        /// - The GroupMembership.group_id matches the zone group's ID
        /// - The GroupMembership.is_coordinator is true only for the coordinator
        #[test]
        fn prop_topology_event_processing_round_trip(event in topology_event_strategy()) {
            let result = decode_topology_event(&event);

            // Property: Each zone group in the event corresponds to a GroupInfo
            prop_assert_eq!(
                result.groups.len(),
                event.zone_groups.len(),
                "Number of groups should match number of zone groups in event"
            );

            // Property: Each member has a GroupMembership
            let total_members: usize = event.zone_groups.iter()
                .map(|zg| zg.members.len())
                .sum();
            prop_assert_eq!(
                result.memberships.len(),
                total_members,
                "Number of memberships should match total members across all groups"
            );

            // Property: GroupInfo matches zone group data
            for (group_info, zone_group) in result.groups.iter().zip(event.zone_groups.iter()) {
                prop_assert_eq!(
                    group_info.id.as_str(),
                    &zone_group.id,
                    "GroupInfo ID should match zone group ID"
                );
                prop_assert_eq!(
                    group_info.coordinator_id.as_str(),
                    &zone_group.coordinator,
                    "GroupInfo coordinator should match zone group coordinator"
                );
                prop_assert_eq!(
                    group_info.member_ids.len(),
                    zone_group.members.len(),
                    "GroupInfo member count should match zone group member count"
                );
            }

            // Property: GroupMembership.group_id matches and is_coordinator is correct
            for zone_group in &event.zone_groups {
                for member in &zone_group.members {
                    let membership = result.memberships.iter()
                        .find(|(sid, _)| sid.as_str() == member.uuid)
                        .map(|(_, m)| m);

                    prop_assert!(
                        membership.is_some(),
                        "Each member should have a GroupMembership"
                    );

                    let membership = membership.unwrap();
                    prop_assert_eq!(
                        membership.group_id.as_str(),
                        &zone_group.id,
                        "GroupMembership.group_id should match zone group ID"
                    );

                    let is_coordinator = member.uuid == zone_group.coordinator;
                    prop_assert_eq!(
                        membership.is_coordinator,
                        is_coordinator,
                        "is_coordinator should be true only for the coordinator"
                    );
                }
            }
        }

        /// *For any* decoded topology, the coordinator_id should always be present
        /// in the member_ids list.
        #[test]
        fn prop_coordinator_always_in_members(event in topology_event_strategy()) {
            let result = decode_topology_event(&event);

            for group_info in &result.groups {
                prop_assert!(
                    group_info.member_ids.contains(&group_info.coordinator_id),
                    "Coordinator should always be in member_ids"
                );
            }
        }

        /// *For any* decoded topology, each group should have exactly one member
        /// marked as coordinator in the memberships.
        #[test]
        fn prop_exactly_one_coordinator_per_group(event in topology_event_strategy()) {
            let result = decode_topology_event(&event);

            for group_info in &result.groups {
                let coordinator_count = result.memberships.iter()
                    .filter(|(sid, membership)| {
                        group_info.member_ids.contains(sid) && membership.is_coordinator
                    })
                    .count();

                prop_assert_eq!(
                    coordinator_count,
                    1,
                    "Each group should have exactly one coordinator"
                );
            }
        }
    }
}
