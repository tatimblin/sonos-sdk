---
name: implement-service
description: Implement a new UPnP service in sonos-api by extracting operations from documentation URLs (like https://sonos.svrooij.io/services/av-transport), testing against real speakers, and generating complete Rust code with operations, validation, and tests. Use when implementing new Sonos service modules in sonos-api/src/services/.
---

# Implement Sonos Service

Implement a complete UPnP service module in `sonos-api/src/services/` by:
1. Extracting operations from documentation
2. Testing operations against real Sonos speakers
3. Generating Rust code with macros, validation, and tests

## Workflow

### Step 1: Gather Requirements

Ask user for:
- **Documentation URL** (e.g., `https://sonos.svrooij.io/services/av-transport`)
- **Service name** (e.g., `content_directory`, use snake_case)

### Step 2: Fetch and Parse Documentation

Use WebFetch to extract operations from the documentation URL:

```
WebFetch: {url}
Prompt: "Extract all UPnP operations. For each, list: action name, input parameters (name, type, description), output parameters (name, type), and any notes about valid values."
```

Document each operation found with:
- Action name (PascalCase)
- Input parameters and their types
- Output parameters and their types
- Known valid values (for enums/ranges)

### Step 3: Discover Speakers

Run speaker discovery:

```bash
cargo run -p sonos-discovery --example discover_json
```

Present discovered speakers to user. Get user selection for which speaker to test against.

### Step 4: Test Operations and Discover Validation Requirements

**IMPORTANT**: This step determines what validation is needed for each operation.

For each operation with parameters, test against the real speaker:

```bash
# Test with valid values to get response format
cargo run -p sonos-api --example test_operation -- <ip> <service> <action> Param=ValidValue

# Test with INVALID values to discover validation requirements
cargo run -p sonos-api --example test_operation -- <ip> <service> <action> Param=INVALID
```

Examples:
```bash
# Valid - returns 200 with response
cargo run -p sonos-api --example test_operation -- 192.168.1.100 AVTransport Seek Unit=TRACK_NR Target=1

# Invalid - returns 500 error (needs validation)
cargo run -p sonos-api --example test_operation -- 192.168.1.100 AVTransport Seek Unit=INVALID Target=1

# Valid channel
cargo run -p sonos-api --example test_operation -- 192.168.1.100 RenderingControl GetVolume Channel=Master

# Invalid channel - returns 500 (needs enum validation)
cargo run -p sonos-api --example test_operation -- 192.168.1.100 RenderingControl GetVolume Channel=InvalidChannel
```

Record which parameters need validation:
- **HTTP 500 on invalid value** = Add validation
- **Parameters with documented allowed values** = Add enum validation
- **Numeric parameters with bounds** = Add range validation
- **URI/metadata strings** = Usually no validation (device validates)

### Step 5: Register Service

If this is a new service, add to `sonos-api/src/service.rs`:

```rust
// Add to Service enum
pub enum Service {
    // ... existing ...
    NewService,
}

// Add to name() match
Service::NewService => "NewService",

// Add to info() match
Service::NewService => ServiceInfo {
    endpoint: "MediaRenderer/NewService/Control",  // Get from docs
    service_uri: "urn:schemas-upnp-org:service:NewService:1",  // Get from docs
    event_endpoint: "MediaRenderer/NewService/Event",
},

// Add to scope() match
Service::NewService => ServiceScope::PerSpeaker,  // Or PerNetwork/PerCoordinator
```

### Step 6: Create Service Module

Create directory: `sonos-api/src/services/{service_name}/`

Create files:
- `mod.rs` - Module exports
- `operations.rs` - UPnP operations
- `events.rs` - Event parsing (if needed)

Register in `sonos-api/src/services/mod.rs`:
```rust
pub mod {service_name};
```

### Step 7: Implement Operations with Validation

**CRITICAL**: Every operation MUST have a `Validate` implementation or the code will not compile.

Read `references/macro-patterns.md` for detailed patterns including validation.

For each operation:

**1. Define the operation:**
```rust
define_upnp_operation! {
    operation: ActionOperation,
    action: "Action",
    service: ServiceName,
    request: { param: Type, },
    response: (),
    payload: |req| format!("<InstanceID>{}</InstanceID><Param>{}</Param>", req.instance_id, req.param),
    parse: |_xml| Ok(()),
}
```

**2. Implement Validate (REQUIRED):**

```rust
// No validation needed (parameterless or device validates)
impl Validate for ActionOperationRequest {}

// With validation
impl Validate for ActionOperationRequest {
    fn validate_basic(&self) -> Result<(), crate::operation::ValidationError> {
        // Add validation based on Step 4 testing results
        match self.param.as_str() {
            "VALID1" | "VALID2" => Ok(()),
            other => Err(crate::operation::ValidationError::Custom {
                parameter: "param".to_string(),
                message: format!("Invalid param '{}'", other),
            })
        }
    }
}
```

**Validation patterns based on testing:**

| Test Result | Validation Pattern |
|-------------|-------------------|
| 500 on out-of-range number | `ValidationError::range_error("param", min, max, value)` |
| 500 on invalid enum value | `match` with `ValidationError::Custom` |
| 500 on empty string | Check `is_empty()` with `ValidationError::invalid_value` |
| Any value accepted | Empty impl: `impl Validate for XxxRequest {}` |

### Step 8: Add Tests

For each operation, add at minimum:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::operation::UPnPOperation;

    #[test]
    fn test_{operation}_builder() {
        let op = {operation}_operation(/* params */).build().unwrap();
        assert_eq!(op.metadata().action, "{Action}");
    }

    #[test]
    fn test_{operation}_payload() {
        let request = {Operation}OperationRequest { /* fields */ };
        let payload = {Operation}Operation::build_payload(&request).unwrap();
        assert!(payload.contains("<ExpectedElement>"));
    }

    // If validation was added:
    #[test]
    fn test_{operation}_validation() {
        let valid = {Operation}OperationRequest { instance_id: 0, param: "VALID".to_string() };
        assert!(valid.validate_basic().is_ok());

        let invalid = {Operation}OperationRequest { instance_id: 0, param: "INVALID".to_string() };
        assert!(invalid.validate_basic().is_err());
    }
}
```

### Step 9: Verify

Run tests:
```bash
cargo test -p sonos-api --lib
```

Test against real speaker:
```bash
cargo run -p sonos-api --example test_operation -- <ip> <Service> <Action>
```

## References

- **Macro patterns & validation**: Read `references/macro-patterns.md` for detailed patterns
- **Service structure**: Read `references/service-structure.md` for complete module templates

## Type Mapping

| UPnP Type | Rust Type |
|-----------|-----------|
| string | `String` |
| ui2 | `u16` |
| ui4 | `u32` |
| i2 | `i16` |
| i4 | `i32` |
| boolean | `bool` |

## Common Validation Values (Discovered Through Testing)

| Parameter | Valid Values |
|-----------|--------------|
| Channel | "Master", "LF", "RF" |
| Volume | 0-100 |
| Speed | "1", "0", numeric strings |
| Unit (Seek) | "TRACK_NR", "REL_TIME", "TIME_DELTA" |
| PlayMode | "NORMAL", "REPEAT_ALL", "REPEAT_ONE", "SHUFFLE_NOREPEAT", "SHUFFLE", "SHUFFLE_REPEAT_ONE" |

## Common Services

| Service | Endpoint Path | URN |
|---------|--------------|-----|
| AVTransport | MediaRenderer/AVTransport/Control | urn:schemas-upnp-org:service:AVTransport:1 |
| RenderingControl | MediaRenderer/RenderingControl/Control | urn:schemas-upnp-org:service:RenderingControl:1 |
| ContentDirectory | MediaServer/ContentDirectory/Control | urn:schemas-upnp-org:service:ContentDirectory:1 |
| ConnectionManager | MediaServer/ConnectionManager/Control | urn:schemas-upnp-org:service:ConnectionManager:1 |
| Queue | MediaRenderer/Queue/Control | urn:sonos-com:service:Queue:1 |
