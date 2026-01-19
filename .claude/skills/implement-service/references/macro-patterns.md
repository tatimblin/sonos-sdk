# Macro Patterns Reference

## Table of Contents

1. [define_upnp_operation! Macro](#define_upnp_operation-macro)
2. [define_operation_with_response! Macro](#define_operation_with_response-macro)
3. [Validate Trait Implementation](#validate-trait-implementation)
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

// REQUIRED: Every operation needs a Validate impl
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

## Validate Trait Implementation

**IMPORTANT**: Every operation request struct MUST have a `Validate` implementation. The code will not compile without it.

### The Validate Trait

```rust
pub trait Validate {
    /// Perform basic validation - type checks and range validation
    fn validate_basic(&self) -> Result<(), ValidationError> {
        Ok(()) // Default: no validation
    }
}
```

### ValidationError Types

```rust
// Range error - for numeric values outside valid range
ValidationError::range_error("parameter_name", min, max, actual_value)

// Invalid value - for malformed or empty values
ValidationError::invalid_value("parameter_name", &value)

// Custom error - for enum-like validation or complex rules
ValidationError::Custom {
    parameter: "parameter_name".to_string(),
    message: "Detailed error message".to_string(),
}
```

### Discovering Valid Values

To determine what validation is needed, test operations against a real speaker:

```bash
# Test with valid values
cargo run -p sonos-api --example test_operation -- <ip> <Service> <Action> Param=ValidValue

# Test with invalid values to see error behavior
cargo run -p sonos-api --example test_operation -- <ip> <Service> <Action> Param=INVALID
```

Invalid values typically return HTTP 500. Common validation patterns discovered through testing:

| Parameter | Valid Values | Validation Type |
|-----------|--------------|-----------------|
| Channel | "Master", "LF", "RF" | Enum |
| Volume/DesiredVolume | 0-100 | Range |
| Speed | "1", "0", numeric string | Enum + Parse |
| Unit (Seek) | "TRACK_NR", "REL_TIME", "TIME_DELTA" | Enum |
| PlayMode | "NORMAL", "REPEAT_ALL", "REPEAT_ONE", "SHUFFLE_NOREPEAT", "SHUFFLE", "SHUFFLE_REPEAT_ONE" | Enum |
| Duration/Time | "H:MM:SS" format | Format (usually no validation) |
| URI strings | Any string | None (device validates) |
| Boolean | true/false, 1/0 | Type (handled by Rust) |

### Validation Patterns

#### 1. Empty Validation (No Parameters or Device Validates)

```rust
impl Validate for MyOperationRequest {
    // No validation needed
}
```

Use when:
- Operation has no parameters beyond instance_id
- Parameters are strings the device will validate (URIs, metadata)
- Format is complex and device provides better error messages

#### 2. Range Validation (Numeric Bounds)

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

Use when:
- Parameter has numeric bounds (volume 0-100, bass -10 to +10)
- Out-of-range values cause device errors

#### 3. Enum Validation (Fixed Set of Values)

```rust
impl Validate for SeekOperationRequest {
    fn validate_basic(&self) -> Result<(), crate::operation::ValidationError> {
        match self.unit.as_str() {
            "TRACK_NR" | "REL_TIME" | "TIME_DELTA" => Ok(()),
            other => Err(crate::operation::ValidationError::Custom {
                parameter: "unit".to_string(),
                message: format!(
                    "Invalid unit '{}'. Must be 'TRACK_NR', 'REL_TIME', or 'TIME_DELTA'",
                    other
                ),
            }),
        }
    }
}
```

Use when:
- Parameter must be one of a fixed set of string values
- Documentation or testing reveals allowed values

#### 4. Empty String Validation

```rust
impl Validate for PlayOperationRequest {
    fn validate_basic(&self) -> Result<(), crate::operation::ValidationError> {
        if self.speed.is_empty() {
            return Err(crate::operation::ValidationError::invalid_value("speed", &self.speed));
        }
        // ... additional validation
        Ok(())
    }
}
```

Use when:
- Empty string would cause device error
- Parameter is required

#### 5. Combined Validation

```rust
impl Validate for SetVolumeOperationRequest {
    fn validate_basic(&self) -> Result<(), crate::operation::ValidationError> {
        // Check range first
        if self.desired_volume > 100 {
            return Err(crate::operation::ValidationError::range_error(
                "desired_volume", 0, 100, self.desired_volume
            ));
        }

        // Then check enum values
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

### Testing Validation

Add tests for validation logic:

```rust
#[test]
fn test_operation_validation() {
    // Test valid values
    let request = MyOperationRequest {
        instance_id: 0,
        param: "VALID".to_string(),
    };
    assert!(request.validate_basic().is_ok());

    // Test invalid values
    let request = MyOperationRequest {
        instance_id: 0,
        param: "INVALID".to_string(),
    };
    assert!(request.validate_basic().is_err());
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

### Boolean Parameters in Payloads

Convert Rust bool to "1"/"0" or "true"/"false":

```rust
payload: |req| format!(
    "<InstanceID>{}</InstanceID><CrossfadeMode>{}</CrossfadeMode>",
    req.instance_id,
    if req.crossfade_mode { "1" } else { "0" }
)
```

### Service Names

Available services in `crate::service::Service`:

- `AVTransport` - Playback control
- `RenderingControl` - Volume, mute, audio settings
- `GroupRenderingControl` - Group audio settings
- `ZoneGroupTopology` - Speaker grouping
