//! ZoneGroupTopology service event types and parsing
//!
//! This module handles events from the ZoneGroupTopology UPnP service, which manages
//! the topology of zone groups, speaker groupings, and network configuration.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::IpAddr;

use crate::{Result, Service, ApiError};
use crate::events::{EnrichedEvent, EventSource, EventParser, xml_utils};

/// Event data for ZoneGroupTopology service containing complete topology information.
/// This passes through the entire parsed topology state without any delta processing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZoneGroupTopologyEvent {
    /// Complete zone group topology data
    pub zone_groups: Vec<ZoneGroupInfo>,

    /// Devices that have vanished from the network
    pub vanished_devices: Vec<String>, // Can be expanded later if needed
}

/// Information about a single zone group.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZoneGroupInfo {
    /// The coordinator (master) speaker UUID for this group
    pub coordinator: String,

    /// Unique identifier for this zone group
    pub id: String,

    /// All speakers that are members of this zone group
    pub members: Vec<ZoneGroupMemberInfo>,
}

/// Information about a speaker in a zone group.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZoneGroupMemberInfo {
    /// Unique identifier for this speaker (RINCON_...)
    pub uuid: String,

    /// Network location URL of the speaker
    pub location: String,

    /// Human-readable name of the room/zone
    pub zone_name: String,

    /// Software version running on the speaker
    pub software_version: String,

    /// Network configuration (WiFi, ethernet, etc.)
    pub network_info: NetworkInfo,

    /// Satellite speakers for home theater configurations
    pub satellites: Vec<SatelliteInfo>,

    /// Additional metadata (can be extended as needed)
    pub metadata: HashMap<String, String>,
}

/// Network configuration information for a speaker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkInfo {
    /// Wireless mode (0=wired, 1=2.4GHz, 2=5GHz)
    pub wireless_mode: String,

    /// Whether WiFi is enabled
    pub wifi_enabled: String,

    /// Ethernet link status
    pub eth_link: String,

    /// WiFi channel frequency
    pub channel_freq: String,

    /// Whether behind a WiFi extender
    pub behind_wifi_extender: String,
}

/// Information about a satellite speaker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SatelliteInfo {
    /// Unique identifier for this satellite speaker
    pub uuid: String,

    /// Network location of the satellite
    pub location: String,

    /// Zone name (usually same as main speaker)
    pub zone_name: String,

    /// Home theater satellite channel mapping
    pub ht_sat_chan_map_set: String,

    /// Whether this satellite is invisible in UI
    pub invisible: String,
}

// Serde parsing structures for ZoneGroupTopology UPnP events (moved from sonos-parser)

/// Root parser structure for ZoneGroupTopology UPnP events.
///
/// UPnP events are wrapped in a propertyset structure:
/// ```xml
/// <e:propertyset xmlns:e="urn:schemas-upnp-org:event-1-0">
///   <e:property>
///     <ZoneGroupState>...</ZoneGroupState>
///   </e:property>
/// </e:propertyset>
/// ```
#[derive(Debug, Clone, Deserialize)]
#[serde(rename = "propertyset")]
struct ZoneGroupTopologyPropertySet {
    /// The property elements (ZoneGroupState is one of them)
    #[serde(rename = "property")]
    properties: Vec<ZoneGroupTopologyProperty>,
}

/// Property wrapper that can contain ZoneGroupState.
#[derive(Debug, Clone, Deserialize)]
struct ZoneGroupTopologyProperty {
    /// The ZoneGroupState element with nested XML content
    #[serde(rename = "ZoneGroupState", deserialize_with = "xml_utils::deserialize_nested", default)]
    zone_group_state: Option<ParsedZoneGroupState>,
}

/// The root element for decoded ZoneGroupState content.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename = "ZoneGroupState")]
struct ParsedZoneGroupState {
    /// All current zone groups in the system
    #[serde(rename = "ZoneGroups")]
    zone_groups: ParsedZoneGroups,

    /// Devices that have vanished from the network
    #[serde(rename = "VanishedDevices", default)]
    vanished_devices: Option<ParsedVanishedDevices>,
}

/// Container for all zone groups in the system.
#[derive(Debug, Clone, Deserialize)]
struct ParsedZoneGroups {
    /// List of all zone groups
    #[serde(rename = "ZoneGroup", default)]
    zone_groups: Vec<ParsedZoneGroup>,
}

/// A single zone group representing speakers playing together.
#[derive(Debug, Clone, Deserialize)]
struct ParsedZoneGroup {
    /// The coordinator (master) speaker for this group
    #[serde(rename = "@Coordinator")]
    coordinator: String,

    /// Unique identifier for this zone group
    #[serde(rename = "@ID")]
    id: String,

    /// All speakers that are members of this zone group
    #[serde(rename = "ZoneGroupMember", default)]
    zone_group_members: Vec<ParsedZoneGroupMember>,
}

/// A speaker that is part of a zone group.
#[derive(Debug, Clone, Deserialize)]
struct ParsedZoneGroupMember {
    /// Unique identifier for this speaker (RINCON_...)
    #[serde(rename = "@UUID")]
    uuid: String,

    /// Network location of the speaker
    #[serde(rename = "@Location")]
    location: String,

    /// Human-readable name of the room/zone
    #[serde(rename = "@ZoneName")]
    zone_name: String,

    /// Icon identifier for the speaker
    #[serde(rename = "@Icon", default)]
    icon: String,

    /// Configuration flags
    #[serde(rename = "@Configuration", default)]
    configuration: String,

    /// Software version running on the speaker
    #[serde(rename = "@SoftwareVersion")]
    software_version: String,

    /// Software generation
    #[serde(rename = "@SWGen", default)]
    sw_gen: String,

    /// Minimum compatible software version
    #[serde(rename = "@MinCompatibleVersion", default)]
    min_compatible_version: String,

    /// Legacy compatible software version
    #[serde(rename = "@LegacyCompatibleVersion", default)]
    legacy_compatible_version: String,

    /// Boot sequence number
    #[serde(rename = "@BootSeq", default)]
    boot_seq: String,

    /// Wireless mode (0=wired, 1=2.4GHz, 2=5GHz)
    #[serde(rename = "@WirelessMode", default)]
    wireless_mode: String,

    /// WiFi channel frequency
    #[serde(rename = "@ChannelFreq", default)]
    channel_freq: String,

    /// Whether behind a WiFi extender
    #[serde(rename = "@BehindWifiExtender", default)]
    behind_wifi_extender: String,

    /// Whether WiFi is enabled
    #[serde(rename = "@WifiEnabled", default)]
    wifi_enabled: String,

    /// Ethernet link status
    #[serde(rename = "@EthLink", default)]
    eth_link: String,

    /// Speaker orientation
    #[serde(rename = "@Orientation", default)]
    orientation: String,

    /// Room calibration state
    #[serde(rename = "@RoomCalibrationState", default)]
    room_calibration_state: String,

    /// Secure registration state
    #[serde(rename = "@SecureRegState", default)]
    secure_reg_state: String,

    /// Voice configuration state
    #[serde(rename = "@VoiceConfigState", default)]
    voice_config_state: String,

    /// Whether microphone is enabled
    #[serde(rename = "@MicEnabled", default)]
    mic_enabled: String,

    /// Whether AirPlay is enabled
    #[serde(rename = "@AirPlayEnabled", default)]
    airplay_enabled: String,

    /// Virtual line-in source (optional)
    #[serde(rename = "@VirtualLineInSource", default)]
    virtual_line_in_source: Option<String>,

    /// Idle state
    #[serde(rename = "@IdleState", default)]
    idle_state: String,

    /// Additional information
    #[serde(rename = "@MoreInfo", default)]
    more_info: String,

    /// SSL port
    #[serde(rename = "@SSLPort", default)]
    ssl_port: String,

    /// Household SSL port
    #[serde(rename = "@HHSSLPort", default)]
    hhssl_port: String,

    /// Home theater satellite channel mapping (optional)
    #[serde(rename = "@HTSatChanMapSet", default)]
    ht_sat_chan_map_set: Option<String>,

    /// Active zone ID (optional)
    #[serde(rename = "@ActiveZoneID", default)]
    active_zone_id: Option<String>,

    /// Satellite speakers (for home theater configurations)
    #[serde(rename = "Satellite", default)]
    satellites: Vec<ParsedSatellite>,
}

/// A satellite speaker (part of a home theater setup).
#[derive(Debug, Clone, Deserialize)]
struct ParsedSatellite {
    /// Unique identifier for this satellite speaker
    #[serde(rename = "@UUID")]
    uuid: String,

    /// Network location of the satellite
    #[serde(rename = "@Location")]
    location: String,

    /// Zone name (usually same as main speaker)
    #[serde(rename = "@ZoneName")]
    zone_name: String,

    /// Whether this satellite is invisible in UI
    #[serde(rename = "@Invisible", default)]
    invisible: String,

    /// Home theater satellite channel mapping
    #[serde(rename = "@HTSatChanMapSet", default)]
    ht_sat_chan_map_set: String,
}

/// Container for vanished devices.
#[derive(Debug, Clone, Deserialize)]
struct ParsedVanishedDevices {
    /// List of vanished device UUIDs
    #[serde(rename = "Device", default)]
    devices: Vec<String>,
}

impl ZoneGroupTopologyEvent {
    /// Parse ZoneGroupTopology event from XML using self-parsing.
    ///
    /// This method uses serde-based parsing but implements it directly
    /// in the event type, removing the need for separate parser types.
    ///
    /// # Arguments
    ///
    /// * `xml` - The raw UPnP event XML
    ///
    /// # Returns
    ///
    /// The parsed ZoneGroupTopologyEvent, or an error if parsing fails.
    pub fn from_xml(xml: &str) -> Result<Self> {
        // Try primary serde-based parsing
        match Self::parse_with_serde(xml) {
            Ok(event) => Ok(event),
            Err(_) => {
                // Fallback to empty topology if parsing fails
                // Zone topology events are too complex for meaningful fallback parsing
                Ok(ZoneGroupTopologyEvent {
                    zone_groups: Vec::new(),
                    vanished_devices: Vec::new(),
                })
            }
        }
    }

    /// Primary serde-based parsing implementation.
    fn parse_with_serde(xml: &str) -> Result<Self> {
        let clean_xml = xml_utils::strip_namespaces(xml);
        let property_set: ZoneGroupTopologyPropertySet = quick_xml::de::from_str(&clean_xml)
            .map_err(|e| ApiError::ParseError(format!("Failed to parse ZoneGroupTopology XML: {}", e)))?;

        // Find the ZoneGroupState property
        let zone_group_state = property_set.properties
            .iter()
            .find_map(|prop| prop.zone_group_state.as_ref())
            .ok_or_else(|| ApiError::ParseError("No ZoneGroupState found in event".to_string()))?;

        // Convert from parsed structures to event types
        let zone_groups = zone_group_state.zone_groups.zone_groups
            .iter()
            .map(|group| {
                let members = group.zone_group_members
                    .iter()
                    .map(|member| Self::convert_member(member))
                    .collect();

                ZoneGroupInfo {
                    coordinator: group.coordinator.clone(),
                    id: group.id.clone(),
                    members,
                }
            })
            .collect();

        let vanished_devices = zone_group_state.vanished_devices
            .as_ref()
            .map(|vd| vd.devices.clone())
            .unwrap_or_default();

        Ok(ZoneGroupTopologyEvent {
            zone_groups,
            vanished_devices,
        })
    }

    /// Convert parsed zone group member to event structure.
    fn convert_member(member: &ParsedZoneGroupMember) -> ZoneGroupMemberInfo {
        // Collect all additional metadata
        let mut metadata = HashMap::new();
        metadata.insert("icon".to_string(), member.icon.clone());
        metadata.insert("configuration".to_string(), member.configuration.clone());
        metadata.insert("sw_gen".to_string(), member.sw_gen.clone());
        metadata.insert("min_compatible_version".to_string(), member.min_compatible_version.clone());
        metadata.insert("legacy_compatible_version".to_string(), member.legacy_compatible_version.clone());
        metadata.insert("boot_seq".to_string(), member.boot_seq.clone());
        metadata.insert("ssl_port".to_string(), member.ssl_port.clone());
        metadata.insert("hhssl_port".to_string(), member.hhssl_port.clone());
        metadata.insert("idle_state".to_string(), member.idle_state.clone());
        metadata.insert("more_info".to_string(), member.more_info.clone());
        metadata.insert("orientation".to_string(), member.orientation.clone());
        metadata.insert("room_calibration_state".to_string(), member.room_calibration_state.clone());
        metadata.insert("secure_reg_state".to_string(), member.secure_reg_state.clone());
        metadata.insert("voice_config_state".to_string(), member.voice_config_state.clone());
        metadata.insert("mic_enabled".to_string(), member.mic_enabled.clone());
        metadata.insert("airplay_enabled".to_string(), member.airplay_enabled.clone());

        if let Some(ref ht_sat_chan) = member.ht_sat_chan_map_set {
            metadata.insert("ht_sat_chan_map_set".to_string(), ht_sat_chan.clone());
        }
        if let Some(ref active_zone) = member.active_zone_id {
            metadata.insert("active_zone_id".to_string(), active_zone.clone());
        }
        if let Some(ref vli_source) = member.virtual_line_in_source {
            metadata.insert("virtual_line_in_source".to_string(), vli_source.clone());
        }

        // Convert satellites
        let satellites = member.satellites
            .iter()
            .map(|sat| SatelliteInfo {
                uuid: sat.uuid.clone(),
                location: sat.location.clone(),
                zone_name: sat.zone_name.clone(),
                ht_sat_chan_map_set: sat.ht_sat_chan_map_set.clone(),
                invisible: sat.invisible.clone(),
            })
            .collect();

        ZoneGroupMemberInfo {
            uuid: member.uuid.clone(),
            location: member.location.clone(),
            zone_name: member.zone_name.clone(),
            software_version: member.software_version.clone(),
            network_info: NetworkInfo {
                wireless_mode: member.wireless_mode.clone(),
                wifi_enabled: member.wifi_enabled.clone(),
                eth_link: member.eth_link.clone(),
                channel_freq: member.channel_freq.clone(),
                behind_wifi_extender: member.behind_wifi_extender.clone(),
            },
            satellites,
            metadata,
        }
    }
}

/// Event parser for ZoneGroupTopology service
pub struct ZoneGroupTopologyEventParser;

impl EventParser for ZoneGroupTopologyEventParser {
    type EventData = ZoneGroupTopologyEvent;

    fn parse_upnp_event(&self, xml: &str) -> Result<Self::EventData> {
        // Use self-parsing method
        ZoneGroupTopologyEvent::from_xml(xml)
    }

    fn service_type(&self) -> Service {
        Service::ZoneGroupTopology
    }
}

/// Create an enriched ZoneGroupTopology event
pub fn create_enriched_event(
    speaker_ip: IpAddr,
    event_source: EventSource,
    event_data: ZoneGroupTopologyEvent,
) -> EnrichedEvent<ZoneGroupTopologyEvent> {
    EnrichedEvent::new(speaker_ip, Service::ZoneGroupTopology, event_source, event_data)
}

/// Create an enriched ZoneGroupTopology event with registration ID (for sonos-stream integration)
pub fn create_enriched_event_with_registration_id(
    registration_id: u64,
    speaker_ip: IpAddr,
    event_source: EventSource,
    event_data: ZoneGroupTopologyEvent,
) -> EnrichedEvent<ZoneGroupTopologyEvent> {
    EnrichedEvent::with_registration_id(
        registration_id,
        speaker_ip,
        Service::ZoneGroupTopology,
        event_source,
        event_data,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zone_group_topology_parser_service_type() {
        let parser = ZoneGroupTopologyEventParser;
        assert_eq!(parser.service_type(), Service::ZoneGroupTopology);
    }

    #[test]
    fn test_zone_group_topology_event_creation() {
        let member = ZoneGroupMemberInfo {
            uuid: "RINCON_123456789".to_string(),
            location: "http://192.168.1.100:1400/xml/device_description.xml".to_string(),
            zone_name: "Living Room".to_string(),
            software_version: "56.0-76060".to_string(),
            network_info: NetworkInfo {
                wireless_mode: "0".to_string(),
                wifi_enabled: "1".to_string(),
                eth_link: "1".to_string(),
                channel_freq: "2412".to_string(),
                behind_wifi_extender: "0".to_string(),
            },
            satellites: Vec::new(),
            metadata: HashMap::new(),
        };

        let zone_group = ZoneGroupInfo {
            coordinator: "RINCON_123456789".to_string(),
            id: "RINCON_123456789:0".to_string(),
            members: vec![member],
        };

        let event = ZoneGroupTopologyEvent {
            zone_groups: vec![zone_group],
            vanished_devices: Vec::new(),
        };

        assert_eq!(event.zone_groups.len(), 1);
        assert_eq!(event.zone_groups[0].coordinator, "RINCON_123456789");
        assert_eq!(event.zone_groups[0].members.len(), 1);
    }

    #[test]
    fn test_enriched_event_creation() {
        let ip: IpAddr = "192.168.1.100".parse().unwrap();
        let source = EventSource::UPnPNotification {
            subscription_id: "uuid:123".to_string(),
        };
        let event_data = ZoneGroupTopologyEvent {
            zone_groups: Vec::new(),
            vanished_devices: Vec::new(),
        };

        let enriched = create_enriched_event(ip, source, event_data);

        assert_eq!(enriched.speaker_ip, ip);
        assert_eq!(enriched.service, Service::ZoneGroupTopology);
        assert!(enriched.registration_id.is_none());
    }

    #[test]
    fn test_enriched_event_with_registration_id() {
        let ip: IpAddr = "192.168.1.100".parse().unwrap();
        let source = EventSource::UPnPNotification {
            subscription_id: "uuid:123".to_string(),
        };
        let event_data = ZoneGroupTopologyEvent {
            zone_groups: Vec::new(),
            vanished_devices: Vec::new(),
        };

        let enriched = create_enriched_event_with_registration_id(42, ip, source, event_data);

        assert_eq!(enriched.registration_id, Some(42));
    }

    #[test]
    fn test_fallback_parsing() {
        let parser = ZoneGroupTopologyEventParser;

        // Test with invalid XML that would cause sonos-parser to fail
        let xml = "<InvalidXML></InvalidXML>";

        let result = parser.parse_upnp_event(xml);
        assert!(result.is_ok());

        let event = result.unwrap();
        assert_eq!(event.zone_groups.len(), 0);
        assert_eq!(event.vanished_devices.len(), 0);
    }

    #[test]
    fn test_self_parsing_basic() {
        // Test basic ZoneGroupTopology XML parsing with the new from_xml method
        let xml = r#"<e:propertyset xmlns:e="urn:schemas-upnp-org:event-1-0">
            <e:property>
                <ZoneGroupState>&lt;ZoneGroupState&gt;
                    &lt;ZoneGroups&gt;
                        &lt;ZoneGroup Coordinator="RINCON_123456789" ID="RINCON_123456789:0"&gt;
                            &lt;ZoneGroupMember UUID="RINCON_123456789" Location="http://192.168.1.100:1400/xml/device_description.xml" ZoneName="Living Room" SoftwareVersion="56.0-76060" WirelessMode="0" WifiEnabled="1" EthLink="1" ChannelFreq="2412" BehindWifiExtender="0" Orientation="0" RoomCalibrationState="4" SecureRegState="3" VoiceConfigState="0" MicEnabled="1" AirPlayEnabled="1" IdleState="1" MoreInfo="1" SSLPort="1443" HHSSLPort="1843"/&gt;
                        &lt;/ZoneGroup&gt;
                    &lt;/ZoneGroups&gt;
                    &lt;VanishedDevices/&gt;
                &lt;/ZoneGroupState&gt;</ZoneGroupState>
            </e:property>
        </e:propertyset>"#;

        let result = ZoneGroupTopologyEvent::from_xml(xml);
        assert!(result.is_ok(), "Failed to parse ZoneGroupTopology XML: {:?}", result.err());

        let event = result.unwrap();
        assert_eq!(event.zone_groups.len(), 1);
        assert_eq!(event.zone_groups[0].coordinator, "RINCON_123456789");
        assert_eq!(event.zone_groups[0].id, "RINCON_123456789:0");
        assert_eq!(event.zone_groups[0].members.len(), 1);
        assert_eq!(event.zone_groups[0].members[0].uuid, "RINCON_123456789");
        assert_eq!(event.zone_groups[0].members[0].zone_name, "Living Room");
        assert_eq!(event.zone_groups[0].members[0].software_version, "56.0-76060");
    }

    #[test]
    fn test_self_parsing_fallback() {
        // Test fallback parsing with malformed XML
        let xml = "<InvalidXML></InvalidXML>";

        let result = ZoneGroupTopologyEvent::from_xml(xml);
        assert!(result.is_ok());

        let event = result.unwrap();
        assert_eq!(event.zone_groups.len(), 0);
        assert_eq!(event.vanished_devices.len(), 0);
    }
}