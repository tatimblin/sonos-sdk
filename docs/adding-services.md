# Adding New Sonos Services

This guide explains how to add support for a new Sonos UPnP service to the SDK. The implementation spans 4 layers, each with specific responsibilities.

## Overview

Adding a service requires implementing across these layers:

| Layer | Crate | Purpose | Key Files |
|-------|-------|---------|-----------|
| 1. API | `sonos-api` | UPnP SOAP operations | `services/{service}/operations.rs` |
| 2. Stream | `sonos-stream` | Event streaming/polling | `events/types.rs`, `polling/strategies.rs` |
| 3. State | `sonos-state` | Reactive state store | `property.rs`, `decoder.rs` |
| 4. SDK | `sonos-sdk` | DOM-like public API | `property/handles.rs`, `speaker.rs` |

## Prerequisites

Before starting, gather:
- **Service documentation URL** (e.g., `https://sonos.svrooij.io/services/alarm-clock`)
- **Operations list** - What actions the service supports
- **Properties list** - What state values to expose
- **Property scope** - Speaker, Group, or System (see below)
- **Real Sonos speaker** - For testing (recommended)

### Determining Property Scope

| Scope | When to Use | Examples |
|-------|-------------|----------|
| `Speaker` | Property differs per speaker | Volume, Mute, Playback |
| `Group` | Property applies to group coordinator | GroupVolume, GroupMute |
| `System` | Property is network-wide | Topology, Alarms, MusicServices |

## Available Services

| Service | Documentation | Typical Scope |
|---------|--------------|---------------|
| AlarmClock | [Link](https://sonos.svrooij.io/services/alarm-clock) | System |
| AudioIn | [Link](https://sonos.svrooij.io/services/audio-in) | Speaker |
| ConnectionManager | [Link](https://sonos.svrooij.io/services/connection-manager) | Speaker |
| ContentDirectory | [Link](https://sonos.svrooij.io/services/content-directory) | System |
| DeviceProperties | [Link](https://sonos.svrooij.io/services/device-properties) | Speaker |
| GroupManagement | [Link](https://sonos.svrooij.io/services/group-management) | Group |
| GroupRenderingControl | [Link](https://sonos.svrooij.io/services/group-rendering-control) | Group |
| HTControl | [Link](https://sonos.svrooij.io/services/ht-control) | Speaker |
| MusicServices | [Link](https://sonos.svrooij.io/services/music-services) | System |
| Queue | [Link](https://sonos.svrooij.io/services/queue) | Speaker |
| SystemProperties | [Link](https://sonos.svrooij.io/services/system-properties) | System |
| VirtualLineIn | [Link](https://sonos.svrooij.io/services/virtual-line-in) | Speaker |

## Quick Start

Use the skills in `.claude/skills/` for guided implementation:

```bash
# Check implementation status
python .claude/skills/add-service/scripts/service_status.py NewService

# After implementation, run integration test
python .claude/skills/add-service/scripts/integration_test.py NewService 192.168.1.100
```

## Layer 1: API Implementation

### 1.1 Register Service

Add to `sonos-api/src/service.rs`:

```rust
pub enum Service {
    // ... existing ...
    NewService,
}

impl Service {
    pub fn name(&self) -> &'static str {
        match self {
            Service::NewService => "NewService",
            // ...
        }
    }

    pub fn info(&self) -> ServiceInfo {
        match self {
            Service::NewService => ServiceInfo {
                endpoint: "MediaRenderer/NewService/Control",
                service_uri: "urn:schemas-upnp-org:service:NewService:1",
                event_endpoint: "MediaRenderer/NewService/Event",
            },
            // ...
        }
    }
}
```

### 1.2 Create Service Module

```
sonos-api/src/services/new_service/
├── mod.rs          # Module exports
├── operations.rs   # UPnP operations
└── events.rs       # Event parsing (optional)
```

### 1.3 Implement Operations

**Simple operation (no response):**
```rust
define_upnp_operation! {
    operation: DoSomethingOperation,
    action: "DoSomething",
    service: NewService,
    request: { param: String },
    response: (),
    payload: |req| format!(
        "<InstanceID>{}</InstanceID><Param>{}</Param>",
        req.instance_id, req.param
    ),
    parse: |_xml| Ok(()),
}

impl Validate for DoSomethingOperationRequest {}
```

**Operation with response:**
```rust
define_operation_with_response! {
    operation: GetInfoOperation,
    action: "GetInfo",
    service: NewService,
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

impl Validate for GetInfoOperationRequest {}
```

### 1.4 Add Validation

Test against real speakers to discover validation requirements:

```bash
# Valid value
cargo run -p sonos-api --example test_operation -- 192.168.1.100 NewService Action Param=Valid

# Invalid value (HTTP 500 = needs validation)
cargo run -p sonos-api --example test_operation -- 192.168.1.100 NewService Action Param=Invalid
```

```rust
impl Validate for ActionOperationRequest {
    fn validate_basic(&self) -> Result<(), ValidationError> {
        match self.param.as_str() {
            "Valid1" | "Valid2" => Ok(()),
            other => Err(ValidationError::Custom {
                parameter: "param".to_string(),
                message: format!("Invalid param '{}'", other),
            })
        }
    }
}
```

## Layer 2: Stream Implementation

### 2.1 Define Event Struct

Add to `sonos-stream/src/events/types.rs`:

```rust
#[derive(Debug, Clone, Default)]
pub struct NewServiceEvent {
    pub field_one: Option<String>,
    pub field_two: Option<String>,
}

pub enum EventData {
    // ... existing ...
    NewServiceEvent(NewServiceEvent),
}
```

### 2.2 Implement Event Conversion

Add to `sonos-stream/src/events/processor.rs`:

```rust
fn convert_api_event_data(service: Service, api_event: ApiEventData) -> Option<EventData> {
    match service {
        Service::NewService => Some(EventData::NewServiceEvent(NewServiceEvent {
            field_one: extract_field(&api_event, "FieldOne"),
            field_two: extract_field(&api_event, "FieldTwo"),
        })),
        // ...
    }
}
```

## Layer 3: State Implementation

### 3.1 Define Property

Add to `sonos-state/src/property.rs`:

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct NewProperty {
    value: String,
}

impl Property for NewProperty {
    const KEY: &'static str = "new_property";
}

impl SonosProperty for NewProperty {
    const SCOPE: Scope = Scope::Speaker;
    const SERVICE: Service = Service::NewService;
}
```

### 3.2 Add Decoder

Add to `sonos-state/src/decoder.rs`:

```rust
pub enum PropertyChange {
    // ... existing ...
    NewProperty(NewProperty),
}

pub fn decode_new_service(event: &NewServiceEvent) -> Vec<PropertyChange> {
    let mut changes = Vec::new();
    if let Some(ref value) = event.field_one {
        changes.push(PropertyChange::NewProperty(NewProperty::new(value.clone())));
    }
    changes
}
```

## Layer 4: SDK Implementation

### 4.1 Implement Fetchable

Add to `sonos-sdk/src/property/handles.rs`:

```rust
impl Fetchable for NewProperty {
    type Request = GetInfoOperationRequest;
    type Response = GetInfoResponse;

    fn build_request() -> Self::Request {
        GetInfoOperationRequest { instance_id: 0 }
    }

    fn from_response(response: Self::Response) -> Self {
        NewProperty::new(response.field_one)
    }

    fn execute(client: &SonosClient, ip: &str, request: Self::Request) -> Result<Self::Response, SdkError> {
        let op = new_service::get_info().build()
            .map_err(|e| SdkError::FetchFailed(e.to_string()))?;
        client.execute(ip, op).map_err(SdkError::ApiError)
    }
}

pub type NewPropertyHandle = PropertyHandle<NewProperty>;
```

### 4.2 Add to Speaker/System

Based on scope, add to the appropriate struct:

```rust
// Speaker-scoped
pub struct Speaker {
    pub new_property: NewPropertyHandle,
}

// System-scoped
pub struct SonosSystem {
    pub new_property: NewPropertyHandle,
}
```

## Verification

```bash
# Run all tests
cargo test

# Test specific crate
cargo test -p sonos-api

# Check for errors
cargo check

# Lint
cargo clippy

# Format
cargo fmt
```

## Troubleshooting

| Problem | Check |
|---------|-------|
| Operation fails | API implementation, SOAP payload format |
| Events not received | EventData variant, processor case |
| Polling not working | ServicePoller impl, registration |
| State not updating | PropertyChange variant, decoder |
| Property None | Decoder not parsing field |
| fetch() missing | Fetchable trait not implemented |

## Related Documentation

- [Implement Service Skill](../.claude/skills/implement-service/SKILL.md)
- [Add Service Orchestrator](../.claude/skills/add-service/SKILL.md)
- [Macro Patterns](../.claude/skills/implement-service/references/macro-patterns.md)
- [Service Structure](../.claude/skills/implement-service/references/service-structure.md)
