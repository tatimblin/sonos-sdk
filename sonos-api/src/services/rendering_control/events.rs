//! RenderingControl service event types and parsing
//!
//! This module handles events from the RenderingControl UPnP service, which manages
//! audio rendering settings like volume, mute, bass, treble, and channel-specific controls.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::IpAddr;

use crate::{Result, Service};
use crate::events::{EnrichedEvent, EventSource, EventParser, extract_xml_value};

/// Complete RenderingControl event data containing all rendering state information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenderingControlEvent {
    /// Current volume level (0-100) for Master channel
    pub master_volume: Option<String>,

    /// Current volume level (0-100) for Left Front channel
    pub lf_volume: Option<String>,

    /// Current volume level (0-100) for Right Front channel
    pub rf_volume: Option<String>,

    /// Current mute state for Master channel
    pub master_mute: Option<String>,

    /// Current mute state for Left Front channel
    pub lf_mute: Option<String>,

    /// Current mute state for Right Front channel
    pub rf_mute: Option<String>,

    /// Current bass level
    pub bass: Option<String>,

    /// Current treble level
    pub treble: Option<String>,

    /// Current loudness setting
    pub loudness: Option<String>,

    /// Balance setting (-100 to +100)
    pub balance: Option<String>,

    /// Additional channel configurations (can be extended)
    pub other_channels: HashMap<String, String>,
}

impl RenderingControlEvent {
    /// Parse RenderingControl event from XML using self-parsing.
    ///
    /// This method uses basic XML extraction since RenderingControl events
    /// are simpler than other services and don't require complex serde parsing.
    ///
    /// # Arguments
    ///
    /// * `xml` - The raw UPnP event XML
    ///
    /// # Returns
    ///
    /// The parsed RenderingControlEvent, or an error if parsing fails.
    pub fn from_xml(xml: &str) -> Result<Self> {
        // Use basic XML extraction for all fields
        let event = RenderingControlEvent {
            master_volume: extract_xml_value(xml, "Volume")
                .or_else(|| extract_xml_value(xml, "MasterVolume")),
            lf_volume: extract_xml_value(xml, "LFVolume"),
            rf_volume: extract_xml_value(xml, "RFVolume"),
            master_mute: extract_xml_value(xml, "Mute")
                .or_else(|| extract_xml_value(xml, "MasterMute")),
            lf_mute: extract_xml_value(xml, "LFMute"),
            rf_mute: extract_xml_value(xml, "RFMute"),
            bass: extract_xml_value(xml, "Bass"),
            treble: extract_xml_value(xml, "Treble"),
            loudness: extract_xml_value(xml, "Loudness"),
            balance: extract_xml_value(xml, "Balance"),
            other_channels: HashMap::new(), // TODO: Parse additional channels if needed
        };

        Ok(event)
    }
}

/// Event parser for RenderingControl service
pub struct RenderingControlEventParser;

impl EventParser for RenderingControlEventParser {
    type EventData = RenderingControlEvent;

    fn parse_upnp_event(&self, xml: &str) -> Result<Self::EventData> {
        // TODO: Implement proper RenderingControl parser when sonos-parser supports it
        // For now, create a basic event structure using XML extraction

        // Parse basic XML structure to extract any volume/mute information
        // This is a simplified implementation until a proper parser is available
        let event = RenderingControlEvent {
            master_volume: extract_xml_value(xml, "Volume")
                .or_else(|| extract_xml_value(xml, "MasterVolume")),
            lf_volume: extract_xml_value(xml, "LFVolume"),
            rf_volume: extract_xml_value(xml, "RFVolume"),
            master_mute: extract_xml_value(xml, "Mute")
                .or_else(|| extract_xml_value(xml, "MasterMute")),
            lf_mute: extract_xml_value(xml, "LFMute"),
            rf_mute: extract_xml_value(xml, "RFMute"),
            bass: extract_xml_value(xml, "Bass"),
            treble: extract_xml_value(xml, "Treble"),
            loudness: extract_xml_value(xml, "Loudness"),
            balance: extract_xml_value(xml, "Balance"),
            other_channels: HashMap::new(),
        };

        Ok(event)
    }

    fn service_type(&self) -> Service {
        Service::RenderingControl
    }
}

/// Create an enriched RenderingControl event
pub fn create_enriched_event(
    speaker_ip: IpAddr,
    event_source: EventSource,
    event_data: RenderingControlEvent,
) -> EnrichedEvent<RenderingControlEvent> {
    EnrichedEvent::new(speaker_ip, Service::RenderingControl, event_source, event_data)
}

/// Create an enriched RenderingControl event with registration ID (for sonos-stream integration)
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
        let mut other_channels = HashMap::new();
        other_channels.insert("Sub".to_string(), "50".to_string());

        let event = RenderingControlEvent {
            master_volume: Some("75".to_string()),
            lf_volume: None,
            rf_volume: None,
            master_mute: Some("false".to_string()),
            lf_mute: None,
            rf_mute: None,
            bass: Some("0".to_string()),
            treble: Some("0".to_string()),
            loudness: Some("true".to_string()),
            balance: Some("0".to_string()),
            other_channels,
        };

        assert_eq!(event.master_volume, Some("75".to_string()));
        assert_eq!(event.master_mute, Some("false".to_string()));
        assert_eq!(event.other_channels.get("Sub"), Some(&"50".to_string()));
    }

    #[test]
    fn test_basic_xml_parsing() {
        let parser = RenderingControlEventParser;

        let xml = r#"<Event>
            <Volume>50</Volume>
            <Mute>false</Mute>
            <Bass>2</Bass>
            <Treble>-1</Treble>
        </Event>"#;

        let result = parser.parse_upnp_event(xml);
        assert!(result.is_ok());

        let event = result.unwrap();
        assert_eq!(event.master_volume, Some("50".to_string()));
        assert_eq!(event.master_mute, Some("false".to_string()));
        assert_eq!(event.bass, Some("2".to_string()));
        assert_eq!(event.treble, Some("-1".to_string()));
    }

    #[test]
    fn test_enriched_event_creation() {
        let ip: IpAddr = "192.168.1.100".parse().unwrap();
        let source = EventSource::UPnPNotification {
            subscription_id: "uuid:123".to_string(),
        };
        let event_data = RenderingControlEvent {
            master_volume: Some("50".to_string()),
            lf_volume: None,
            rf_volume: None,
            master_mute: Some("false".to_string()),
            lf_mute: None,
            rf_mute: None,
            bass: None,
            treble: None,
            loudness: None,
            balance: None,
            other_channels: HashMap::new(),
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
            master_volume: Some("50".to_string()),
            lf_volume: None,
            rf_volume: None,
            master_mute: Some("false".to_string()),
            lf_mute: None,
            rf_mute: None,
            bass: None,
            treble: None,
            loudness: None,
            balance: None,
            other_channels: HashMap::new(),
        };

        let enriched = create_enriched_event_with_registration_id(42, ip, source, event_data);

        assert_eq!(enriched.registration_id, Some(42));
    }

    #[test]
    fn test_self_parsing_basic() {
        // Test basic RenderingControl XML parsing with the new from_xml method
        let xml = r#"<Event>
            <Volume>75</Volume>
            <Mute>false</Mute>
            <Bass>2</Bass>
            <Treble>-1</Treble>
            <Loudness>true</Loudness>
            <Balance>5</Balance>
        </Event>"#;

        let result = RenderingControlEvent::from_xml(xml);
        assert!(result.is_ok(), "Failed to parse RenderingControl XML: {:?}", result.err());

        let event = result.unwrap();
        assert_eq!(event.master_volume, Some("75".to_string()));
        assert_eq!(event.master_mute, Some("false".to_string()));
        assert_eq!(event.bass, Some("2".to_string()));
        assert_eq!(event.treble, Some("-1".to_string()));
        assert_eq!(event.loudness, Some("true".to_string()));
        assert_eq!(event.balance, Some("5".to_string()));
    }

    #[test]
    fn test_self_parsing_empty() {
        // Test parsing with empty/no values
        let xml = r#"<Event></Event>"#;

        let result = RenderingControlEvent::from_xml(xml);
        assert!(result.is_ok());

        let event = result.unwrap();
        assert_eq!(event.master_volume, None);
        assert_eq!(event.master_mute, None);
        assert_eq!(event.bass, None);
        assert_eq!(event.treble, None);
        assert_eq!(event.loudness, None);
        assert_eq!(event.balance, None);
    }
}