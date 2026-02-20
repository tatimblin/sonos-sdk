//! GroupManagement service event types and parsing
//!
//! Provides direct serde-based XML parsing with no business logic,
//! replicating exactly what Sonos produces for sonos-stream consumption.

use serde::{Deserialize, Serialize};
use std::net::IpAddr;

use crate::events::{xml_utils, EnrichedEvent, EventParser, EventSource};
use crate::{ApiError, Result, Service};

/// GroupManagement event - direct serde mapping from UPnP event XML
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename = "propertyset")]
pub struct GroupManagementEvent {
    /// Multiple property elements can exist in a single event
    #[serde(rename = "property", default)]
    properties: Vec<GroupManagementProperty>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GroupManagementProperty {
    #[serde(rename = "GroupCoordinatorIsLocal", default)]
    group_coordinator_is_local: Option<String>,

    #[serde(rename = "LocalGroupUUID", default)]
    local_group_uuid: Option<String>,

    #[serde(rename = "ResetVolumeAfter", default)]
    reset_volume_after: Option<String>,

    #[serde(rename = "VirtualLineInGroupID", default)]
    virtual_line_in_group_id: Option<String>,

    #[serde(rename = "VolumeAVTransportURI", default)]
    volume_av_transport_uri: Option<String>,
}

impl GroupManagementEvent {
    /// Get whether this speaker is the group coordinator
    ///
    /// Returns `true` if the value is "1" or "true" (case-insensitive)
    pub fn group_coordinator_is_local(&self) -> Option<bool> {
        self.properties
            .iter()
            .find_map(|p| p.group_coordinator_is_local.as_ref())
            .map(|s| s == "1" || s.to_lowercase() == "true")
    }

    /// Get the local group UUID
    pub fn local_group_uuid(&self) -> Option<String> {
        self.properties
            .iter()
            .find_map(|p| p.local_group_uuid.clone())
    }

    /// Get whether to reset volume after group changes
    ///
    /// Returns `true` if the value is "1" or "true" (case-insensitive)
    pub fn reset_volume_after(&self) -> Option<bool> {
        self.properties
            .iter()
            .find_map(|p| p.reset_volume_after.as_ref())
            .map(|s| s == "1" || s.to_lowercase() == "true")
    }

    /// Get the virtual line-in group ID
    pub fn virtual_line_in_group_id(&self) -> Option<String> {
        self.properties
            .iter()
            .find_map(|p| p.virtual_line_in_group_id.clone())
    }

    /// Get the volume AV transport URI
    pub fn volume_av_transport_uri(&self) -> Option<String> {
        self.properties
            .iter()
            .find_map(|p| p.volume_av_transport_uri.clone())
    }

    /// Parse from UPnP event XML using serde
    pub fn from_xml(xml: &str) -> Result<Self> {
        let clean_xml = xml_utils::strip_namespaces(xml);
        quick_xml::de::from_str(&clean_xml).map_err(|e| {
            ApiError::ParseError(format!("Failed to parse GroupManagement XML: {}", e))
        })
    }
}

/// Parser implementation for GroupManagement events
pub struct GroupManagementEventParser;

impl EventParser for GroupManagementEventParser {
    type EventData = GroupManagementEvent;

    fn parse_upnp_event(&self, xml: &str) -> Result<Self::EventData> {
        GroupManagementEvent::from_xml(xml)
    }

    fn service_type(&self) -> Service {
        Service::GroupManagement
    }
}

/// Create enriched event for sonos-stream integration
pub fn create_enriched_event(
    speaker_ip: IpAddr,
    event_source: EventSource,
    event_data: GroupManagementEvent,
) -> EnrichedEvent<GroupManagementEvent> {
    EnrichedEvent::new(
        speaker_ip,
        Service::GroupManagement,
        event_source,
        event_data,
    )
}

/// Create enriched event with registration ID
pub fn create_enriched_event_with_registration_id(
    registration_id: u64,
    speaker_ip: IpAddr,
    event_source: EventSource,
    event_data: GroupManagementEvent,
) -> EnrichedEvent<GroupManagementEvent> {
    EnrichedEvent::with_registration_id(
        registration_id,
        speaker_ip,
        Service::GroupManagement,
        event_source,
        event_data,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_group_management_parser_service_type() {
        let parser = GroupManagementEventParser;
        assert_eq!(parser.service_type(), Service::GroupManagement);
    }

    #[test]
    fn test_group_management_event_creation() {
        let event = GroupManagementEvent {
            properties: vec![GroupManagementProperty {
                group_coordinator_is_local: Some("1".to_string()),
                local_group_uuid: Some("RINCON_123456789:0".to_string()),
                reset_volume_after: Some("0".to_string()),
                virtual_line_in_group_id: Some("".to_string()),
                volume_av_transport_uri: Some("".to_string()),
            }],
        };

        assert_eq!(event.group_coordinator_is_local(), Some(true));
        assert_eq!(
            event.local_group_uuid(),
            Some("RINCON_123456789:0".to_string())
        );
        assert_eq!(event.reset_volume_after(), Some(false));
    }

    #[test]
    fn test_boolean_parsing_with_1_and_0() {
        let event = GroupManagementEvent {
            properties: vec![GroupManagementProperty {
                group_coordinator_is_local: Some("1".to_string()),
                local_group_uuid: None,
                reset_volume_after: Some("0".to_string()),
                virtual_line_in_group_id: None,
                volume_av_transport_uri: None,
            }],
        };

        assert_eq!(event.group_coordinator_is_local(), Some(true));
        assert_eq!(event.reset_volume_after(), Some(false));
    }

    #[test]
    fn test_boolean_parsing_with_true_and_false() {
        let event = GroupManagementEvent {
            properties: vec![GroupManagementProperty {
                group_coordinator_is_local: Some("true".to_string()),
                local_group_uuid: None,
                reset_volume_after: Some("false".to_string()),
                virtual_line_in_group_id: None,
                volume_av_transport_uri: None,
            }],
        };

        assert_eq!(event.group_coordinator_is_local(), Some(true));
        assert_eq!(event.reset_volume_after(), Some(false));
    }

    #[test]
    fn test_boolean_parsing_case_insensitive() {
        let event = GroupManagementEvent {
            properties: vec![GroupManagementProperty {
                group_coordinator_is_local: Some("TRUE".to_string()),
                local_group_uuid: None,
                reset_volume_after: Some("True".to_string()),
                virtual_line_in_group_id: None,
                volume_av_transport_uri: None,
            }],
        };

        assert_eq!(event.group_coordinator_is_local(), Some(true));
        assert_eq!(event.reset_volume_after(), Some(true));
    }

    #[test]
    fn test_enriched_event_creation() {
        let ip: IpAddr = "192.168.1.100".parse().unwrap();
        let source = EventSource::UPnPNotification {
            subscription_id: "uuid:123".to_string(),
        };
        let event_data = GroupManagementEvent {
            properties: vec![GroupManagementProperty {
                group_coordinator_is_local: Some("1".to_string()),
                local_group_uuid: None,
                reset_volume_after: None,
                virtual_line_in_group_id: None,
                volume_av_transport_uri: None,
            }],
        };

        let enriched = create_enriched_event(ip, source, event_data);

        assert_eq!(enriched.speaker_ip, ip);
        assert_eq!(enriched.service, Service::GroupManagement);
        assert!(enriched.registration_id.is_none());
    }

    #[test]
    fn test_enriched_event_with_registration_id() {
        let ip: IpAddr = "192.168.1.100".parse().unwrap();
        let source = EventSource::UPnPNotification {
            subscription_id: "uuid:123".to_string(),
        };
        let event_data = GroupManagementEvent {
            properties: vec![GroupManagementProperty {
                group_coordinator_is_local: None,
                local_group_uuid: None,
                reset_volume_after: None,
                virtual_line_in_group_id: None,
                volume_av_transport_uri: None,
            }],
        };

        let enriched = create_enriched_event_with_registration_id(42, ip, source, event_data);

        assert_eq!(enriched.registration_id, Some(42));
    }

    #[test]
    fn test_basic_xml_parsing() {
        let xml = r#"<e:propertyset xmlns:e="urn:schemas-upnp-org:event-1-0">
            <e:property>
                <GroupCoordinatorIsLocal>1</GroupCoordinatorIsLocal>
            </e:property>
            <e:property>
                <LocalGroupUUID>RINCON_123456789:0</LocalGroupUUID>
            </e:property>
            <e:property>
                <ResetVolumeAfter>0</ResetVolumeAfter>
            </e:property>
        </e:propertyset>"#;

        let result = GroupManagementEvent::from_xml(xml);
        assert!(
            result.is_ok(),
            "Failed to parse GroupManagement XML: {:?}",
            result
        );

        let event = result.unwrap();
        assert_eq!(event.group_coordinator_is_local(), Some(true));
        assert_eq!(
            event.local_group_uuid(),
            Some("RINCON_123456789:0".to_string())
        );
        assert_eq!(event.reset_volume_after(), Some(false));
    }

    #[test]
    fn test_xml_parsing_all_fields() {
        let xml = r#"<e:propertyset xmlns:e="urn:schemas-upnp-org:event-1-0">
            <e:property>
                <GroupCoordinatorIsLocal>1</GroupCoordinatorIsLocal>
            </e:property>
            <e:property>
                <LocalGroupUUID>RINCON_123456789:0</LocalGroupUUID>
            </e:property>
            <e:property>
                <ResetVolumeAfter>1</ResetVolumeAfter>
            </e:property>
            <e:property>
                <VirtualLineInGroupID>virtual-group-123</VirtualLineInGroupID>
            </e:property>
            <e:property>
                <VolumeAVTransportURI>x-rincon:RINCON_123456789</VolumeAVTransportURI>
            </e:property>
        </e:propertyset>"#;

        let result = GroupManagementEvent::from_xml(xml);
        assert!(result.is_ok(), "Failed to parse: {:?}", result);

        let event = result.unwrap();
        assert_eq!(event.group_coordinator_is_local(), Some(true));
        assert_eq!(
            event.local_group_uuid(),
            Some("RINCON_123456789:0".to_string())
        );
        assert_eq!(event.reset_volume_after(), Some(true));
        assert_eq!(
            event.virtual_line_in_group_id(),
            Some("virtual-group-123".to_string())
        );
        assert_eq!(
            event.volume_av_transport_uri(),
            Some("x-rincon:RINCON_123456789".to_string())
        );
    }

    #[test]
    fn test_empty_properties() {
        let xml = r#"<e:propertyset xmlns:e="urn:schemas-upnp-org:event-1-0">
            <e:property>
                <GroupCoordinatorIsLocal></GroupCoordinatorIsLocal>
            </e:property>
        </e:propertyset>"#;

        let result = GroupManagementEvent::from_xml(xml);
        assert!(result.is_ok());

        let event = result.unwrap();
        // Empty string should not match "1" or "true"
        assert_eq!(event.group_coordinator_is_local(), Some(false));
    }

    #[test]
    fn test_missing_properties() {
        let xml = r#"<e:propertyset xmlns:e="urn:schemas-upnp-org:event-1-0">
            <e:property>
                <LocalGroupUUID>RINCON_123:0</LocalGroupUUID>
            </e:property>
        </e:propertyset>"#;

        let result = GroupManagementEvent::from_xml(xml);
        assert!(result.is_ok());

        let event = result.unwrap();
        assert_eq!(event.group_coordinator_is_local(), None);
        assert_eq!(event.local_group_uuid(), Some("RINCON_123:0".to_string()));
        assert_eq!(event.reset_volume_after(), None);
    }
}


// =============================================================================
// PROPERTY-BASED TESTS
// =============================================================================

#[cfg(test)]
mod property_tests {
    use super::*;
    use proptest::prelude::*;

    // =========================================================================
    // Property 3: Event boolean parsing consistency
    // =========================================================================
    // *For any* GroupManagement event XML containing GroupCoordinatorIsLocal or
    // ResetVolumeAfter with value "1" or "true", parsing SHALL return `true`,
    // and for value "0" or "false", parsing SHALL return `false`.
    // **Validates: Requirements 7.1, 7.3**
    // =========================================================================

    /// Strategy for generating boolean string representations
    fn bool_string_strategy() -> impl Strategy<Value = (String, bool)> {
        prop_oneof![
            Just(("1".to_string(), true)),
            Just(("0".to_string(), false)),
            Just(("true".to_string(), true)),
            Just(("false".to_string(), false)),
            Just(("TRUE".to_string(), true)),
            Just(("FALSE".to_string(), false)),
            Just(("True".to_string(), true)),
            Just(("False".to_string(), false)),
        ]
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        /// Feature: group-management, Property 3: Event boolean parsing consistency (GroupCoordinatorIsLocal)
        #[test]
        fn prop_event_group_coordinator_is_local_parsing((bool_str, expected) in bool_string_strategy()) {
            let event = GroupManagementEvent {
                properties: vec![GroupManagementProperty {
                    group_coordinator_is_local: Some(bool_str.clone()),
                    local_group_uuid: None,
                    reset_volume_after: None,
                    virtual_line_in_group_id: None,
                    volume_av_transport_uri: None,
                }],
            };

            let result = event.group_coordinator_is_local();
            prop_assert_eq!(
                result,
                Some(expected),
                "GroupCoordinatorIsLocal '{}' should parse to {}",
                bool_str,
                expected
            );
        }

        /// Feature: group-management, Property 3: Event boolean parsing consistency (ResetVolumeAfter)
        #[test]
        fn prop_event_reset_volume_after_parsing((bool_str, expected) in bool_string_strategy()) {
            let event = GroupManagementEvent {
                properties: vec![GroupManagementProperty {
                    group_coordinator_is_local: None,
                    local_group_uuid: None,
                    reset_volume_after: Some(bool_str.clone()),
                    virtual_line_in_group_id: None,
                    volume_av_transport_uri: None,
                }],
            };

            let result = event.reset_volume_after();
            prop_assert_eq!(
                result,
                Some(expected),
                "ResetVolumeAfter '{}' should parse to {}",
                bool_str,
                expected
            );
        }
    }
}
