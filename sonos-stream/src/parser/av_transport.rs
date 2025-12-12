//! AVTransport event parser using serde.
//!
//! This module provides serde-based parsing for AVTransport UPnP events.
//! It handles the complex nested XML structure including escaped DIDL-Lite metadata.

use serde::{Deserialize, Serialize};

use super::xml_decode::{deserialize_nested, NestedAttribute, ValueAttribute};

/// Root parser for AVTransport UPnP events.
///
/// UPnP events are wrapped in a propertyset structure:
/// ```xml
/// <e:propertyset xmlns:e="urn:schemas-upnp-org:event-1-0">
///   <e:property>
///     <LastChange>...</LastChange>
///   </e:property>
/// </e:propertyset>
/// ```
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename = "propertyset")]
pub struct AVTransportParser {
    /// The property element containing LastChange
    #[serde(rename = "property")]
    pub property: Property,
}

/// Property wrapper containing the LastChange element.
#[derive(Debug, Deserialize, Serialize)]
pub struct Property {
    /// The LastChange element with nested XML content
    #[serde(
        rename = "LastChange",
        deserialize_with = "deserialize_nested"
    )]
    pub last_change: LastChangeEvent,
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
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename = "Event")]
pub struct LastChangeEvent {
    /// The instance containing all state variables
    #[serde(rename = "InstanceID")]
    pub instance: InstanceID,
}

/// Instance containing AVTransport state variables.
#[derive(Debug, Deserialize, Serialize)]
pub struct InstanceID {
    /// Instance ID (usually "0")
    #[serde(rename = "@val")]
    pub id: String,

    /// Current transport state (PLAYING, PAUSED_PLAYBACK, STOPPED, TRANSITIONING)
    #[serde(rename = "TransportState")]
    pub transport_state: ValueAttribute,

    /// Current play mode (NORMAL, REPEAT_ALL, SHUFFLE, etc.)
    #[serde(rename = "CurrentPlayMode", default)]
    pub current_play_mode: Option<ValueAttribute>,

    /// Crossfade mode (0 or 1)
    #[serde(rename = "CurrentCrossfadeMode", default)]
    pub current_crossfade_mode: Option<ValueAttribute>,

    /// Number of tracks in queue
    #[serde(rename = "NumberOfTracks", default)]
    pub number_of_tracks: Option<ValueAttribute>,

    /// Current track number
    #[serde(rename = "CurrentTrack", default)]
    pub current_track: Option<ValueAttribute>,

    /// Current section
    #[serde(rename = "CurrentSection", default)]
    pub current_section: Option<ValueAttribute>,

    /// URI of the current track
    #[serde(rename = "CurrentTrackURI", default)]
    pub current_track_uri: Option<ValueAttribute>,

    /// Duration of the current track (HH:MM:SS format)
    #[serde(rename = "CurrentTrackDuration", default)]
    pub current_track_duration: Option<ValueAttribute>,

    /// DIDL-Lite metadata for the current track
    #[serde(rename = "CurrentTrackMetaData", default)]
    pub current_track_metadata: Option<NestedAttribute<DidlLite>>,

    /// URI of the next track
    #[serde(rename = "NextTrackURI", default)]
    pub next_track_uri: Option<ValueAttribute>,

    /// Metadata for the next track
    #[serde(rename = "NextTrackMetaData", default)]
    pub next_track_metadata: Option<ValueAttribute>,

    /// Enqueued transport URI
    #[serde(rename = "EnqueuedTransportURI", default)]
    pub enqueued_transport_uri: Option<ValueAttribute>,

    /// Enqueued transport URI metadata
    #[serde(rename = "EnqueuedTransportURIMetaData", default)]
    pub enqueued_transport_uri_metadata: Option<ValueAttribute>,

    /// Transport status
    #[serde(rename = "TransportStatus", default)]
    pub transport_status: Option<ValueAttribute>,

    /// Available transport actions
    #[serde(rename = "CurrentTransportActions", default)]
    pub current_transport_actions: Option<ValueAttribute>,

    /// AVTransport URI
    #[serde(rename = "AVTransportURI", default)]
    pub av_transport_uri: Option<ValueAttribute>,

    /// AVTransport URI metadata
    #[serde(rename = "AVTransportURIMetaData", default)]
    pub av_transport_uri_metadata: Option<ValueAttribute>,

    /// Relative time position
    #[serde(rename = "RelativeTimePosition", default)]
    pub relative_time_position: Option<ValueAttribute>,

    /// Absolute time position
    #[serde(rename = "AbsoluteTimePosition", default)]
    pub absolute_time_position: Option<ValueAttribute>,
}

/// DIDL-Lite metadata structure for track information.
///
/// DIDL-Lite format example:
/// ```xml
/// <DIDL-Lite xmlns:dc="http://purl.org/dc/elements/1.1/" ...>
///   <item id="-1" parentID="-1">
///     <dc:title>Song Title</dc:title>
///     <dc:creator>Artist Name</dc:creator>
///     <upnp:album>Album Name</upnp:album>
///     <res duration="0:03:58">uri</res>
///   </item>
/// </DIDL-Lite>
/// ```
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename = "DIDL-Lite")]
pub struct DidlLite {
    /// The item element containing track metadata
    #[serde(rename = "item")]
    pub item: DidlItem,
}

/// Individual item in DIDL-Lite metadata.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DidlItem {
    /// Item ID
    #[serde(rename = "@id", default)]
    pub id: String,

    /// Parent ID
    #[serde(rename = "@parentID", default)]
    pub parent_id: String,

    /// Whether the item is restricted
    #[serde(rename = "@restricted", default)]
    pub restricted: Option<String>,

    /// Resource element with URI and duration
    #[serde(rename = "res", default)]
    pub res: Option<DidlResource>,

    /// Album art URI
    #[serde(rename = "albumArtURI", default)]
    pub album_art_uri: Option<String>,

    /// Item class (e.g., object.item.audioItem.musicTrack)
    #[serde(rename = "class", default)]
    pub class: Option<String>,

    /// Track title
    #[serde(rename = "title", default)]
    pub title: Option<String>,

    /// Track creator/artist
    #[serde(rename = "creator", default)]
    pub creator: Option<String>,

    /// Album name
    #[serde(rename = "album", default)]
    pub album: Option<String>,

    /// Stream info
    #[serde(rename = "streamInfo", default)]
    pub stream_info: Option<String>,
}

/// Resource element in DIDL-Lite.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct DidlResource {
    /// Duration in HH:MM:SS format
    #[serde(rename = "@duration", default)]
    pub duration: Option<String>,

    /// The resource URI
    #[serde(rename = "$value", default)]
    pub uri: Option<String>,
}

impl AVTransportParser {
    /// Parse AVTransport event XML.
    ///
    /// # Arguments
    ///
    /// * `xml` - The raw UPnP event XML
    ///
    /// # Returns
    ///
    /// The parsed AVTransport event, or an error if parsing fails.
    pub fn from_xml(xml: &str) -> Result<Self, quick_xml::DeError> {
        super::xml_decode::parse(xml)
    }

    /// Get the transport state from the parsed event.
    pub fn transport_state(&self) -> &str {
        &self.property.last_change.instance.transport_state.val
    }

    /// Get the current track URI if present.
    pub fn current_track_uri(&self) -> Option<&str> {
        self.property
            .last_change
            .instance
            .current_track_uri
            .as_ref()
            .map(|v| v.val.as_str())
            .filter(|s| !s.is_empty())
    }

    /// Get the current track duration if present.
    pub fn current_track_duration(&self) -> Option<&str> {
        self.property
            .last_change
            .instance
            .current_track_duration
            .as_ref()
            .map(|v| v.val.as_str())
            .filter(|s| !s.is_empty())
    }

    /// Get the DIDL-Lite metadata if present.
    pub fn track_metadata(&self) -> Option<&DidlLite> {
        self.property
            .last_change
            .instance
            .current_track_metadata
            .as_ref()
            .and_then(|n| n.val.as_ref())
    }

    /// Get the track title from metadata.
    pub fn track_title(&self) -> Option<&str> {
        self.track_metadata()
            .and_then(|d| d.item.title.as_deref())
    }

    /// Get the track artist from metadata.
    pub fn track_artist(&self) -> Option<&str> {
        self.track_metadata()
            .and_then(|d| d.item.creator.as_deref())
    }

    /// Get the track album from metadata.
    pub fn track_album(&self) -> Option<&str> {
        self.track_metadata()
            .and_then(|d| d.item.album.as_deref())
    }

    /// Parse duration string (HH:MM:SS or H:MM:SS) to milliseconds.
    pub fn parse_duration_to_ms(duration: &str) -> Option<u64> {
        let parts: Vec<&str> = duration.split(':').collect();
        
        match parts.len() {
            3 => {
                let hours: u64 = parts[0].parse().ok()?;
                let minutes: u64 = parts[1].parse().ok()?;
                let seconds: f64 = parts[2].parse().ok()?;
                Some((hours * 3600 + minutes * 60) * 1000 + (seconds * 1000.0) as u64)
            }
            2 => {
                let minutes: u64 = parts[0].parse().ok()?;
                let seconds: f64 = parts[1].parse().ok()?;
                Some((minutes * 60) * 1000 + (seconds * 1000.0) as u64)
            }
            _ => None,
        }
    }

    /// Get the track duration in milliseconds.
    pub fn track_duration_ms(&self) -> Option<u64> {
        self.current_track_duration()
            .and_then(Self::parse_duration_to_ms)
    }
}

impl LastChangeEvent {
    /// Parse LastChange XML directly.
    pub fn from_xml(xml: &str) -> Result<Self, quick_xml::DeError> {
        super::xml_decode::parse(xml)
    }
}

impl DidlLite {
    /// Parse DIDL-Lite XML directly.
    pub fn from_xml(xml: &str) -> Result<Self, quick_xml::DeError> {
        super::xml_decode::parse(xml)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_XML: &str = r#"<e:propertyset xmlns:e="urn:schemas-upnp-org:event-1-0"><e:property><LastChange>&lt;Event xmlns=&quot;urn:schemas-upnp-org:metadata-1-0/AVT/&quot;&gt;&lt;InstanceID val=&quot;0&quot;&gt;&lt;TransportState val=&quot;PAUSED_PLAYBACK&quot;/&gt;&lt;CurrentPlayMode val=&quot;REPEAT_ALL&quot;/&gt;&lt;CurrentTrackURI val=&quot;x-sonos-spotify:spotify:track:123&quot;/&gt;&lt;CurrentTrackDuration val=&quot;0:03:57&quot;/&gt;&lt;CurrentTrackMetaData val=&quot;&amp;lt;DIDL-Lite xmlns:dc=&amp;quot;http://purl.org/dc/elements/1.1/&amp;quot;&amp;gt;&amp;lt;item id=&amp;quot;-1&amp;quot; parentID=&amp;quot;-1&amp;quot;&amp;gt;&amp;lt;dc:title&amp;gt;Test Song&amp;lt;/dc:title&amp;gt;&amp;lt;dc:creator&amp;gt;Test Artist&amp;lt;/dc:creator&amp;gt;&amp;lt;upnp:album&amp;gt;Test Album&amp;lt;/upnp:album&amp;gt;&amp;lt;/item&amp;gt;&amp;lt;/DIDL-Lite&amp;gt;&quot;/&gt;&lt;/InstanceID&gt;&lt;/Event&gt;</LastChange></e:property></e:propertyset>"#;

    #[test]
    fn test_parse_av_transport_xml() {
        let result = AVTransportParser::from_xml(SAMPLE_XML);
        assert!(result.is_ok(), "Failed to parse: {:?}", result.err());

        let parsed = result.unwrap();
        assert_eq!(parsed.transport_state(), "PAUSED_PLAYBACK");
        assert_eq!(parsed.current_track_uri(), Some("x-sonos-spotify:spotify:track:123"));
        assert_eq!(parsed.current_track_duration(), Some("0:03:57"));
    }

    #[test]
    fn test_parse_track_metadata() {
        let result = AVTransportParser::from_xml(SAMPLE_XML).unwrap();
        
        assert_eq!(result.track_title(), Some("Test Song"));
        assert_eq!(result.track_artist(), Some("Test Artist"));
        assert_eq!(result.track_album(), Some("Test Album"));
    }

    #[test]
    fn test_parse_duration_to_ms() {
        assert_eq!(AVTransportParser::parse_duration_to_ms("0:03:57"), Some(237000));
        assert_eq!(AVTransportParser::parse_duration_to_ms("1:00:00"), Some(3600000));
        assert_eq!(AVTransportParser::parse_duration_to_ms("0:00:30"), Some(30000));
        assert_eq!(AVTransportParser::parse_duration_to_ms("invalid"), None);
    }

    #[test]
    fn test_track_duration_ms() {
        let result = AVTransportParser::from_xml(SAMPLE_XML).unwrap();
        assert_eq!(result.track_duration_ms(), Some(237000));
    }

    #[test]
    fn test_parse_didl_lite_directly() {
        let didl_xml = r#"<DIDL-Lite xmlns:dc="http://purl.org/dc/elements/1.1/"><item id="-1" parentID="-1"><dc:title>Song</dc:title><dc:creator>Artist</dc:creator></item></DIDL-Lite>"#;
        
        let result = DidlLite::from_xml(didl_xml);
        assert!(result.is_ok());
        
        let didl = result.unwrap();
        assert_eq!(didl.item.title, Some("Song".to_string()));
        assert_eq!(didl.item.creator, Some("Artist".to_string()));
    }
}
