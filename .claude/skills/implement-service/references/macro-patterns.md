# Macro Patterns Reference

## Table of Contents

1. [define_upnp_operation! Macro](#define_upnp_operation-macro)
2. [define_operation_with_response! Macro](#define_operation_with_response-macro)
3. [Validation Implementations](#validation-implementations)
4. [Generated Code](#generated-code)
5. [Common Patterns](#common-patterns)

## define_upnp_operation! Macro

Use for operations with no meaningful response data (just success/failure).

```rust
use crate::{define_upnp_operation, Validate};
use paste::paste;

define_upnp_operation! {
    operation: PauseOperation,
    action: "Pause",
    service: AVTransport,
    request: {
        // No additional fields beyond instance_id
    },
    response: (),
    payload: |req| format!("<InstanceID>{}</InstanceID>", req.instance_id),
    parse: |_xml| Ok(()),
}

// Validation implementation (required)
impl Validate for PauseOperationRequest {
    // No validation needed - instance_id is always valid
}
```

### With Additional Parameters

```rust
define_upnp_operation! {
    operation: PlayOperation,
    action: "Play",
    service: AVTransport,
    request: {
        speed: String,
    },
    response: (),
    payload: |req| {
        format!("<InstanceID>{}</InstanceID><Speed>{}</Speed>", req.instance_id, req.speed)
    },
    parse: |_xml| Ok(()),
}

impl Validate for PlayOperationRequest {
    fn validate_basic(&self) -> Result<(), crate::operation::ValidationError> {
        if self.speed.is_empty() {
            return Err(crate::operation::ValidationError::invalid_value("speed", &self.speed));
        }
        match self.speed.as_str() {
            "1" | "0" => Ok(()),
            other => {
                if other.parse::<f32>().is_ok() {
                    Ok(())
                } else {
                    Err(crate::operation::ValidationError::Custom {
                        parameter: "speed".to_string(),
                        message: "Speed must be '1', '0', or a numeric value".to_string(),
                    })
                }
            }
        }
    }
}
```

## define_operation_with_response! Macro

Use for operations that return structured data.

```rust
use crate::{define_operation_with_response, Validate};
use paste::paste;

define_operation_with_response! {
    operation: GetTransportInfoOperation,
    action: "GetTransportInfo",
    service: AVTransport,
    request: {},
    response: GetTransportInfoResponse {
        current_transport_state: String,
        current_transport_status: String,
        current_speed: String,
    },
    xml_mapping: {
        current_transport_state: "CurrentTransportState",
        current_transport_status: "CurrentTransportStatus",
        current_speed: "CurrentSpeed",
    },
}

impl Validate for GetTransportInfoOperationRequest {
    // No validation needed for parameterless operation
}
```

### With Request Parameters

```rust
define_operation_with_response! {
    operation: GetVolumeOperation,
    action: "GetVolume",
    service: RenderingControl,
    request: {
        channel: String,
    },
    response: GetVolumeResponse {
        current_volume: u8,
    },
    xml_mapping: {
        current_volume: "CurrentVolume",
    },
}

impl Validate for GetVolumeOperationRequest {
    fn validate_basic(&self) -> Result<(), crate::operation::ValidationError> {
        match self.channel.as_str() {
            "Master" | "LF" | "RF" => Ok(()),
            other => Err(crate::operation::ValidationError::Custom {
                parameter: "channel".to_string(),
                message: format!("Invalid channel '{}'. Must be 'Master', 'LF', or 'RF'", other),
            })
        }
    }
}
```

## Validation Implementations

### Empty Validation (No Parameters)

```rust
impl Validate for MyOperationRequest {
    // No validation needed
}
```

### Range Validation

```rust
impl Validate for SetVolumeOperationRequest {
    fn validate_basic(&self) -> Result<(), crate::operation::ValidationError> {
        if self.desired_volume > 100 {
            return Err(crate::operation::ValidationError::range_error(
                "desired_volume", 0, 100, self.desired_volume
            ));
        }
        Ok(())
    }
}
```

### Enum-like Validation

```rust
impl Validate for MyOperationRequest {
    fn validate_basic(&self) -> Result<(), crate::operation::ValidationError> {
        match self.channel.as_str() {
            "Master" | "LF" | "RF" => Ok(()),
            other => Err(crate::operation::ValidationError::Custom {
                parameter: "channel".to_string(),
                message: format!("Invalid channel '{}'. Must be 'Master', 'LF', or 'RF'", other),
            })
        }
    }
}
```

### Combined Validation

```rust
impl Validate for SetVolumeOperationRequest {
    fn validate_basic(&self) -> Result<(), crate::operation::ValidationError> {
        // Range check
        if self.desired_volume > 100 {
            return Err(crate::operation::ValidationError::range_error(
                "desired_volume", 0, 100, self.desired_volume
            ));
        }

        // Enum check
        match self.channel.as_str() {
            "Master" | "LF" | "RF" => Ok(()),
            other => Err(crate::operation::ValidationError::Custom {
                parameter: "channel".to_string(),
                message: format!("Invalid channel '{}'. Must be 'Master', 'LF', or 'RF'", other),
            })
        }
    }
}
```

## Generated Code

The macros generate these types:

- `{Operation}Request` - Request struct with `instance_id: u32` plus custom fields
- `{Operation}Response` - Response struct (unit type `()` or custom fields)
- `{Operation}` - Zero-sized type implementing `UPnPOperation` trait
- `{operation_snake_case}()` - Convenience function returning `OperationBuilder`

### Convenience Function Pattern

```rust
// Generated for PlayOperation:
pub fn play_operation(speed: String) -> OperationBuilder<PlayOperation> {
    let request = PlayOperationRequest {
        speed,
        instance_id: 0,
    };
    OperationBuilder::new(request)
}

// Legacy alias
pub use play_operation as play;
```

## Common Patterns

### Response Field Types

| XML Type | Rust Type | Notes |
|----------|-----------|-------|
| String | `String` | Default for text content |
| ui2, ui4 | `u8`, `u16`, `u32` | Unsigned integers |
| i2, i4 | `i8`, `i16`, `i32` | Signed integers |
| boolean | `bool` | true/false, 1/0 |

### XML Element Names

Use PascalCase for XML mappings as returned by Sonos:

```rust
xml_mapping: {
    current_volume: "CurrentVolume",      // Not "currentVolume"
    transport_state: "CurrentTransportState",
}
```

### Payload Building

Always include `InstanceID` first:

```rust
payload: |req| format!(
    "<InstanceID>{}</InstanceID><Channel>{}</Channel><DesiredVolume>{}</DesiredVolume>",
    req.instance_id, req.channel, req.desired_volume
)
```

### Service Names

Available services in `crate::service::Service`:

- `AVTransport` - Playback control
- `RenderingControl` - Volume, mute, audio settings
- `GroupRenderingControl` - Group audio settings
- `ZoneGroupTopology` - Speaker grouping
