//! AVTransport service event types and parsing
//!
//! Provides direct serde-based XML parsing with no business logic,
//! replicating exactly what Sonos produces for sonos-stream consumption.

use serde::{Deserialize, Serialize};
use std::net::IpAddr;

use crate::{Result, Service, ApiError};
use crate::events::{EnrichedEvent, EventSource, EventParser, xml_utils};

/// Minimal AVTransport event - direct serde mapping from UPnP event XML
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename = "propertyset")]
pub struct AVTransportEvent {
    #[serde(rename = "property")]
    property: AVTransportProperty,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AVTransportProperty {
    #[serde(rename = "LastChange", deserialize_with = "xml_utils::deserialize_nested")]
    last_change: AVTransportEventData,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename = "Event")]
pub struct AVTransportEventData {
    #[serde(rename = "InstanceID")]
    instance: AVTransportInstance,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AVTransportInstance {
    #[serde(rename = "TransportState", default)]
    pub transport_state: Option<xml_utils::ValueAttribute>,

    #[serde(rename = "TransportStatus", default)]
    pub transport_status: Option<xml_utils::ValueAttribute>,

    #[serde(rename = "TransportPlaySpeed", default)]
    pub speed: Option<xml_utils::ValueAttribute>,

    #[serde(rename = "CurrentTrackURI", default)]
    pub current_track_uri: Option<xml_utils::ValueAttribute>,

    #[serde(rename = "CurrentTrackDuration", default)]
    pub track_duration: Option<xml_utils::ValueAttribute>,

    #[serde(rename = "RelativeTimePosition", default)]
    pub rel_time: Option<xml_utils::ValueAttribute>,

    #[serde(rename = "AbsoluteTimePosition", default)]
    pub abs_time: Option<xml_utils::ValueAttribute>,

    #[serde(rename = "CurrentTrack", default)]
    pub rel_count: Option<xml_utils::ValueAttribute>,

    #[serde(rename = "CurrentPlayMode", default)]
    pub play_mode: Option<xml_utils::ValueAttribute>,

    #[serde(rename = "CurrentTrackMetaData", default)]
    pub track_metadata: Option<xml_utils::ValueAttribute>,

    #[serde(rename = "NextTrackURI", default)]
    pub next_track_uri: Option<xml_utils::ValueAttribute>,

    #[serde(rename = "NextTrackMetaData", default)]
    pub next_track_metadata: Option<xml_utils::ValueAttribute>,

    #[serde(rename = "NumberOfTracks", default)]
    pub queue_length: Option<xml_utils::ValueAttribute>,
}

impl AVTransportEvent {
    /// Get transport state
    pub fn transport_state(&self) -> Option<String> {
        self.property.last_change.instance.transport_state.as_ref().map(|v| v.val.clone())
    }

    /// Get transport status
    pub fn transport_status(&self) -> Option<String> {
        self.property.last_change.instance.transport_status.as_ref().map(|v| v.val.clone())
    }

    /// Get speed
    pub fn speed(&self) -> Option<String> {
        self.property.last_change.instance.speed.as_ref().map(|v| v.val.clone())
    }

    /// Get current track URI
    pub fn current_track_uri(&self) -> Option<String> {
        self.property.last_change.instance.current_track_uri.as_ref().map(|v| v.val.clone())
    }

    /// Get track duration
    pub fn track_duration(&self) -> Option<String> {
        self.property.last_change.instance.track_duration.as_ref().map(|v| v.val.clone())
    }

    /// Get relative time
    pub fn rel_time(&self) -> Option<String> {
        self.property.last_change.instance.rel_time.as_ref().map(|v| v.val.clone())
    }

    /// Get absolute time
    pub fn abs_time(&self) -> Option<String> {
        self.property.last_change.instance.abs_time.as_ref().map(|v| v.val.clone())
    }

    /// Get relative count
    pub fn rel_count(&self) -> Option<u32> {
        self.property.last_change.instance.rel_count.as_ref().and_then(|v| v.val.parse().ok())
    }

    /// Get absolute count (not available)
    pub fn abs_count(&self) -> Option<u32> {
        None
    }

    /// Get play mode
    pub fn play_mode(&self) -> Option<String> {
        self.property.last_change.instance.play_mode.as_ref().map(|v| v.val.clone())
    }

    /// Get track metadata
    pub fn track_metadata(&self) -> Option<String> {
        self.property.last_change.instance.track_metadata.as_ref().map(|v| v.val.clone())
    }

    /// Get next track URI
    pub fn next_track_uri(&self) -> Option<String> {
        self.property.last_change.instance.next_track_uri.as_ref().map(|v| v.val.clone())
    }

    /// Get next track metadata
    pub fn next_track_metadata(&self) -> Option<String> {
        self.property.last_change.instance.next_track_metadata.as_ref().map(|v| v.val.clone())
    }

    /// Get queue length
    pub fn queue_length(&self) -> Option<u32> {
        self.property.last_change.instance.queue_length.as_ref().and_then(|v| v.val.parse().ok())
    }

    /// Parse from UPnP event XML using serde
    pub fn from_xml(xml: &str) -> Result<Self> {
        let clean_xml = xml_utils::strip_namespaces(xml);
        quick_xml::de::from_str(&clean_xml)
            .map_err(|e| ApiError::ParseError(format!("Failed to parse AVTransport XML: {}", e)))
    }
}

/// Minimal parser implementation
pub struct AVTransportEventParser;

impl EventParser for AVTransportEventParser {
    type EventData = AVTransportEvent;

    fn parse_upnp_event(&self, xml: &str) -> Result<Self::EventData> {
        AVTransportEvent::from_xml(xml)
    }

    fn service_type(&self) -> Service {
        Service::AVTransport
    }
}

/// Create enriched event for sonos-stream integration
pub fn create_enriched_event(
    speaker_ip: IpAddr,
    event_source: EventSource,
    event_data: AVTransportEvent,
) -> EnrichedEvent<AVTransportEvent> {
    EnrichedEvent::new(speaker_ip, Service::AVTransport, event_source, event_data)
}

/// Create enriched event with registration ID
pub fn create_enriched_event_with_registration_id(
    registration_id: u64,
    speaker_ip: IpAddr,
    event_source: EventSource,
    event_data: AVTransportEvent,
) -> EnrichedEvent<AVTransportEvent> {
    EnrichedEvent::with_registration_id(
        registration_id,
        speaker_ip,
        Service::AVTransport,
        event_source,
        event_data,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_av_transport_parser_service_type() {
        let parser = AVTransportEventParser;
        assert_eq!(parser.service_type(), Service::AVTransport);
    }

    #[test]
    fn test_av_transport_event_creation() {
        let event_data = AVTransportEventData {
            instance: AVTransportInstance {
                transport_state: Some(xml_utils::ValueAttribute { val: "PLAYING".to_string() }),
                transport_status: Some(xml_utils::ValueAttribute { val: "OK".to_string() }),
                speed: Some(xml_utils::ValueAttribute { val: "1".to_string() }),
                current_track_uri: None,
                track_duration: None,
                rel_time: None,
                abs_time: None,
                rel_count: None,
                play_mode: None,
                track_metadata: None,
                next_track_uri: None,
                next_track_metadata: None,
                queue_length: None,
            }
        };

        let event = AVTransportEvent {
            property: AVTransportProperty {
                last_change: event_data,
            }
        };

        assert_eq!(event.transport_state(), Some("PLAYING".to_string()));
        assert_eq!(event.transport_status(), Some("OK".to_string()));
    }

    #[test]
    fn test_enriched_event_creation() {
        let ip: IpAddr = "192.168.1.100".parse().unwrap();
        let source = EventSource::UPnPNotification {
            subscription_id: "uuid:123".to_string(),
        };
        let event_data = AVTransportEvent {
            property: AVTransportProperty {
                last_change: AVTransportEventData {
                    instance: AVTransportInstance {
                        transport_state: Some(xml_utils::ValueAttribute { val: "PLAYING".to_string() }),
                        transport_status: None,
                        speed: None,
                        current_track_uri: None,
                        track_duration: None,
                        rel_time: None,
                        abs_time: None,
                        rel_count: None,
                        play_mode: None,
                        track_metadata: None,
                        next_track_uri: None,
                        next_track_metadata: None,
                        queue_length: None,
                    }
                }
            }
        };

        let enriched = create_enriched_event(ip, source, event_data);

        assert_eq!(enriched.speaker_ip, ip);
        assert_eq!(enriched.service, Service::AVTransport);
        assert!(enriched.registration_id.is_none());
    }

    #[test]
    fn test_enriched_event_with_registration_id() {
        let ip: IpAddr = "192.168.1.100".parse().unwrap();
        let source = EventSource::UPnPNotification {
            subscription_id: "uuid:123".to_string(),
        };
        let event_data = AVTransportEvent {
            property: AVTransportProperty {
                last_change: AVTransportEventData {
                    instance: AVTransportInstance {
                        transport_state: Some(xml_utils::ValueAttribute { val: "PLAYING".to_string() }),
                        transport_status: None,
                        speed: None,
                        current_track_uri: None,
                        track_duration: None,
                        rel_time: None,
                        abs_time: None,
                        rel_count: None,
                        play_mode: None,
                        track_metadata: None,
                        next_track_uri: None,
                        next_track_metadata: None,
                        queue_length: None,
                    }
                }
            }
        };

        let enriched = create_enriched_event_with_registration_id(42, ip, source, event_data);

        assert_eq!(enriched.registration_id, Some(42));
    }

    #[test]
    fn test_basic_xml_parsing() {
        let xml = r#"<e:propertyset xmlns:e="urn:schemas-upnp-org:event-1-0">
            <e:property>
                <LastChange>&lt;Event xmlns="urn:schemas-upnp-org:metadata-1-0/AVT/"&gt;
                    &lt;InstanceID val="0"&gt;
                        &lt;TransportState val="PLAYING"/&gt;
                        &lt;TransportStatus val="OK"/&gt;
                        &lt;CurrentTrack val="1"/&gt;
                        &lt;NumberOfTracks val="5"/&gt;
                    &lt;/InstanceID&gt;
                &lt;/Event&gt;</LastChange>
            </e:property>
        </e:propertyset>"#;

        let event = AVTransportEvent::from_xml(xml).unwrap();
        assert_eq!(event.transport_state(), Some("PLAYING".to_string()));
        assert_eq!(event.transport_status(), Some("OK".to_string()));
        assert_eq!(event.rel_count(), Some(1));
        assert_eq!(event.queue_length(), Some(5));
    }
}