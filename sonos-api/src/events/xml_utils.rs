//! XML parsing utilities for Sonos UPnP event processing.
//!
//! This module provides reusable XML parsing components that were consolidated
//! from the sonos-parser crate. It includes attribute parsing and DIDL-Lite
//! metadata structures.

use crate::{ApiError, Result};
use serde::de::{DeserializeOwned, Deserializer};
use serde::{Deserialize, Serialize};

/// Parse XML string into a deserializable type.
///
/// UPnP XML is heavily namespaced (`e:property`, `dc:title`, `upnp:album`), but
/// quick-xml's serde deserializer matches on *local* names only, so struct fields
/// and `#[serde(rename = "...")]` values are written without prefixes and no
/// preprocessing is required.
///
/// # Arguments
///
/// * `xml` - The XML string to parse
///
/// # Returns
///
/// The parsed value of type `T`, or an error if parsing fails.
pub fn parse<T: DeserializeOwned>(xml: &str) -> Result<T> {
    quick_xml::de::from_str(xml)
        .map_err(|e| ApiError::ParseError(format!("XML deserialization failed: {e}")))
}

/// Custom deserializer for nested XML content.
///
/// This deserializer handles elements where the text content is XML-escaped
/// and needs to be parsed into a structured type. Used with serde's
/// `deserialize_with` attribute.
///
/// # Example
///
/// ```rust,ignore
/// #[derive(Deserialize)]
/// struct Property {
///     #[serde(deserialize_with = "deserialize_nested")]
///     last_change: LastChangeEvent,
/// }
/// ```
pub fn deserialize_nested<'de, D, T>(deserializer: D) -> std::result::Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned,
{
    let s = String::deserialize(deserializer)?;
    parse::<T>(&s).map_err(serde::de::Error::custom)
}

/// Deserialize ZoneGroupState from nested XML string.
///
/// Similar to `deserialize_nested` but specifically for ZoneGroupState XML content
/// that comes nested within the event XML structure.
pub fn deserialize_zone_group_state<'de, D, T>(
    deserializer: D,
) -> std::result::Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned,
{
    let s = String::deserialize(deserializer)?;
    if s.trim().is_empty() {
        return Ok(None);
    }
    let parsed = parse::<T>(&s).map_err(serde::de::Error::custom)?;
    Ok(Some(parsed))
}

/// Represents an XML element with a `val` attribute.
///
/// Many UPnP state variables are represented as empty elements with a `val` attribute:
/// ```xml
/// <TransportState val="PLAYING"/>
/// <CurrentTrackDuration val="0:03:57"/>
/// ```
///
/// This struct captures that pattern for easy deserialization.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct ValueAttribute {
    /// The value from the `val` attribute
    #[serde(rename = "@val", default)]
    pub val: String,
}

/// Represents an XML element with a `val` attribute containing nested XML.
///
/// Some UPnP elements contain XML-escaped content in their `val` attribute that
/// should be parsed into a structured type. For example, `CurrentTrackMetaData`
/// contains escaped DIDL-Lite XML.
///
/// This struct automatically deserializes the escaped XML content into the
/// specified type `T`.
#[derive(Debug, Clone, Default, Serialize)]
pub struct NestedAttribute<T> {
    /// The parsed value from the nested XML, or None if empty/unparseable
    pub val: Option<T>,
}

impl<'de, T: DeserializeOwned> Deserialize<'de> for NestedAttribute<T> {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawAttr {
            #[serde(rename = "@val", default)]
            val: String,
        }

        let raw = RawAttr::deserialize(deserializer)?;

        if raw.val.is_empty() {
            return Ok(NestedAttribute { val: None });
        }

        // Try to parse the nested XML
        match parse::<T>(&raw.val) {
            Ok(parsed) => Ok(NestedAttribute { val: Some(parsed) }),
            Err(_) => Ok(NestedAttribute { val: None }),
        }
    }
}

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
    /// The item elements containing track metadata
    #[serde(rename = "item", default)]
    pub items: Vec<DidlItem>,
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
    pub fn from_xml(xml: &str) -> Result<Self> {
        parse(xml)
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

    /// Resource elements with URI and duration
    #[serde(rename = "res", default)]
    pub resources: Vec<DidlResource>,

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

    /// Comments and CDATA containing `>` used to be truncated at that `>` by the
    /// hand-rolled namespace stripper, corrupting the document.
    #[test]
    fn test_parse_survives_cdata_and_comment_containing_gt() {
        let xml = r#"<e:propertyset xmlns:e="urn:schemas-upnp-org:event-1-0"><!-- a > b --><e:property><TransportState val="PLAYING"/><Note><![CDATA[3 > 2 && <ok>]]></Note></e:property></e:propertyset>"#;

        #[derive(Debug, Deserialize)]
        struct PropertySet {
            property: Property,
        }
        #[derive(Debug, Deserialize)]
        struct Property {
            #[serde(rename = "TransportState")]
            transport_state: ValueAttribute,
            #[serde(rename = "Note")]
            note: String,
        }

        let parsed: PropertySet = parse(xml).unwrap();
        assert_eq!(parsed.property.transport_state.val, "PLAYING");
        assert_eq!(parsed.property.note, "3 > 2 && <ok>");
    }

    #[test]
    fn test_value_attribute_deserialize() {
        let xml = r#"<Root><TransportState val="PLAYING"/></Root>"#;

        #[derive(Debug, Deserialize)]
        struct Root {
            #[serde(rename = "TransportState")]
            transport_state: ValueAttribute,
        }

        let result: Root = parse(xml).unwrap();
        assert_eq!(result.transport_state.val, "PLAYING");
    }

    #[test]
    fn test_value_attribute_empty() {
        let xml = r#"<Root><TransportState val=""/></Root>"#;

        #[derive(Debug, Deserialize)]
        struct Root {
            #[serde(rename = "TransportState")]
            transport_state: ValueAttribute,
        }

        let result: Root = parse(xml).unwrap();
        assert_eq!(result.transport_state.val, "");
    }

    #[test]
    fn test_value_attribute_default() {
        let xml = r#"<Root><TransportState/></Root>"#;

        #[derive(Debug, Deserialize)]
        struct Root {
            #[serde(rename = "TransportState")]
            transport_state: ValueAttribute,
        }

        let result: Root = parse(xml).unwrap();
        assert_eq!(result.transport_state.val, "");
    }

    #[test]
    fn test_parse_didl_lite_basic() {
        let didl_xml = r#"<DIDL-Lite xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:upnp="urn:schemas-upnp-org:metadata-1-0/upnp/"><item id="-1" parentID="-1"><dc:title>Test Song</dc:title><dc:creator>Test Artist</dc:creator><upnp:album>Test Album</upnp:album></item></DIDL-Lite>"#;

        let result = DidlLite::from_xml(didl_xml);
        assert!(
            result.is_ok(),
            "Failed to parse DIDL-Lite: {:?}",
            result.err()
        );

        let didl = result.unwrap();
        assert_eq!(didl.items.len(), 1);
        let item = &didl.items[0];
        assert_eq!(item.id, "-1");
        assert_eq!(item.parent_id, "-1");
        assert_eq!(item.title, Some("Test Song".to_string()));
        assert_eq!(item.creator, Some("Test Artist".to_string()));
        assert_eq!(item.album, Some("Test Album".to_string()));
    }

    #[test]
    fn test_parse_didl_lite_with_resource() {
        let didl_xml = r#"<DIDL-Lite xmlns:dc="http://purl.org/dc/elements/1.1/"><item id="-1" parentID="-1"><dc:title>Song</dc:title><dc:creator>Artist</dc:creator><res duration="0:03:58" protocolInfo="http-get:*:audio/mpeg:*">http://example.com/song.mp3</res></item></DIDL-Lite>"#;

        let result = DidlLite::from_xml(didl_xml);
        assert!(
            result.is_ok(),
            "Failed to parse DIDL-Lite with resource: {:?}",
            result.err()
        );

        let didl = result.unwrap();
        let item = &didl.items[0];
        assert_eq!(item.title, Some("Song".to_string()));
        assert_eq!(item.creator, Some("Artist".to_string()));

        let res = &item.resources[0];
        assert_eq!(res.duration, Some("0:03:58".to_string()));
        assert_eq!(
            res.protocol_info,
            Some("http-get:*:audio/mpeg:*".to_string())
        );
        assert_eq!(res.uri, Some("http://example.com/song.mp3".to_string()));
    }

    #[test]
    fn test_parse_didl_lite_minimal() {
        let didl_xml = r#"<DIDL-Lite><item id="1" parentID="0"></item></DIDL-Lite>"#;

        let result = DidlLite::from_xml(didl_xml);
        assert!(
            result.is_ok(),
            "Failed to parse minimal DIDL-Lite: {:?}",
            result.err()
        );

        let didl = result.unwrap();
        let item = &didl.items[0];
        assert_eq!(item.id, "1");
        assert_eq!(item.parent_id, "0");
        assert_eq!(item.title, None);
        assert_eq!(item.creator, None);
        assert_eq!(item.album, None);
    }
}
