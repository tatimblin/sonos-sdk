# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

This is a Rust-based modular SDK for interacting with Sonos devices via their UPnP/SOAP interface. The project is structured as a Cargo workspace with multiple interdependent crates, each handling a specific aspect of Sonos device communication.

## Development Commands

### Building
```bash
# Build entire workspace
cargo build

# Build specific crate
cargo build -p sonos-api
cargo build -p sonos-discovery
cargo build -p sonos-stream
cargo build -p soap-client
cargo build -p callback-server

# Release build
cargo build --release
```

### Testing
```bash
# Run all tests
cargo test

# Test specific crate
cargo test -p sonos-api
cargo test -p sonos-discovery
cargo test -p soap-client

# Run tests with output
cargo test -- --nocapture
```

### Running Examples
```bash
# Interactive CLI example for testing operations
cargo run --example cli_example

# Integration example (when enabled)
cargo run --bin integration-example
```

### Linting and Formatting
```bash
# Format code
cargo fmt

# Run clippy
cargo clippy

# Check without building
cargo check
```

## Workspace Architecture

### Core Crates

**sonos-api** - High-level type-safe API layer
- Implements the `SonosOperation` trait for all UPnP operations
- Provides `SonosClient` for simplified operation execution
- Supports AVTransport, RenderingControl, DeviceProperties services
- Stateless design - no connection or state management

**sonos-discovery** - Network device discovery
- SSDP-based discovery of Sonos devices on local network
- Provides simple `get()` and `get_with_timeout()` functions
- Iterator-based streaming API with `get_iter()`
- Automatic device filtering and deduplication

**soap-client** - Low-level SOAP communication
- Private crate handling HTTP/SOAP transport
- Used internally by other crates, not meant for direct use
- Manages XML payload construction and response parsing

**sonos-stream** - Event streaming and subscriptions
- Manages UPnP event subscriptions for real-time updates
- Handles event processing and streaming capabilities
- Integrates with callback-server for event notifications

**callback-server** - Callback and event handling
- Implements HTTP server for receiving UPnP event notifications
- Handles firewall traversal and callback URL management
- Provides async interfaces for event processing

### Key Design Patterns

**SonosOperation Trait** - Central abstraction for all operations:
```rust
pub trait SonosOperation {
    type Request: Serialize;
    type Response: for<'de> Deserialize<'de>;

    const SERVICE: Service;
    const ACTION: &'static str;

    fn build_payload(request: &Self::Request) -> String;
    fn parse_response(xml: &Element) -> Result<Self::Response, ApiError>;
}
```

**Stateless Design** - No connection pooling or device state management. Each operation is independent.

**Type Safety** - Strong typing for all requests and responses with serde serialization.

**Modular Services** - Operations grouped by UPnP service (AVTransport, RenderingControl, etc.).

**Resource Efficiency** - All clients share a singleton SOAP client with shared HTTP connection pool, reducing memory usage by ~95% in multi-client scenarios.

## Typical Development Workflow

1. **Device Discovery**: Use `sonos-discovery::get()` to find devices
2. **Operation Construction**: Create typed requests using structs from `sonos-api`
3. **Execution**: Use `SonosClient::execute()` to send operations
4. **Event Handling**: Use `sonos-stream` for real-time device events
5. **Testing**: Use the CLI example to test operations interactively

## Common Patterns

### Basic Operation Execution (Resource Efficient)
```rust
use sonos_api::{SonosClient, operations::av_transport::{PlayOperation, PlayRequest}};

// SonosClient::new() automatically uses shared SOAP client for efficiency
let client = SonosClient::new();
let request = PlayRequest { instance_id: 0, speed: "1".to_string() };
client.execute::<PlayOperation>("192.168.1.100", &request)?;
```

### Multiple Client Usage (Shares HTTP Resources)
```rust
// All clients automatically share the same HTTP agent and connection pool
let client1 = SonosClient::new(); // Efficient shared resources
let client2 = SonosClient::new(); // Shares same HTTP agent as client1
```

### Direct SOAP Client Access (Advanced)
```rust
use soap_client::SoapClient;

// For advanced use cases requiring direct SOAP client access
let soap_client = SoapClient::get(); // Singleton access
let cloned_client = soap_client.clone(); // Efficient Arc clone
```

### Device Discovery
```rust
use sonos_discovery::get;

let devices = get();
for device in devices {
    println!("Found {} at {}", device.name, device.ip_address);
}
```

### Event Subscriptions
```rust
let subscribe_request = SubscribeRequest {
    callback_url: "http://192.168.1.50:8080/callback".to_string(),
    timeout_seconds: 1800,
};
let subscription = client.subscribe(device_ip, Service::AVTransport, &subscribe_request)?;
```

## Adding New Operations

1. Create request/response structs with serde derives
2. Implement `SonosOperation` trait
3. Add to appropriate service module
4. Write comprehensive tests for payload construction and response parsing
5. Update the CLI example if the operation should be exposed

## Testing Strategy

- Unit tests for all operations covering payload construction and response parsing
- Integration tests using the CLI example for end-to-end validation
- Mock tests for network operations using fixtures
- Property-based tests for edge cases (using rstest/proptest)

## Important Notes

- The `integration-example` crate is temporarily disabled during UPnP client refactoring
- All network operations are blocking by default (some crates support async)
- Device communication happens on port 1400 typically
- Event subscriptions require firewall configuration for callbacks
- The project uses standard Rust 2021 edition features