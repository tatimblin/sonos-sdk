---
name: implement-service-stream
description: Add event streaming support for a Sonos service in sonos-stream. Implements event types, event processor integration, and polling fallback strategies. Use after implementing the service in sonos-api.
---

# Implement Service Stream Layer

## Overview

This skill adds event streaming support for a UPnP service to the sonos-stream crate. It handles:
1. **Event Types** - Define structs for service events in `types.rs`
2. **Event Processing** - Convert sonos-api events to sonos-stream events
3. **Polling Fallback** - Implement polling strategy for firewall scenarios

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

### Step 1: Define Event Struct

Add a new event struct in `sonos-stream/src/events/types.rs`:

```rust
/// Complete NewService event data containing all state information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewServiceEvent {
    /// Description of field1
    pub field1: Option<String>,

    /// Description of field2
    pub field2: Option<u32>,

    // Add all relevant state fields from the service
}
```

**Key conventions:**
- Use `Option<T>` for all fields (events may contain partial updates)
- Derive `Debug, Clone, Serialize, Deserialize`
- Add doc comments for each field
- Match field names to sonos-api event structure

### Step 2: Add EventData Variant

Update the `EventData` enum in `types.rs`:

```rust
pub enum EventData {
    AVTransportEvent(AVTransportEvent),
    RenderingControlEvent(RenderingControlEvent),
    DevicePropertiesEvent(DevicePropertiesEvent),
    ZoneGroupTopologyEvent(ZoneGroupTopologyEvent),
    NewServiceEvent(NewServiceEvent),  // Add new variant
}
```

Update `EventData::service_type()`:

```rust
impl EventData {
    pub fn service_type(&self) -> sonos_api::Service {
        match self {
            // ... existing matches ...
            EventData::NewServiceEvent(_) => sonos_api::Service::NewService,
        }
    }
}
```

### Step 3: Add Event Processor Case

Update `convert_api_event_data()` in `sonos-stream/src/events/processor.rs`:

```rust
fn convert_api_event_data(
    &self,
    service: &sonos_api::Service,
    api_event_data: Box<dyn std::any::Any + Send + Sync>,
) -> EventProcessingResult<EventData> {
    match service {
        // ... existing matches ...

        sonos_api::Service::NewService => {
            let api_event = api_event_data
                .downcast::<sonos_api::services::new_service::NewServiceEvent>()
                .map_err(|_| EventProcessingError::Parsing(
                    "Failed to downcast NewService event".to_string()
                ))?;

            let stream_event = crate::events::types::NewServiceEvent {
                field1: api_event.field1().map(|s| s.to_string()),
                field2: api_event.field2(),
            };

            Ok(EventData::NewServiceEvent(stream_event))
        }
    }
}
```

### Step 4: Implement Polling Strategy

Add a poller in `sonos-stream/src/polling/strategies.rs`:

```rust
/// Poller for NewService
pub struct NewServicePoller;

#[async_trait]
impl ServicePoller for NewServicePoller {
    async fn poll_state(
        &self,
        client: &SonosClient,
        pair: &SpeakerServicePair,
    ) -> PollingResult<String> {
        // Build and execute the appropriate Get operation
        let operation = new_service::get_state_operation()
            .build()
            .map_err(|e| PollingError::Network(e.to_string()))?;

        let response = tokio::task::spawn_blocking({
            let client = client.clone();
            let ip = pair.ip.to_string();
            move || client.execute_enhanced(&ip, operation)
        })
        .await
        .map_err(|e| PollingError::Network(e.to_string()))?
        .map_err(|e| PollingError::Network(e.to_string()))?;

        // Serialize state for comparison
        let state = serde_json::json!({
            "field1": response.field1,
            "field2": response.field2,
        });

        Ok(serde_json::to_string(&state).unwrap_or_default())
    }

    async fn parse_for_changes(
        &self,
        old_state: &str,
        new_state: &str,
    ) -> Vec<StateChange> {
        let mut changes = vec![];

        let old: serde_json::Value = serde_json::from_str(old_state).unwrap_or_default();
        let new: serde_json::Value = serde_json::from_str(new_state).unwrap_or_default();

        // Compare each field
        if old["field1"] != new["field1"] {
            changes.push(StateChange {
                field: "field1".to_string(),
                old_value: old["field1"].to_string(),
                new_value: new["field1"].to_string(),
            });
        }

        changes
    }

    fn service_type(&self) -> Service {
        Service::NewService
    }
}
```

Register the poller in `DeviceStatePoller::new()`:

```rust
impl DeviceStatePoller {
    pub fn new() -> Self {
        let mut service_pollers: HashMap<Service, Box<dyn ServicePoller>> = HashMap::new();

        service_pollers.insert(Service::AVTransport, Box::new(AVTransportPoller));
        service_pollers.insert(Service::RenderingControl, Box::new(RenderingControlPoller));
        service_pollers.insert(Service::NewService, Box::new(NewServicePoller));  // Add

        // ...
    }
}
```

### Step 5: Add Tests

Add tests in the appropriate `#[cfg(test)]` modules:

```rust
#[test]
fn test_new_service_event_creation() {
    let event = NewServiceEvent {
        field1: Some("value".to_string()),
        field2: Some(42),
    };

    let event_data = EventData::NewServiceEvent(event);
    assert_eq!(event_data.service_type(), sonos_api::Service::NewService);
}
```

### Step 6: Verify

```bash
# Run tests
cargo test -p sonos-stream

# Test polling against real speaker
python .claude/skills/implement-service-stream/scripts/test_polling.py <speaker_ip> NewService
```

## Files Modified

| File | Changes |
|------|---------|
| `sonos-stream/src/events/types.rs` | Add event struct + EventData variant |
| `sonos-stream/src/events/processor.rs` | Add case in `convert_api_event_data()` |
| `sonos-stream/src/polling/strategies.rs` | Add ServicePoller impl + register |

## Common Issues

### Event Fields Mismatch
If the sonos-api event has different field names than expected, check:
- The actual XML structure from UPnP events
- The getter methods on the sonos-api event struct

### Polling Not Detecting Changes
- Ensure state serialization is deterministic
- Check that the Get operation returns all relevant state
- Verify field comparison logic

## References

- [Event Types Patterns](references/event-types-patterns.md)
- [Processor Patterns](references/processor-patterns.md)
- [Polling Strategy Patterns](references/polling-strategy-patterns.md)
