//! AVTransport event data types.
//!
//! This module contains the strongly-typed event data structures for AVTransport events.
//! These types provide a convenient interface for accessing parsed AVTransport event data
//! without needing to work with raw XML or generic key-value pairs.

use crate::event::EventData;
use crate::types::ServiceType;
use sonos_parser::services::av_transport::AVTransportParser;
use sonos_parser::common::DidlLite;

/// AVTransport event data with strongly-typed fields.
///
/// This struct provides a strongly-typed representation of AVTransport events,
/// extracting commonly used fields from the AVTransportParser for easier access.
/// It implements the EventData trait to work with the new typed event system.
///
/// # Examples
///
/// ```rust,ignore
/// use sonos_stream::{AVTransportEvent, TypedEvent};
///
/// // Create from parser
/// let parser = AVTransportParser::from_xml(xml_string)?;
/// let av_event = AVTransportEvent::from_parser(parser);
/// let typed_event = TypedEvent::new(Box::new(av_event));
///
/// // Access typed data
/// if let Some(av_data) = typed_event.downcast_ref::<AVTransportEvent>() {
///     println!("Transport state: {}", av_data.transport_state);
///     if let Some(uri) = &av_data.track_uri {
///         println!("Playing: {}", uri);
///     }
/// }
/// ```
#[derive(Debug, Clone)]
pub struct AVTransportEvent {
    /// Current transport state (PLAYING, PAUSED_PLAYBACK, STOPPED, TRANSITIONING)
    pub transport_state: String,
    
    /// URI of the current track, if present
    pub track_uri: Option<String>,
    
    /// DIDL-Lite metadata for the current track, if present
    pub track_metadata: Option<DidlLite>,
    
    /// Duration of the current track in HH:MM:SS format, if present
    pub current_track_duration: Option<String>,
    
    /// Current track number, if present
    pub current_track: Option<u32>,
    
    /// Total number of tracks in queue, if present
    pub number_of_tracks: Option<u32>,
    
    /// Current play mode (NORMAL, REPEAT_ALL, SHUFFLE, etc.), if present
    pub current_play_mode: Option<String>,
}

impl AVTransportEvent {
    /// Create a new AVTransportEvent from an AVTransportParser.
    ///
    /// This method extracts the commonly used fields from the parser and
    /// converts them to a more convenient strongly-typed format.
    ///
    /// # Arguments
    ///
    /// * `parser` - The AVTransportParser containing the parsed event data
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let parser = AVTransportParser::from_xml(xml_string)?;
    /// let event = AVTransportEvent::from_parser(parser);
    /// ```
    pub fn from_parser(parser: AVTransportParser) -> Self {
        Self {
            transport_state: parser.transport_state().to_string(),
            track_uri: parser.current_track_uri().map(|s| s.to_string()),
            track_metadata: parser.track_metadata().cloned(),
            current_track_duration: parser.current_track_duration().map(|s| s.to_string()),
            current_track: parser.property.last_change.instance.current_track
                .as_ref()
                .and_then(|v| v.val.parse().ok()),
            number_of_tracks: parser.property.last_change.instance.number_of_tracks
                .as_ref()
                .and_then(|v| v.val.parse().ok()),
            current_play_mode: parser.property.last_change.instance.current_play_mode
                .as_ref()
                .map(|v| v.val.clone()),
        }
    }
    
    /// Get the track title from metadata, if available.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// if let Some(title) = av_event.track_title() {
    ///     println!("Now playing: {}", title);
    /// }
    /// ```
    pub fn track_title(&self) -> Option<&str> {
        self.track_metadata
            .as_ref()
            .and_then(|d| d.item.title.as_deref())
    }
    
    /// Get the track artist from metadata, if available.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// if let Some(artist) = av_event.track_artist() {
    ///     println!("Artist: {}", artist);
    /// }
    /// ```
    pub fn track_artist(&self) -> Option<&str> {
        self.track_metadata
            .as_ref()
            .and_then(|d| d.item.creator.as_deref())
    }
    
    /// Get the track album from metadata, if available.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// if let Some(album) = av_event.track_album() {
    ///     println!("Album: {}", album);
    /// }
    /// ```
    pub fn track_album(&self) -> Option<&str> {
        self.track_metadata
            .as_ref()
            .and_then(|d| d.item.album.as_deref())
    }
    
    /// Parse the track duration to milliseconds, if available.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// if let Some(duration_ms) = av_event.track_duration_ms() {
    ///     println!("Duration: {}ms", duration_ms);
    /// }
    /// ```
    pub fn track_duration_ms(&self) -> Option<u64> {
        self.current_track_duration
            .as_ref()
            .and_then(|d| AVTransportParser::parse_duration_to_ms(d))
    }
}

impl EventData for AVTransportEvent {
    /// Get the event type identifier.
    ///
    /// Returns "av_transport_event" for all AVTransport events.
    fn event_type(&self) -> &str {
        "av_transport_event"
    }
    
    /// Get the service type that produced this event.
    ///
    /// Returns ServiceType::AVTransport for all AVTransport events.
    fn service_type(&self) -> ServiceType {
        ServiceType::AVTransport
    }
    
    /// Convert to Any for downcasting.
    ///
    /// This enables type-safe downcasting from TypedEvent back to AVTransportEvent.
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    
    /// Clone the event data for Clone support.
    ///
    /// Returns a boxed clone of this AVTransportEvent.
    fn clone_box(&self) -> Box<dyn EventData> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_av_transport_event_from_parser() {
        let xml = r#"<e:propertyset xmlns:e="urn:schemas-upnp-org:event-1-0"><e:property><LastChange>&lt;Event xmlns=&quot;urn:schemas-upnp-org:metadata-1-0/AVT/&quot;&gt;&lt;InstanceID val=&quot;0&quot;&gt;&lt;TransportState val=&quot;PLAYING&quot;/&gt;&lt;CurrentTrackURI val=&quot;x-sonos-spotify:track123&quot;/&gt;&lt;CurrentTrackDuration val=&quot;0:04:32&quot;/&gt;&lt;CurrentTrack val=&quot;5&quot;/&gt;&lt;NumberOfTracks val=&quot;12&quot;/&gt;&lt;CurrentPlayMode val=&quot;REPEAT_ALL&quot;/&gt;&lt;CurrentTrackMetaData val=&quot;&amp;lt;DIDL-Lite xmlns:dc=&amp;quot;http://purl.org/dc/elements/1.1/&amp;quot; xmlns:upnp=&amp;quot;urn:schemas-upnp-org:metadata-1-0/upnp/&amp;quot;&amp;gt;&amp;lt;item id=&amp;quot;-1&amp;quot; parentID=&amp;quot;-1&amp;quot;&amp;gt;&amp;lt;dc:title&amp;gt;Test Song&amp;lt;/dc:title&amp;gt;&amp;lt;dc:creator&amp;gt;Test Artist&amp;lt;/dc:creator&amp;gt;&amp;lt;upnp:album&amp;gt;Test Album&amp;lt;/upnp:album&amp;gt;&amp;lt;/item&amp;gt;&amp;lt;/DIDL-Lite&amp;gt;&quot;/&gt;&lt;/InstanceID&gt;&lt;/Event&gt;</LastChange></e:property></e:propertyset>"#;
        
        let parser = AVTransportParser::from_xml(xml).unwrap();
        let av_event = AVTransportEvent::from_parser(parser);

        assert_eq!(av_event.transport_state, "PLAYING");
        assert_eq!(av_event.track_uri, Some("x-sonos-spotify:track123".to_string()));
        assert_eq!(av_event.current_track_duration, Some("0:04:32".to_string()));
        assert_eq!(av_event.current_track, Some(5));
        assert_eq!(av_event.number_of_tracks, Some(12));
        assert_eq!(av_event.current_play_mode, Some("REPEAT_ALL".to_string()));
        
        // Test metadata access
        assert!(av_event.track_metadata.is_some());
        assert_eq!(av_event.track_title(), Some("Test Song"));
        assert_eq!(av_event.track_artist(), Some("Test Artist"));
        assert_eq!(av_event.track_album(), Some("Test Album"));
        
        // Test duration parsing
        assert_eq!(av_event.track_duration_ms(), Some(272000));
    }

    #[test]
    fn test_av_transport_event_minimal() {
        let xml = r#"<e:propertyset xmlns:e="urn:schemas-upnp-org:event-1-0"><e:property><LastChange>&lt;Event xmlns=&quot;urn:schemas-upnp-org:metadata-1-0/AVT/&quot;&gt;&lt;InstanceID val=&quot;0&quot;&gt;&lt;TransportState val=&quot;STOPPED&quot;/&gt;&lt;/InstanceID&gt;&lt;/Event&gt;</LastChange></e:property></e:propertyset>"#;
        
        let parser = AVTransportParser::from_xml(xml).unwrap();
        let av_event = AVTransportEvent::from_parser(parser);

        assert_eq!(av_event.transport_state, "STOPPED");
        assert_eq!(av_event.track_uri, None);
        assert_eq!(av_event.current_track_duration, None);
        assert_eq!(av_event.current_track, None);
        assert_eq!(av_event.number_of_tracks, None);
        assert_eq!(av_event.current_play_mode, None);
        assert!(av_event.track_metadata.is_none());
        
        // Test metadata access with no metadata
        assert_eq!(av_event.track_title(), None);
        assert_eq!(av_event.track_artist(), None);
        assert_eq!(av_event.track_album(), None);
        assert_eq!(av_event.track_duration_ms(), None);
    }

    #[test]
    fn test_av_transport_event_data_trait() {
        let xml = r#"<e:propertyset xmlns:e="urn:schemas-upnp-org:event-1-0"><e:property><LastChange>&lt;Event xmlns=&quot;urn:schemas-upnp-org:metadata-1-0/AVT/&quot;&gt;&lt;InstanceID val=&quot;0&quot;&gt;&lt;TransportState val=&quot;PAUSED_PLAYBACK&quot;/&gt;&lt;/InstanceID&gt;&lt;/Event&gt;</LastChange></e:property></e:propertyset>"#;
        
        let parser = AVTransportParser::from_xml(xml).unwrap();
        let av_event = AVTransportEvent::from_parser(parser);

        // Test EventData trait methods
        assert_eq!(av_event.event_type(), "av_transport_event");
        assert_eq!(av_event.service_type(), ServiceType::AVTransport);

        // Test as_any method
        let any_ref = av_event.as_any();
        let downcast_result = any_ref.downcast_ref::<AVTransportEvent>();
        assert!(downcast_result.is_some());

        // Test clone_box method
        let cloned_box = av_event.clone_box();
        assert_eq!(cloned_box.event_type(), "av_transport_event");
        assert_eq!(cloned_box.service_type(), ServiceType::AVTransport);
    }
}