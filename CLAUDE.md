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
# Interactive CLI for testing operations (sonos-api)
cargo run --example cli_example

# Reactive state management examples (sonos-state)
cargo run -p sonos-state --example live_dashboard
cargo run -p sonos-state --example reactive_dashboard

# Event streaming examples (sonos-stream - internal)
cargo run -p sonos-stream --example basic_usage
cargo run -p sonos-stream --example async_realtime
cargo run -p sonos-stream --example firewall_handling
cargo run -p sonos-stream --example filtering_and_batch

# Event manager example (sonos-event-manager - internal)
cargo run -p sonos-event-manager --example smart_dashboard

# Integration example (temporarily disabled)
# cargo run --bin integration-example
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

#### Public-Facing Crates (User APIs)

**sonos-api** - High-level type-safe API layer (228 KB)
- Implements the `SonosOperation` trait for all UPnP operations
- Provides `SonosClient` for simplified operation execution
- Supports AVTransport, RenderingControl, DeviceProperties, ZoneGroupTopology, GroupRenderingControl, Events services
- Stateless design - no connection or state management

**sonos-discovery** - Network device discovery (40 KB)
- SSDP-based discovery of Sonos devices on local network
- Provides simple `get()` and `get_with_timeout()` functions
- Iterator-based streaming API with `get_iter()`
- Automatic device filtering and deduplication

**sonos-state** - Reactive state management (148 KB)
- High-level reactive API using `tokio::sync::watch` channels
- Automatic UPnP subscription management with reference counting
- Demand-driven subscription lifecycle (subscribes only when properties are watched)
- Supports Volume, Mute, Bass, Treble, Loudness, PlaybackState, Position, CurrentTrack, GroupMembership, Topology properties
- Main entry point: `StateManager` with `watch_property<P>(speaker_id)` and `get_property<P>(speaker_id)` methods

#### Internal Crates (Workspace-only)

**sonos-stream** - Event streaming and subscriptions (204 KB, largest crate)
- Internal event streaming layer with transparent UPnP event/polling switching
- Proactive firewall detection with automatic polling fallback
- Complete event enrichment with source attribution
- Used exclusively by sonos-state, not for direct use

**sonos-event-manager** - Subscription orchestration (20 KB)
- Reference-counted subscription management bridge between sonos-state and sonos-stream
- Implements Reference-Counted Observable pattern (similar to RxJS refCount)
- Automatic subscription creation/cleanup based on consumer count

**callback-server** - HTTP event reception (56 KB)
- Generic HTTP server for receiving UPnP NOTIFY event callbacks using warp framework
- Device-agnostic event routing via `EventRouter`
- Handles firewall traversal and callback URL management

**soap-client** - Low-level SOAP transport (20 KB, smallest crate)
- Private crate handling HTTP/SOAP transport using ureq (blocking HTTP)
- Singleton pattern with shared HTTP connection pool
- Used internally by other crates, not meant for direct use

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

**Reference-Counted Observable Pattern** - Used in sonos-event-manager for efficient subscription management:
- First property watcher creates UPnP subscription (ref count 0→1)
- Multiple watchers share same subscription without duplication
- Last watcher dropping triggers cleanup (ref count 1→0)

**Multi-Layer Architecture** - Clear separation of concerns across 7 layers:
```
End Users → sonos-state → sonos-event-manager → sonos-stream → callback-server → sonos-api → sonos-discovery → soap-client
```

**Event Transparency with Fallback** - sonos-stream provides seamless switching:
- Prefers real-time UPnP events when available
- Proactive firewall detection switches to polling
- Automatic fallback maintains consistent event stream

## Typical Development Workflow

### For Reactive Applications (Recommended)

1. **Device Discovery**: Use `sonos-discovery::get()` to find devices
2. **State Management**: Create `StateManager` from `sonos-state` crate
3. **Property Watching**: Use `watch_property<P>(speaker_id)` for reactive updates with automatic subscriptions
4. **Property Access**: Use `get_property<P>(speaker_id)` for non-reactive property access
5. **Testing**: Use the reactive dashboard examples to test state management

### For Direct Control (Lower-level)

1. **Device Discovery**: Use `sonos-discovery::get()` to find devices
2. **Operation Construction**: Create typed requests using structs from `sonos-api`
3. **Execution**: Use `SonosClient::execute()` to send operations
4. **Testing**: Use the CLI example to test operations interactively

## Common Patterns

### Reactive State Management (Recommended)
```rust
use sonos_state::{StateManager, Volume, Mute, PlaybackState, SpeakerId};
use sonos_discovery;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create state manager with automatic event processing
    let manager = StateManager::new().await?;

    // Discover and add devices
    let devices = sonos_discovery::get();
    manager.add_devices(devices).await?;

    let speaker_id = SpeakerId::new(&devices[0].id);

    // Watch for volume changes - automatically subscribes to RenderingControl
    let mut volume_watcher = manager.watch_property::<Volume>(speaker_id.clone()).await?;

    // React to changes
    while volume_watcher.changed().await.is_ok() {
        if let Some(volume) = volume_watcher.current() {
            println!("Volume changed: {}%", volume.0);
        }
    }

    Ok(())
}
```

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

### Adding UPnP Operations (sonos-api)
1. Create request/response structs with serde derives
2. Implement `SonosOperation` trait with `SERVICE`, `ACTION`, `build_payload()`, and `parse_response()`
3. Add to appropriate service module in `sonos-api/src/services/`
4. Write comprehensive tests for payload construction and response parsing
5. Update the CLI example if the operation should be exposed for testing

### Adding Reactive Properties (sonos-state)
1. Define property struct implementing the `Property` trait
2. Specify `KEY`, `SCOPE` (Speaker/Group/System), and `SERVICE`
3. Implement property decoder in appropriate service module
4. Add property type to `sonos-state/src/lib.rs` exports
5. Test with reactive dashboard examples

### Adding Event Types (Internal Crates)
- **sonos-stream**: Add event parsing and enrichment logic
- **sonos-event-manager**: Update subscription management if needed
- **callback-server**: Usually no changes needed (device-agnostic)

## Testing Strategy

- Unit tests for all operations covering payload construction and response parsing
- Integration tests using the CLI example for end-to-end validation
- Mock tests for network operations using fixtures
- Property-based tests for edge cases (using rstest/proptest)

## Key Dependencies

### External Libraries by Purpose
- **Async Runtime**: `tokio` (full features) - Used for reactive state management and event processing
- **Serialization**: `serde` + `quick-xml` - UPnP XML request/response handling
- **HTTP**:
  - `reqwest` (async) - Used in discovery and callback-server
  - `ureq` (blocking) - Used in soap-client for SOAP transport
  - `warp` - HTTP server framework for callback-server
- **Concurrency**: `dashmap`, `crossbeam` - Lock-free data structures for event processing
- **XML**: `xmltree` - Low-level XML parsing in soap-client
- **Error Handling**: `thiserror` - Structured error types across all crates
- **Tracing**: `tracing` - Distributed logging and diagnostics

### Crate Dependencies Overview
```
sonos-state ──┬── sonos-api ──── soap-client
              ├── sonos-stream ──┬── callback-server
              └── sonos-event-manager  └── sonos-discovery
```

## Important Notes

- The `integration-example` crate is temporarily disabled during UPnP client refactoring
- Mix of async (sonos-state, sonos-stream, callback-server) and blocking (sonos-api, soap-client) APIs
- Device communication happens on port 1400 typically
- Event subscriptions require firewall configuration for callbacks - automatic fallback to polling provided
- The project uses standard Rust 2021 edition features
- **User-Facing APIs**: Only sonos-api, sonos-discovery, and sonos-state are intended for direct use
- **Internal Crates**: sonos-stream, sonos-event-manager, callback-server, soap-client are workspace implementation details