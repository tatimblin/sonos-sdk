---
name: implement-service-stream
description: Add event streaming support for a Sonos service in sonos-stream. Implements event types, event processor integration, and polling fallback strategies. Use after implementing the service in sonos-api.
---

# Implement Service Stream Layer

## Overview

This skill adds event streaming support for a UPnP service to the sonos-stream crate. It handles:
1. **State Type** - Define the canonical `{Service}State` in sonos-api
2. **Event Processing** - Convert sonos-api events into `EventData` variants
3. **Polling Fallback** - Implement a polling strategy for firewall scenarios

## Prerequisites

- Service already implemented in sonos-api (operations, events)
- Understanding of what state changes the service produces
- Access to a Sonos speaker for testing

## Quick Start

```bash
# 1. List existing event types
python .claude/skills/implement-service-stream/scripts/analyze_stream_events.py --list

# 2. Check what's already implemented for a service
python .claude/skills/implement-service-stream/scripts/analyze_stream_events.py --service AVTransport

# 3. After implementation, test polling
python .claude/skills/implement-service-stream/scripts/test_polling.py <speaker_ip> NewService
```

## Workflow

### Step 1: Define the Canonical State Type

The canonical per-service state type lives in **sonos-api**, not sonos-stream. Both the
UPnP event path and the polling path produce it, which is what keeps the two in parity.

Add `sonos-api/src/services/new_service/state.rs`:

```rust
//! Canonical NewService service state type.
//!
//! Used by both UPnP event streaming (via `into_state()`) and polling (via `poll()`).

use serde::{Deserialize, Serialize};

use crate::SonosClient;

/// Complete NewService service state.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NewServiceState {
    /// Description of field1
    pub field1: Option<String>,

    /// Description of field2
    pub field2: Option<u32>,
}

/// Poll a speaker for complete NewService state.
pub fn poll(client: &SonosClient, ip: &str) -> crate::Result<NewServiceState> {
    let info = client.execute_enhanced(
        ip,
        super::get_state_operation()
            .build()
            .map_err(|e| crate::ApiError::ParseError(e.to_string()))?,
    )?;

    Ok(NewServiceState {
        field1: Some(info.field1),
        field2: Some(info.field2),
    })
}
```

**Key conventions:**
- Use `Option<T>` for every field — a UPnP event carries only what changed, and a poll may
  not be able to read every field (a property with no Get operation is always `None` when
  polled)
- Derive `Debug, Clone, Serialize, Deserialize, PartialEq`. `Serialize`/`Deserialize` are
  load-bearing: the poller round-trips this type through JSON to diff snapshots
- Add doc comments for each field
- The UPnP event struct `NewServiceEvent` lives in `events.rs` and converts to this type
  through `into_state()`

### Step 2: Add EventData Variant

Update the `EventData` enum in `sonos-stream/src/events/types.rs`. The variant is named
for the service with **no `Event` suffix**, and it wraps the sonos-api State type:

```rust
pub enum EventData {
    /// AVTransport service state
    AVTransport(AVTransportState),

    /// RenderingControl service state
    RenderingControl(RenderingControlState),

    /// ZoneGroupTopology service state
    ZoneGroupTopology(ZoneGroupTopologyState),

    /// GroupManagement service state
    GroupManagement(GroupManagementState),

    /// GroupRenderingControl service state
    GroupRenderingControl(GroupRenderingControlState),

    /// NewService service state
    NewService(NewServiceState),  // Add new variant
}
```

`EventData` has no `service_type()` method. The originating service travels on the
enclosing `EnrichedEvent`, not on the payload.

### Step 3: Add Event Processor Case

Update `convert_api_event_data()` in `sonos-stream/src/events/processor.rs`. The incoming
event is type-erased, so the arm downcasts it and calls `into_state()`:

```rust
fn convert_api_event_data(
    &self,
    service: &sonos_api::Service,
    api_event_data: Box<dyn std::any::Any + Send + Sync>,
) -> EventProcessingResult<EventData> {
    match service {
        // ... existing matches ...

        sonos_api::Service::NewService => {
            let event = api_event_data
                .downcast::<sonos_api::services::new_service::NewServiceEvent>()
                .map_err(|_| {
                    EventProcessingError::Parsing(
                        "Failed to downcast NewService event".to_string(),
                    )
                })?;
            Ok(EventData::NewService(event.into_state()))
        }
    }
}
```

The match is exhaustive over `Service`, so adding a `Service` variant without this arm is
a compile error rather than a silent drop.

### Step 4: Implement Polling Strategy

Add a poller in `sonos-stream/src/polling/strategies.rs`. The trait has exactly three
methods — `poll_state`, `state_to_event_data` and `service_type`. Change detection is the
scheduler's job: it diffs consecutive JSON snapshots, so a poller never compares fields
itself.

```rust
/// Polling strategy for NewService.
///
/// Delegates to `sonos_api::services::new_service::state::poll()`.
pub struct NewServicePoller;

#[async_trait]
impl ServicePoller for NewServicePoller {
    async fn poll_state(
        &self,
        client: &SonosClient,
        pair: &SpeakerServicePair,
    ) -> PollingResult<String> {
        let client = client.clone();
        let ip = pair.speaker_ip.to_string();

        // `poll()` is blocking (ureq), so it must not run on the async worker.
        let state = tokio::task::spawn_blocking(move || {
            sonos_api::services::new_service::state::poll(&client, &ip)
        })
        .await
        .map_err(|e| PollingError::Network(format!("Polling task panicked: {e}")))?
        .map_err(|e| PollingError::Network(e.to_string()))?;

        serde_json::to_string(&state)
            .map_err(|e| PollingError::StateParsing(format!("Failed to serialize state: {e}")))
    }

    fn state_to_event_data(&self, json_state: &str) -> PollingResult<EventData> {
        let state: sonos_api::services::new_service::state::NewServiceState =
            serde_json::from_str(json_state).map_err(|e| {
                PollingError::StateParsing(format!("Failed to deserialize NewService state: {e}"))
            })?;
        Ok(EventData::NewService(state))
    }

    fn service_type(&self) -> Service {
        Service::NewService
    }
}
```

An action-only service (one with no Get operations, like GroupManagement) still needs a
poller: return a stable empty state so the scheduler never sees a diff and never emits a
spurious change event.

Register the poller in `DeviceStatePoller::new()`:

```rust
impl DeviceStatePoller {
    pub fn new() -> Self {
        let mut service_pollers: HashMap<Service, Box<dyn ServicePoller>> = HashMap::new();

        service_pollers.insert(Service::AVTransport, Box::new(AVTransportPoller));
        service_pollers.insert(Service::RenderingControl, Box::new(RenderingControlPoller));
        service_pollers.insert(Service::ZoneGroupTopology, Box::new(ZoneGroupTopologyPoller));
        service_pollers.insert(Service::GroupManagement, Box::new(GroupManagementPoller));
        service_pollers.insert(
            Service::GroupRenderingControl,
            Box::new(GroupRenderingControlPoller),
        );
        service_pollers.insert(Service::NewService, Box::new(NewServicePoller));  // Add

        Self {
            service_pollers,
            sonos_client: SonosClient::new(),
        }
    }
}
```

### Step 5: Add Tests

Add tests in the appropriate `#[cfg(test)]` modules. Assert the round trip a poller
depends on:

```rust
#[test]
fn test_new_service_state_round_trips_through_json() {
    let state = NewServiceState {
        field1: Some("value".to_string()),
        field2: Some(42),
    };

    let json = serde_json::to_string(&state).unwrap();
    let decoded = NewServicePoller.state_to_event_data(&json).unwrap();

    match decoded {
        EventData::NewService(s) => assert_eq!(s, state),
        other => panic!("wrong variant: {other:?}"),
    }
}
```

### Step 6: Verify

```bash
# Run tests
cargo test -p sonos-sdk-stream

# Test polling against real speaker
python .claude/skills/implement-service-stream/scripts/test_polling.py <speaker_ip> NewService
```

## Files Modified

| File | Changes |
|------|---------|
| `sonos-api/src/services/{service}/state.rs` | Add `{Service}State` + `poll()` |
| `sonos-api/src/services/{service}/events.rs` | Add `{Service}Event` + `into_state()` |
| `sonos-stream/src/events/types.rs` | Add EventData variant wrapping `{Service}State` |
| `sonos-stream/src/events/processor.rs` | Add case in `convert_api_event_data()` |
| `sonos-stream/src/polling/strategies.rs` | Add ServicePoller impl + register |

## Common Issues

### Event Fields Mismatch
If the sonos-api event has different field names than expected, check:
- The actual XML structure from UPnP events
- The getter methods on the sonos-api event struct

### Polling Not Detecting Changes
- Ensure state serialization is deterministic — the scheduler diffs JSON strings, so a
  non-deterministic field order looks like a change on every poll
- Check that `poll()` reads all relevant state
- A field with no Get operation is always `None` when polled and can only change via events

## References

- [Event Types Patterns](references/event-types-patterns.md)
- [Processor Patterns](references/processor-patterns.md)
- [Polling Strategy Patterns](references/polling-strategy-patterns.md)
