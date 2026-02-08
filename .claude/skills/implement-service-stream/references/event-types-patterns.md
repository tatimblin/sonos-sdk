# Event Types Patterns

## Overview

Event types in sonos-stream represent the complete state information received from UPnP events or polling. They are defined in `sonos-stream/src/events/types.rs`.

## Event Struct Pattern

### Basic Structure

```rust
/// Complete {Service} event data containing all state information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewServiceEvent {
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

The `EventData` enum provides a unified type for all service events:

```rust
pub enum EventData {
    AVTransportEvent(AVTransportEvent),
    RenderingControlEvent(RenderingControlEvent),
    DevicePropertiesEvent(DevicePropertiesEvent),
    ZoneGroupTopologyEvent(ZoneGroupTopologyEvent),
    // Add new variants here
}
```

### service_type() Method

Each variant maps to a `sonos_api::Service`:

```rust
impl EventData {
    pub fn service_type(&self) -> sonos_api::Service {
        match self {
            EventData::AVTransportEvent(_) => sonos_api::Service::AVTransport,
            EventData::RenderingControlEvent(_) => sonos_api::Service::RenderingControl,
            EventData::DevicePropertiesEvent(_) => sonos_api::Service::ZoneGroupTopology,
            EventData::ZoneGroupTopologyEvent(_) => sonos_api::Service::ZoneGroupTopology,
        }
    }
}
```

## Existing Event Struct Examples

### AVTransportEvent (Transport State)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AVTransportEvent {
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

### RenderingControlEvent (Audio Settings)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenderingControlEvent {
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

### ZoneGroupTopologyEvent (Complex Nested)

For complex hierarchical data, use nested structs:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZoneGroupTopologyEvent {
    pub zone_groups: Vec<ZoneGroupInfo>,
    pub vanished_devices: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZoneGroupInfo {
    pub coordinator: String,
    pub id: String,
    pub members: Vec<ZoneGroupMemberInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
        let event = NewServiceEvent {
            field1: Some("value".to_string()),
            field2: None,
        };

        assert!(event.field1.is_some());
        assert!(event.field2.is_none());
    }

    #[test]
    fn test_event_data_service_type() {
        let event = EventData::NewServiceEvent(NewServiceEvent {
            field1: None,
            field2: None,
        });

        assert_eq!(event.service_type(), sonos_api::Service::NewService);
    }

    #[test]
    fn test_serialization_roundtrip() {
        let event = NewServiceEvent {
            field1: Some("test".to_string()),
            field2: Some(42),
        };

        let json = serde_json::to_string(&event).unwrap();
        let restored: NewServiceEvent = serde_json::from_str(&json).unwrap();

        assert_eq!(event.field1, restored.field1);
        assert_eq!(event.field2, restored.field2);
    }
}
```

## Checklist for New Event Types

- [ ] Event struct defined with `#[derive(Debug, Clone, Serialize, Deserialize)]`
- [ ] All fields are `Option<T>`
- [ ] Doc comments on struct and fields
- [ ] EventData variant added
- [ ] service_type() match arm added
- [ ] Unit tests for creation and service_type()
- [ ] Serialization roundtrip test
