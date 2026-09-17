# Event Types Patterns

## Overview

A service's state type carries the complete state information received from UPnP events or
polling. It lives in **`sonos-api/src/services/{service}/state.rs`** as `{Service}State`.
`sonos-stream/src/events/types.rs` holds only the `EventData` enum that wraps those types.

Both paths converge on the same type — `{Service}Event::into_state()` for UPnP events,
`state::poll()` for polling — which is what keeps events and polling in parity.

## State Struct Pattern

### Basic Structure

```rust
/// Complete {Service} event data containing all state information
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NewServiceState {
    /// Description of what this field represents
    pub field1: Option<String>,

    /// Description of numeric field
    pub field2: Option<u32>,

    /// Boolean state
    pub enabled: Option<String>,  // Note: UPnP sends as string "true"/"false" or "1"/"0"
}
```

### Why Option<T>?

UPnP events often contain partial updates. A speaker might send:
- Only changed fields
- Different fields depending on the event trigger
- Missing fields for unsupported features

Using `Option<T>` allows:
- Partial event deserialization
- Distinguishing between "not present" and "empty value"
- Safe field access without panics

## EventData Enum

`EventData` in `sonos-stream/src/events/types.rs` is the unified payload type. Each variant
is named for its service with **no `Event` suffix**, and wraps the canonical State type
from `sonos-api`:

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

    // Add new variants here
}
```

`EventData` carries no service accessor. The originating `Service` travels on the
enclosing `EnrichedEvent`, so the payload never has to re-derive it — and there is no map
that can disagree with the event's own routing.

## Existing State Type Examples

### AVTransportState (Transport State)

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AVTransportState {
    pub transport_state: Option<String>,      // PLAYING, PAUSED_PLAYBACK, STOPPED
    pub transport_status: Option<String>,     // OK, ERROR_OCCURRED
    pub speed: Option<String>,                // Playback speed
    pub current_track_uri: Option<String>,    // URI of current track
    pub track_duration: Option<String>,       // HH:MM:SS format
    pub rel_time: Option<String>,             // Current position HH:MM:SS
    pub abs_time: Option<String>,             // Absolute time
    pub rel_count: Option<u32>,               // Relative track number
    pub abs_count: Option<u32>,               // Absolute track number
    pub play_mode: Option<String>,            // NORMAL, REPEAT_ALL, SHUFFLE
    pub track_metadata: Option<String>,       // DIDL-Lite XML
    pub next_track_uri: Option<String>,
    pub next_track_metadata: Option<String>,
    pub queue_length: Option<u32>,
}
```

### RenderingControlState (Audio Settings)

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RenderingControlState {
    pub master_volume: Option<String>,        // 0-100 as string
    pub lf_volume: Option<String>,            // Left front channel
    pub rf_volume: Option<String>,            // Right front channel
    pub master_mute: Option<String>,          // "true"/"false" or "1"/"0"
    pub lf_mute: Option<String>,
    pub rf_mute: Option<String>,
    pub bass: Option<String>,                 // -10 to +10
    pub treble: Option<String>,               // -10 to +10
    pub loudness: Option<String>,             // Boolean as string
    pub balance: Option<String>,              // -100 to +100
    pub other_channels: std::collections::HashMap<String, String>,
}
```

### ZoneGroupTopologyState (Complex Nested)

For complex hierarchical data, use nested structs:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ZoneGroupTopologyState {
    pub zone_groups: Vec<ZoneGroupInfo>,
    pub vanished_devices: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ZoneGroupInfo {
    pub coordinator: String,
    pub id: String,
    pub members: Vec<ZoneGroupMemberInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ZoneGroupMemberInfo {
    pub uuid: String,
    pub location: String,
    pub zone_name: String,
    pub software_version: String,
    pub network_info: NetworkInfo,
    pub satellites: Vec<SatelliteInfo>,
}
```

## Field Naming Conventions

| UPnP XML Element | Rust Field Name | Notes |
|-----------------|-----------------|-------|
| `CurrentTransportState` | `transport_state` | Drop "Current" prefix, snake_case |
| `CurrentVolume` | `master_volume` | Add channel qualifier |
| `ZoneName` | `zone_name` | Direct snake_case |
| `WirelessMode` | `wireless_mode` | Direct snake_case |

## Type Conversions

| UPnP Type | Rust Type | Notes |
|-----------|-----------|-------|
| String | `Option<String>` | Default for text |
| ui2, ui4 | `Option<u32>` or `Option<String>` | Often received as string |
| Boolean | `Option<String>` | UPnP sends "1"/"0" or "true"/"false" |
| XML (DIDL-Lite) | `Option<String>` | Store raw, parse later |
| Time (HH:MM:SS) | `Option<String>` | Parse in decoder layer |

## Testing Patterns

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_creation() {
        let event = NewServiceState {
            field1: Some("value".to_string()),
            field2: None,
        };

        assert!(event.field1.is_some());
        assert!(event.field2.is_none());
    }

    #[test]
    fn test_event_data_wraps_state() {
        let event = EventData::NewService(NewServiceState {
            field1: None,
            field2: None,
        });

        assert!(matches!(event, EventData::NewService(_)));
    }

    #[test]
    fn test_serialization_roundtrip() {
        let event = NewServiceState {
            field1: Some("test".to_string()),
            field2: Some(42),
        };

        let json = serde_json::to_string(&event).unwrap();
        let restored: NewServiceState = serde_json::from_str(&json).unwrap();

        assert_eq!(event.field1, restored.field1);
        assert_eq!(event.field2, restored.field2);
    }
}
```

## Checklist for New State Types

- [ ] `{Service}State` defined in `sonos-api/src/services/{service}/state.rs` with
      `#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]`
- [ ] All fields are `Option<T>`
- [ ] Doc comments on struct and fields
- [ ] `{Service}Event::into_state()` in `events.rs` produces it
- [ ] `state::poll()` produces it
- [ ] `EventData::{Service}({Service}State)` variant added
- [ ] `convert_api_event_data()` arm added in `sonos-stream/src/events/processor.rs`
- [ ] Unit tests for creation
- [ ] Serialization roundtrip test — the poller round-trips this type through JSON
