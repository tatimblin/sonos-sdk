# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

> **IMPORTANT: AI agents must read and follow the rules in [AGENTS.md](AGENTS.md) before making any changes to this repository.** This includes reading the relevant SPEC file before working on any crate, keeping documentation up to date, and following the standard development workflow.

## Project Overview

This is a Rust-based modular SDK for interacting with Sonos devices via their UPnP/SOAP interface. The project is structured as a Cargo workspace with multiple interdependent crates, each handling a specific aspect of Sonos device communication.

> **Project Status**: See [docs/STATUS.md](docs/STATUS.md) for the service completion matrix and development roadmap.

## Development Commands

### Building
```bash
# Build entire workspace
cargo build

# Build specific crate. Note the published package names differ from the
# directory names for every crate except sonos-sdk and sonos-api.
cargo build -p sonos-sdk
cargo build -p sonos-api
cargo build -p sonos-sdk-discovery      # sonos-discovery/
cargo build -p sonos-sdk-stream         # sonos-stream/
cargo build -p sonos-sdk-state          # sonos-state/
cargo build -p sonos-sdk-event-manager  # sonos-event-manager/
cargo build -p sonos-sdk-callback-server # callback-server/
cargo build -p sonos-sdk-soap-client    # soap-client/
cargo build -p sonos-sdk-state-store    # state-store/

# Release build
cargo build --release
```

### Testing
```bash
# Run all tests (this is what CI runs)
cargo test --workspace --features sonos-sdk/test-support --locked

# Test specific crate (`--features` is package-scoped, so the sonos-sdk
# feature can only be named when sonos-sdk is in the selection)
cargo test -p sonos-api
cargo test -p sonos-sdk-discovery
cargo test -p sonos-sdk-soap-client
cargo test -p sonos-sdk --features test-support

# Run tests with output
cargo test --workspace --features sonos-sdk/test-support -- --nocapture
```

> Bare `cargo test` fails to compile. See [docs/CONTRIBUTING.md](docs/CONTRIBUTING.md) for
> the full pre-push command set and what each flag is for.

### Running Examples
```bash
# Interactive CLI for testing operations (sonos-api)
cargo run --example cli_example

# Reactive state management examples (sonos-sdk)
cargo run -p sonos-sdk --example smart_dashboard
cargo run -p sonos-sdk --example property_observer
cargo run -p sonos-sdk --example sdk_demo

# State layer example (sonos-sdk-state - internal)
cargo run -p sonos-sdk-state --example minimal_example

# Event streaming examples (sonos-sdk-stream - internal)
cargo run -p sonos-sdk-stream --example basic_usage
cargo run -p sonos-sdk-stream --example async_realtime
cargo run -p sonos-sdk-stream --example firewall_handling
cargo run -p sonos-sdk-stream --example filtering_and_batch

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

**sonos-sdk** - High-level SDK facade
- Primary entry point for end users
- Re-exports discovery, state management, and API types
- Sync-first, DOM-like interface for controlling Sonos speakers

**sonos-api** - High-level type-safe API layer (largest crate)
- Implements the `SonosOperation` trait for all UPnP operations
- Provides `SonosClient` for simplified operation execution
- Supports AVTransport (30 ops), RenderingControl (11 ops: Get/Set Volume, Mute, Bass, Treble, Loudness + SetRelativeVolume), GroupRenderingControl (6 ops), GroupManagement (4 ops), ZoneGroupTopology, DeviceProperties, Events services
- Stateless design - no connection or state management

#### Internal Crates (Workspace-only)

**sonos-state** - Reactive state management
- Sync-first API: a store of current values plus a change-event stream. No `.await` required
- Change delivery is a reference-counted set of watched `(speaker_id, property_key)` pairs plus
  an `EventFanout` that gives each subscriber its own unbounded `std::sync::mpsc` queue.
  Deliberately **not** `tokio::sync::watch`/`broadcast` — `blocking_recv()` panics inside a
  Tokio runtime, and a bounded ring buffer would drop events for slow consumers
- Only watched pairs emit events, so nothing decodes or fans out work nobody asked for
- Automatic UPnP subscription management with reference counting
- Demand-driven subscription lifecycle (subscribes only when properties are watched)
- Supports Volume, Mute, Bass, Treble, Loudness, PlaybackState, Position, CurrentTrack, GroupVolume, GroupMute, GroupMembership, Topology properties
- Main entry point: `StateManager` with `register_watch()`, `iter()` and `get_property::<P>(speaker_id)`

**sonos-discovery** - Network device discovery
- SSDP-based discovery of Sonos devices on local network
- Provides simple `get()` and `get_with_timeout()` functions
- Iterator-based streaming API with `get_iter()`
- Automatic device filtering and deduplication
- Re-exported through `sonos-sdk` for end-user access

**sonos-stream** - Event streaming and subscriptions
- Internal event streaming layer with transparent UPnP event/polling switching
- Proactive firewall detection with automatic polling fallback
- Complete event enrichment with source attribution
- Used exclusively by sonos-state, not for direct use

**sonos-event-manager** - Subscription orchestration
- Reference-counted subscription management bridge between sonos-state and sonos-stream
- Implements Reference-Counted Observable pattern (similar to RxJS refCount)
- Automatic subscription creation/cleanup based on consumer count

**callback-server** - HTTP event reception
- Generic HTTP server for receiving UPnP NOTIFY event callbacks, built on `axum`
- Single catch-all route: NOTIFY-only, 64 KiB body cap, independent SID/NT/NTS validation
- Device-agnostic event routing via `EventRouter`
- Handles firewall traversal and callback URL management

**soap-client** - Low-level SOAP transport (smallest crate)
- Private crate handling HTTP/SOAP transport using ureq (blocking HTTP)
- `call()` returns the raw response body as a `String`; response *shape* is `sonos-api`'s
  business. Fault detection stays here (quick-xml scan) so callers can trust an `Ok`
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
    fn parse_response(xml: &str) -> Result<Self::Response, ApiError>;
}
```

`parse_response` takes the raw response body as `&str`. `soap-client` hands back text
rather than a DOM: it owns transport and fault detection, while response *shape* is
service-specific and belongs here.

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

Sync-first: register the watches you care about, then block on the change iterator.
The new value rides along on the event, so a burst of queued events shows every
value rather than the latest one repeated.

```rust
use sonos_state::property::SonosProperty;
use sonos_state::{PropertyChange, SpeakerId, StateManager, Volume};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create state manager (sync - no .await)
    let manager = StateManager::new()?;

    // Discover and add devices
    let devices = sonos_discovery::get();
    manager.add_devices(devices.clone())?;

    let speaker_id = SpeakerId::new(&devices[0].id);

    // Non-reactive read of whatever the store already has
    if let Some(volume) = manager.get_property::<Volume>(&speaker_id) {
        println!("Current volume: {}%", volume.0);
    }

    // Register interest. Only watched (speaker, property) pairs emit events.
    manager.register_watch(&speaker_id, Volume::KEY);

    // Blocking iteration over changes
    for event in manager.iter() {
        if let PropertyChange::Volume(volume) = &event.change {
            println!("Volume changed: {}%", volume.0);
        }
    }

    Ok(())
}
```

See `sonos-state/examples/minimal_example.rs` for the full version including the
UPnP subscription each watch needs. Most end users should reach for `sonos-sdk`'s
`Speaker`/`Group` handles instead of driving `StateManager` directly.

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
- **Async Runtime**: `tokio` (full features) - Used by sonos-stream, sonos-event-manager and callback-server. sonos-state and sonos-sdk are sync
- **XML**: `quick-xml` + `serde` - the single XML parser across the workspace. Used for SOAP request/response bodies, UPnP event `LastChange` payloads, DIDL-Lite metadata, SSDP device descriptions and SOAP fault scanning. There is no hand-rolled XML anywhere
- **HTTP**:
  - `ureq` (blocking) - SOAP transport in soap-client (pinned to 2.x) and device-description fetching in sonos-discovery (3.x)
  - `axum` - HTTP server framework for callback-server's single NOTIFY route
  - `reqwest` - **dev-dependency only**, the test client that drives callback-server and discovery fixtures end to end
- **URLs**: `url` - parsing SSDP `LOCATION` and topology `location` values in sonos-discovery and sonos-state
- **Concurrency**: `parking_lot` - non-poisoning locks (Drop safety) in sonos-state and sonos-event-manager
- **Error Handling**: `thiserror` (2.x) - all error types are derived; no hand-written `Display`/`Error` impls remain
- **Tracing**: `tracing` - Distributed logging and diagnostics
- **Macros**: `paste` - identifier concatenation in sonos-api's operation macros

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
- **User-Facing APIs**: Only sonos-sdk and sonos-api are intended for direct use
- **Internal Crates**: sonos-state, sonos-discovery, sonos-stream, sonos-event-manager, callback-server, soap-client, state-store are workspace implementation details (published to crates.io as transitive dependencies)