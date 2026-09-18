# callback-server

A generic UPnP callback server for receiving event notifications.

## Overview

This is an internal implementation detail of [sonos-sdk](https://crates.io/crates/sonos-sdk). It is published to crates.io as `sonos-sdk-callback-server` as a transitive dependency, and is not intended for direct use.

The callback-server crate provides a lightweight HTTP server for handling UPnP NOTIFY requests. It is generic and has no knowledge of device-specific protocols or implementations.

## Purpose

Keeping the HTTP layer separate from device logic means:

- HTTP server concerns stay out of business logic
- The same foundation serves any UPnP eventing consumer
- Device-specific crates stay focused on their domain
- Both sides are testable on their own

## Components

- **CallbackServer**: HTTP server that receives UPnP NOTIFY requests on a local port
- **EventRouter**: Routes incoming events based on subscription IDs
- **NotificationPayload**: Generic data structure containing subscription ID and event XML
- **firewall_detection**: `FirewallStatus` plus the probe consumers use to decide whether callbacks reach them at all

## Usage

Depend on it by its published name, bound to the `callback_server` lib name:

```toml
[dependencies]
callback-server = { package = "sonos-sdk-callback-server", path = "../callback-server", version = "0.8.0" }
```

Device-specific crates should create adapter layers that wrap the generic types and add domain-specific context.

### Adapter Pattern

The recommended pattern for using this crate is to create an adapter layer in the consuming crate:

1. **Create a channel** for receiving generic `NotificationPayload` from callback-server
2. **Spawn an adapter task** that receives notifications and adds device-specific context
3. **Maintain a mapping** from subscription IDs to device-specific information
4. **Convert** generic notifications into domain-specific events
5. **Send** enriched events to your application's event processor

Example adapter structure:

```rust,ignore
pub struct DeviceCallbackServer {
    inner: callback_server::CallbackServer,
    subscription_map: Arc<RwLock<HashMap<String, DeviceContext>>>,
    device_event_sender: mpsc::UnboundedSender<DeviceEvent>,
}
```

This pattern keeps the callback-server generic while allowing device-specific crates to add their own context and types.

## Architecture

The callback-server is a thin layer that:

1. Binds to an available port in a specified range
2. Validates incoming UPnP NOTIFY requests
3. Extracts subscription IDs and event XML
4. Routes events to registered handlers via channels

All device-specific logic (speaker IDs, service types, event parsing) is handled by the consuming crate.

Requests are rejected with a status code rather than forwarded when the SID header is missing,
the subscription is not registered, `NT` appears without `NTS`, `Content-Length` is absent, or
the body exceeds the size cap. A NOTIFY that arrives before its subscription is registered is
held and replayed once registration lands, since Sonos can deliver the first event before the
SUBSCRIBE response has been processed.

## Dependencies

- `tokio`: Async runtime
- `axum`: HTTP server framework (one catch-all NOTIFY route)
- `if-addrs`: IPv4 interface enumeration for callback address selection
- `tracing`: Structured logging

Dev-only: `reqwest` (test client for driving the server end to end).

## Testing

Run tests from the crate directory:

```bash
cd callback-server
cargo test
```

Or from the workspace root, using the published package name:

```bash
cargo test -p sonos-sdk-callback-server
```

## License

Licensed under either of [Apache License, Version 2.0](../LICENSE-APACHE) or
[MIT license](../LICENSE-MIT), at your option.
