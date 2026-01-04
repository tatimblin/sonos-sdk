//! ZoneGroupTopology service parser implementation
//!
//! This module provides serde-based parsing for ZoneGroupTopology UPnP events.
//! It handles the complex nested XML structure containing speaker groupings,
//! coordinator relationships, and comprehensive speaker metadata.

use serde::{Deserialize, Serialize};
use crate::error::ParseResult;
use crate::common::xml_decode;

/// Root parser for ZoneGroupTopology UPnP events.
///
/// UPnP events are wrapped in a propertyset structure:
/// ```xml
/// <e:propertyset xmlns:e="urn:schemas-upnp-org:event-1-0">
///   <e:property>
///     <ZoneGroupState>...</ZoneGroupState>
///   </e:property>
/// </e:propertyset>
/// ```
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename = "propertyset")]
pub struct ZoneGroupTopologyParser {
    /// The property elements (ZoneGroupState is one of them)
    #[serde(rename = "property")]
    pub properties: Vec<Property>,
}

/// Property wrapper that can contain various ZoneGroupTopology properties.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Property {
    /// The ZoneGroupState element with nested XML content
    #[serde(
        rename = "ZoneGroupState",
        deserialize_with = "xml_decode::deserialize_nested",
        default
    )]
    pub zone_group_state: Option<ZoneGroupState>,
}

/// The root element for decoded ZoneGroupState content.
///
/// Contains the complete topology of all speakers and groups in the household.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename = "ZoneGroupState")]
pub struct ZoneGroupState {
    /// All current zone groups in the system
    #[serde(rename = "ZoneGroups")]
    pub zone_groups: ZoneGroups,

    /// Devices that have vanished from the network
    #[serde(rename = "VanishedDevices")]
    pub vanished_devices: VanishedDevices,
}

/// Container for all zone groups in the system.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ZoneGroups {
    /// List of all zone groups
    #[serde(rename = "ZoneGroup", default)]
    pub zone_groups: Vec<ZoneGroup>,
}

/// A single zone group representing speakers playing together.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ZoneGroup {
    /// The coordinator (master) speaker for this group
    #[serde(rename = "@Coordinator")]
    pub coordinator: String,

    /// Unique identifier for this zone group
    #[serde(rename = "@ID")]
    pub id: String,

    /// All speakers that are members of this zone group
    #[serde(rename = "ZoneGroupMember", default)]
    pub zone_group_members: Vec<ZoneGroupMember>,
}

/// A speaker that is part of a zone group.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ZoneGroupMember {
    /// Unique identifier for this speaker (RINCON_...)
    #[serde(rename = "@UUID")]
    pub uuid: String,

    /// Network location of the speaker
    #[serde(rename = "@Location")]
    pub location: String,

    /// Human-readable name of the room/zone
    #[serde(rename = "@ZoneName")]
    pub zone_name: String,

    /// Icon identifier for the speaker
    #[serde(rename = "@Icon")]
    pub icon: String,

    /// Configuration flags
    #[serde(rename = "@Configuration")]
    pub configuration: String,

    /// Software version running on the speaker
    #[serde(rename = "@SoftwareVersion")]
    pub software_version: String,

    /// Software generation
    #[serde(rename = "@SWGen")]
    pub sw_gen: String,

    /// Minimum compatible software version
    #[serde(rename = "@MinCompatibleVersion")]
    pub min_compatible_version: String,

    /// Legacy compatible software version
    #[serde(rename = "@LegacyCompatibleVersion")]
    pub legacy_compatible_version: String,

    /// Home theater satellite channel mapping (optional)
    #[serde(rename = "@HTSatChanMapSet", default)]
    pub ht_sat_chan_map_set: Option<String>,

    /// Active zone ID (optional)
    #[serde(rename = "@ActiveZoneID", default)]
    pub active_zone_id: Option<String>,

    /// Boot sequence number
    #[serde(rename = "@BootSeq")]
    pub boot_seq: String,

    /// TV configuration error status
    #[serde(rename = "@TVConfigurationError")]
    pub tv_configuration_error: String,

    /// HDMI CEC availability
    #[serde(rename = "@HdmiCecAvailable")]
    pub hdmi_cec_available: String,

    /// Wireless mode (0=wired, 1=2.4GHz, 2=5GHz)
    #[serde(rename = "@WirelessMode")]
    pub wireless_mode: String,

    /// Whether this is a wireless leaf node only
    #[serde(rename = "@WirelessLeafOnly")]
    pub wireless_leaf_only: String,

    /// WiFi channel frequency
    #[serde(rename = "@ChannelFreq")]
    pub channel_freq: String,

    /// Whether behind a WiFi extender
    #[serde(rename = "@BehindWifiExtender")]
    pub behind_wifi_extender: String,

    /// Whether WiFi is enabled
    #[serde(rename = "@WifiEnabled")]
    pub wifi_enabled: String,

    /// Ethernet link status
    #[serde(rename = "@EthLink")]
    pub eth_link: String,

    /// Speaker orientation
    #[serde(rename = "@Orientation")]
    pub orientation: String,

    /// Room calibration state
    #[serde(rename = "@RoomCalibrationState")]
    pub room_calibration_state: String,

    /// Secure registration state
    #[serde(rename = "@SecureRegState")]
    pub secure_reg_state: String,

    /// Voice configuration state
    #[serde(rename = "@VoiceConfigState")]
    pub voice_config_state: String,

    /// Whether microphone is enabled
    #[serde(rename = "@MicEnabled")]
    pub mic_enabled: String,

    /// Headphone swap active status
    #[serde(rename = "@HeadphoneSwapActive")]
    pub headphone_swap_active: String,

    /// Whether AirPlay is enabled
    #[serde(rename = "@AirPlayEnabled")]
    pub airplay_enabled: String,

    /// Virtual line-in source (optional)
    #[serde(rename = "@VirtualLineInSource", default)]
    pub virtual_line_in_source: Option<String>,

    /// Idle state
    #[serde(rename = "@IdleState")]
    pub idle_state: String,

    /// Additional information
    #[serde(rename = "@MoreInfo")]
    pub more_info: String,

    /// SSL port
    #[serde(rename = "@SSLPort")]
    pub ssl_port: String,

    /// Household SSL port
    #[serde(rename = "@HHSSLPort")]
    pub hhssl_port: String,

    /// Satellite speakers (for home theater configurations)
    #[serde(rename = "Satellite", default)]
    pub satellites: Vec<Satellite>,
}

/// A satellite speaker (part of a home theater setup).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Satellite {
    /// Unique identifier for this satellite speaker
    #[serde(rename = "@UUID")]
    pub uuid: String,

    /// Network location of the satellite
    #[serde(rename = "@Location")]
    pub location: String,

    /// Zone name (usually same as main speaker)
    #[serde(rename = "@ZoneName")]
    pub zone_name: String,

    /// Icon identifier
    #[serde(rename = "@Icon")]
    pub icon: String,

    /// Configuration flags
    #[serde(rename = "@Configuration")]
    pub configuration: String,

    /// Whether this satellite is invisible in UI
    #[serde(rename = "@Invisible")]
    pub invisible: String,

    /// Software version
    #[serde(rename = "@SoftwareVersion")]
    pub software_version: String,

    /// Software generation
    #[serde(rename = "@SWGen")]
    pub sw_gen: String,

    /// Minimum compatible version
    #[serde(rename = "@MinCompatibleVersion")]
    pub min_compatible_version: String,

    /// Legacy compatible version
    #[serde(rename = "@LegacyCompatibleVersion")]
    pub legacy_compatible_version: String,

    /// Home theater satellite channel mapping
    #[serde(rename = "@HTSatChanMapSet")]
    pub ht_sat_chan_map_set: String,

    /// Active zone ID
    #[serde(rename = "@ActiveZoneID")]
    pub active_zone_id: String,

    /// Boot sequence number
    #[serde(rename = "@BootSeq")]
    pub boot_seq: String,

    /// TV configuration error status
    #[serde(rename = "@TVConfigurationError")]
    pub tv_configuration_error: String,

    /// HDMI CEC availability
    #[serde(rename = "@HdmiCecAvailable")]
    pub hdmi_cec_available: String,

    /// Wireless mode
    #[serde(rename = "@WirelessMode")]
    pub wireless_mode: String,

    /// Wireless leaf only
    #[serde(rename = "@WirelessLeafOnly")]
    pub wireless_leaf_only: String,

    /// Channel frequency
    #[serde(rename = "@ChannelFreq")]
    pub channel_freq: String,

    /// Behind WiFi extender
    #[serde(rename = "@BehindWifiExtender")]
    pub behind_wifi_extender: String,

    /// WiFi enabled
    #[serde(rename = "@WifiEnabled")]
    pub wifi_enabled: String,

    /// Ethernet link status
    #[serde(rename = "@EthLink")]
    pub eth_link: String,

    /// Orientation
    #[serde(rename = "@Orientation")]
    pub orientation: String,

    /// Room calibration state
    #[serde(rename = "@RoomCalibrationState")]
    pub room_calibration_state: String,

    /// Secure registration state
    #[serde(rename = "@SecureRegState")]
    pub secure_reg_state: String,

    /// Voice configuration state
    #[serde(rename = "@VoiceConfigState")]
    pub voice_config_state: String,

    /// Microphone enabled
    #[serde(rename = "@MicEnabled")]
    pub mic_enabled: String,

    /// Headphone swap active
    #[serde(rename = "@HeadphoneSwapActive")]
    pub headphone_swap_active: String,

    /// AirPlay enabled
    #[serde(rename = "@AirPlayEnabled")]
    pub airplay_enabled: String,

    /// Idle state
    #[serde(rename = "@IdleState")]
    pub idle_state: String,

    /// More information
    #[serde(rename = "@MoreInfo")]
    pub more_info: String,

    /// SSL port
    #[serde(rename = "@SSLPort")]
    pub ssl_port: String,

    /// Household SSL port
    #[serde(rename = "@HHSSLPort")]
    pub hhssl_port: String,
}

/// Container for vanished devices.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct VanishedDevices {
    // Currently empty, can be extended if needed
}

impl ZoneGroupTopologyParser {
    /// Parse ZoneGroupTopology event XML.
    ///
    /// # Arguments
    ///
    /// * `xml` - The raw UPnP event XML
    ///
    /// # Returns
    ///
    /// The parsed ZoneGroupTopology event, or an error if parsing fails.
    pub fn from_xml(xml: &str) -> ParseResult<Self> {
        xml_decode::parse(xml)
    }

    /// Find and return the ZoneGroupState property.
    pub fn zone_group_state(&self) -> Option<&ZoneGroupState> {
        self.properties
            .iter()
            .find_map(|p| p.zone_group_state.as_ref())
    }

    /// Get all zone groups from the topology.
    pub fn zone_groups(&self) -> Option<&[ZoneGroup]> {
        self.zone_group_state()
            .map(|zgs| zgs.zone_groups.zone_groups.as_slice())
    }

    /// Find a specific zone group by coordinator UUID.
    pub fn find_group_by_coordinator(&self, coordinator_uuid: &str) -> Option<&ZoneGroup> {
        self.zone_groups()?
            .iter()
            .find(|group| group.coordinator == coordinator_uuid)
    }

    /// Find a zone group that contains a specific speaker UUID.
    pub fn find_group_containing_speaker(&self, speaker_uuid: &str) -> Option<&ZoneGroup> {
        self.zone_groups()?
            .iter()
            .find(|group| {
                group.zone_group_members
                    .iter()
                    .any(|member| member.uuid == speaker_uuid ||
                         member.satellites.iter().any(|sat| sat.uuid == speaker_uuid))
            })
    }

    /// Get the total number of speakers across all groups.
    pub fn total_speaker_count(&self) -> usize {
        self.zone_groups()
            .map(|groups| {
                groups.iter()
                    .map(|group| {
                        group.zone_group_members.len() +
                        group.zone_group_members
                            .iter()
                            .map(|member| member.satellites.len())
                            .sum::<usize>()
                    })
                    .sum()
            })
            .unwrap_or(0)
    }

    /// Get all coordinator UUIDs in the system.
    pub fn coordinators(&self) -> Vec<&str> {
        self.zone_groups()
            .map(|groups| {
                groups.iter()
                    .map(|group| group.coordinator.as_str())
                    .collect()
            })
            .unwrap_or_default()
    }
}

impl ZoneGroup {
    /// Check if this group has multiple speakers.
    pub fn is_multi_speaker_group(&self) -> bool {
        self.zone_group_members.len() > 1
    }

    /// Get the coordinator member details.
    pub fn coordinator_member(&self) -> Option<&ZoneGroupMember> {
        self.zone_group_members
            .iter()
            .find(|member| member.uuid == self.coordinator)
    }

    /// Get all speaker UUIDs in this group (including satellites).
    pub fn all_speaker_uuids(&self) -> Vec<&str> {
        let mut uuids = Vec::new();

        for member in &self.zone_group_members {
            uuids.push(member.uuid.as_str());
            for satellite in &member.satellites {
                uuids.push(satellite.uuid.as_str());
            }
        }

        uuids
    }
}

impl ZoneGroupMember {
    /// Check if this speaker has satellite speakers.
    pub fn has_satellites(&self) -> bool {
        !self.satellites.is_empty()
    }

    /// Check if this speaker is using WiFi.
    pub fn is_wireless(&self) -> bool {
        self.wireless_mode != "0" && self.wifi_enabled == "1"
    }

    /// Get the IP address from the location URL.
    pub fn ip_address(&self) -> Option<&str> {
        // Extract IP from URL like "http://192.168.4.40:1400/xml/device_description.xml"
        self.location
            .strip_prefix("http://")?
            .split(':')
            .next()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_XML: &str = r#"<e:propertyset xmlns:e="urn:schemas-upnp-org:event-1-0"><e:property><ZoneGroupState>&lt;ZoneGroupState&gt;&lt;ZoneGroups&gt;&lt;ZoneGroup Coordinator=&quot;RINCON_5CAAFDAE58BD01400&quot; ID=&quot;RINCON_5CAAFDAE58BD01400:361632566&quot;&gt;&lt;ZoneGroupMember UUID=&quot;RINCON_5CAAFDAE58BD01400&quot; Location=&quot;http://192.168.4.40:1400/xml/device_description.xml&quot; ZoneName=&quot;Basement&quot; Icon=&quot;&quot; Configuration=&quot;1&quot; SoftwareVersion=&quot;85.0-64200&quot; SWGen=&quot;2&quot; MinCompatibleVersion=&quot;84.0-00000&quot; LegacyCompatibleVersion=&quot;58.0-00000&quot; HTSatChanMapSet=&quot;RINCON_5CAAFDAE58BD01400:LF,RF;RINCON_7828CAFB9D9C01400:LR;RINCON_7828CA128F0001400:RR&quot; ActiveZoneID=&quot;289a89bc-23ff-4122-82c5-837f2f288e3b&quot; BootSeq=&quot;24&quot; TVConfigurationError=&quot;0&quot; HdmiCecAvailable=&quot;0&quot; WirelessMode=&quot;1&quot; WirelessLeafOnly=&quot;0&quot; ChannelFreq=&quot;2412&quot; BehindWifiExtender=&quot;0&quot; WifiEnabled=&quot;1&quot; EthLink=&quot;0&quot; Orientation=&quot;0&quot; RoomCalibrationState=&quot;4&quot; SecureRegState=&quot;3&quot; VoiceConfigState=&quot;0&quot; MicEnabled=&quot;0&quot; HeadphoneSwapActive=&quot;0&quot; AirPlayEnabled=&quot;0&quot; IdleState=&quot;1&quot; MoreInfo=&quot;&quot; SSLPort=&quot;1443&quot; HHSSLPort=&quot;1843&quot;&gt;&lt;Satellite UUID=&quot;RINCON_7828CA128F0001400&quot; Location=&quot;http://192.168.4.29:1400/xml/device_description.xml&quot; ZoneName=&quot;Basement&quot; Icon=&quot;&quot; Configuration=&quot;1&quot; Invisible=&quot;1&quot; SoftwareVersion=&quot;85.0-64200&quot; SWGen=&quot;2&quot; MinCompatibleVersion=&quot;84.0-00000&quot; LegacyCompatibleVersion=&quot;58.0-00000&quot; HTSatChanMapSet=&quot;RINCON_5CAAFDAE58BD01400:LF,RF;RINCON_7828CA128F0001400:RR&quot; ActiveZoneID=&quot;289a89bc-23ff-4122-82c5-837f2f288e3b&quot; BootSeq=&quot;28&quot; TVConfigurationError=&quot;0&quot; HdmiCecAvailable=&quot;0&quot; WirelessMode=&quot;2&quot; WirelessLeafOnly=&quot;0&quot; ChannelFreq=&quot;5825&quot; BehindWifiExtender=&quot;0&quot; WifiEnabled=&quot;1&quot; EthLink=&quot;0&quot; Orientation=&quot;0&quot; RoomCalibrationState=&quot;5&quot; SecureRegState=&quot;3&quot; VoiceConfigState=&quot;0&quot; MicEnabled=&quot;0&quot; HeadphoneSwapActive=&quot;0&quot; AirPlayEnabled=&quot;0&quot; IdleState=&quot;1&quot; MoreInfo=&quot;&quot; SSLPort=&quot;1443&quot; HHSSLPort=&quot;1843&quot;/&gt;&lt;/ZoneGroupMember&gt;&lt;/ZoneGroup&gt;&lt;/ZoneGroups&gt;&lt;VanishedDevices&gt;&lt;/VanishedDevices&gt;&lt;/ZoneGroupState&gt;</ZoneGroupState></e:property></e:propertyset>"#;

    #[test]
    fn test_parse_zone_group_topology_xml() {
        let result = ZoneGroupTopologyParser::from_xml(SAMPLE_XML);
        assert!(result.is_ok(), "Failed to parse: {:?}", result.err());

        let parsed = result.unwrap();
        let zone_groups = parsed.zone_groups().unwrap();

        assert_eq!(zone_groups.len(), 1);
        let group = &zone_groups[0];
        assert_eq!(group.coordinator, "RINCON_5CAAFDAE58BD01400");
        assert_eq!(group.id, "RINCON_5CAAFDAE58BD01400:361632566");
        assert_eq!(group.zone_group_members.len(), 1);

        let member = &group.zone_group_members[0];
        assert_eq!(member.zone_name, "Basement");
        assert_eq!(member.uuid, "RINCON_5CAAFDAE58BD01400");
        assert_eq!(member.satellites.len(), 1);
        assert_eq!(member.satellites[0].uuid, "RINCON_7828CA128F0001400");
    }

    #[test]
    fn test_helper_methods() {
        let parsed = ZoneGroupTopologyParser::from_xml(SAMPLE_XML).unwrap();

        // Test total speaker count (1 main + 1 satellite)
        assert_eq!(parsed.total_speaker_count(), 2);

        // Test coordinators
        let coordinators = parsed.coordinators();
        assert_eq!(coordinators, vec!["RINCON_5CAAFDAE58BD01400"]);

        // Test find by coordinator
        let group = parsed.find_group_by_coordinator("RINCON_5CAAFDAE58BD01400");
        assert!(group.is_some());

        // Test find group containing speaker
        let group = parsed.find_group_containing_speaker("RINCON_7828CA128F0001400");
        assert!(group.is_some());
    }

    #[test]
    fn test_zone_group_methods() {
        let parsed = ZoneGroupTopologyParser::from_xml(SAMPLE_XML).unwrap();
        let group = &parsed.zone_groups().unwrap()[0];

        assert!(!group.is_multi_speaker_group()); // Only has 1 member

        let coordinator_member = group.coordinator_member();
        assert!(coordinator_member.is_some());
        assert_eq!(coordinator_member.unwrap().uuid, "RINCON_5CAAFDAE58BD01400");

        let all_uuids = group.all_speaker_uuids();
        assert_eq!(all_uuids.len(), 2); // Main speaker + satellite
        assert!(all_uuids.contains(&"RINCON_5CAAFDAE58BD01400"));
        assert!(all_uuids.contains(&"RINCON_7828CA128F0001400"));
    }

    #[test]
    fn test_zone_member_methods() {
        let parsed = ZoneGroupTopologyParser::from_xml(SAMPLE_XML).unwrap();
        let member = &parsed.zone_groups().unwrap()[0].zone_group_members[0];

        assert!(member.has_satellites());
        assert!(member.is_wireless()); // WirelessMode=1, WifiEnabled=1
        assert_eq!(member.ip_address(), Some("192.168.4.40"));
    }
}