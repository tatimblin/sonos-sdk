//! XML parsing utilities for Sonos UPnP event processing.
//!
//! This module provides reusable XML parsing components that were consolidated
//! from the sonos-parser crate. It includes namespace stripping, attribute parsing,
//! and DIDL-Lite metadata structures.

use crate::{Result, ApiError};
use serde::de::{DeserializeOwned, Deserializer};
use serde::{Deserialize, Serialize};

/// Parse XML string into a deserializable type with namespace stripping.
///
/// This function handles the common case of parsing UPnP XML that contains
/// namespace prefixes. It strips namespace prefixes before parsing to allow
/// simpler serde struct definitions.
///
/// # Arguments
///
/// * `xml` - The XML string to parse
///
/// # Returns
///
/// The parsed value of type `T`, or an error if parsing fails.
pub fn parse<T: DeserializeOwned>(xml: &str) -> Result<T> {
    let stripped = strip_namespaces(xml);
    quick_xml::de::from_str(&stripped)
        .map_err(|e| ApiError::ParseError(format!("XML deserialization failed: {}", e)))
}

/// Strip namespace prefixes from XML content to simplify parsing.
///
/// UPnP XML often contains namespace prefixes like `e:`, `dc:`, `upnp:`, etc.
/// This function removes these prefixes to simplify parsing with serde.
///
/// # Example
///
/// Input: `<e:propertyset><dc:title>Song</dc:title></e:propertyset>`
/// Output: `<propertyset><title>Song</title></propertyset>`
pub fn strip_namespaces(xml: &str) -> String {
    let mut result = String::with_capacity(xml.len());
    let mut chars = xml.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '<' {
            result.push(c);

            // Check for closing tag or special tags
            let is_closing = chars.peek() == Some(&'/');
            if is_closing {
                result.push(chars.next().unwrap());
            }

            // Check for special tags (?, !)
            if let Some(&next) = chars.peek() {
                if next == '?' || next == '!' {
                    // Copy until '>'
                    while let Some(ch) = chars.next() {
                        result.push(ch);
                        if ch == '>' {
                            break;
                        }
                    }
                    continue;
                }
            }

            // Read the tag name (possibly with namespace prefix)
            let mut tag_name = String::new();
            while let Some(&ch) = chars.peek() {
                if ch.is_whitespace() || ch == '>' || ch == '/' {
                    break;
                }
                tag_name.push(chars.next().unwrap());
            }

            // Strip namespace prefix from tag name
            if let Some(pos) = tag_name.find(':') {
                result.push_str(&tag_name[pos + 1..]);
            } else {
                result.push_str(&tag_name);
            }

            // Process attributes
            while let Some(&ch) = chars.peek() {
                if ch == '>' {
                    result.push(chars.next().unwrap());
                    break;
                }
                if ch == '/' {
                    result.push(chars.next().unwrap());
                    continue;
                }
                if ch.is_whitespace() {
                    result.push(chars.next().unwrap());
                    continue;
                }

                // Read attribute name
                let mut attr_name = String::new();
                while let Some(&ach) = chars.peek() {
                    if ach == '=' || ach.is_whitespace() || ach == '>' || ach == '/' {
                        break;
                    }
                    attr_name.push(chars.next().unwrap());
                }

                // Strip namespace prefix from attribute name (but keep xmlns declarations)
                if attr_name.starts_with("xmlns") {
                    // Skip xmlns declarations entirely
                    // Skip '='
                    if chars.peek() == Some(&'=') {
                        chars.next();
                    }
                    // Skip quoted value
                    if let Some(&quote) = chars.peek() {
                        if quote == '"' || quote == '\'' {
                            chars.next();
                            while let Some(ch) = chars.next() {
                                if ch == quote {
                                    break;
                                }
                            }
                        }
                    }
                } else {
                    // Keep the attribute, stripping namespace prefix
                    if let Some(pos) = attr_name.find(':') {
                        result.push_str(&attr_name[pos + 1..]);
                    } else {
                        result.push_str(&attr_name);
                    }

                    // Copy '=' and value
                    while let Some(&ach) = chars.peek() {
                        if ach == '>' || ach == '/' {
                            break;
                        }
                        if ach == '"' || ach == '\'' {
                            let quote = chars.next().unwrap();
                            result.push(quote);
                            while let Some(ch) = chars.next() {
                                result.push(ch);
                                if ch == quote {
                                    break;
                                }
                            }
                            break;
                        }
                        result.push(chars.next().unwrap());
                    }
                }
            }
        } else {
            result.push(c);
        }
    }

    result
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

    #[test]
    fn test_strip_namespaces_basic() {
        let input = r#"<e:propertyset><e:property>test</e:property></e:propertyset>"#;
        let expected = r#"<propertyset><property>test</property></propertyset>"#;
        assert_eq!(strip_namespaces(input), expected);
    }

    #[test]
    fn test_strip_namespaces_with_attributes() {
        let input = r#"<dc:title id="1">Song</dc:title>"#;
        let expected = r#"<title id="1">Song</title>"#;
        assert_eq!(strip_namespaces(input), expected);
    }

    #[test]
    fn test_strip_namespaces_multiple() {
        let input = r#"<dc:title>Song</dc:title><upnp:album>Album</upnp:album>"#;
        let expected = r#"<title>Song</title><album>Album</album>"#;
        assert_eq!(strip_namespaces(input), expected);
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
        assert!(result.is_ok(), "Failed to parse DIDL-Lite: {:?}", result.err());

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
        assert!(result.is_ok(), "Failed to parse DIDL-Lite with resource: {:?}", result.err());

        let didl = result.unwrap();
        let item = &didl.items[0];
        assert_eq!(item.title, Some("Song".to_string()));
        assert_eq!(item.creator, Some("Artist".to_string()));

        let res = &item.resources[0];
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
        let item = &didl.items[0];
        assert_eq!(item.id, "1");
        assert_eq!(item.parent_id, "0");
        assert_eq!(item.title, None);
        assert_eq!(item.creator, None);
        assert_eq!(item.album, None);
    }
}