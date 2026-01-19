# Service Module Structure Reference

## Table of Contents

1. [Directory Structure](#directory-structure)
2. [Module File (mod.rs)](#module-file-modrs)
3. [Operations File (operations.rs)](#operations-file-operationsrs)
4. [Events File (events.rs)](#events-file-eventsrs)
5. [Service Registration](#service-registration)
6. [Test Patterns](#test-patterns)

## Directory Structure

```
sonos-api/src/services/
├── mod.rs                      # Service module registry
├── events.rs                   # Common event types
│
├── av_transport/               # AVTransport service
│   ├── mod.rs                 # Module exports
│   ├── operations.rs          # UPnP operations
│   └── events.rs              # Event parsing
│
├── rendering_control/          # RenderingControl service
│   ├── mod.rs
│   ├── operations.rs
│   └── events.rs
│
└── zone_group_topology/        # ZoneGroupTopology service
    ├── mod.rs
    ├── operations.rs
    └── events.rs
```

## Module File (mod.rs)

```rust
//! MyService service for [description]
//!
//! This service handles [operations] and related events.

pub mod operations;
pub mod events;

// Re-export operations for convenience
pub use operations::*;

// Re-export event types and parsers
pub use events::{MyServiceEvent, MyServiceEventParser, create_enriched_event};
```

## Operations File (operations.rs)

### Complete Template

```rust
use crate::{define_upnp_operation, define_operation_with_response, Validate};
use paste::paste;

// === Operations ===

// Simple operation (no meaningful response)
define_upnp_operation! {
    operation: ActionOneOperation,
    action: "ActionOne",
    service: MyService,  // Must match Service enum variant
    request: {},
    response: (),
    payload: |req| format!("<InstanceID>{}</InstanceID>", req.instance_id),
    parse: |_xml| Ok(()),
}

impl Validate for ActionOneOperationRequest {
    // No validation needed
}

// Operation with parameters
define_upnp_operation! {
    operation: ActionTwoOperation,
    action: "ActionTwo",
    service: MyService,
    request: {
        param_one: String,
        param_two: u32,
    },
    response: (),
    payload: |req| {
        format!(
            "<InstanceID>{}</InstanceID><ParamOne>{}</ParamOne><ParamTwo>{}</ParamTwo>",
            req.instance_id, req.param_one, req.param_two
        )
    },
    parse: |_xml| Ok(()),
}

impl Validate for ActionTwoOperationRequest {
    fn validate_basic(&self) -> Result<(), crate::operation::ValidationError> {
        if self.param_one.is_empty() {
            return Err(crate::operation::ValidationError::invalid_value("param_one", &self.param_one));
        }
        Ok(())
    }
}

// Operation with response
define_operation_with_response! {
    operation: GetInfoOperation,
    action: "GetInfo",
    service: MyService,
    request: {},
    response: GetInfoResponse {
        field_one: String,
        field_two: u32,
    },
    xml_mapping: {
        field_one: "FieldOne",
        field_two: "FieldTwo",
    },
}

impl Validate for GetInfoOperationRequest {
    // No validation needed for parameterless operation
}

// === Legacy Aliases ===

pub use action_one_operation as action_one;
pub use action_two_operation as action_two;
pub use get_info_operation as get_info;

// === Service Constants ===

pub const SERVICE: crate::Service = crate::Service::MyService;

// === Subscription Helpers ===

pub fn subscribe(
    client: &crate::SonosClient,
    ip: &str,
    callback_url: &str,
) -> crate::Result<crate::ManagedSubscription> {
    client.subscribe(ip, SERVICE, callback_url)
}

pub fn subscribe_with_timeout(
    client: &crate::SonosClient,
    ip: &str,
    callback_url: &str,
    timeout_seconds: u32,
) -> crate::Result<crate::ManagedSubscription> {
    client.subscribe_with_timeout(ip, SERVICE, callback_url, timeout_seconds)
}

// === Tests ===

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operation::UPnPOperation;

    #[test]
    fn test_operation_builder() {
        let op = action_one_operation().build().unwrap();
        assert_eq!(op.metadata().action, "ActionOne");
    }

    #[test]
    fn test_operation_with_params() {
        let op = action_two_operation("value".to_string(), 42).build().unwrap();
        assert_eq!(op.request().param_one, "value");
        assert_eq!(op.request().param_two, 42);
    }

    #[test]
    fn test_validation() {
        let request = ActionTwoOperationRequest {
            instance_id: 0,
            param_one: "".to_string(),
            param_two: 42,
        };
        assert!(request.validate_basic().is_err());

        let request = ActionTwoOperationRequest {
            instance_id: 0,
            param_one: "valid".to_string(),
            param_two: 42,
        };
        assert!(request.validate_basic().is_ok());
    }

    #[test]
    fn test_payload_generation() {
        let request = ActionTwoOperationRequest {
            instance_id: 0,
            param_one: "test".to_string(),
            param_two: 42,
        };
        let payload = ActionTwoOperation::build_payload(&request).unwrap();
        assert!(payload.contains("<InstanceID>0</InstanceID>"));
        assert!(payload.contains("<ParamOne>test</ParamOne>"));
        assert!(payload.contains("<ParamTwo>42</ParamTwo>"));
    }

    #[test]
    fn test_service_constant() {
        assert_eq!(SERVICE, crate::Service::MyService);
    }
}
```

## Events File (events.rs)

### Complete Template

```rust
use serde::{Deserialize, Serialize};
use std::net::IpAddr;

use crate::{Result, Service, ApiError};
use crate::events::{EnrichedEvent, EventSource, EventParser, xml_utils};

/// MyService event - direct serde mapping from UPnP event XML
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename = "propertyset")]
pub struct MyServiceEvent {
    #[serde(rename = "property")]
    property: MyServiceProperty,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MyServiceProperty {
    #[serde(rename = "LastChange", deserialize_with = "xml_utils::deserialize_nested")]
    last_change: MyServiceEventData,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename = "Event")]
pub struct MyServiceEventData {
    #[serde(rename = "InstanceID")]
    instance: MyServiceInstance,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MyServiceInstance {
    #[serde(rename = "FieldOne", default)]
    pub field_one: Option<xml_utils::ValueAttribute>,

    #[serde(rename = "FieldTwo", default)]
    pub field_two: Option<xml_utils::ValueAttribute>,
}

impl MyServiceEvent {
    pub fn field_one(&self) -> Option<String> {
        self.property.last_change.instance.field_one
            .as_ref()
            .map(|v| v.val.clone())
    }

    pub fn field_two(&self) -> Option<String> {
        self.property.last_change.instance.field_two
            .as_ref()
            .map(|v| v.val.clone())
    }

    pub fn from_xml(xml: &str) -> Result<Self> {
        let clean_xml = xml_utils::strip_namespaces(xml);
        quick_xml::de::from_str(&clean_xml)
            .map_err(|e| ApiError::ParseError(format!("Failed to parse MyService XML: {}", e)))
    }
}

pub struct MyServiceEventParser;

impl EventParser for MyServiceEventParser {
    type EventData = MyServiceEvent;

    fn parse_upnp_event(&self, xml: &str) -> Result<Self::EventData> {
        MyServiceEvent::from_xml(xml)
    }

    fn service_type(&self) -> Service {
        Service::MyService
    }
}

pub fn create_enriched_event(
    speaker_ip: IpAddr,
    event_source: EventSource,
    event_data: MyServiceEvent,
) -> EnrichedEvent<MyServiceEvent> {
    EnrichedEvent::new(speaker_ip, Service::MyService, event_source, event_data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_parsing() {
        let xml = r#"<e:propertyset xmlns:e="urn:schemas-upnp-org:event-1-0">
            <e:property>
                <LastChange>&lt;Event&gt;
                    &lt;InstanceID val="0"&gt;
                        &lt;FieldOne val="test_value"/&gt;
                    &lt;/InstanceID&gt;
                &lt;/Event&gt;</LastChange>
            </e:property>
        </e:propertyset>"#;

        let event = MyServiceEvent::from_xml(xml).unwrap();
        assert_eq!(event.field_one(), Some("test_value".to_string()));
    }
}
```

## Service Registration

### Step 1: Add to Service Enum

Edit `sonos-api/src/service.rs`:

```rust
pub enum Service {
    AVTransport,
    RenderingControl,
    GroupRenderingControl,
    ZoneGroupTopology,
    MyService,  // Add new service
}

impl Service {
    pub fn name(&self) -> &'static str {
        match self {
            // ... existing ...
            Service::MyService => "MyService",
        }
    }

    pub fn info(&self) -> ServiceInfo {
        match self {
            // ... existing ...
            Service::MyService => ServiceInfo {
                endpoint: "MediaRenderer/MyService/Control",
                service_uri: "urn:schemas-upnp-org:service:MyService:1",
                event_endpoint: "MediaRenderer/MyService/Event",
            },
        }
    }

    pub fn scope(&self) -> ServiceScope {
        match self {
            // ... existing ...
            Service::MyService => ServiceScope::PerSpeaker, // or PerNetwork/PerCoordinator
        }
    }
}
```

### Step 2: Register in Services Module

Edit `sonos-api/src/services/mod.rs`:

```rust
pub mod av_transport;
pub mod rendering_control;
pub mod zone_group_topology;
pub mod my_service;  // Add new service module
pub mod events;
```

## Test Patterns

### Minimum Required Tests

1. **Operation builder test** - Verify operation builds correctly
2. **Validation test** - Test validation logic for invalid inputs
3. **Payload generation test** - Verify XML payload format
4. **Service constant test** - Verify SERVICE matches expected value

### Optional Tests

- Response parsing (if operation returns data)
- Subscription helper signature tests
- Integration tests with real devices (manual)
