//! XML decoding utilities for UPnP event parsing.
//!
//! This module provides custom serde deserializers for handling the complex
//! nested and escaped XML structures found in UPnP events. Sonos devices
//! send events with XML-escaped content that needs to be decoded and parsed
//! in multiple stages.

use crate::error::{ParseError, ParseResult};
use serde::de::{DeserializeOwned, Deserializer};
use serde::Deserialize;

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
pub fn parse<T: DeserializeOwned>(xml: &str) -> ParseResult<T> {
    let stripped = strip_namespaces(xml);
    quick_xml::de::from_str(&stripped)
        .map_err(|e| ParseError::XmlDeserializationFailed(e.to_string()))
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
pub fn deserialize_nested<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned,
{
    let s = String::deserialize(deserializer)?;
    parse::<T>(&s).map_err(serde::de::Error::custom)
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
    fn test_parse_with_namespaces() {
        let xml = r#"<e:propertyset xmlns:e="urn:test"><e:property><Value val="test"/></e:property></e:propertyset>"#;
        
        #[derive(Debug, Deserialize)]
        struct PropertySet {
            property: Property,
        }
        
        #[derive(Debug, Deserialize)]
        struct Property {
            #[serde(rename = "Value")]
            value: crate::common::attributes::ValueAttribute,
        }
        
        let result: PropertySet = parse(xml).unwrap();
        assert_eq!(result.property.value.val, "test");
    }
}