//! ZoneGroupTopology service event types and parsing
//!
//! Provides direct serde-based XML parsing with no business logic,
//! replicating exactly what Sonos produces for sonos-stream consumption.

use serde::{Deserialize, Serialize};
use std::net::IpAddr;

use crate::{Result, Service, ApiError};
use crate::events::{EnrichedEvent, EventSource, EventParser, xml_utils};

/// Minimal ZoneGroupTopology event - direct serde mapping from UPnP event XML
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename = "propertyset")]
pub struct ZoneGroupTopologyEvent {
    /// Multiple property elements can exist in a single event
    #[serde(rename = "property", default)]
    properties: Vec<ZoneGroupTopologyProperty>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ZoneGroupTopologyProperty {
    #[serde(rename = "ZoneGroupState", default, deserialize_with = "xml_utils::deserialize_zone_group_state")]
    zone_group_state: Option<ZoneGroupState>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ZoneGroupState {
    #[serde(rename = "ZoneGroups")]
    zone_groups: ZoneGroups,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ZoneGroups {
    #[serde(rename = "ZoneGroup", default)]
    zone_groups: Vec<ZoneGroup>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ZoneGroup {
    #[serde(rename = "@Coordinator")]
    coordinator: String,

    #[serde(rename = "@ID")]
    id: String,

    #[serde(rename = "ZoneGroupMember", default)]
    members: Vec<ZoneGroupMember>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ZoneGroupMember {
    #[serde(rename = "@UUID")]
    uuid: String,

    #[serde(rename = "@Location")]
    location: String,

    #[serde(rename = "@ZoneName")]
    zone_name: String,

    #[serde(rename = "@SoftwareVersion", default)]
    software_version: Option<String>,

    #[serde(rename = "@WirelessMode", default)]
    wireless_mode: Option<String>,

    #[serde(rename = "@WifiEnabled", default)]
    wifi_enabled: Option<String>,

    #[serde(rename = "@EthLink", default)]
    eth_link: Option<String>,

    #[serde(rename = "@ChannelFreq", default)]
    channel_freq: Option<String>,

    #[serde(rename = "@BehindWifiExtender", default)]
    behind_wifi_extender: Option<String>,

    #[serde(rename = "@HTSatChanMapSet", default)]
    ht_sat_chan_map_set: Option<String>,

    #[serde(rename = "@Icon", default)]
    icon: Option<String>,

    #[serde(rename = "@Invisible", default)]
    invisible: Option<String>,

    #[serde(rename = "@IsZoneBridge", default)]
    is_zone_bridge: Option<String>,

    #[serde(rename = "@BootSeq", default)]
    boot_seq: Option<String>,

    #[serde(rename = "@TVConfigurationError", default)]
    tv_configuration_error: Option<String>,

    #[serde(rename = "@HdmiCecAvailable", default)]
    hdmi_cec_available: Option<String>,

    #[serde(rename = "@HasConfiguredSSID", default)]
    has_configured_ssid: Option<String>,

    #[serde(rename = "@MicEnabled", default)]
    mic_enabled: Option<String>,

    #[serde(rename = "@AirPlayEnabled", default)]
    airplay_enabled: Option<String>,

    #[serde(rename = "@IdleState", default)]
    idle_state: Option<String>,

    #[serde(rename = "@MoreInfo", default)]
    more_info: Option<String>,

    /// Nested satellite speakers (for home theater setups with sub/surrounds)
    #[serde(rename = "Satellite", default)]
    satellites: Vec<Satellite>,
}

/// A satellite speaker in a home theater setup (subwoofer, surround speakers)
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Satellite {
    #[serde(rename = "@UUID")]
    uuid: String,

    #[serde(rename = "@Location", default)]
    location: Option<String>,

    #[serde(rename = "@ZoneName", default)]
    zone_name: Option<String>,

    #[serde(rename = "@HTSatChanMapSet", default)]
    ht_sat_chan_map_set: Option<String>,

    #[serde(rename = "@Invisible", default)]
    invisible: Option<String>,
}

/// Information about a single zone group (public interface for sonos-stream)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZoneGroupInfo {
    pub coordinator: String,
    pub id: String,
    pub members: Vec<ZoneGroupMemberInfo>,
}

/// Information about a speaker in a zone group (public interface for sonos-stream)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZoneGroupMemberInfo {
    pub uuid: String,
    pub location: String,
    pub zone_name: String,
    pub software_version: String,
    pub network_info: NetworkInfo,
    pub satellites: Vec<SatelliteInfo>,
}

/// Network configuration information for a speaker
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkInfo {
    pub wireless_mode: String,
    pub wifi_enabled: String,
    pub eth_link: String,
    pub channel_freq: String,
    pub behind_wifi_extender: String,
}

impl Default for NetworkInfo {
    fn default() -> Self {
        Self {
            wireless_mode: "0".to_string(),
            wifi_enabled: "0".to_string(),
            eth_link: "0".to_string(),
            channel_freq: "0".to_string(),
            behind_wifi_extender: "0".to_string(),
        }
    }
}

/// Information about a satellite speaker
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SatelliteInfo {
    pub uuid: String,
    pub location: String,
    pub zone_name: String,
    pub ht_sat_chan_map_set: String,
    pub invisible: String,
}

impl ZoneGroupTopologyEvent {
    /// Get zone groups from the topology event
    pub fn zone_groups(&self) -> Vec<ZoneGroupInfo> {
        // Find the first property with zone_group_state
        let zone_group_state = self.properties.iter()
            .find_map(|p| p.zone_group_state.as_ref());

        if let Some(zone_group_state) = zone_group_state {
            zone_group_state.zone_groups.zone_groups.iter().map(|group| {
                ZoneGroupInfo {
                    coordinator: group.coordinator.clone(),
                    id: group.id.clone(),
                    members: group.members.iter().map(|member| {
                        ZoneGroupMemberInfo {
                            uuid: member.uuid.clone(),
                            location: member.location.clone(),
                            zone_name: member.zone_name.clone(),
                            software_version: member.software_version.clone().unwrap_or_default(),
                            network_info: NetworkInfo {
                                wireless_mode: member.wireless_mode.clone().unwrap_or_default(),
                                wifi_enabled: member.wifi_enabled.clone().unwrap_or_default(),
                                eth_link: member.eth_link.clone().unwrap_or_default(),
                                channel_freq: member.channel_freq.clone().unwrap_or_default(),
                                behind_wifi_extender: member.behind_wifi_extender.clone().unwrap_or_default(),
                            },
                            satellites: member.satellites.iter().map(|sat| {
                                SatelliteInfo {
                                    uuid: sat.uuid.clone(),
                                    location: sat.location.clone().unwrap_or_default(),
                                    zone_name: sat.zone_name.clone().unwrap_or_default(),
                                    ht_sat_chan_map_set: sat.ht_sat_chan_map_set.clone().unwrap_or_default(),
                                    invisible: sat.invisible.clone().unwrap_or_default(),
                                }
                            }).collect(),
                        }
                    }).collect(),
                }
            }).collect()
        } else {
            Vec::new()
        }
    }

    /// Get vanished devices from the topology event
    pub fn vanished_devices(&self) -> Vec<String> {
        Vec::new() // Simplified for now
    }

    /// Parse from UPnP event XML using serde
    pub fn from_xml(xml: &str) -> Result<Self> {
        let clean_xml = xml_utils::strip_namespaces(xml);
        quick_xml::de::from_str(&clean_xml)
            .map_err(|e| ApiError::ParseError(format!("Failed to parse ZoneGroupTopology XML: {}", e)))
    }
}

/// Minimal parser implementation
pub struct ZoneGroupTopologyEventParser;

impl EventParser for ZoneGroupTopologyEventParser {
    type EventData = ZoneGroupTopologyEvent;

    fn parse_upnp_event(&self, xml: &str) -> Result<Self::EventData> {
        ZoneGroupTopologyEvent::from_xml(xml)
    }

    fn service_type(&self) -> Service {
        Service::ZoneGroupTopology
    }
}

/// Create enriched event for sonos-stream integration
pub fn create_enriched_event(
    speaker_ip: IpAddr,
    event_source: EventSource,
    event_data: ZoneGroupTopologyEvent,
) -> EnrichedEvent<ZoneGroupTopologyEvent> {
    EnrichedEvent::new(speaker_ip, Service::ZoneGroupTopology, event_source, event_data)
}

/// Create enriched event with registration ID
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
        };

        let zone_group = ZoneGroupInfo {
            coordinator: "RINCON_123456789".to_string(),
            id: "RINCON_123456789:0".to_string(),
            members: vec![member],
        };

        let event_data = ZoneGroupState {
            zone_groups: ZoneGroups {
                zone_groups: vec![ZoneGroup {
                    coordinator: zone_group.coordinator.clone(),
                    id: zone_group.id.clone(),
                    members: Vec::new(),
                }]
            }
        };

        let event = ZoneGroupTopologyEvent {
            properties: vec![ZoneGroupTopologyProperty {
                zone_group_state: Some(event_data),
            }]
        };

        let zone_groups = event.zone_groups();
        assert_eq!(zone_groups.len(), 1);
        assert_eq!(zone_groups[0].coordinator, "RINCON_123456789");
    }

    #[test]
    fn test_enriched_event_creation() {
        let ip: IpAddr = "192.168.1.100".parse().unwrap();
        let source = EventSource::UPnPNotification {
            subscription_id: "uuid:123".to_string(),
        };
        let event_data = ZoneGroupTopologyEvent {
            properties: vec![ZoneGroupTopologyProperty {
                zone_group_state: None,
            }]
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
            properties: vec![ZoneGroupTopologyProperty {
                zone_group_state: None,
            }]
        };

        let enriched = create_enriched_event_with_registration_id(42, ip, source, event_data);

        assert_eq!(enriched.registration_id, Some(42));
    }
}
#[cfg(test)]
mod xml_parsing_tests {
    use super::*;

    #[test]
    fn test_multi_property_event() {
        // Real Sonos events can have multiple <e:property> elements
        let xml = r#"<e:propertyset xmlns:e="urn:schemas-upnp-org:event-1-0">
<e:property>
<ZoneGroupState>&lt;ZoneGroupState&gt;&lt;ZoneGroups&gt;&lt;ZoneGroup Coordinator="RINCON_5CAAFDAE58BD01400" ID="RINCON_5CAAFDAE58BD01400:0"&gt;&lt;ZoneGroupMember UUID="RINCON_5CAAFDAE58BD01400" Location="http://192.168.1.100:1400/xml/device_description.xml" ZoneName="Living Room"/&gt;&lt;/ZoneGroup&gt;&lt;/ZoneGroups&gt;&lt;/ZoneGroupState&gt;</ZoneGroupState>
</e:property>
<e:property>
<ThirdPartyMediaServersX></ThirdPartyMediaServersX>
</e:property>
</e:propertyset>"#;

        let result = ZoneGroupTopologyEvent::from_xml(xml);
        assert!(result.is_ok(), "Failed to parse multi-property event: {:?}", result);

        let event = result.unwrap();
        let zone_groups = event.zone_groups();
        assert_eq!(zone_groups.len(), 1);
        assert_eq!(zone_groups[0].members[0].zone_name, "Living Room");
    }

    #[test]
    fn test_empty_zone_group_state() {
        let xml = r#"<e:propertyset xmlns:e="urn:schemas-upnp-org:event-1-0">
<e:property>
<ZoneGroupState></ZoneGroupState>
</e:property>
</e:propertyset>"#;

        let result = ZoneGroupTopologyEvent::from_xml(xml);
        assert!(result.is_ok(), "Failed with empty ZoneGroupState: {:?}", result);

        let event = result.unwrap();
        assert!(event.zone_groups().is_empty());
    }

    #[test]
    fn test_non_zone_group_state_property() {
        let xml = r#"<e:propertyset xmlns:e="urn:schemas-upnp-org:event-1-0">
<e:property>
<ThirdPartyMediaServersX></ThirdPartyMediaServersX>
</e:property>
</e:propertyset>"#;

        let result = ZoneGroupTopologyEvent::from_xml(xml);
        assert!(result.is_ok());

        let event = result.unwrap();
        assert!(event.zone_groups().is_empty());
    }

    #[test]
    fn test_home_theater_with_satellites() {
        // Test with nested Satellite elements inside ZoneGroupMember (common in Sonos home theater setups)
        let xml = r#"<e:propertyset xmlns:e="urn:schemas-upnp-org:event-1-0">
<e:property>
<ZoneGroupState>&lt;ZoneGroupState&gt;&lt;ZoneGroups&gt;&lt;ZoneGroup Coordinator=&quot;RINCON_123&quot; ID=&quot;RINCON_123:0&quot;&gt;&lt;ZoneGroupMember UUID=&quot;RINCON_123&quot; Location=&quot;http://192.168.1.100:1400/xml/device_description.xml&quot; ZoneName=&quot;Living Room&quot;&gt;&lt;Satellite UUID=&quot;RINCON_456&quot; Location=&quot;http://192.168.1.101:1400/xml/device_description.xml&quot; ZoneName=&quot;Sub&quot;/&gt;&lt;/ZoneGroupMember&gt;&lt;/ZoneGroup&gt;&lt;/ZoneGroups&gt;&lt;/ZoneGroupState&gt;</ZoneGroupState>
</e:property>
</e:propertyset>"#;

        let result = ZoneGroupTopologyEvent::from_xml(xml);
        assert!(result.is_ok(), "Failed with satellites: {:?}", result);

        let event = result.unwrap();
        let zone_groups = event.zone_groups();
        assert_eq!(zone_groups.len(), 1);
        assert_eq!(zone_groups[0].members.len(), 1);
        assert_eq!(zone_groups[0].members[0].satellites.len(), 1);
        assert_eq!(zone_groups[0].members[0].satellites[0].uuid, "RINCON_456");
    }
}
