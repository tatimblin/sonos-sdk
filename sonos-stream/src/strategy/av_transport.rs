//! AVTransport strategy implementation.
//!
//! This module provides the AVTransport strategy for subscribing to and parsing
//! events from the AVTransport UPnP service on Sonos devices. The AVTransport
//! service handles media transport operations like play, pause, stop, and provides
//! events for playback state changes and track information.

use std::collections::HashMap;

use crate::error::StrategyError;
use crate::event::ParsedEvent;
use crate::types::{ServiceType, SpeakerId, SubscriptionScope};
use sonos_parser::services::av_transport::AVTransportParser;

use super::SubscriptionStrategy;

/// Strategy for handling AVTransport service subscriptions and events.
///
/// The AVTransport service provides events for:
/// - Transport state changes (PLAYING, PAUSED, STOPPED, etc.)
/// - Track metadata changes (title, artist, album, duration)
/// - Current track URI changes
///
/// This strategy implements the `SubscriptionStrategy` trait to provide
/// service-specific logic for creating subscriptions and parsing events.
///
/// # Thread Safety
///
/// This strategy is stateless and thread-safe. All subscription state is
/// managed by `UPnPSubscription` instances.
///
/// # Example
///
/// ```rust,ignore
/// use sonos_stream::{EventBrokerBuilder, AVTransportStrategy};
///
/// let broker = EventBrokerBuilder::new()
///     .with_strategy(Box::new(AVTransportStrategy))
///     .build().await?;
/// ```
#[derive(Debug, Clone)]
pub struct AVTransportStrategy;

impl AVTransportStrategy {
    /// Create a new AVTransport strategy instance.
    pub fn new() -> Self {
        Self
    }
}

impl Default for AVTransportStrategy {
    fn default() -> Self {
        Self::new()
    }
}

/// Convert a JSON value to a flat HashMap<String, String> for use in ParsedEvent.
/// 
/// This function recursively flattens nested JSON objects using dot notation for keys.
/// Arrays are converted to comma-separated strings.
fn json_to_string_map(value: serde_json::Value) -> HashMap<String, String> {
    let mut map = HashMap::new();
    json_to_string_map_recursive("", value, &mut map);
    map
}

fn json_to_string_map_recursive(prefix: &str, value: serde_json::Value, map: &mut HashMap<String, String>) {
    match value {
        serde_json::Value::Null => {
            if !prefix.is_empty() {
                map.insert(prefix.to_string(), "null".to_string());
            }
        }
        serde_json::Value::Bool(b) => {
            map.insert(prefix.to_string(), b.to_string());
        }
        serde_json::Value::Number(n) => {
            map.insert(prefix.to_string(), n.to_string());
        }
        serde_json::Value::String(s) => {
            map.insert(prefix.to_string(), s);
        }
        serde_json::Value::Array(arr) => {
            // Convert array to comma-separated string
            let array_str = arr.iter()
                .map(|v| match v {
                    serde_json::Value::String(s) => s.clone(),
                    _ => v.to_string(),
                })
                .collect::<Vec<_>>()
                .join(",");
            map.insert(prefix.to_string(), array_str);
        }
        serde_json::Value::Object(obj) => {
            for (key, val) in obj {
                let new_prefix = if prefix.is_empty() {
                    key
                } else {
                    format!("{}.{}", prefix, key)
                };
                json_to_string_map_recursive(&new_prefix, val, map);
            }
        }
    }
}

impl SubscriptionStrategy for AVTransportStrategy {
    fn service_type(&self) -> ServiceType {
        ServiceType::AVTransport
    }

    fn subscription_scope(&self) -> SubscriptionScope {
        SubscriptionScope::PerSpeaker
    }

    fn service_endpoint_path(&self) -> &'static str {
        "/MediaRenderer/AVTransport/Event"
    }

    fn parse_event(
        &self,
        _speaker_id: &SpeakerId,
        event_xml: &str,
    ) -> Result<Vec<ParsedEvent>, StrategyError> {
        // Parse using serde-based parser
        let parsed = AVTransportParser::from_xml(event_xml)
            .map_err(|e| StrategyError::EventParseFailed(format!("Failed to parse AVTransport event: {}", e)))?;
        
        // Serialize the entire parsed structure to JSON, then convert to HashMap<String, String>
        let json_value = serde_json::to_value(&parsed)
            .map_err(|e| StrategyError::EventParseFailed(format!("Failed to serialize parsed data: {}", e)))?;

        let data = json_to_string_map(json_value);
        
        // Create a single event with the entire parsed structure
        let event = ParsedEvent::custom("av_transport_event", data);
        Ok(vec![event])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sonos_parser::common::DidlLite;
    use crate::types::{SpeakerId, Speaker, SubscriptionConfig};
    use std::net::IpAddr;

    #[test]
    fn test_av_transport_strategy_creation() {
        let strategy = AVTransportStrategy::new();
        assert_eq!(strategy.service_type(), ServiceType::AVTransport);
        assert_eq!(strategy.subscription_scope(), SubscriptionScope::PerSpeaker);
    }

    #[test]
    fn test_av_transport_strategy_default() {
        let strategy = AVTransportStrategy::default();
        assert_eq!(strategy.service_type(), ServiceType::AVTransport);
        assert_eq!(strategy.subscription_scope(), SubscriptionScope::PerSpeaker);
    }

    #[test]
    fn test_service_endpoint_path() {
        let strategy = AVTransportStrategy::new();
        assert_eq!(strategy.service_endpoint_path(), "/MediaRenderer/AVTransport/Event");
    }

    #[test]
    fn test_thread_safety() {
        // Test that the strategy can be shared across threads
        let strategy = AVTransportStrategy::new();
        
        // This test ensures the strategy implements Send + Sync
        fn assert_send_sync<T: Send + Sync>(_: T) {}
        assert_send_sync(strategy);
    }

    #[test]
    fn test_endpoint_url_construction() {
        let strategy = AVTransportStrategy::new();
        let speaker = Speaker::new(
            SpeakerId::new("RINCON_123"),
            "192.168.1.100".parse::<IpAddr>().unwrap(),
            "Living Room".to_string(),
            "Living Room".to_string(),
        );
        let _config = SubscriptionConfig::new(1800, "http://192.168.1.50:3400/callback".to_string());
        
        // We can't easily test the full create_subscription without a mock server,
        // but we can verify the endpoint URL construction logic by checking the path
        let expected_path = "/MediaRenderer/AVTransport/Event";
        assert_eq!(strategy.service_endpoint_path(), expected_path);
        
        // The full URL would be: http://192.168.1.100:1400/MediaRenderer/AVTransport/Event
        let expected_url = format!("http://{}:1400{}", speaker.ip, expected_path);
        assert_eq!(expected_url, "http://192.168.1.100:1400/MediaRenderer/AVTransport/Event");
    }

    #[test]
    fn test_parse_event_invalid_xml() {
        let strategy = AVTransportStrategy::new();
        let speaker_id = SpeakerId::new("test_speaker");
        let result = strategy.parse_event(&speaker_id, "<xml></xml>");
        
        // Invalid XML should return an error with serde-based parsing
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_duration_to_ms() {
        // Standard format HH:MM:SS
        assert_eq!(AVTransportParser::parse_duration_to_ms("0:04:32"), Some(272000));
        assert_eq!(AVTransportParser::parse_duration_to_ms("1:00:00"), Some(3600000));
        assert_eq!(AVTransportParser::parse_duration_to_ms("0:00:30"), Some(30000));
        
        // MM:SS format
        assert_eq!(AVTransportParser::parse_duration_to_ms("04:32"), Some(272000));
        
        // Invalid format
        assert_eq!(AVTransportParser::parse_duration_to_ms("invalid"), None);
        assert_eq!(AVTransportParser::parse_duration_to_ms(""), None);
    }

    #[test]
    fn test_parse_didl_lite_basic() {
        let didl_xml = r#"<DIDL-Lite xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:upnp="urn:schemas-upnp-org:metadata-1-0/upnp/"><item id="-1" parentID="-1"><dc:title>Test Song</dc:title><dc:creator>Test Artist</dc:creator><upnp:album>Test Album</upnp:album></item></DIDL-Lite>"#;
        
        let result = DidlLite::from_xml(didl_xml);
        assert!(result.is_ok());
        
        let didl = result.unwrap();
        assert_eq!(didl.item.title, Some("Test Song".to_string()));
        assert_eq!(didl.item.creator, Some("Test Artist".to_string()));
        assert_eq!(didl.item.album, Some("Test Album".to_string()));
    }

    #[test]
    fn test_parse_full_event() {
        let event_xml = r#"<e:propertyset xmlns:e="urn:schemas-upnp-org:event-1-0"><e:property><LastChange>&lt;Event xmlns="urn:schemas-upnp-org:metadata-1-0/AVT/"&gt;&lt;InstanceID val="0"&gt;&lt;TransportState val="PAUSED_PLAYBACK"/&gt;&lt;/InstanceID&gt;&lt;/Event&gt;</LastChange></e:property></e:propertyset>"#;
        
        let strategy = AVTransportStrategy::new();
        let speaker_id = SpeakerId::new("test_speaker");
        let result = strategy.parse_event(&speaker_id, event_xml);
        
        assert!(result.is_ok(), "Parse failed: {:?}", result.err());
        let events = result.unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type(), "av_transport_event");
        assert_eq!(events[0].data().get("property.LastChange.InstanceID.@val"), Some(&"0".to_string()));
        assert_eq!(events[0].data().get("property.LastChange.InstanceID.TransportState.@val"), Some(&"PAUSED_PLAYBACK".to_string()));
    }

    #[test]
    fn test_json_serialization_approach() {
        let event_xml = r#"<e:propertyset xmlns:e="urn:schemas-upnp-org:event-1-0"><e:property><LastChange>&lt;Event xmlns="urn:schemas-upnp-org:metadata-1-0/AVT/"&gt;&lt;InstanceID val="0"&gt;&lt;TransportState val="PLAYING"/&gt;&lt;CurrentPlayMode val="REPEAT_ALL"/&gt;&lt;/InstanceID&gt;&lt;/Event&gt;</LastChange></e:property></e:propertyset>"#;
        
        let strategy = AVTransportStrategy::new();
        let speaker_id = SpeakerId::new("test_speaker");
        let result = strategy.parse_event(&speaker_id, event_xml);
        
        assert!(result.is_ok(), "Parse failed: {:?}", result.err());
        let events = result.unwrap();
        assert_eq!(events.len(), 1);
        
        let event = &events[0];
        assert_eq!(event.event_type(), "av_transport_event");
        
        // Verify we have the complete parsed structure
        assert_eq!(event.data().get("property.LastChange.InstanceID.@val"), Some(&"0".to_string()));
        assert_eq!(event.data().get("property.LastChange.InstanceID.TransportState.@val"), Some(&"PLAYING".to_string()));
        assert_eq!(event.data().get("property.LastChange.InstanceID.CurrentPlayMode.@val"), Some(&"REPEAT_ALL".to_string()));
        
        // Verify null fields are handled correctly
        assert_eq!(event.data().get("property.LastChange.InstanceID.CurrentTrackURI"), Some(&"null".to_string()));
        
        // Print all keys for debugging
        println!("All event data keys:");
        for key in event.data().keys() {
            println!("  {}", key);
        }
    }

    #[test]
    fn test_parse_full_event_with_track_info() {
        let event_xml = r#"<e:propertyset xmlns:e="urn:schemas-upnp-org:event-1-0"><e:property><LastChange>&lt;Event xmlns="urn:schemas-upnp-org:metadata-1-0/AVT/"&gt;&lt;InstanceID val="0"&gt;&lt;TransportState val="PLAYING"/&gt;&lt;CurrentTrackURI val="x-sonos-spotify:track123"/&gt;&lt;CurrentTrackDuration val="0:04:32"/&gt;&lt;CurrentTrackMetaData val="&amp;lt;DIDL-Lite&amp;gt;&amp;lt;item id=&amp;quot;-1&amp;quot; parentID=&amp;quot;-1&amp;quot;&amp;gt;&amp;lt;dc:title&amp;gt;Test Song&amp;lt;/dc:title&amp;gt;&amp;lt;dc:creator&amp;gt;Test Artist&amp;lt;/dc:creator&amp;gt;&amp;lt;/item&amp;gt;&amp;lt;/DIDL-Lite&amp;gt;"/&gt;&lt;/InstanceID&gt;&lt;/Event&gt;</LastChange></e:property></e:propertyset>"#;
        
        let strategy = AVTransportStrategy::new();
        let speaker_id = SpeakerId::new("test_speaker");
        let result = strategy.parse_event(&speaker_id, event_xml);
        
        assert!(result.is_ok(), "Parse failed: {:?}", result.err());
        let events = result.unwrap();
        assert_eq!(events.len(), 1);
        
        // Single event with all parsed data
        assert_eq!(events[0].event_type(), "av_transport_event");
        assert_eq!(events[0].data().get("property.LastChange.InstanceID.@val"), Some(&"0".to_string()));
        assert_eq!(events[0].data().get("property.LastChange.InstanceID.TransportState.@val"), Some(&"PLAYING".to_string()));
        assert_eq!(events[0].data().get("property.LastChange.InstanceID.CurrentTrackURI.@val"), Some(&"x-sonos-spotify:track123".to_string()));
        assert_eq!(events[0].data().get("property.LastChange.InstanceID.CurrentTrackDuration.@val"), Some(&"0:04:32".to_string()));
        assert_eq!(events[0].data().get("property.LastChange.InstanceID.CurrentTrackMetaData.val.item.title"), Some(&"Test Song".to_string()));
        assert_eq!(events[0].data().get("property.LastChange.InstanceID.CurrentTrackMetaData.val.item.creator"), Some(&"Test Artist".to_string()));
    }
}