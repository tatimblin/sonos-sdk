---
name: implement-service-state
description: Add reactive state management for a Sonos service in sonos-state. Defines property types, implements decoders, and integrates with the state store. Use after implementing the service in sonos-stream.
---

# Implement Service State Layer

## Overview

This skill adds reactive state management for a UPnP service to the sonos-state crate. It handles:
1. **Property Types** - Define structs for each property with Property/SonosProperty traits
2. **Decoders** - Convert sonos-stream events to typed property changes
3. **State Integration** - Wire properties into the reactive state system

## Prerequisites

- Service implemented in sonos-api (operations)
- Service implemented in sonos-stream (event types, polling)
- Understanding of what properties the service exposes

## Quick Start

```bash
# 1. List existing properties
python .claude/skills/implement-service-state/scripts/analyze_properties.py --list

# 2. Check property coverage for a service
python .claude/skills/implement-service-state/scripts/analyze_properties.py --service RenderingControl

# 3. After implementation, validate decoder
python .claude/skills/implement-service-state/scripts/validate_decoder.py sample_event.json
```

## Workflow

### Step 1: Define Property Structs

Add property types in `sonos-state/src/property.rs`:

```rust
// ============================================================================
// NewService Properties
// ============================================================================

/// Description of what this property represents
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewProperty(pub ValueType);

impl Property for NewProperty {
    const KEY: &'static str = "new_property";
}

impl SonosProperty for NewProperty {
    const SCOPE: Scope = Scope::Speaker;  // or Group, System
    const SERVICE: Service = Service::NewService;
}

impl NewProperty {
    /// Create a new NewProperty value
    pub fn new(value: ValueType) -> Self {
        Self(value)
    }

    /// Get the current value
    pub fn value(&self) -> ValueType {
        self.0
    }
}
```

### Step 2: Add PropertyChange Variants

Update the `PropertyChange` enum in `sonos-state/src/decoder.rs`:

```rust
pub enum PropertyChange {
    Volume(Volume),
    Mute(Mute),
    Bass(Bass),
    // ... existing variants ...
    NewProperty(NewProperty),  // Add new variant
}
```

Update `PropertyChange::key()`:

```rust
pub fn key(&self) -> &'static str {
    use crate::property::Property;
    match self {
        // ... existing matches ...
        PropertyChange::NewProperty(_) => NewProperty::KEY,
    }
}
```

Update `PropertyChange::service()`:

```rust
pub fn service(&self) -> Service {
    use crate::property::SonosProperty;
    match self {
        // ... existing matches ...
        PropertyChange::NewProperty(_) => NewProperty::SERVICE,
    }
}
```

### Step 3: Implement Decoder Function

Add a decoder function in `sonos-state/src/decoder.rs`:

```rust
/// Decode NewService event data
fn decode_new_service(event: &NewServiceEvent) -> Vec<PropertyChange> {
    let mut changes = vec![];

    // Parse each field from the event
    if let Some(value_str) = &event.field1 {
        if let Ok(value) = value_str.parse::<ValueType>() {
            changes.push(PropertyChange::NewProperty(NewProperty(value)));
        }
    }

    // Boolean fields
    if let Some(bool_str) = &event.enabled {
        let enabled = bool_str == "1" || bool_str.eq_ignore_ascii_case("true");
        changes.push(PropertyChange::Enabled(Enabled(enabled)));
    }

    changes
}
```

### Step 4: Register Decoder in decode_event()

Update `decode_event()` to handle the new service:

```rust
pub fn decode_event(event: &EnrichedEvent, speaker_id: SpeakerId) -> DecodedChanges {
    let changes = match &event.event_data {
        EventData::RenderingControlEvent(rc) => decode_rendering_control(rc),
        EventData::AVTransportEvent(avt) => decode_av_transport(avt),
        EventData::ZoneGroupTopologyEvent(zgt) => decode_topology(zgt),
        EventData::DevicePropertiesEvent(_) => vec![],
        EventData::NewServiceEvent(ns) => decode_new_service(ns),  // Add
    };

    DecodedChanges { speaker_id, changes }
}
```

### Step 5: Re-export Properties

Update `sonos-state/src/lib.rs` to export new properties:

```rust
pub use property::{
    // ... existing exports ...
    NewProperty,
};
```

### Step 6: Add Tests

Add tests in `sonos-state/src/decoder.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_new_service() {
        let event = NewServiceEvent {
            field1: Some("42".to_string()),
            enabled: Some("true".to_string()),
        };

        let changes = decode_new_service(&event);

        assert!(changes.len() >= 1);
        if let PropertyChange::NewProperty(prop) = &changes[0] {
            assert_eq!(prop.value(), 42);
        }
    }

    #[test]
    fn test_new_property_key() {
        use crate::property::Property;
        let change = PropertyChange::NewProperty(NewProperty(42));
        assert_eq!(change.key(), NewProperty::KEY);
    }
}
```

### Step 7: Verify

```bash
# Run tests
cargo test -p sonos-state

# Check property coverage
python .claude/skills/implement-service-state/scripts/analyze_properties.py --coverage
```

## Files Modified

| File | Changes |
|------|---------|
| `sonos-state/src/property.rs` | Add property struct with traits |
| `sonos-state/src/decoder.rs` | Add PropertyChange variant + decoder function |
| `sonos-state/src/lib.rs` | Re-export new property |

## Property Type Guidelines

### Value Types by Use Case

| Use Case | Type | Example |
|----------|------|---------|
| Percentage (0-100) | `u8` | Volume, Brightness |
| Signed range (-10 to +10) | `i8` | Bass, Treble |
| Boolean state | `bool` | Mute, Loudness |
| Text value | `String` | TrackTitle, Artist |
| Timestamp (ms) | `u64` | Position, Duration |
| Complex data | Custom struct | CurrentTrack, GroupMembership |

### Scope Selection

| Scope | When to Use |
|-------|-------------|
| `Scope::Speaker` | Property is per-speaker (Volume, Mute) |
| `Scope::Group` | Property applies to speaker group (GroupVolume) |
| `Scope::System` | Property is system-wide (Topology) |

## Common Issues

### Parse Errors Silently Ignored
The decoder silently skips fields that fail to parse. If values aren't appearing:
1. Check the raw event data format
2. Verify parse logic matches actual values
3. Add tracing to debug parsing

### Missing Properties in State
If properties aren't updating:
1. Verify decoder is called (add logging)
2. Check PropertyChange variant is handled
3. Ensure StateStore applies the change

## References

- [Property Patterns](references/property-patterns.md)
- [Decoder Patterns](references/decoder-patterns.md)
