# callback-server

A generic UPnP callback server for receiving event notifications.

## Overview

This is a **private workspace crate** used internally by other crates in this workspace. It is not intended for external use and is not published to crates.io.

The callback-server crate provides a lightweight HTTP server for handling UPnP NOTIFY requests. It is completely generic and has no knowledge of device-specific protocols or implementations.

## Purpose

This crate was extracted from device-specific implementations to:

- Separate HTTP server concerns from business logic
- Provide a reusable foundation for UPnP event handling
- Keep device-specific crates focused on their domain
- Enable easier testing and maintenance

## Components

- **CallbackServer**: HTTP server that receives UPnP NOTIFY requests on a local port
- **EventRouter**: Routes incoming events based on subscription IDs
- **NotificationPayload**: Generic data structure containing subscription ID and event XML

## Usage

This crate is used by adding it as a path dependency in other workspace crates:

```toml
[dependencies]
callback-server = { path = "../callback-server" }
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

```rust
pub struct DeviceCallbackServer {
    inner: callback_server::CallbackServer,
    subscription_map: Arc<RwLock<HashMap<String, DeviceContext>>>,
    device_event_sender: mpsc::UnboundedSender<DeviceEvent>,
}
```

This pattern keeps the callback-server generic while allowing device-specific crates to add their own context and types.

## Architecture

The callback-server is designed to be a thin layer that:

1. Binds to an available port in a specified range
2. Validates incoming UPnP NOTIFY requests
3. Extracts subscription IDs and event XML
4. Routes events to registered handlers via channels

All device-specific logic (speaker IDs, service types, event parsing) should be handled by the consuming crate.

## Dependencies

- `tokio`: Async runtime
- `warp`: HTTP server framework
- `bytes`: Efficient byte buffer handling

## Testing

Run tests from the crate directory:

```bash
cd callback-server
cargo test
```

Or from the workspace root:

```bash
cargo test -p callback-server
```
