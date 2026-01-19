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

### Step 4: Test Operations

For each operation, test against the real speaker using:

```bash
cargo run -p sonos-api --example test_operation -- <ip> <service> <action> [Param=Value...]
```

Example:
```bash
cargo run -p sonos-api --example test_operation -- 192.168.1.100 AVTransport GetTransportInfo
cargo run -p sonos-api --example test_operation -- 192.168.1.100 RenderingControl GetVolume Channel=Master
```

Capture and analyze the XML responses to understand:
- Response field names (exact XML element names)
- Response value types
- Error conditions

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

### Step 7: Implement Operations

Read `references/macro-patterns.md` for macro usage patterns.

For each operation, use appropriate macro:

**No response data:**
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

**With response data:**
```rust
define_operation_with_response! {
    operation: GetInfoOperation,
    action: "GetInfo",
    service: ServiceName,
    request: {},
    response: GetInfoResponse {
        field_one: String,
        field_two: u32,
    },
    xml_mapping: {
        field_one: "FieldOne",  // Exact XML element name from response
        field_two: "FieldTwo",
    },
}
```

### Step 8: Implement Validation

Every request struct needs a `Validate` implementation:

```rust
impl Validate for MyOperationRequest {
    fn validate_basic(&self) -> Result<(), crate::operation::ValidationError> {
        // Range check
        if self.volume > 100 {
            return Err(crate::operation::ValidationError::range_error("volume", 0, 100, self.volume));
        }
        // Enum check
        match self.channel.as_str() {
            "Master" | "LF" | "RF" => Ok(()),
            other => Err(crate::operation::ValidationError::Custom {
                parameter: "channel".to_string(),
                message: format!("Invalid channel '{}'", other),
            })
        }
    }
}
```

### Step 9: Add Tests

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
}
```

### Step 10: Verify

Run tests:
```bash
cargo test -p sonos-api {service_name}
```

Test against real speaker:
```bash
cargo run -p sonos-api --example test_operation -- <ip> <Service> <Action>
```

## References

- **Macro patterns**: Read `references/macro-patterns.md` for detailed macro usage
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

## Common Services

| Service | Endpoint Path | URN |
|---------|--------------|-----|
| AVTransport | MediaRenderer/AVTransport/Control | urn:schemas-upnp-org:service:AVTransport:1 |
| RenderingControl | MediaRenderer/RenderingControl/Control | urn:schemas-upnp-org:service:RenderingControl:1 |
| ContentDirectory | MediaServer/ContentDirectory/Control | urn:schemas-upnp-org:service:ContentDirectory:1 |
| ConnectionManager | MediaServer/ConnectionManager/Control | urn:schemas-upnp-org:service:ConnectionManager:1 |
| Queue | MediaRenderer/Queue/Control | urn:sonos-com:service:Queue:1 |
