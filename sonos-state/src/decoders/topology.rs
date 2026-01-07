//! ZoneGroupTopology event decoder
//!
//! Handles topology changes (speakers, groups) from ZoneGroupTopology events.
//! This decoder is special because it can add/remove speakers and groups,
//! not just update properties.

use sonos_api::Service;

use crate::decoder::{EventData, EventDecoder, PropertyUpdate, RawEvent, ZoneMemberData};
use crate::model::{GroupId, SpeakerId, SpeakerInfo};
use crate::property::{GroupInfo, GroupMembership, Topology};
use crate::store::StateStore;

/// Decoder for ZoneGroupTopology events
///
/// Handles speaker discovery, group changes, and topology updates.
/// This is the primary way speakers and groups are added to the system.
pub struct TopologyDecoder;

impl EventDecoder for TopologyDecoder {
    fn services(&self) -> &[Service] {
        &[Service::ZoneGroupTopology]
    }

    fn decode(&self, event: &RawEvent, store: &StateStore) -> Vec<PropertyUpdate> {
        let EventData::ZoneGroupTopology(data) = &event.data else {
            return vec![];
        };

        let mut updates = Vec::new();

        // Collect all speakers and groups from the topology
        let mut all_speakers = Vec::new();
        let mut all_groups = Vec::new();

        for zone_group in &data.zone_groups {
            let group_id = GroupId::new(&zone_group.id);
            let coordinator_id = SpeakerId::new(&zone_group.coordinator);

            let mut member_ids = Vec::new();

            for member in &zone_group.members {
                let speaker_id = SpeakerId::new(&member.uuid);
                member_ids.push(speaker_id.clone());

                // Create speaker info from member data
                let speaker_info = member_to_speaker_info(member);
                all_speakers.push((speaker_info, group_id.clone(), speaker_id == coordinator_id));

                // Add satellites
                for sat_uuid in &member.satellites {
                    member_ids.push(SpeakerId::new(sat_uuid));
                }
            }

            all_groups.push(GroupInfo::new(group_id, coordinator_id, member_ids));
        }

        // Create updates to add/update all speakers
        for (speaker_info, group_id, is_coordinator) in all_speakers {
            let speaker_id = speaker_info.id.clone();

            // Add speaker metadata
            updates.push(PropertyUpdate::new(
                format!("Add/update speaker {}", speaker_info.name),
                Service::ZoneGroupTopology,
                move |store| {
                    store.add_speaker(speaker_info);
                },
            ));

            // Update group membership
            let gid = group_id.clone();
            let sid = speaker_id.clone();
            updates.push(PropertyUpdate::new(
                format!("Set {} group membership", sid),
                Service::ZoneGroupTopology,
                move |store| {
                    store.set(&sid, GroupMembership::new(Some(gid), is_coordinator));
                },
            ));
        }

        // Create updates to add/update all groups
        for group_info in all_groups.clone() {
            updates.push(PropertyUpdate::new(
                format!("Add/update group {}", group_info.id),
                Service::ZoneGroupTopology,
                move |store| {
                    store.add_group(group_info);
                },
            ));
        }

        // Update system topology
        let speakers_for_topology: Vec<SpeakerInfo> = store.speakers();
        let topology = Topology::new(speakers_for_topology, all_groups);
        updates.push(PropertyUpdate::new(
            "Update system topology",
            Service::ZoneGroupTopology,
            move |store| {
                store.set_system(topology);
            },
        ));

        // Handle vanished devices
        for vanished_uuid in &data.vanished_devices {
            let speaker_id = SpeakerId::new(vanished_uuid);
            updates.push(PropertyUpdate::new(
                format!("Remove vanished speaker {}", speaker_id),
                Service::ZoneGroupTopology,
                move |store| {
                    store.remove_speaker(&speaker_id);
                },
            ));
        }

        updates
    }

    fn name(&self) -> &'static str {
        "TopologyDecoder"
    }
}

/// Convert zone member data to speaker info
fn member_to_speaker_info(member: &ZoneMemberData) -> SpeakerInfo {
    // Parse IP from location URL
    let ip_address = member
        .ip_address
        .or_else(|| parse_ip_from_location(&member.location))
        .unwrap_or_else(|| "0.0.0.0".parse().unwrap());

    SpeakerInfo {
        id: SpeakerId::new(&member.uuid),
        name: member.zone_name.clone(),
        room_name: member.zone_name.clone(),
        ip_address,
        port: 1400,
        model_name: "Unknown".to_string(), // Would need device description for this
        software_version: member.software_version.clone(),
        satellites: member.satellites.iter().map(|s| SpeakerId::new(s)).collect(),
    }
}

/// Parse IP address from a location URL like "http://192.168.1.100:1400/xml/device_description.xml"
fn parse_ip_from_location(location: &str) -> Option<std::net::IpAddr> {
    // Remove http:// prefix
    let without_scheme = location.strip_prefix("http://")?;

    // Find the host:port part (before the path)
    let host_port = without_scheme.split('/').next()?;

    // Split host:port and get just the host
    let host = host_port.split(':').next()?;

    // Parse as IP address
    host.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decoder::{TopologyData, ZoneGroupData, ZoneMemberData};

    fn create_test_topology_data() -> TopologyData {
        TopologyData {
            zone_groups: vec![ZoneGroupData {
                id: "RINCON_123:0".to_string(),
                coordinator: "RINCON_123".to_string(),
                members: vec![
                    ZoneMemberData {
                        uuid: "RINCON_123".to_string(),
                        location: "http://192.168.1.100:1400/xml/device_description.xml"
                            .to_string(),
                        zone_name: "Living Room".to_string(),
                        software_version: "56.0-76060".to_string(),
                        ip_address: Some("192.168.1.100".parse().unwrap()),
                        satellites: vec![],
                    },
                    ZoneMemberData {
                        uuid: "RINCON_456".to_string(),
                        location: "http://192.168.1.101:1400/xml/device_description.xml"
                            .to_string(),
                        zone_name: "Kitchen".to_string(),
                        software_version: "56.0-76060".to_string(),
                        ip_address: Some("192.168.1.101".parse().unwrap()),
                        satellites: vec![],
                    },
                ],
            }],
            vanished_devices: vec![],
        }
    }

    #[test]
    fn test_decode_topology() {
        let decoder = TopologyDecoder;
        let store = StateStore::new();

        let event = RawEvent::new(
            "192.168.1.100".parse().unwrap(),
            Service::ZoneGroupTopology,
            EventData::ZoneGroupTopology(create_test_topology_data()),
        );

        let updates = decoder.decode(&event, &store);

        // Should have updates for:
        // - 2 speakers (add)
        // - 2 group memberships
        // - 1 group
        // - 1 topology
        assert!(updates.len() >= 5);

        // Apply updates
        for update in updates {
            update.apply(&store);
        }

        // Verify speakers were added
        assert_eq!(store.speaker_count(), 2);

        // Verify group was added
        assert_eq!(store.group_count(), 1);

        // Verify group membership
        let id1 = SpeakerId::new("RINCON_123");
        let membership = store.get::<GroupMembership>(&id1).unwrap();
        assert!(membership.is_coordinator);
        assert_eq!(membership.group_id, Some(GroupId::new("RINCON_123:0")));

        let id2 = SpeakerId::new("RINCON_456");
        let membership2 = store.get::<GroupMembership>(&id2).unwrap();
        assert!(!membership2.is_coordinator);
    }

    #[test]
    fn test_parse_ip_from_location() {
        assert_eq!(
            parse_ip_from_location("http://192.168.1.100:1400/xml/device_description.xml"),
            Some("192.168.1.100".parse().unwrap())
        );
        assert_eq!(
            parse_ip_from_location("http://10.0.0.1:1400/xml"),
            Some("10.0.0.1".parse().unwrap())
        );
        assert_eq!(parse_ip_from_location("invalid"), None);
    }

    #[test]
    fn test_vanished_devices() {
        let decoder = TopologyDecoder;
        let store = StateStore::new();

        // First add a speaker
        store.add_speaker(SpeakerInfo {
            id: SpeakerId::new("RINCON_OLD"),
            name: "Old Speaker".to_string(),
            room_name: "Old Room".to_string(),
            ip_address: "192.168.1.200".parse().unwrap(),
            port: 1400,
            model_name: "Test".to_string(),
            software_version: "1.0".to_string(),
            satellites: vec![],
        });

        assert_eq!(store.speaker_count(), 1);

        // Now decode topology that marks it as vanished
        let mut topology_data = create_test_topology_data();
        topology_data.vanished_devices = vec!["RINCON_OLD".to_string()];

        let event = RawEvent::new(
            "192.168.1.100".parse().unwrap(),
            Service::ZoneGroupTopology,
            EventData::ZoneGroupTopology(topology_data),
        );

        let updates = decoder.decode(&event, &store);

        for update in updates {
            update.apply(&store);
        }

        // Old speaker should be gone
        assert!(store.speaker(&SpeakerId::new("RINCON_OLD")).is_none());
    }
}
