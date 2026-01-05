//! RenderingControl service event types and parsing
//!
//! Provides direct serde-based XML parsing with no business logic,
//! replicating exactly what Sonos produces for sonos-stream consumption.

use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use std::collections::HashMap;

use crate::{Result, Service, ApiError};
use crate::events::{EnrichedEvent, EventSource, EventParser, xml_utils};

/// Minimal RenderingControl event - direct serde mapping from UPnP event XML
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename = "propertyset")]
pub struct RenderingControlEvent {
    #[serde(rename = "property")]
    property: RenderingControlProperty,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RenderingControlProperty {
    #[serde(rename = "LastChange", deserialize_with = "xml_utils::deserialize_nested")]
    last_change: RenderingControlEventData,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename = "Event")]
pub struct RenderingControlEventData {
    #[serde(rename = "InstanceID")]
    instance: RenderingControlInstance,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RenderingControlInstance {
    #[serde(rename = "Volume", default)]
    pub volumes: Vec<ChannelValueAttribute>,

    #[serde(rename = "Mute", default)]
    pub mutes: Vec<ChannelValueAttribute>,

    #[serde(rename = "Bass", default)]
    pub bass: Option<xml_utils::ValueAttribute>,

    #[serde(rename = "Treble", default)]
    pub treble: Option<xml_utils::ValueAttribute>,

    #[serde(rename = "Loudness", default)]
    pub loudness: Option<xml_utils::ValueAttribute>,

    #[serde(rename = "Balance", default)]
    pub balance: Option<xml_utils::ValueAttribute>,
}

/// Represents an XML element with both val and channel attributes
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ChannelValueAttribute {
    #[serde(rename = "@val", default)]
    pub val: String,

    #[serde(rename = "@channel", default)]
    pub channel: String,
}

impl RenderingControlEvent {
    /// Get master volume
    pub fn master_volume(&self) -> Option<String> {
        self.get_volume_for_channel("Master")
    }

    /// Get left front volume
    pub fn lf_volume(&self) -> Option<String> {
        self.get_volume_for_channel("LF")
    }

    /// Get right front volume
    pub fn rf_volume(&self) -> Option<String> {
        self.get_volume_for_channel("RF")
    }

    /// Get master mute
    pub fn master_mute(&self) -> Option<String> {
        self.get_mute_for_channel("Master")
    }

    /// Get left front mute
    pub fn lf_mute(&self) -> Option<String> {
        self.get_mute_for_channel("LF")
    }

    /// Get right front mute
    pub fn rf_mute(&self) -> Option<String> {
        self.get_mute_for_channel("RF")
    }

    /// Get bass
    pub fn bass(&self) -> Option<String> {
        self.property.last_change.instance.bass.as_ref().map(|v| v.val.clone())
    }

    /// Get treble
    pub fn treble(&self) -> Option<String> {
        self.property.last_change.instance.treble.as_ref().map(|v| v.val.clone())
    }

    /// Get loudness
    pub fn loudness(&self) -> Option<String> {
        self.property.last_change.instance.loudness.as_ref().map(|v| v.val.clone())
    }

    /// Get balance
    pub fn balance(&self) -> Option<String> {
        self.property.last_change.instance.balance.as_ref().map(|v| v.val.clone())
    }

    /// Get other channels as a map of all non-standard channels
    pub fn other_channels(&self) -> HashMap<String, String> {
        let mut channels = HashMap::new();

        // Add all volume channels that aren't Master, LF, or RF
        for volume in &self.property.last_change.instance.volumes {
            if !["Master", "LF", "RF"].contains(&volume.channel.as_str()) {
                channels.insert(format!("{}Volume", volume.channel), volume.val.clone());
            }
        }

        // Add all mute channels that aren't Master, LF, or RF
        for mute in &self.property.last_change.instance.mutes {
            if !["Master", "LF", "RF"].contains(&mute.channel.as_str()) {
                channels.insert(format!("{}Mute", mute.channel), mute.val.clone());
            }
        }

        channels
    }

    /// Helper method to get volume for a specific channel
    fn get_volume_for_channel(&self, channel: &str) -> Option<String> {
        self.property.last_change.instance.volumes
            .iter()
            .find(|v| v.channel == channel)
            .map(|v| v.val.clone())
    }

    /// Helper method to get mute for a specific channel
    fn get_mute_for_channel(&self, channel: &str) -> Option<String> {
        self.property.last_change.instance.mutes
            .iter()
            .find(|m| m.channel == channel)
            .map(|m| m.val.clone())
    }

    /// Parse from UPnP event XML using serde
    pub fn from_xml(xml: &str) -> Result<Self> {
        let clean_xml = xml_utils::strip_namespaces(xml);
        quick_xml::de::from_str(&clean_xml)
            .map_err(|e| ApiError::ParseError(format!("Failed to parse RenderingControl XML: {}", e)))
    }
}

/// Minimal parser implementation
pub struct RenderingControlEventParser;

impl EventParser for RenderingControlEventParser {
    type EventData = RenderingControlEvent;

    fn parse_upnp_event(&self, xml: &str) -> Result<Self::EventData> {
        RenderingControlEvent::from_xml(xml)
    }

    fn service_type(&self) -> Service {
        Service::RenderingControl
    }
}

/// Create enriched event for sonos-stream integration
pub fn create_enriched_event(
    speaker_ip: IpAddr,
    event_source: EventSource,
    event_data: RenderingControlEvent,
) -> EnrichedEvent<RenderingControlEvent> {
    EnrichedEvent::new(speaker_ip, Service::RenderingControl, event_source, event_data)
}

/// Create enriched event with registration ID
pub fn create_enriched_event_with_registration_id(
    registration_id: u64,
    speaker_ip: IpAddr,
    event_source: EventSource,
    event_data: RenderingControlEvent,
) -> EnrichedEvent<RenderingControlEvent> {
    EnrichedEvent::with_registration_id(
        registration_id,
        speaker_ip,
        Service::RenderingControl,
        event_source,
        event_data,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rendering_control_parser_service_type() {
        let parser = RenderingControlEventParser;
        assert_eq!(parser.service_type(), Service::RenderingControl);
    }

    #[test]
    fn test_rendering_control_event_creation() {
        let event = RenderingControlEvent {
            property: RenderingControlProperty {
                last_change: RenderingControlEventData {
                    instance: RenderingControlInstance {
                        volumes: vec![
                            ChannelValueAttribute { val: "75".to_string(), channel: "Master".to_string() },
                        ],
                        mutes: vec![
                            ChannelValueAttribute { val: "false".to_string(), channel: "Master".to_string() },
                        ],
                        bass: Some(xml_utils::ValueAttribute { val: "0".to_string() }),
                        treble: Some(xml_utils::ValueAttribute { val: "0".to_string() }),
                        loudness: Some(xml_utils::ValueAttribute { val: "true".to_string() }),
                        balance: Some(xml_utils::ValueAttribute { val: "0".to_string() }),
                    }
                }
            }
        };

        assert_eq!(event.master_volume(), Some("75".to_string()));
        assert_eq!(event.master_mute(), Some("false".to_string()));
        assert_eq!(event.other_channels().is_empty(), true);
    }

    #[test]
    fn test_basic_xml_parsing() {
        let xml = r#"<e:propertyset xmlns:e="urn:schemas-upnp-org:event-1-0">
            <e:property>
                <LastChange>&lt;Event xmlns="urn:schemas-upnp-org:metadata-1-0/RCS/"&gt;
                    &lt;InstanceID val="0"&gt;
                        &lt;Volume channel="Master" val="75"/&gt;
                        &lt;Mute channel="Master" val="0"/&gt;
                        &lt;Bass val="2"/&gt;
                        &lt;Treble val="-1"/&gt;
                    &lt;/InstanceID&gt;
                &lt;/Event&gt;</LastChange>
            </e:property>
        </e:propertyset>"#;

        let event = RenderingControlEvent::from_xml(xml).unwrap();
        assert_eq!(event.master_volume(), Some("75".to_string()));
        assert_eq!(event.master_mute(), Some("0".to_string()));
        assert_eq!(event.bass(), Some("2".to_string()));
        assert_eq!(event.treble(), Some("-1".to_string()));
    }

    #[test]
    fn test_channel_specific_volume() {
        let xml = r#"<e:propertyset xmlns:e="urn:schemas-upnp-org:event-1-0">
            <e:property>
                <LastChange>&lt;Event xmlns="urn:schemas-upnp-org:metadata-1-0/RCS/"&gt;
                    &lt;InstanceID val="0"&gt;
                        &lt;Volume channel="Master" val="50"/&gt;
                        &lt;Volume channel="LF" val="80"/&gt;
                        &lt;Volume channel="RF" val="85"/&gt;
                        &lt;Mute channel="LF" val="1"/&gt;
                    &lt;/InstanceID&gt;
                &lt;/Event&gt;</LastChange>
            </e:property>
        </e:propertyset>"#;

        let event = RenderingControlEvent::from_xml(xml).unwrap();
        assert_eq!(event.master_volume(), Some("50".to_string()));
        assert_eq!(event.lf_volume(), Some("80".to_string()));
        assert_eq!(event.rf_volume(), Some("85".to_string()));
        assert_eq!(event.lf_mute(), Some("1".to_string()));
    }

    #[test]
    fn test_enriched_event_creation() {
        let ip: IpAddr = "192.168.1.100".parse().unwrap();
        let source = EventSource::UPnPNotification {
            subscription_id: "uuid:123".to_string(),
        };
        let event_data = RenderingControlEvent {
            property: RenderingControlProperty {
                last_change: RenderingControlEventData {
                    instance: RenderingControlInstance {
                        volumes: vec![
                            ChannelValueAttribute { val: "50".to_string(), channel: "Master".to_string() },
                        ],
                        mutes: vec![
                            ChannelValueAttribute { val: "0".to_string(), channel: "Master".to_string() },
                        ],
                        bass: None,
                        treble: None,
                        loudness: None,
                        balance: None,
                    }
                }
            }
        };

        let enriched = create_enriched_event(ip, source, event_data);

        assert_eq!(enriched.speaker_ip, ip);
        assert_eq!(enriched.service, Service::RenderingControl);
        assert!(enriched.registration_id.is_none());
    }

    #[test]
    fn test_enriched_event_with_registration_id() {
        let ip: IpAddr = "192.168.1.100".parse().unwrap();
        let source = EventSource::UPnPNotification {
            subscription_id: "uuid:123".to_string(),
        };
        let event_data = RenderingControlEvent {
            property: RenderingControlProperty {
                last_change: RenderingControlEventData {
                    instance: RenderingControlInstance {
                        volumes: vec![
                            ChannelValueAttribute { val: "50".to_string(), channel: "Master".to_string() },
                        ],
                        mutes: vec![
                            ChannelValueAttribute { val: "0".to_string(), channel: "Master".to_string() },
                        ],
                        bass: None,
                        treble: None,
                        loudness: None,
                        balance: None,
                    }
                }
            }
        };

        let enriched = create_enriched_event_with_registration_id(42, ip, source, event_data);

        assert_eq!(enriched.registration_id, Some(42));
    }
}