# Adding New Sonos Services

This guide explains how to add support for a new Sonos UPnP service to the SDK. The implementation spans 4 layers, each with specific responsibilities.

## Overview

Adding a service requires implementing across these layers:

| Layer | Crate | Purpose | Key Files |
|-------|-------|---------|-----------|
| 1. API | `sonos-api` | UPnP SOAP operations, event parsing, canonical state | `service.rs`, `services/{service}/{operations,events,state}.rs` |
| 2. Stream | `sonos-stream` | Event streaming/polling | `events/types.rs`, `events/processor.rs`, `polling/strategies.rs` |
| 3. State | `sonos-state` | Reactive state store | `property.rs`, `decoder.rs` |
| 4. SDK | `sonos-sdk` | DOM-like public API | `property/handles.rs`, `speaker.rs`, `group.rs` |

The canonical per-service state type lives in `sonos-api`, not `sonos-stream`. Both the
UPnP event path (`{Service}Event::into_state()`) and the polling path (`poll()`) produce
the same `{Service}State`, which is what keeps the two in parity.

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
├── events.rs       # UPnP event parsing + `into_state()`
└── state.rs        # `NewServiceState` + `poll()`
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

### 2.1 Define the Canonical State Type

Add to `sonos-api/src/services/new_service/state.rs`. Every field is `Option`, because a
UPnP event carries only the properties that changed:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NewServiceState {
    pub field_one: Option<String>,
    pub field_two: Option<u16>,
}

/// Poll a speaker for complete NewService state.
pub fn poll(client: &SonosClient, ip: &str) -> crate::Result<NewServiceState> {
    let info = client.execute_enhanced(
        ip,
        super::get_info_operation()
            .build()
            .map_err(|e| crate::ApiError::ParseError(e.to_string()))?,
    )?;

    Ok(NewServiceState {
        field_one: Some(info.field_one),
        field_two: Some(info.field_two),
    })
}
```

`NewServiceEvent` in `events.rs` deserializes the UPnP event payload and converts
to the same type through `into_state()`.

### 2.2 Add the EventData Variant

Add to `sonos-stream/src/events/types.rs`. The variant is named for the service with **no
`Event` suffix**, and it wraps the `sonos-api` State type:

```rust
pub enum EventData {
    // ... existing ...
    NewService(NewServiceState),
}
```

### 2.3 Implement Event Conversion

Add an arm to `convert_api_event_data` in `sonos-stream/src/events/processor.rs`. The
incoming event is type-erased, so the arm downcasts it and calls `into_state()`:

```rust
sonos_api::Service::NewService => {
    let event = api_event_data
        .downcast::<sonos_api::services::new_service::NewServiceEvent>()
        .map_err(|_| {
            EventProcessingError::Parsing("Failed to downcast NewService event".to_string())
        })?;
    Ok(EventData::NewService(event.into_state()))
}
```

The match on `Service` is exhaustive, so adding a `Service` variant without this arm is a
compile error rather than a silent drop.

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

    // Required for any watchable property. `to_change()` defaults to `None`,
    // and a property that returns `None` still writes to the store but emits
    // no `ChangeEvent` — so `watch()` on it never fires. Only `Topology`,
    // which is written wholesale by `initialize()`, legitimately omits this.
    fn to_change(&self) -> Option<crate::decoder::PropertyChange> {
        Some(crate::decoder::PropertyChange::NewProperty(self.clone()))
    }
}
```

### 3.2 Add Decoder

Add to `sonos-state/src/decoder.rs`:

```rust
pub enum PropertyChange {
    // ... existing ...
    NewProperty(NewProperty),
}

fn decode_new_service(event: &NewServiceState) -> Vec<PropertyChange> {
    let mut changes = Vec::new();
    if let Some(ref value) = event.field_one {
        changes.push(PropertyChange::NewProperty(NewProperty::new(value.clone())));
    }
    changes
}
```

Then wire it into `decode_event`, and add the arms `PropertyChange::key()` and
`PropertyChange::service()` need:

```rust
pub fn decode_event(event: &EnrichedEvent, speaker_id: SpeakerId) -> DecodedChanges {
    let changes = match &event.event_data {
        // ... existing ...
        EventData::NewService(ns) => decode_new_service(ns),
    };

    DecodedChanges { speaker_id, changes }
}
```

## Layer 4: SDK Implementation

### 4.1 Implement Fetchable

Add to `sonos-sdk/src/property/handles.rs`:

`Fetchable` names the operation and converts its response. It does not execute anything —
`PropertyHandle::fetch()` owns the call, so there is no `execute` to write:

```rust
impl Fetchable for NewProperty {
    type Operation = GetInfoOperation;

    fn build_operation() -> Result<ComposableOperation<Self::Operation>, SdkError> {
        new_service::get_info_operation()
            .build()
            .map_err(|e| SdkError::FetchFailed(e.to_string()))
    }

    fn from_response(response: GetInfoResponse) -> Self {
        NewProperty::new(response.field_one)
    }
}

pub type NewPropertyHandle = PropertyHandle<NewProperty>;
```

Two variants exist for properties that do not fit:

| Trait | Use when | Differences |
|-------|----------|-------------|
| `Fetchable` | The response is this property, for this speaker | — |
| `FetchableWithContext` | The response covers several speakers and the right one must be picked out (e.g. `GroupMembership` from `GetZoneGroupState`) | `from_response_with_context(response, &speaker_id) -> Option<Self>` |
| `GroupFetchable` | The property is group-scoped and aliases `GroupPropertyHandle` | Handle type is `GroupPropertyHandle<P>` |

A property with no Get operation (like `GroupVolumeChangeable`) implements none of these.
It is event-only: `get()` and `watch()` work, `fetch()` does not exist.

### 4.2 Add to Speaker/System

Based on scope, add to the appropriate struct:

```rust
// Speaker-scoped — sonos-sdk/src/speaker.rs
pub struct Speaker {
    pub new_property: NewPropertyHandle,
}

// Group-scoped — sonos-sdk/src/group.rs
pub struct Group {
    pub new_property: NewGroupPropertyHandle,
}
```

System-scoped properties (`Topology`) have no handle; they are read off `SonosSystem`
directly.

## Verification

```bash
# Run all tests. The feature flag is required: sonos-sdk's
# tests/property_tests.rs does not compile without it, so bare
# `cargo test` fails. See docs/CONTRIBUTING.md.
cargo test --workspace --features sonos-sdk/test-support --locked

# Test specific crate
cargo test -p sonos-api

# Check for errors
cargo check --workspace

# Lint
cargo clippy

# Format
cargo fmt

# Confirm the tree agrees with docs/STATUS.md, then update STATUS.md
python .claude/skills/add-service/scripts/service_status.py --all
```

## Troubleshooting

| Problem | Check |
|---------|-------|
| Operation fails | API implementation, SOAP payload format |
| Events not received | EventData variant, processor case |
| Polling not working | ServicePoller impl, registration |
| State not updating | PropertyChange variant, decoder |
| Property None | Decoder not parsing field |
| fetch() missing | `Fetchable`/`GroupFetchable`/`FetchableWithContext` not implemented |
| watch() never fires | `to_change()` not overridden — it defaults to `None` |

## Related Documentation

- [Implement Service Skill](../.claude/skills/implement-service/SKILL.md)
- [Add Service Orchestrator](../.claude/skills/add-service/SKILL.md)
- [Macro Patterns](../.claude/skills/implement-service/references/macro-patterns.md)
- [Service Structure](../.claude/skills/implement-service/references/service-structure.md)
