# sonos-event-manager

> ⚠️ **INTERNAL CRATE - NOT FOR DIRECT USE**
> This crate is an **internal implementation detail** of the sonos-sdk workspace, specifically designed to bridge [`sonos-state`](../sonos-state) and [`sonos-stream`](../sonos-stream). It is not intended for direct use by end-users and may change at any time without notice.

## Overview

`sonos-event-manager` provides intelligent, reference-counted subscription management for Sonos device events. It acts as a high-level facade over [`sonos-stream`](../sonos-stream), implementing demand-driven UPnP subscription lifecycle management for the reactive state system in [`sonos-state`](../sonos-state).

## Architecture Role

```text
┌─────────────────┐     ┌──────────────────┐     ┌────────────────────┐     ┌─────────────────┐
│   End Users     │────▶│   sonos-state    │────▶│ sonos-event-manager │────▶│  sonos-stream   │
│                 │     │  (Public API)    │     │   (Internal)       │     │   (Internal)    │
└─────────────────┘     └──────────────────┘     └────────────────────┘     └─────────────────┘
                               ▲                           ▲                          ▲
                               │                           │                          │
                       Property Watchers            Reference Counting         Raw Event Processing
                       State Management             Subscription Lifecycle     UPnP/Polling Fallback
```

## Why This Crate Exists

This crate was created to provide a clean abstraction layer between the high-level reactive state management and low-level event streaming:

- **🔢 Reference Counting**: Automatically creates UPnP subscriptions only when needed and cleans them up when no longer used
- **📡 Subscription Lifecycle**: Manages the complete lifecycle of UPnP event subscriptions
- **🎯 Demand-Driven**: Only subscribes to services that are actively being watched
- **🛡️ Resource Efficiency**: Prevents subscription leaks and unnecessary network traffic
- **🔗 Clean Integration**: Provides a simpler API for `sonos-state` to consume events

## Key Features

- **Automatic Lifecycle Management**: UPnP subscriptions created on first consumer, destroyed on last drop
- **Thread-Safe Reference Counting**: Atomic tracking of active consumers per device/service pair
- **Resource Efficient**: Only subscribe to events that are actually being consumed
- **Discovery Integration**: Easy integration with `sonos-discovery` for adding devices
- **Event Multiplexing**: Single event stream from `sonos-stream` with intelligent routing

## Internal API Overview

**For `sonos-state` integration only**:

```rust
// Create event manager (used internally by StateManager)
let mut event_manager = SonosEventManager::new().await?;

// Add devices from discovery
event_manager.add_devices(devices).await?;

// Reference-counted subscription management
event_manager.ensure_service_subscribed(device_ip, Service::RenderingControl).await?;
event_manager.ensure_service_subscribed(device_ip, Service::RenderingControl).await?; // Ref count: 2

// Get multiplexed event stream
let mut events = event_manager.get_event_iterator()?;
while let Some(enriched_event) = events.next_async().await {
    // sonos-state processes these events
}

// Automatic cleanup when references drop
event_manager.release_service_subscription(device_ip, Service::RenderingControl).await?; // Ref count: 1
event_manager.release_service_subscription(device_ip, Service::RenderingControl).await?; // Ref count: 0 -> cleanup
```

## Reference-Counted Observable Pattern

The crate implements a **Reference-Counted Observable** pattern similar to RxJS's `refCount()` operator:

1. **Device Registration**: Discovered devices are registered with the manager
2. **Demand-Driven Subscriptions**: UPnP subscriptions created only when first consumer requests them
3. **Reference Counting**: Each consumer increments a reference count for the (device_ip, service) pair
4. **Automatic Cleanup**: When reference count reaches zero, UPnP subscription is terminated
5. **Event Distribution**: Events are multiplexed from the single `sonos-stream` EventBroker

## Integration with sonos-state

`sonos-state`'s `StateManager` uses this crate internally:

1. **Initialization**: Creates a `SonosEventManager` instance
2. **Device Management**: Registers discovered devices
3. **Property Subscription**: Calls `ensure_service_subscribed()` when properties are first watched
4. **Event Processing**: Consumes the multiplexed event stream
5. **Cleanup**: Calls `release_service_subscription()` when property watchers are dropped

## Subscription Reference Counting

```text
Timeline: Multiple Volume watchers for same device

T1: First Volume watcher created
    └─ ensure_service_subscribed(device, RenderingControl) [count: 0→1]
    └─ Creates UPnP subscription to device RenderingControl service

T2: Second Volume watcher created
    └─ ensure_service_subscribed(device, RenderingControl) [count: 1→2]
    └─ Reuses existing UPnP subscription (no network call)

T3: First watcher dropped
    └─ release_service_subscription(device, RenderingControl) [count: 2→1]
    └─ UPnP subscription remains active

T4: Second watcher dropped
    └─ release_service_subscription(device, RenderingControl) [count: 1→0]
    └─ UPnP subscription terminated (cleanup)
```

## Internal Components

- **SonosEventManager**: Main facade managing devices and subscriptions
- **Reference Counting**: Thread-safe `DashMap<(IpAddr, Service), AtomicUsize>`
- **Device Registry**: `HashMap<IpAddr, Device>` for device lookup
- **Event Stream**: Multiplexed access to `sonos-stream`'s `EventIterator`

## Error Handling

Structured error types for different failure scenarios:
- `BrokerInitialization` - EventBroker setup failures
- `DeviceRegistration` - UPnP subscription failures
- `DeviceNotFound` - Invalid device IP lookups
- `SubscriptionNotFound` - Reference counting inconsistencies
- `ChannelClosed` - Event stream interruption

## Performance Characteristics

- **Memory Efficient**: Reference counting prevents duplicate subscriptions
- **Network Efficient**: Only creates necessary UPnP subscriptions
- **CPU Efficient**: Single event stream with routing vs. multiple streams
- **Thread Safe**: Lock-free reference counting with `DashMap` and `AtomicUsize`

## Configuration

Supports custom `BrokerConfig` for underlying `sonos-stream` configuration:

```rust
let config = BrokerConfig::default()
    .with_callback_port_range(8000..8100)
    .with_polling_interval(Duration::from_secs(30));

let manager = SonosEventManager::with_config(config).await?;
```

## Monitoring and Debugging

Internal APIs for subscription monitoring:

```rust
// Get current reference counts
let stats = manager.service_subscription_stats();
for ((device_ip, service), ref_count) in stats {
    println!("{} {:?}: {} references", device_ip, service, ref_count);
}

// Check if service is subscribed
let is_subscribed = manager.is_service_subscribed(device_ip, Service::AVTransport);
```

## Dependencies

This internal crate wraps:
- **[`sonos-stream`](../sonos-stream)** - Low-level event streaming and UPnP management
- **[`sonos-api`](../sonos-api)** - Service definitions and types
- **[`sonos-discovery`](../sonos-discovery)** - Device discovery integration

## Limitations

- **Internal API**: Not designed for direct external use
- **Single Event Stream**: All events flow through one multiplexed iterator
- **No Consumer Isolation**: Events are not filtered per consumer (handled by `sonos-state`)
- **Reference Counting Only**: No time-based subscription expiration

## Development Notes

**For sonos-sdk workspace maintainers**:

- Reference counting logic in `manager.rs:ensure_service_subscribed()`
- Subscription cleanup in `manager.rs:release_service_subscription()`
- Device management in `manager.rs:add_devices()` and `manager.rs:device_by_ip()`
- Event stream access via `manager.rs:get_event_iterator()`

## Example (Development/Testing Only)

The crate includes a smart dashboard example that demonstrates the integrated `sonos-state` API:

```bash
cargo run -p sonos-event-manager --example smart_dashboard
```

Note: This example actually uses `sonos-state`, showing the intended usage pattern.

## Migration Guidance

**If you're currently using this crate directly**: Please migrate to [`sonos-state`](../sonos-state) which provides the intended user-facing reactive state management API with automatic subscription management.

## License

MIT OR Apache-2.0

## See Also

- **[`sonos-state`](../sonos-state)** - **Recommended user-facing API** for reactive state management
- [`sonos-stream`](../sonos-stream) - Low-level event streaming and UPnP subscriptions
- [`sonos-api`](../sonos-api) - Core Sonos UPnP API definitions