//! AVTransport service event types and parsing
//!
//! This module handles events from the AVTransport UPnP service, which manages
//! playback control (play/pause/stop), track information, and transport state.

use serde::{Deserialize, Serialize};
use std::net::IpAddr;

use crate::{Result, Service};
use crate::events::{EnrichedEvent, EventSource, EventParser, extract_xml_value, xml_utils};

/// Complete AVTransport event data containing all transport state information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AVTransportEvent {
    /// Current transport state (PLAYING, PAUSED_PLAYBACK, STOPPED, etc.)
    pub transport_state: Option<String>,

    /// Current transport status (OK, ERROR_OCCURRED, etc.)
    pub transport_status: Option<String>,

    /// Current playback speed
    pub speed: Option<String>,

    /// Current track URI
    pub current_track_uri: Option<String>,

    /// Track duration
    pub track_duration: Option<String>,

    /// Relative time position in current track
    pub rel_time: Option<String>,

    /// Absolute time position
    pub abs_time: Option<String>,

    /// Relative track number in queue
    pub rel_count: Option<u32>,

    /// Absolute track number
    pub abs_count: Option<u32>,

    /// Current play mode (NORMAL, REPEAT_ALL, REPEAT_ONE, SHUFFLE, etc.)
    pub play_mode: Option<String>,

    /// Current track metadata (DIDL-Lite XML)
    pub track_metadata: Option<String>,

    /// Next track URI
    pub next_track_uri: Option<String>,

    /// Next track metadata
    pub next_track_metadata: Option<String>,

    /// Queue size/length
    pub queue_length: Option<u32>,
}

// Serde parsing structures for AVTransport UPnP events (moved from sonos-parser)

/// Root parser structure for AVTransport UPnP events.
///
/// UPnP events are wrapped in a propertyset structure:
/// ```xml
/// <e:propertyset xmlns:e="urn:schemas-upnp-org:event-1-0">
///   <e:property>
///     <LastChange>...</LastChange>
///   </e:property>
/// </e:propertyset>
/// ```
#[derive(Debug, Clone, Deserialize)]
#[serde(rename = "propertyset")]
struct AVTransportPropertySet {
    /// The property element containing LastChange
    #[serde(rename = "property")]
    property: AVTransportProperty,
}

/// Property wrapper containing the LastChange element.
#[derive(Debug, Clone, Deserialize)]
struct AVTransportProperty {
    /// The LastChange element with nested XML content
    #[serde(rename = "LastChange", deserialize_with = "xml_utils::deserialize_nested")]
    last_change: AVTransportLastChange,
}

/// The root element for decoded LastChange content.
///
/// The LastChange content follows this structure:
/// ```xml
/// <Event xmlns="urn:schemas-upnp-org:metadata-1-0/AVT/">
///   <InstanceID val="0">
///     <TransportState val="PLAYING"/>
///     ...
///   </InstanceID>
/// </Event>
/// ```
#[derive(Debug, Clone, Deserialize)]
#[serde(rename = "Event")]
struct AVTransportLastChange {
    /// The instance containing all state variables
    #[serde(rename = "InstanceID")]
    instance: AVTransportInstance,
}

/// Instance containing AVTransport state variables.
#[derive(Debug, Clone, Deserialize)]
struct AVTransportInstance {
    /// Instance ID (usually "0")
    #[serde(rename = "@val")]
    id: String,

    /// Current transport state (PLAYING, PAUSED_PLAYBACK, STOPPED, TRANSITIONING)
    #[serde(rename = "TransportState")]
    transport_state: xml_utils::ValueAttribute,

    /// Current play mode (NORMAL, REPEAT_ALL, SHUFFLE, etc.)
    #[serde(rename = "CurrentPlayMode", default)]
    current_play_mode: Option<xml_utils::ValueAttribute>,

    /// Crossfade mode (0 or 1)
    #[serde(rename = "CurrentCrossfadeMode", default)]
    current_crossfade_mode: Option<xml_utils::ValueAttribute>,

    /// Number of tracks in queue
    #[serde(rename = "NumberOfTracks", default)]
    number_of_tracks: Option<xml_utils::ValueAttribute>,

    /// Current track number
    #[serde(rename = "CurrentTrack", default)]
    current_track: Option<xml_utils::ValueAttribute>,

    /// URI of the current track
    #[serde(rename = "CurrentTrackURI", default)]
    current_track_uri: Option<xml_utils::ValueAttribute>,

    /// Duration of the current track (HH:MM:SS format)
    #[serde(rename = "CurrentTrackDuration", default)]
    current_track_duration: Option<xml_utils::ValueAttribute>,

    /// DIDL-Lite metadata for the current track
    #[serde(rename = "CurrentTrackMetaData", default)]
    current_track_metadata: Option<xml_utils::NestedAttribute<xml_utils::DidlLite>>,

    /// URI of the next track
    #[serde(rename = "NextTrackURI", default)]
    next_track_uri: Option<xml_utils::ValueAttribute>,

    /// Metadata for the next track
    #[serde(rename = "NextTrackMetaData", default)]
    next_track_metadata: Option<xml_utils::ValueAttribute>,

    /// Transport status
    #[serde(rename = "TransportStatus", default)]
    transport_status: Option<xml_utils::ValueAttribute>,

    /// Relative time position
    #[serde(rename = "RelativeTimePosition", default)]
    relative_time_position: Option<xml_utils::ValueAttribute>,

    /// Absolute time position
    #[serde(rename = "AbsoluteTimePosition", default)]
    absolute_time_position: Option<xml_utils::ValueAttribute>,

    /// Current playback speed
    #[serde(rename = "TransportPlaySpeed", default)]
    transport_play_speed: Option<xml_utils::ValueAttribute>,
}

impl AVTransportEvent {
    /// Parse AVTransport event from XML using self-parsing.
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
    /// The parsed AVTransportEvent, or an error if parsing fails.
    pub fn from_xml(xml: &str) -> Result<Self> {
        // Try primary serde-based parsing
        match Self::parse_with_serde(xml) {
            Ok(event) => Ok(event),
            Err(_) => {
                // Fallback to basic XML extraction if serde parsing fails
                Ok(AVTransportEvent {
                    transport_state: extract_xml_value(xml, "TransportState"),
                    transport_status: extract_xml_value(xml, "TransportStatus"),
                    speed: extract_xml_value(xml, "TransportPlaySpeed"),
                    current_track_uri: extract_xml_value(xml, "CurrentTrackURI"),
                    track_duration: extract_xml_value(xml, "CurrentTrackDuration"),
                    rel_time: extract_xml_value(xml, "RelativeTimePosition"),
                    abs_time: extract_xml_value(xml, "AbsoluteTimePosition"),
                    rel_count: extract_xml_value(xml, "CurrentTrack").and_then(|s| s.parse().ok()),
                    abs_count: None, // Not available in fallback parsing
                    play_mode: extract_xml_value(xml, "CurrentPlayMode"),
                    track_metadata: extract_xml_value(xml, "CurrentTrackMetaData"),
                    next_track_uri: extract_xml_value(xml, "NextTrackURI"),
                    next_track_metadata: extract_xml_value(xml, "NextTrackMetaData"),
                    queue_length: extract_xml_value(xml, "NumberOfTracks").and_then(|s| s.parse().ok()),
                })
            }
        }
    }

    /// Primary serde-based parsing implementation.
    fn parse_with_serde(xml: &str) -> Result<Self> {
        let clean_xml = xml_utils::strip_namespaces(xml);
        let property_set: AVTransportPropertySet = quick_xml::de::from_str(&clean_xml)
            .map_err(|e| crate::ApiError::ParseError(format!("Failed to parse AVTransport XML: {}", e)))?;

        let instance = &property_set.property.last_change.instance;

        Ok(AVTransportEvent {
            transport_state: Some(instance.transport_state.val.clone()),
            transport_status: instance.transport_status.as_ref().map(|v| v.val.clone()),
            speed: instance.transport_play_speed.as_ref().map(|v| v.val.clone()),
            current_track_uri: instance.current_track_uri.as_ref().map(|v| v.val.clone()),
            track_duration: instance.current_track_duration.as_ref().map(|v| v.val.clone()),
            rel_time: instance.relative_time_position.as_ref().map(|v| v.val.clone()),
            abs_time: instance.absolute_time_position.as_ref().map(|v| v.val.clone()),
            rel_count: instance.current_track.as_ref().and_then(|v| v.val.parse().ok()),
            abs_count: None, // Not available in current XML structure
            play_mode: instance.current_play_mode.as_ref().map(|v| v.val.clone()),
            track_metadata: instance.current_track_metadata.as_ref()
                .and_then(|nested| nested.val.as_ref())
                .map(|didl| Self::format_didl_metadata(didl)),
            next_track_uri: instance.next_track_uri.as_ref().map(|v| v.val.clone()),
            next_track_metadata: instance.next_track_metadata.as_ref().map(|v| v.val.clone()),
            queue_length: instance.number_of_tracks.as_ref().and_then(|v| v.val.parse().ok()),
        })
    }

    /// Convert structured DIDL-Lite metadata to string representation.
    ///
    /// This maintains compatibility with the existing string-based metadata field
    /// while using the full DIDL-Lite parsing capabilities.
    fn format_didl_metadata(didl: &xml_utils::DidlLite) -> String {
        if let Some(item) = didl.items.first() {
            let title = item.title.as_ref().map(|s| s.as_str()).unwrap_or("Unknown");
            let artist = item.creator.as_ref().map(|s| s.as_str()).unwrap_or("Unknown");
            let album = item.album.as_ref().map(|s| s.as_str()).unwrap_or("Unknown");

            format!("Title: {}, Artist: {}, Album: {}", title, artist, album)
        } else {
            "No metadata".to_string()
        }
    }
}

/// Event parser for AVTransport service
pub struct AVTransportEventParser;

impl EventParser for AVTransportEventParser {
    type EventData = AVTransportEvent;

    fn parse_upnp_event(&self, xml: &str) -> Result<Self::EventData> {
        // Use self-parsing method
        AVTransportEvent::from_xml(xml)
    }

    fn service_type(&self) -> Service {
        Service::AVTransport
    }
}

/// Create an enriched AVTransport event
pub fn create_enriched_event(
    speaker_ip: IpAddr,
    event_source: EventSource,
    event_data: AVTransportEvent,
) -> EnrichedEvent<AVTransportEvent> {
    EnrichedEvent::new(speaker_ip, Service::AVTransport, event_source, event_data)
}

/// Create an enriched AVTransport event with registration ID (for sonos-stream integration)
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
        let event = AVTransportEvent {
            transport_state: Some("PLAYING".to_string()),
            transport_status: Some("OK".to_string()),
            speed: Some("1".to_string()),
            current_track_uri: None,
            track_duration: None,
            rel_time: None,
            abs_time: None,
            rel_count: None,
            abs_count: None,
            play_mode: None,
            track_metadata: None,
            next_track_uri: None,
            next_track_metadata: None,
            queue_length: None,
        };

        assert_eq!(event.transport_state, Some("PLAYING".to_string()));
        assert_eq!(event.transport_status, Some("OK".to_string()));
    }

    #[test]
    fn test_enriched_event_creation() {
        let ip: IpAddr = "192.168.1.100".parse().unwrap();
        let source = EventSource::UPnPNotification {
            subscription_id: "uuid:123".to_string(),
        };
        let event_data = AVTransportEvent {
            transport_state: Some("PLAYING".to_string()),
            transport_status: None,
            speed: None,
            current_track_uri: None,
            track_duration: None,
            rel_time: None,
            abs_time: None,
            rel_count: None,
            abs_count: None,
            play_mode: None,
            track_metadata: None,
            next_track_uri: None,
            next_track_metadata: None,
            queue_length: None,
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
            transport_state: Some("PLAYING".to_string()),
            transport_status: None,
            speed: None,
            current_track_uri: None,
            track_duration: None,
            rel_time: None,
            abs_time: None,
            rel_count: None,
            abs_count: None,
            play_mode: None,
            track_metadata: None,
            next_track_uri: None,
            next_track_metadata: None,
            queue_length: None,
        };

        let enriched = create_enriched_event_with_registration_id(42, ip, source, event_data);

        assert_eq!(enriched.registration_id, Some(42));
    }

    #[test]
    fn test_fallback_parsing() {
        let parser = AVTransportEventParser;

        // Test with invalid XML that would cause sonos-parser to fail
        let xml = "<InvalidXML><TransportState>PLAYING</TransportState></InvalidXML>";

        let result = parser.parse_upnp_event(xml);
        assert!(result.is_ok());

        let event = result.unwrap();
        assert_eq!(event.transport_state, Some("PLAYING".to_string()));
    }

    #[test]
    fn test_self_parsing_basic() {
        // Test basic AVTransport XML parsing with the new from_xml method
        let xml = r#"<e:propertyset xmlns:e="urn:schemas-upnp-org:event-1-0">
            <e:property>
                <LastChange>&lt;Event xmlns="urn:schemas-upnp-org:metadata-1-0/AVT/"&gt;
                    &lt;InstanceID val="0"&gt;
                        &lt;TransportState val="PLAYING"/&gt;
                        &lt;TransportStatus val="OK"/&gt;
                        &lt;CurrentPlayMode val="NORMAL"/&gt;
                        &lt;CurrentTrack val="1"/&gt;
                        &lt;NumberOfTracks val="5"/&gt;
                    &lt;/InstanceID&gt;
                &lt;/Event&gt;</LastChange>
            </e:property>
        </e:propertyset>"#;

        let result = AVTransportEvent::from_xml(xml);
        assert!(result.is_ok(), "Failed to parse AVTransport XML: {:?}", result.err());

        let event = result.unwrap();
        assert_eq!(event.transport_state, Some("PLAYING".to_string()));
        assert_eq!(event.transport_status, Some("OK".to_string()));
        assert_eq!(event.play_mode, Some("NORMAL".to_string()));
        assert_eq!(event.rel_count, Some(1));
        assert_eq!(event.queue_length, Some(5));
    }

    #[test]
    fn test_self_parsing_fallback() {
        // Test fallback parsing with malformed XML
        let xml = "<InvalidXML><TransportState>STOPPED</TransportState><NumberOfTracks>3</NumberOfTracks></InvalidXML>";

        let result = AVTransportEvent::from_xml(xml);
        assert!(result.is_ok());

        let event = result.unwrap();
        assert_eq!(event.transport_state, Some("STOPPED".to_string()));
        assert_eq!(event.queue_length, Some(3));
    }
}