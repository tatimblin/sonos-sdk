# Sonos API Services

This directory contains service implementations for the Sonos UPnP API. Each service corresponds to a UPnP service that Sonos devices expose, providing **control operations** (commands), **event handling** (real-time state changes), and a **state snapshot type** that polling and eventing both produce.

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
| [`av_transport`](av_transport/) | Playback control | Play, Pause, Stop, Seek, queue, sleep timer, alarms | Transport state, track changes |
| [`rendering_control`](rendering_control/) | Audio control | Volume, Mute, Bass, Treble, Loudness | Volume changes, audio settings |
| [`group_rendering_control`](group_rendering_control/) | Group audio control | GroupVolume, GroupMute, SnapshotGroupVolume | Group volume and mute changes |
| [`zone_group_topology`](zone_group_topology/) | Speaker grouping | GetZoneGroupState | Group membership changes |
| [`group_management`](group_management/) | Group membership | AddMember, RemoveMember, ReportTrackBufferingResult | Group coordinator and member state |

### Service Mapping

Each module maps to one variant of the `Service` enum in [`src/service.rs`](../service.rs):

```rust,ignore
pub enum Service {
    AVTransport,           // urn:schemas-upnp-org:service:AVTransport:1
    RenderingControl,      // urn:schemas-upnp-org:service:RenderingControl:1
    GroupRenderingControl, // urn:schemas-upnp-org:service:GroupRenderingControl:1
    ZoneGroupTopology,     // urn:schemas-upnp-org:service:ZoneGroupTopology:1
    GroupManagement,       // urn:schemas-upnp-org:service:GroupManagement:1
}
```

`Service::info()` returns the control endpoint, service URI and event endpoint.
`Service::scope()` returns a `ServiceScope` — `PerSpeaker`, `PerNetwork` or `PerCoordinator` —
which tells subscribers how many subscriptions the service warrants. `RenderingControl` is
per-speaker, `ZoneGroupTopology` is per-network, and the remaining three are per-coordinator.

## Directory Structure

Each service follows a consistent structure:

```
services/
├── README.md                    # This file
├── mod.rs                       # Main services module
│
├── av_transport/                # AVTransport service
│   ├── mod.rs                   # Service module, subscribe helpers and re-exports
│   ├── operations.rs            # UPnP operations (Play, Pause, etc.)
│   ├── events.rs                # Event parsing and types
│   └── state.rs                 # AVTransportState snapshot type
│
├── rendering_control/           # Same four files, per service
├── group_rendering_control/
├── zone_group_topology/
├── group_management/
│
└── events.rs                    # Shared event helpers across services
```

`state.rs` holds the service's snapshot type (`AVTransportState`, `RenderingControlState`, …).
Both a parsed UPnP event and a polling round produce one of these, which is what lets
`sonos-stream` fall back to polling without the consumer noticing.

## Using Services

### Import Pattern

Import services individually to avoid naming conflicts:

```rust
use sonos_api::services::av_transport;
use sonos_api::services::rendering_control;
use sonos_api::{OperationBuilder, SonosClient};
```

### Control Operations

Every operation has a generated snake_case constructor returning an `OperationBuilder`:

```rust
use sonos_api::services::{av_transport, rendering_control};
use sonos_api::SonosClient;

let client = SonosClient::new();

// Simple operation
let play_op = av_transport::play_operation("1".to_string()).build()?;
client.execute_enhanced("192.168.1.100", play_op)?;

// Operation with response
let volume_op = rendering_control::get_volume_operation("Master".to_string()).build()?;
let response = client.execute_enhanced("192.168.1.100", volume_op)?;
println!("Current volume: {}", response.current_volume);
```

To set `instance_id` or otherwise build the request by hand, construct the generated
`…OperationRequest` struct and pass it to `OperationBuilder`:

```rust
use sonos_api::services::av_transport::{PlayOperation, PlayOperationRequest};
use sonos_api::{OperationBuilder, SonosClient};

let client = SonosClient::new();

let request = PlayOperationRequest {
    instance_id: 0,
    speed: "1".to_string(),
};
let play_op = OperationBuilder::<PlayOperation>::new(request).build()?;
client.execute_enhanced("192.168.1.100", play_op)?;
```

### Event Handling

Handle real-time state change events:

```rust
use sonos_api::events::{EventParser, EventSource};
use sonos_api::services::av_transport::{create_enriched_event, AVTransportEventParser};

let speaker_ip: std::net::IpAddr = "192.168.1.100".parse()?;
let xml_content = r#"<e:propertyset xmlns:e="urn:schemas-upnp-org:event-1-0">
    <e:property>
        <LastChange>&lt;Event&gt;&lt;InstanceID val="0"&gt;
            &lt;TransportState val="PLAYING"/&gt;
        &lt;/InstanceID&gt;&lt;/Event&gt;</LastChange>
    </e:property>
</e:propertyset>"#;

// Parse event XML
let event_data = AVTransportEventParser.parse_upnp_event(xml_content)?;

// Create enriched event with metadata
let source = EventSource::UPnPNotification {
    subscription_id: "uuid:123".to_string(),
};
let enriched = create_enriched_event(speaker_ip, source, event_data);

// Access event data
if let Some(state) = enriched.event_data.transport_state() {
    println!("Transport state: {state}");
}
```

### Subscriptions

Each service module exposes `subscribe` and `subscribe_with_timeout`, which delegate to the
client with the module's own `Service` value:

```rust
use sonos_api::services::rendering_control;
use sonos_api::SonosClient;

let client = SonosClient::new();

let subscription = rendering_control::subscribe(
    &client,
    "192.168.1.100",
    "http://192.168.1.50:8080/callback",
)?;
subscription.unsubscribe()?;
```

## Implementing New Services

To implement a new UPnP service, follow this pattern.

### 1. Create Service Directory

```bash
mkdir src/services/my_service
```

### 2. Service Module (`mod.rs`)

```rust,ignore
//! MyService service for [description]
//!
//! This service handles [operations] and related events.

pub mod events;
pub mod operations;
pub mod state;

// Re-export operations for convenience
pub use operations::*;

// Re-export event types and parsers
pub use events::{create_enriched_event, MyServiceEvent, MyServiceEventParser};
pub use state::MyServiceState;

pub const SERVICE: crate::Service = crate::Service::MyService;

pub fn subscribe(
    client: &crate::SonosClient,
    ip: &str,
    callback_url: &str,
) -> crate::Result<crate::ManagedSubscription> {
    client.subscribe(ip, SERVICE, callback_url)
}
```

### 3. Operations (`operations.rs`)

Define UPnP operations using the declarative macros:

```rust,ignore
use crate::{define_operation_with_response, define_upnp_operation, Validate};

// Simple operation with no out-arguments
define_upnp_operation! {
    operation: MyActionOperation,
    action: "MyAction",
    service: MyService,  // Must match a Service enum variant
    request: {
        parameter: String,
    },
    response: (),
    payload: |req| format!(
        "<InstanceID>{}</InstanceID><Parameter>{}</Parameter>",
        req.instance_id, req.parameter
    ),
    parse: |_xml| Ok(()),
}

// Operation with a structured response
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

impl Validate for MyActionOperationRequest {
    fn validate_basic(&self) -> Result<(), crate::operation::ValidationError> {
        if self.parameter.is_empty() {
            return Err(crate::operation::ValidationError::invalid_value(
                "parameter",
                &self.parameter,
            ));
        }
        Ok(())
    }
}
```

Both macros generate, for `MyActionOperation`: the request struct `MyActionOperationRequest`
(your fields plus `instance_id: u32`), the `UPnPOperation` implementation, and the constructor
`my_action_operation(parameter: String) -> OperationBuilder<MyActionOperation>`.

Request element names are derived by capitalizing the first character of each field, which only
works for single-word fields. A multi-word field requires an explicit `request_xml_mapping:`
block, because UPnP casing (`ObjectID`, `EnqueuedURI`, `NumberOfTracks`) cannot be recovered
from snake_case. Omitting a field from that block is a compile error:

```rust,ignore
define_operation_with_response! {
    operation: SaveQueueOperation,
    action: "SaveQueue",
    service: AVTransport,
    request: {
        title: String,
        object_id: String,
    },
    response: SaveQueueResponse {
        assigned_object_id: String,
    },
    request_xml_mapping: {
        title: "Title",
        object_id: "ObjectID",
    },
    xml_mapping: {
        assigned_object_id: "AssignedObjectID",
    },
}
```

### 4. Events (`events.rs`)

Implement event parsing using serde-based XML deserialization:

```rust,ignore
use serde::{Deserialize, Serialize};
use std::net::IpAddr;

use crate::events::{xml_utils, EnrichedEvent, EventParser, EventSource};
use crate::{ApiError, Result, Service};

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
}

impl MyServiceEvent {
    /// Get my_field value
    pub fn my_field(&self) -> Option<String> {
        self.property
            .last_change
            .instance
            .my_field
            .as_ref()
            .map(|v| v.val.clone())
    }

    /// Parse from UPnP event XML using serde.
    ///
    /// No namespace preprocessing is needed: quick-xml's serde deserializer
    /// matches on element *local* names, so `<e:propertyset>` deserializes as
    /// `propertyset`. Write `rename` values without prefixes.
    pub fn from_xml(xml: &str) -> Result<Self> {
        quick_xml::de::from_str(xml)
            .map_err(|e| ApiError::ParseError(format!("Failed to parse MyService XML: {e}")))
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
        Service::MyService
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
```

### 5. State (`state.rs`)

Define the snapshot type the service reports, and a conversion from the event type. Polling
strategies in `sonos-stream` build the same type from GET operations, so downstream consumers
handle one shape regardless of where the data came from.

### 6. Update Service Enum

Add the variant to `Service` in [`src/service.rs`](../service.rs) and cover it in `name()`,
`info()` and `scope()`:

```rust,ignore
pub enum Service {
    // ... existing services
    MyService,
}

impl Service {
    pub fn info(&self) -> ServiceInfo {
        match self {
            // ... existing mappings
            Service::MyService => ServiceInfo {
                endpoint: "MediaRenderer/MyService/Control",
                service_uri: "urn:schemas-upnp-org:service:MyService:1",
                event_endpoint: "MediaRenderer/MyService/Event",
            },
        }
    }
}
```

### 7. Register in Services Module

Add your service to `src/services/mod.rs`:

```rust,ignore
pub mod my_service;
```

## Operations

### Operation Types

1. **Simple Operations**: Commands with no response data (Play, Pause, Stop)
2. **Response Operations**: Commands that return structured data (GetVolume, GetTransportInfo)
3. **Parameter Operations**: Commands with input parameters (SetVolume, Seek)

### Macro Usage

- `define_upnp_operation!` - actions whose response carries no out-arguments
- `define_operation_with_response!` - actions with a structured XML response

### Validation

Implement the `Validate` trait for request validation. `build_payload` calls
`validate(ValidationLevel::Basic)` before producing any XML, so an invalid request never
reaches the network:

```rust,ignore
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

- `quick_xml::de::from_str()` - Deserialize the raw NOTIFY body directly. **No namespace
  stripping**: quick-xml matches on element *local* names, so `<e:property>` deserializes as
  `property` and `<dc:title>` as `title`. Write `#[serde(rename = "...")]` values without
  prefixes — `rename = "dc:title"` can never match
- `xml_utils::parse()` - The same thing, wrapping the error as `ApiError::ParseError`
- `xml_utils::deserialize_nested()` - Handle nested escaped XML content
- `xml_utils::ValueAttribute` - Parse elements with `val` attributes
- `xml_utils::NestedAttribute<T>` - A `val` attribute whose content is escaped XML

### Channel-Based Fields

Bass, Treble, Loudness, Balance, Volume and Mute are per-channel state variables in UPnP RCS.
Stereo pairs and home theater setups emit one element per channel, so each must be a collection
— modelling one as a single value makes the whole event fail to deserialize with "duplicate
field". `rendering_control::events::ChannelValueAttribute` carries the `val` and `channel`
attribute pair:

```rust,ignore
#[serde(rename = "Volume", default)]
pub volumes: Vec<ChannelValueAttribute>,

/// Helper to get specific channel value
fn get_volume_for_channel(&self, channel: &str) -> Option<String> {
    self.volumes
        .iter()
        .find(|v| v.channel == channel)
        .map(|v| v.val.clone())
}
```

## Testing

### Unit Tests

Each service should have tests covering payload construction, response parsing and event
parsing:

```rust,ignore
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
cargo run -p sonos-api --example cli_example
```

### Real Device Testing

`validate_rendering_control` round-trips every RenderingControl operation against the first
discovered speaker, restoring the original value after each write:

```bash
cargo run -p sonos-api --example validate_rendering_control
```

To probe a single action — including ones with no operation defined yet — send a raw SOAP body:

```bash
cargo run -p sonos-api --example test_operation -- 192.168.1.100 AVTransport GetTransportInfo
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

```rust,ignore
pub const SERVICE: Service = Service::MyService;
```

### Operation Constructors

The macros generate a constructor named after the operation in snake_case, taking the request
fields in declaration order and defaulting `instance_id` to `0`:

```rust,ignore
// Generated by define_upnp_operation! for MyActionOperation
pub fn my_action_operation(parameter: String) -> OperationBuilder<MyActionOperation> {
    OperationBuilder::new(MyActionOperationRequest {
        parameter,
        instance_id: 0,
    })
}
```

### Event Integration

`sonos-stream` converts each parsed event into the service's state snapshot and wraps it in
`EventData`:

```rust,ignore
// In sonos-stream
match event.event_data {
    EventData::AVTransport(state) => {
        println!("Transport state: {:?}", state.transport_state);
    }
    _ => {}
}
```

### Resource Efficiency

All services share one HTTP connection pool:

```rust
use sonos_api::SonosClient;

let client = SonosClient::new(); // Handle to the shared SOAP client
```

## Troubleshooting

### Common Issues

1. **XML Parsing Errors**: Check that serde attributes match actual XML structure
2. **Validation Failures**: Ensure validation logic matches UPnP service requirements
3. **Event Parsing**: Use real device XML for testing, not hand-written examples
4. **Service Registration**: Make sure new services are added to all required locations

### Debug Tools

```bash
# Interactive operation execution
cargo run -p sonos-api --example cli_example

# Test event parsing for one service
cargo test -p sonos-api my_service::events -- --nocapture
```
