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
cargo run -p sonos-api --example cli_example

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
- Implements the `UPnPOperation` trait for all UPnP operations
- Provides `SonosClient::execute_enhanced()` for operation execution, plus
  `subscribe()`/`subscribe_with_timeout()` for UPnP event subscriptions
- Covers the five services in the `Service` enum: AVTransport (30 ops),
  RenderingControl (11 ops: Get/Set Volume, Mute, Bass, Treble, Loudness +
  SetRelativeVolume), GroupRenderingControl (6 ops), GroupManagement (4 ops),
  ZoneGroupTopology (1 op)
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

**UPnPOperation Trait** - Central abstraction for all operations:
```rust
pub trait UPnPOperation {
    type Request: Serialize + Validate;
    type Response: for<'de> Deserialize<'de>;

    const SERVICE: Service;
    const ACTION: &'static str;

    fn build_payload(request: &Self::Request) -> Result<String, ValidationError>;
    fn parse_response(xml: &str) -> Result<Self::Response, ApiError>;
}
```

`build_payload` validates the request before serializing it, so an invalid request
fails without a network round trip. Operations are declared through the
`define_upnp_operation!`/`define_operation_with_response!` macros in
`sonos-api/src/operation/macros.rs`, which generate the `{Op}Request` struct, the
`UPnPOperation` impl, and a snake_case builder function (`PlayOperation` ->
`play_operation`) returning an `OperationBuilder`.

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

**Multi-Layer Architecture** - Clear separation of concerns. `soap-client`,
`sonos-discovery` and `callback-server` are leaves with no workspace dependencies;
everything else builds on them:
```
End Users → sonos-sdk → sonos-state → sonos-event-manager → sonos-stream → callback-server
                                                          → sonos-api    → soap-client
                                                          → sonos-discovery
```

**Event Transparency with Fallback** - sonos-stream provides seamless switching:
- Prefers real-time UPnP events when available
- Proactive firewall detection switches to polling
- Automatic fallback maintains consistent event stream

## Typical Development Workflow

### For Reactive Applications (Recommended)

1. **Device Discovery**: Use `sonos-discovery::get()` to find devices
2. **State Management**: Create `StateManager` from `sonos-state` crate
3. **Property Watching**: Use `register_watch(speaker_id, P::KEY)` to mark a pair as
   watched, or `watch_property_with_subscription::<P>(speaker_id)` to also open the
   UPnP subscription. Then block on `iter()`
4. **Property Access**: Use `get_property<P>(speaker_id)` for non-reactive property access
5. **Testing**: Use the reactive dashboard examples to test state management

### For Direct Control (Lower-level)

1. **Device Discovery**: Use `sonos-discovery::get()` to find devices
2. **Operation Construction**: Create typed requests using structs from `sonos-api`
3. **Execution**: Build a `ComposableOperation` with the service's builder function, then send it with `SonosClient::execute_enhanced()`
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
use sonos_api::{services::av_transport, SonosClient};

// SonosClient::new() automatically uses shared SOAP client for efficiency
let client = SonosClient::new();
let play = av_transport::play_operation("1".to_string()).build()?;
client.execute_enhanced("192.168.1.100", play)?;
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
use sonos_api::{Service, SonosClient};

let client = SonosClient::new();
// Returns a ManagedSubscription that renews and unsubscribes on drop.
// `subscribe_with_timeout` takes an explicit lifetime instead of the 1800s default.
let subscription = client.subscribe(
    "192.168.1.100",
    Service::AVTransport,
    "http://192.168.1.50:8080/callback",
)?;
```

## Adding New Operations

### Adding UPnP Operations (sonos-api)
1. Create request/response structs with serde derives
2. Declare the operation with `define_upnp_operation!` (or implement `UPnPOperation`
   directly) giving `SERVICE`, `ACTION`, `build_payload()` and `parse_response()`, plus a
   `Validate` impl for the generated request struct
3. Add to appropriate service module in `sonos-api/src/services/`
4. Write comprehensive tests for payload construction and response parsing
5. Update the CLI example if the operation should be exposed for testing

### Adding Reactive Properties (sonos-state)
1. Define property struct implementing the `Property` trait
2. Implement `SonosProperty`: `KEY`, `SCOPE` (Speaker/Group/System), `SERVICE`, and
   `to_change()`. `to_change()` defaults to `None`, and a property that returns `None`
   updates the store but emits no `ChangeEvent` — override it or the property is
   unwatchable
3. Add a `PropertyChange` variant and decode into it in `sonos-state/src/decoder.rs`
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
- Property-based tests for edge cases with `proptest` (sonos-api, sonos-sdk, sonos-state); `rstest` fixtures in sonos-discovery

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
sonos-sdk ──┬── sonos-state ──┬── sonos-api ──── soap-client
            │                 ├── sonos-stream ──┬── callback-server
            │                 │                  └── sonos-api
            │                 ├── sonos-event-manager
            │                 └── sonos-discovery
            ├── sonos-api
            ├── sonos-discovery
            └── sonos-event-manager
```
`soap-client`, `sonos-discovery` and `callback-server` depend on no other workspace
crate. `callback-server` in particular is device-agnostic: it speaks HTTP NOTIFY and
knows nothing about SOAP or Sonos.

## Important Notes

- Mix of async (sonos-stream, sonos-event-manager, callback-server) and blocking
  (sonos-sdk, sonos-state, sonos-api, soap-client) APIs. The public `sonos-sdk` and
  `sonos-state` surfaces are sync — no `.await`
- Device communication happens on port 1400 typically
- Event subscriptions require firewall configuration for callbacks - automatic fallback to polling provided
- The project uses standard Rust 2021 edition features
- **User-Facing APIs**: Only sonos-sdk and sonos-api are intended for direct use
- **Internal Crates**: sonos-state, sonos-discovery, sonos-stream, sonos-event-manager, callback-server, soap-client are workspace implementation details (published to crates.io as transitive dependencies)