//! Common attribute helper types for UPnP XML patterns.
//!
//! This module provides helper types for handling common UPnP XML patterns
//! where data is stored in attributes rather than element content.

use crate::common::xml_decode::parse;
use serde::de::{DeserializeOwned, Deserializer};
use serde::{Deserialize, Serialize};

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
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::xml_decode::parse;

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
}