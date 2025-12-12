//! DIDL-Lite common structures for media metadata

use serde::{Deserialize, Serialize};
use crate::error::ParseResult;
use crate::common::xml_decode;

/// DIDL-Lite root structure for media metadata.
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
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename = "DIDL-Lite")]
pub struct DidlLite {
    /// The item element containing track metadata
    #[serde(rename = "item")]
    pub item: DidlItem,
}

impl DidlLite {
    /// Parse DIDL-Lite XML content directly.
    ///
    /// # Arguments
    ///
    /// * `xml` - The raw DIDL-Lite XML string
    ///
    /// # Returns
    ///
    /// The parsed DIDL-Lite structure, or an error if parsing fails.
    pub fn from_xml(xml: &str) -> ParseResult<Self> {
        xml_decode::parse(xml).map_err(|e| crate::error::ParseError::XmlDeserializationFailed(e.to_string()))
    }
}

/// Individual item in DIDL-Lite metadata containing track information.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
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

/// Resource element in DIDL-Lite containing media resource information.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, Default)]
pub struct DidlResource {
    /// Duration in HH:MM:SS format
    #[serde(rename = "@duration", default)]
    pub duration: Option<String>,

    /// Protocol info for the resource
    #[serde(rename = "@protocolInfo", default)]
    pub protocol_info: Option<String>,

    /// The resource URI
    #[serde(rename = "$value", default)]
    pub uri: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_didl_lite_basic() {
        let didl_xml = r#"<DIDL-Lite xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:upnp="urn:schemas-upnp-org:metadata-1-0/upnp/"><item id="-1" parentID="-1"><dc:title>Test Song</dc:title><dc:creator>Test Artist</dc:creator><upnp:album>Test Album</upnp:album></item></DIDL-Lite>"#;
        
        let result = DidlLite::from_xml(didl_xml);
        assert!(result.is_ok(), "Failed to parse DIDL-Lite: {:?}", result.err());
        
        let didl = result.unwrap();
        assert_eq!(didl.item.id, "-1");
        assert_eq!(didl.item.parent_id, "-1");
        assert_eq!(didl.item.title, Some("Test Song".to_string()));
        assert_eq!(didl.item.creator, Some("Test Artist".to_string()));
        assert_eq!(didl.item.album, Some("Test Album".to_string()));
    }

    #[test]
    fn test_parse_didl_lite_with_resource() {
        let didl_xml = r#"<DIDL-Lite xmlns:dc="http://purl.org/dc/elements/1.1/"><item id="-1" parentID="-1"><dc:title>Song</dc:title><dc:creator>Artist</dc:creator><res duration="0:03:58" protocolInfo="http-get:*:audio/mpeg:*">http://example.com/song.mp3</res></item></DIDL-Lite>"#;
        
        let result = DidlLite::from_xml(didl_xml);
        assert!(result.is_ok(), "Failed to parse DIDL-Lite with resource: {:?}", result.err());
        
        let didl = result.unwrap();
        assert_eq!(didl.item.title, Some("Song".to_string()));
        assert_eq!(didl.item.creator, Some("Artist".to_string()));
        
        let res = didl.item.res.as_ref().unwrap();
        assert_eq!(res.duration, Some("0:03:58".to_string()));
        assert_eq!(res.protocol_info, Some("http-get:*:audio/mpeg:*".to_string()));
        assert_eq!(res.uri, Some("http://example.com/song.mp3".to_string()));
    }

    #[test]
    fn test_parse_didl_lite_minimal() {
        let didl_xml = r#"<DIDL-Lite><item id="1" parentID="0"></item></DIDL-Lite>"#;
        
        let result = DidlLite::from_xml(didl_xml);
        assert!(result.is_ok(), "Failed to parse minimal DIDL-Lite: {:?}", result.err());
        
        let didl = result.unwrap();
        assert_eq!(didl.item.id, "1");
        assert_eq!(didl.item.parent_id, "0");
        assert_eq!(didl.item.title, None);
        assert_eq!(didl.item.creator, None);
        assert_eq!(didl.item.album, None);
    }

    #[test]
    fn test_parse_didl_lite_invalid_xml() {
        let invalid_xml = r#"<invalid>not didl-lite</invalid>"#;
        
        let result = DidlLite::from_xml(invalid_xml);
        assert!(result.is_err(), "Should fail to parse invalid XML");
    }
}