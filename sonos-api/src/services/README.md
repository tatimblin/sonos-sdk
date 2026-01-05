# Sonos API Services

This directory contains service implementations for the Sonos UPnP API. Each service corresponds to a UPnP service that Sonos devices expose, providing both **control operations** (commands) and **event handling** (real-time state changes).

## Table of Contents

- [Overview](#overview)
- [Available Services](#available-services)
- [Directory Structure](#directory-structure)
- [Using Services](#using-services)
- [Implementing New Services](#implementing-new-services)
- [Operations](#operations)
- [Events](#events)
- [Testing](#testing)

## Overview

Sonos devices expose multiple UPnP services, each handling a specific domain:

- **Control Operations**: Send commands to devices (play, pause, set volume, etc.)
- **Event Subscriptions**: Receive real-time notifications when device state changes
- **Type Safety**: All requests and responses are strongly typed with serde serialization
- **Validation**: Built-in validation for operation parameters
- **Error Handling**: Comprehensive error handling with detailed messages

## Available Services

| Service | Purpose | Operations | Events |
|---------|---------|------------|---------|
| [`av_transport`](av_transport/) | Playback control | Play, Pause, Stop, Seek, etc. | Transport state, track changes |
| [`rendering_control`](rendering_control/) | Audio control | Volume, Mute, Bass, Treble | Volume changes, audio settings |
| [`zone_group_topology`](zone_group_topology/) | Speaker grouping | Get topology state | Group membership changes |

### Service Mapping

Each service maps to a specific UPnP service:

```rust
pub enum Service {
    AVTransport,           // urn:schemas-upnp-org:service:AVTransport:1
    RenderingControl,      // urn:schemas-upnp-org:service:RenderingControl:1
    GroupRenderingControl, // urn:schemas-upnp-org:service:GroupRenderingControl:1
    ZoneGroupTopology,     // urn:schemas-rinconnetworks-com:service:ZoneGroupTopology:1
}
```

## Directory Structure

Each service follows a consistent structure:

```
services/
├── README.md                    # This file
├── mod.rs                      # Main services module
├── events.rs                   # Common event subscription types
│
├── av_transport/               # AVTransport service
│   ├── mod.rs                 # Service module and re-exports
│   ├── operations.rs          # UPnP operations (Play, Pause, etc.)
│   └── events.rs             # Event parsing and types
│
├── rendering_control/          # RenderingControl service
│   ├── mod.rs
│   ├── operations.rs          # Volume, Mute operations
│   └── events.rs             # Volume/audio event handling
│
└── zone_group_topology/        # ZoneGroupTopology service
    ├── mod.rs
    ├── operations.rs          # Topology queries
    └── events.rs             # Group membership events
```

## Using Services

### Import Pattern

Import services individually to avoid naming conflicts:

```rust
use sonos_api::services::av_transport;
use sonos_api::services::rendering_control;
use sonos_api::{SonosClient, OperationBuilder};
```

### Control Operations

Execute operations using the enhanced operation framework:

```rust
let client = SonosClient::new();

// Simple operation
let play_request = av_transport::PlayOperationRequest {
    instance_id: 0,
    speed: "1".to_string(),
};
let play_op = OperationBuilder::<av_transport::PlayOperation>::new(play_request).build()?;
client.execute_enhanced("192.168.1.100", play_op)?;

// Operation with response
let get_volume_request = rendering_control::GetVolumeOperationRequest {
    instance_id: 0,
    channel: "Master".to_string(),
};
let volume_op = OperationBuilder::<rendering_control::GetVolumeOperation>::new(get_volume_request).build()?;
let response = client.execute_enhanced("192.168.1.100", volume_op)?;
println!("Current volume: {}", response.current_volume);
```

### Event Handling

Handle real-time state change events:

```rust
use sonos_api::services::av_transport::events::{AVTransportEventParser, create_enriched_event};
use sonos_api::events::EventSource;

// Parse event XML
let parser = AVTransportEventParser;
let event_data = parser.parse_upnp_event(xml_content)?;

// Create enriched event with metadata
let source = EventSource::UPnPNotification {
    subscription_id: "uuid:123".to_string(),
};
let enriched = create_enriched_event(speaker_ip, source, event_data);

// Access event data
if let Some(state) = enriched.event_data.transport_state() {
    println!("Transport state: {}", state);
}
```

## Implementing New Services

To implement a new UPnP service, follow this pattern:

### 1. Create Service Directory

```bash
mkdir src/services/my_service
```

### 2. Service Module (`mod.rs`)

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

### 3. Operations (`operations.rs`)

Define UPnP operations using the declarative macros:

```rust
use crate::{define_upnp_operation, define_operation_with_response, Validate};

// Simple operation with no response
define_upnp_operation! {
    operation: MyActionOperation,
    action: "MyAction",
    service: MyService,  // Must match Service enum variant
    request: {
        parameter: String,
    },
    response: (),
    payload: |req| format!("<Parameter>{}</Parameter>", req.parameter),
    parse: |_xml| Ok(()),
}

// Operation with complex response
define_operation_with_response! {
    operation: GetMyInfoOperation,
    action: "GetMyInfo",
    service: MyService,
    request: {},
    response: GetMyInfoResponse {
        info_field: String,
        status_field: i32,
    },
    xml_mapping: {
        info_field: "InfoField",
        status_field: "StatusField",
    },
}

// Validation implementation
impl Validate for MyActionOperationRequest {
    fn validate_basic(&self) -> Result<(), crate::operation::ValidationError> {
        if self.parameter.is_empty() {
            return Err(crate::operation::ValidationError::invalid_value("parameter", &self.parameter));
        }
        Ok(())
    }
}

impl Validate for GetMyInfoOperationRequest {
    // No validation needed for parameterless operation
}
```

### 4. Events (`events.rs`)

Implement event parsing using serde-based XML deserialization:

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
    #[serde(rename = "MyField", default)]
    pub my_field: Option<xml_utils::ValueAttribute>,

    // Add more fields as needed
}

impl MyServiceEvent {
    /// Get my_field value
    pub fn my_field(&self) -> Option<String> {
        self.property.last_change.instance.my_field.as_ref().map(|v| v.val.clone())
    }

    /// Parse from UPnP event XML using serde
    pub fn from_xml(xml: &str) -> Result<Self> {
        let clean_xml = xml_utils::strip_namespaces(xml);
        quick_xml::de::from_str(&clean_xml)
            .map_err(|e| ApiError::ParseError(format!("Failed to parse MyService XML: {}", e)))
    }
}

/// Event parser implementation
pub struct MyServiceEventParser;

impl EventParser for MyServiceEventParser {
    type EventData = MyServiceEvent;

    fn parse_upnp_event(&self, xml: &str) -> Result<Self::EventData> {
        MyServiceEvent::from_xml(xml)
    }

    fn service_type(&self) -> Service {
        Service::MyService  // Must match Service enum variant
    }
}

/// Create enriched event for sonos-stream integration
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
                        &lt;MyField val="test_value"/&gt;
                    &lt;/InstanceID&gt;
                &lt;/Event&gt;</LastChange>
            </e:property>
        </e:propertyset>"#;

        let event = MyServiceEvent::from_xml(xml).unwrap();
        assert_eq!(event.my_field(), Some("test_value".to_string()));
    }
}
```

### 5. Update Service Enum

Add your new service to the main Service enum in `src/service.rs`:

```rust
pub enum Service {
    // ... existing services
    MyService,  // Add your service here
}

impl Service {
    pub fn info(&self) -> ServiceInfo {
        match self {
            // ... existing mappings
            Service::MyService => ServiceInfo {
                endpoint: "/MediaRenderer/MyService/Control",
                service_uri: "urn:schemas-upnp-org:service:MyService:1",
                event_sub_url: "/MediaRenderer/MyService/Event",
            },
        }
    }
}
```

### 6. Register in Services Module

Add your service to `src/services/mod.rs`:

```rust
pub mod my_service;  // Add this line
```

## Operations

### Operation Types

1. **Simple Operations**: Commands with no response data (Play, Pause, Stop)
2. **Response Operations**: Commands that return structured data (GetVolume, GetTransportInfo)
3. **Parameter Operations**: Commands with input parameters (SetVolume, Seek)

### Macro Usage

Use the declarative macros to define operations:

- `define_upnp_operation!` - Simple operations with basic responses
- `define_operation_with_response!` - Operations with structured XML responses

### Validation

Implement the `Validate` trait for request validation:

```rust
impl Validate for MyOperationRequest {
    fn validate_basic(&self) -> Result<(), ValidationError> {
        // Add validation logic
        Ok(())
    }
}
```

## Events

### Event Structure

Events follow the UPnP eventing specification:

1. **Outer wrapper**: `<e:propertyset>` containing properties
2. **Property**: `<e:property>` containing the LastChange data
3. **LastChange**: Contains escaped XML with actual event data
4. **Event data**: Structured XML with InstanceID and field values

### Parsing Strategy

Use serde for type-safe XML deserialization:

- `xml_utils::strip_namespaces()` - Remove XML namespace prefixes
- `xml_utils::deserialize_nested()` - Handle nested escaped XML content
- `xml_utils::ValueAttribute` - Parse elements with `val` attributes

### Channel-Based Fields

For fields with channel attributes (like Volume), use collections:

```rust
#[serde(rename = "Volume", default)]
pub volumes: Vec<ChannelValueAttribute>,

/// Helper to get specific channel value
fn get_volume_for_channel(&self, channel: &str) -> Option<String> {
    self.volumes.iter()
        .find(|v| v.channel == channel)
        .map(|v| v.val.clone())
}
```

## Testing

### Unit Tests

Each service should have comprehensive tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_operation_validation() {
        let request = MyOperationRequest { /* fields */ };
        assert!(request.validate_basic().is_ok());
    }

    #[test]
    fn test_event_parsing() {
        let xml = r#"..."#;  // Real UPnP event XML
        let event = MyServiceEvent::from_xml(xml).unwrap();
        assert_eq!(event.my_field(), Some("expected_value".to_string()));
    }

    #[test]
    fn test_event_parser_service_type() {
        let parser = MyServiceEventParser;
        assert_eq!(parser.service_type(), Service::MyService);
    }
}
```

### Integration Testing

Use the CLI example for end-to-end testing:

```bash
cargo run --example cli_example
```

### Real Device Testing

Test with actual Sonos devices using examples:

```bash
cargo run --example basic_usage
```

## Best Practices

1. **Naming**: Use descriptive operation and field names that match UPnP specifications
2. **Validation**: Implement comprehensive validation for all parameters
3. **Error Handling**: Use specific error types and helpful error messages
4. **Documentation**: Document all public APIs with examples
5. **Testing**: Write tests for both success and failure cases
6. **Backward Compatibility**: Maintain compatibility when extending APIs

## Common Patterns

### Service Constants

```rust
pub const SERVICE: Service = Service::MyService;
```

### Operation Builders

The macros automatically generate builder functions:

```rust
// Generated automatically by define_upnp_operation!
pub fn my_action_operation() -> OperationBuilder<MyActionOperation> {
    OperationBuilder::new(MyActionOperationRequest { /* defaults */ })
}
```

### Event Integration

Events integrate with `sonos-stream` for real-time processing:

```rust
// In sonos-stream
match event.event_data {
    EventData::MyServiceEvent(my_event) => {
        println!("My field changed: {:?}", my_event.my_field());
    }
}
```

### Resource Efficiency

All services automatically share HTTP resources:

```rust
let client = SonosClient::new();  // Efficient shared SOAP client
```

## Troubleshooting

### Common Issues

1. **XML Parsing Errors**: Check that serde attributes match actual XML structure
2. **Validation Failures**: Ensure validation logic matches UPnP service requirements
3. **Event Parsing**: Use real device XML for testing, not hand-written examples
4. **Service Registration**: Make sure new services are added to all required locations

### Debug Tools

Use the debug examples to test your implementation:

```bash
# Test operations
cargo run --example cli_example

# Test event parsing
cargo test -p sonos-api my_service::events -- --nocapture
```