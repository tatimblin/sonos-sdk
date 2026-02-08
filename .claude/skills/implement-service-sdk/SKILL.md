---
name: implement-service-sdk
description: Expose service properties through the DOM-like sonos-sdk API. Implements Fetchable trait, type aliases, and Speaker struct fields. Use after implementing the service in sonos-state.
---

# Implement Service SDK Layer

## Overview

This skill exposes service properties through the DOM-like sonos-sdk API. It handles:
1. **Fetchable Trait** - Implement for properties that can be fetched via API calls
2. **Type Aliases** - Create convenient handle type aliases
3. **Speaker Fields** - Add property handles to the Speaker struct

## Prerequisites

- Service implemented in sonos-api (operations)
- Service implemented in sonos-stream (event types)
- Service implemented in sonos-state (properties)
- Understanding of which properties can be fetched (have dedicated API operations)

## Quick Start

```bash
# 1. List existing handles and check coverage
python .claude/skills/implement-service-sdk/scripts/analyze_handles.py --list

# 2. Check which state properties have SDK handles
python .claude/skills/implement-service-sdk/scripts/analyze_handles.py --coverage

# 3. After implementation, test property access
python .claude/skills/implement-service-sdk/scripts/test_sdk_property.py <speaker_ip> volume --get
```

## Workflow

### Step 1: Determine Fetchability

Not all properties can be fetched directly. Check if your property has a dedicated UPnP operation:

| Property Can Be Fetched | If It Has |
|-------------------------|-----------|
| Yes | Dedicated Get/GetXxx operation in sonos-api |
| No | Only available via events (LastChange) |

**Fetchable properties** (examples):
- Volume - has `GetVolumeOperation`
- PlaybackState - has `GetTransportInfoOperation`
- Position - has `GetPositionInfoOperation`

**Non-fetchable properties** (event-only):
- Mute, Bass, Treble, Loudness - from RenderingControl events
- CurrentTrack - from AVTransport events
- GroupMembership - from ZoneGroupTopology events

### Step 2: Implement Fetchable Trait (if applicable)

Add to `sonos-sdk/src/property/handles.rs`:

```rust
use sonos_api::services::new_service::{
    self, GetNewPropertyOperation, GetNewPropertyResponse,
};
use sonos_state::NewProperty;

impl Fetchable for NewProperty {
    type Operation = GetNewPropertyOperation;

    fn build_operation() -> Result<ComposableOperation<Self::Operation>, SdkError> {
        new_service::get_new_property_operation()
            .build()
            .map_err(|e| build_error("GetNewProperty", e))
    }

    fn from_response(response: GetNewPropertyResponse) -> Self {
        NewProperty::new(response.value_field)
    }
}
```

### Step 3: Add Type Alias

Add to the type aliases section in `sonos-sdk/src/property/handles.rs`:

```rust
// ============================================================================
// Type aliases
// ============================================================================

/// Handle for new property
pub type NewPropertyHandle = PropertyHandle<NewProperty>;
```

### Step 4: Add Imports

Update imports in `sonos-sdk/src/property/handles.rs`:

```rust
use sonos_api::services::{
    // ... existing imports ...
    new_service::{self, GetNewPropertyOperation, GetNewPropertyResponse},
};
use sonos_state::{
    // ... existing imports ...
    NewProperty,
};
```

### Step 5: Add to Speaker Struct

Update `sonos-sdk/src/speaker.rs`:

```rust
use crate::property::{
    // ... existing imports ...
    NewPropertyHandle,
};

pub struct Speaker {
    // ... existing fields ...

    // ========================================================================
    // NewService properties
    // ========================================================================
    /// Description of the property
    pub new_property: NewPropertyHandle,
}
```

### Step 6: Initialize in Speaker::new()

Update the `Speaker::new()` constructor:

```rust
impl Speaker {
    pub fn new(/* params */) -> Self {
        let context = SpeakerContext::new(id.clone(), ip, state_manager, api_client);

        Self {
            // ... existing initializations ...
            // NewService properties
            new_property: PropertyHandle::new(Arc::clone(&context)),
        }
    }
}
```

### Step 7: Re-export (if needed)

Update `sonos-sdk/src/lib.rs` to export new types:

```rust
pub use property::{
    // ... existing exports ...
    NewPropertyHandle,
};
```

### Step 8: Add Tests

Add tests in `sonos-sdk/src/property/handles.rs`:

```rust
#[cfg(test)]
mod tests {
    // Test PropertyHandle creation
    #[test]
    fn test_new_property_handle_creation() {
        let state_manager = create_test_state_manager();
        let context = create_test_context(state_manager);

        let handle: NewPropertyHandle = PropertyHandle::new(context);

        assert_eq!(handle.speaker_id().as_str(), "RINCON_TEST123");
    }

    // Test get() returns cached value
    #[test]
    fn test_new_property_get_cached() {
        let state_manager = create_test_state_manager();
        let speaker_id = SpeakerId::new("RINCON_TEST123");

        state_manager.set_property(&speaker_id, NewProperty::new(42));

        let context = create_test_context(Arc::clone(&state_manager));
        let handle: NewPropertyHandle = PropertyHandle::new(context);

        assert_eq!(handle.get(), Some(NewProperty::new(42)));
    }
}
```

### Step 9: Verify

```bash
# Run tests
cargo test -p sonos-sdk

# Check coverage
python .claude/skills/implement-service-sdk/scripts/analyze_handles.py --coverage
```

## Files Modified

| File | Changes |
|------|---------|
| `sonos-sdk/src/property/handles.rs` | Add Fetchable impl (if applicable), type alias, imports |
| `sonos-sdk/src/speaker.rs` | Add field to Speaker struct, initialize in new() |
| `sonos-sdk/src/lib.rs` | Re-export new types (optional) |

## Fetchable Implementation Guidelines

### Build Operation Pattern

```rust
fn build_operation() -> Result<ComposableOperation<Self::Operation>, SdkError> {
    // Use the operation builder from sonos-api
    service_module::operation_function(/* args if any */)
        .build()
        .map_err(|e| build_error("OperationName", e))
}
```

### From Response Pattern

```rust
fn from_response(response: OperationResponse) -> Self {
    // Convert API response to property type
    PropertyType::new(response.relevant_field)
}
```

### With Parsing

```rust
fn from_response(response: GetPositionInfoResponse) -> Self {
    // Parse values if needed
    let position_ms = Position::parse_time_to_ms(&response.rel_time).unwrap_or(0);
    let duration_ms = Position::parse_time_to_ms(&response.track_duration).unwrap_or(0);
    Position::new(position_ms, duration_ms)
}
```

### With Enum Mapping

```rust
fn from_response(response: GetTransportInfoResponse) -> Self {
    match response.current_transport_state.as_str() {
        "PLAYING" => PlaybackState::Playing,
        "PAUSED" | "PAUSED_PLAYBACK" => PlaybackState::Paused,
        "STOPPED" => PlaybackState::Stopped,
        _ => PlaybackState::Transitioning,
    }
}
```

## Common Issues

### Missing Fetchable Implementation
If `fetch()` is not available, the property is event-only. Document this in the handles.rs comments.

### Type Mismatch in from_response
Ensure the response field type matches what the property constructor expects. Use parsing if needed.

### Speaker Field Not Working
Verify:
1. Type alias is defined
2. Import is added to speaker.rs
3. Field is initialized with `PropertyHandle::new(Arc::clone(&context))`

## References

- [Property Handle Patterns](references/property-handle-patterns.md)
