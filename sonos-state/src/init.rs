//! Initialization from a single speaker IP using GetZoneGroupTopology

use std::net::IpAddr;

use sonos_api::services::zone_group_topology::events::ZoneGroupTopologyEvent;
use sonos_api::services::zone_group_topology::get_zone_group_state;
use sonos_api::SonosClient;

use crate::{Group, GroupId, Result, Speaker, SpeakerId, SpeakerRef, StateError};

/// Initialize state from a single speaker IP using GetZoneGroupTopology
///
/// This queries the topology from any speaker in the network and discovers
/// all speakers and groups in the system.
///
/// # Arguments
///
/// * `ip` - IP address of any Sonos speaker in the network
///
/// # Returns
///
/// A tuple of (speakers, groups) representing the full network topology
pub fn initialize_from_ip(ip: IpAddr) -> Result<(Vec<Speaker>, Vec<Group>)> {
    let client = SonosClient::new();

    // Build and execute the GetZoneGroupState operation
    let operation = get_zone_group_state()
        .build()
        .map_err(|e| StateError::Init(format!("Failed to build operation: {}", e)))?;

    let response = client
        .execute_enhanced(&ip.to_string(), operation)
        .map_err(|e| StateError::Init(format!("Failed to get topology: {}", e)))?;

    // The response contains raw XML that we need to wrap in the event format
    // for parsing with ZoneGroupTopologyEvent. The response already has a
    // <ZoneGroupState> wrapper, so we need to extract the inner content
    // and HTML-encode it (since the deserializer expects a string with XML).
    let inner_content = if response.zone_group_state.starts_with("<ZoneGroupState>")
        && response.zone_group_state.ends_with("</ZoneGroupState>") {
        // Extract content between <ZoneGroupState> and </ZoneGroupState>
        &response.zone_group_state[16..response.zone_group_state.len()-17]
    } else {
        // If it doesn't have the wrapper, use as-is
        &response.zone_group_state
    };

    let xml = format!(
        r#"<e:propertyset xmlns:e="urn:schemas-upnp-org:event-1-0">
            <e:property><ZoneGroupState>{}</ZoneGroupState></e:property>
        </e:propertyset>"#,
        escape_xml(inner_content)
    );

    let event = ZoneGroupTopologyEvent::from_xml(&xml)
        .map_err(|e| StateError::Parse(format!("Failed to parse topology: {}", e)))?;

    let zone_groups = event.zone_groups();

    let mut speakers = Vec::new();
    let mut groups = Vec::new();

    for zone_group in &zone_groups {
        let mut member_refs = Vec::new();

        for member in &zone_group.members {
            // Parse IP from location URL
            let member_ip = extract_ip_from_location(&member.location)?;

            let speaker = Speaker {
                id: SpeakerId::new(&member.uuid),
                name: member.zone_name.clone(),
                room_name: member.zone_name.clone(),
                ip_address: member_ip,
                port: 1400,
                model_name: member
                    .metadata
                    .get("ModelName")
                    .cloned()
                    .unwrap_or_default(),
                software_version: member.software_version.clone(),
                satellites: member
                    .satellites
                    .iter()
                    .map(|s| SpeakerId::new(&s.uuid))
                    .collect(),
            };
            speakers.push(speaker);

            member_refs.push(SpeakerRef::new(
                SpeakerId::new(&member.uuid),
                member
                    .satellites
                    .iter()
                    .map(|s| SpeakerId::new(&s.uuid))
                    .collect(),
            ));

            // Add satellite speakers as separate speakers
            for satellite in &member.satellites {
                let sat_ip = extract_ip_from_location(&satellite.location)?;
                let sat_speaker = Speaker {
                    id: SpeakerId::new(&satellite.uuid),
                    name: satellite.zone_name.clone(),
                    room_name: satellite.zone_name.clone(),
                    ip_address: sat_ip,
                    port: 1400,
                    model_name: String::new(),
                    software_version: String::new(),
                    satellites: vec![],
                };
                speakers.push(sat_speaker);
            }
        }

        let group = Group::new(
            GroupId::new(&zone_group.id),
            SpeakerId::new(&zone_group.coordinator),
            member_refs,
        );
        groups.push(group);
    }

    Ok((speakers, groups))
}

/// Extract IP address from a location URL
///
/// # Arguments
///
/// * `location` - URL like "http://192.168.1.100:1400/xml/device_description.xml"
fn extract_ip_from_location(location: &str) -> Result<IpAddr> {
    let url = url::Url::parse(location)
        .map_err(|e| StateError::Parse(format!("Invalid location URL '{}': {}", location, e)))?;

    let host = url
        .host_str()
        .ok_or_else(|| StateError::Parse(format!("No host in location URL: {}", location)))?;

    host.parse()
        .map_err(|e| StateError::Parse(format!("Invalid IP in location '{}': {}", host, e)))
}

/// Escape XML special characters
fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_ip_from_location() {
        let ip = extract_ip_from_location("http://192.168.1.100:1400/xml/device_description.xml");
        assert!(ip.is_ok());
        assert_eq!(ip.unwrap().to_string(), "192.168.1.100");
    }

    #[test]
    fn test_extract_ip_invalid_url() {
        let result = extract_ip_from_location("not-a-url");
        assert!(result.is_err());
    }

    #[test]
    fn test_escape_xml() {
        assert_eq!(escape_xml("a & b"), "a &amp; b");
        assert_eq!(escape_xml("<tag>"), "&lt;tag&gt;");
    }
}
