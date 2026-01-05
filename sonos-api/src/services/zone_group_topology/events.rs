//! ZoneGroupTopology service event types and parsing
//!
//! Provides direct serde-based XML parsing with no business logic,
//! replicating exactly what Sonos produces for sonos-stream consumption.

use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use std::collections::HashMap;

use crate::{Result, Service, ApiError};
use crate::events::{EnrichedEvent, EventSource, EventParser, xml_utils};

/// Minimal ZoneGroupTopology event - direct serde mapping from UPnP event XML
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename = "propertyset")]
pub struct ZoneGroupTopologyEvent {
    #[serde(rename = "property")]
    property: ZoneGroupTopologyProperty,
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

    #[serde(flatten)]
    other_attributes: HashMap<String, String>,
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
    pub metadata: HashMap<String, String>,
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
        if let Some(zone_group_state) = &self.property.zone_group_state {
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
                                wireless_mode: member.other_attributes.get("WirelessMode").cloned().unwrap_or_default(),
                                wifi_enabled: member.other_attributes.get("WifiEnabled").cloned().unwrap_or_default(),
                                eth_link: member.other_attributes.get("EthLink").cloned().unwrap_or_default(),
                                channel_freq: member.other_attributes.get("ChannelFreq").cloned().unwrap_or_default(),
                                behind_wifi_extender: member.other_attributes.get("BehindWifiExtender").cloned().unwrap_or_default(),
                            },
                            satellites: Vec::new(), // Simplified for now
                            metadata: member.other_attributes.clone(),
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
            metadata: HashMap::new(),
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
            property: ZoneGroupTopologyProperty {
                zone_group_state: Some(event_data),
            }
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
            property: ZoneGroupTopologyProperty {
                zone_group_state: None,
            }
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
            property: ZoneGroupTopologyProperty {
                zone_group_state: None,
            }
        };

        let enriched = create_enriched_event_with_registration_id(42, ip, source, event_data);

        assert_eq!(enriched.registration_id, Some(42));
    }
}