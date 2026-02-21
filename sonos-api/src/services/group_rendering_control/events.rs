//! GroupRenderingControl service event types and parsing
//!
//! Provides direct serde-based XML parsing with no business logic,
//! replicating exactly what Sonos produces for sonos-stream consumption.
//!
//! GroupRenderingControl uses a direct property structure (not LastChange-wrapped):
//! ```xml
//! <e:propertyset xmlns:e="urn:schemas-upnp-org:event-1-0">
//!   <e:property><GroupVolume>14</GroupVolume></e:property>
//!   <e:property><GroupMute>0</GroupMute></e:property>
//!   <e:property><GroupVolumeChangeable>1</GroupVolumeChangeable></e:property>
//! </e:propertyset>
//! ```

use serde::{Deserialize, Serialize};

use crate::events::xml_utils;
use crate::{ApiError, Result};

/// GroupRenderingControl event - direct serde mapping from UPnP event XML
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename = "propertyset")]
pub struct GroupRenderingControlEvent {
    #[serde(rename = "property", default)]
    properties: Vec<GroupRenderingControlProperty>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GroupRenderingControlProperty {
    #[serde(rename = "GroupVolume", default)]
    group_volume: Option<String>,

    #[serde(rename = "GroupMute", default)]
    group_mute: Option<String>,

    #[serde(rename = "GroupVolumeChangeable", default)]
    group_volume_changeable: Option<String>,
}

impl GroupRenderingControlEvent {
    /// Get the group volume level (0-100)
    pub fn group_volume(&self) -> Option<u16> {
        self.properties
            .iter()
            .find_map(|p| p.group_volume.as_ref())
            .and_then(|s| s.parse::<u16>().ok())
    }

    /// Get the group mute state
    pub fn group_mute(&self) -> Option<bool> {
        self.properties
            .iter()
            .find_map(|p| p.group_mute.as_ref())
            .map(|s| s == "1" || s.to_lowercase() == "true")
    }

    /// Get whether the group volume is changeable
    pub fn group_volume_changeable(&self) -> Option<bool> {
        self.properties
            .iter()
            .find_map(|p| p.group_volume_changeable.as_ref())
            .map(|s| s == "1" || s.to_lowercase() == "true")
    }

    /// Parse from UPnP event XML using serde
    pub fn from_xml(xml: &str) -> Result<Self> {
        let clean_xml = xml_utils::strip_namespaces(xml);
        quick_xml::de::from_str(&clean_xml).map_err(|e| {
            ApiError::ParseError(format!("Failed to parse GroupRenderingControl XML: {e}"))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_real_event_xml() {
        // Captured from a real Sonos Amp (Living Room)
        let xml = r#"<e:propertyset xmlns:e="urn:schemas-upnp-org:event-1-0"><e:property><GroupVolume>14</GroupVolume></e:property><e:property><GroupMute>0</GroupMute></e:property><e:property><GroupVolumeChangeable>1</GroupVolumeChangeable></e:property></e:propertyset>"#;

        let event = GroupRenderingControlEvent::from_xml(xml).unwrap();
        assert_eq!(event.group_volume(), Some(14));
        assert_eq!(event.group_mute(), Some(false));
        assert_eq!(event.group_volume_changeable(), Some(true));
    }

    #[test]
    fn test_parse_formatted_xml() {
        let xml = r#"<e:propertyset xmlns:e="urn:schemas-upnp-org:event-1-0">
            <e:property>
                <GroupVolume>75</GroupVolume>
            </e:property>
            <e:property>
                <GroupMute>1</GroupMute>
            </e:property>
            <e:property>
                <GroupVolumeChangeable>0</GroupVolumeChangeable>
            </e:property>
        </e:propertyset>"#;

        let event = GroupRenderingControlEvent::from_xml(xml).unwrap();
        assert_eq!(event.group_volume(), Some(75));
        assert_eq!(event.group_mute(), Some(true));
        assert_eq!(event.group_volume_changeable(), Some(false));
    }

    #[test]
    fn test_partial_properties() {
        let xml = r#"<e:propertyset xmlns:e="urn:schemas-upnp-org:event-1-0">
            <e:property>
                <GroupVolume>50</GroupVolume>
            </e:property>
        </e:propertyset>"#;

        let event = GroupRenderingControlEvent::from_xml(xml).unwrap();
        assert_eq!(event.group_volume(), Some(50));
        assert_eq!(event.group_mute(), None);
        assert_eq!(event.group_volume_changeable(), None);
    }

    #[test]
    fn test_volume_boundary_values() {
        let xml = r#"<e:propertyset xmlns:e="urn:schemas-upnp-org:event-1-0">
            <e:property><GroupVolume>0</GroupVolume></e:property>
        </e:propertyset>"#;
        let event = GroupRenderingControlEvent::from_xml(xml).unwrap();
        assert_eq!(event.group_volume(), Some(0));

        let xml = r#"<e:propertyset xmlns:e="urn:schemas-upnp-org:event-1-0">
            <e:property><GroupVolume>100</GroupVolume></e:property>
        </e:propertyset>"#;
        let event = GroupRenderingControlEvent::from_xml(xml).unwrap();
        assert_eq!(event.group_volume(), Some(100));
    }

    #[test]
    fn test_boolean_parsing() {
        let event = GroupRenderingControlEvent {
            properties: vec![GroupRenderingControlProperty {
                group_volume: None,
                group_mute: Some("1".to_string()),
                group_volume_changeable: Some("true".to_string()),
            }],
        };
        assert_eq!(event.group_mute(), Some(true));
        assert_eq!(event.group_volume_changeable(), Some(true));

        let event = GroupRenderingControlEvent {
            properties: vec![GroupRenderingControlProperty {
                group_volume: None,
                group_mute: Some("0".to_string()),
                group_volume_changeable: Some("false".to_string()),
            }],
        };
        assert_eq!(event.group_mute(), Some(false));
        assert_eq!(event.group_volume_changeable(), Some(false));
    }
}
