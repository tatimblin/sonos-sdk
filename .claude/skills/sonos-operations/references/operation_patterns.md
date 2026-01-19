# Sonos Operation Patterns

This guide covers common patterns for Sonos operations, their request structures, and typical parameter values.

## Common Parameter Types

### Instance ID
- **Type**: `u32` (usually 0)
- **Usage**: Most operations require `instance_id: 0`
- **Purpose**: Identifies the service instance on the device

### Speed Parameter
- **Type**: `String`
- **Common values**: `"1"` (normal speed)
- **Usage**: Playback control operations

### Volume Parameters
- **Type**: `u32`
- **Range**: 0-100
- **Usage**: Volume control operations

### URI Parameters
- **Type**: `String`
- **Examples**:
  - Local files: `"file:///path/to/file.mp3"`
  - Streaming services: `"x-sonos-spotify:..."`
  - Radio: `"x-sonosapi-stream:..."`

## Service-Specific Patterns

### AVTransport Operations

#### Playback Control
```rust
// Play operation - minimal parameters
PlayRequest {
    instance_id: 0,
    speed: "1".to_string()
}

// Pause operation - just instance ID
PauseRequest {
    instance_id: 0
}

// Stop operation - just instance ID
StopRequest {
    instance_id: 0
}
```

#### Transport Information
```rust
// Get current transport info
GetTransportInfoRequest {
    instance_id: 0
}

// Get position info (current track, time, etc.)
GetPositionInfoRequest {
    instance_id: 0
}
```

#### Media Management
```rust
// Set media URI
SetAVTransportURIRequest {
    instance_id: 0,
    current_uri: "x-sonos-spotify:spotify%3atrack%3a...".to_string(),
    current_uri_metadata: "<DIDL-Lite>...</DIDL-Lite>".to_string()
}

// Seek to position
SeekRequest {
    instance_id: 0,
    unit: "TRACK_NR".to_string(), // or "REL_TIME"
    target: "5".to_string() // track number or time (HH:MM:SS)
}
```

### RenderingControl Operations

#### Volume Control
```rust
// Set volume
SetVolumeRequest {
    instance_id: 0,
    channel: "Master".to_string(), // or "LF", "RF"
    desired_volume: 50
}

// Get volume
GetVolumeRequest {
    instance_id: 0,
    channel: "Master".to_string()
}
```

#### Mute Control
```rust
// Set mute state
SetMuteRequest {
    instance_id: 0,
    channel: "Master".to_string(),
    desired_mute: true
}
```

#### Bass/Treble/Loudness
```rust
// Set bass level
SetBassRequest {
    instance_id: 0,
    desired_bass: 2 // Range: -10 to 10
}

// Set treble level
SetTrebleRequest {
    instance_id: 0,
    desired_treble: -1 // Range: -10 to 10
}

// Set loudness
SetLoudnessRequest {
    instance_id: 0,
    channel: "Master".to_string(),
    desired_loudness: true
}
```

### DeviceProperties Operations

#### System Information
```rust
// Get zone information
GetZoneInfoRequest {
    // No parameters needed
}

// Get zone attributes
GetZoneAttributesRequest {
    // No parameters needed
}
```

### ZoneGroupTopology Operations

#### Group Management
```rust
// Get zone group state
GetZoneGroupStateRequest {
    // No parameters needed
}
```

## Error Handling Patterns

### Common Error Types
- **Network errors**: Connection timeouts, unreachable hosts
- **SOAP faults**: Invalid parameters, unsupported operations
- **Device errors**: Device busy, invalid state transitions

### Typical Error Responses
- `401 Invalid Action` - Operation not supported
- `402 Invalid Args` - Parameters are incorrect
- `501 Action Failed` - Device couldn't complete operation

## Testing Tips

### Device State Considerations
- Some operations require specific device states (e.g., Play requires media loaded)
- Volume operations work regardless of playback state
- Transport operations may fail if no media is queued

### Parameter Validation
- Always use `instance_id: 0` unless specifically targeting a service instance
- Volume values outside 0-100 range will be rejected
- Empty strings for required parameters will cause errors

### Network Considerations
- Operations timeout after ~5 seconds by default
- Ensure device IP is accessible from your machine
- Some operations may take longer on busy networks