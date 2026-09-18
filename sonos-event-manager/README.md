# sonos-event-manager

> ⚠️ **INTERNAL CRATE - NOT FOR DIRECT USE**
> This crate is an **internal implementation detail** of the sonos-sdk workspace, bridging [`sonos-state`](../sonos-state) and [`sonos-stream`](../sonos-stream). It is published to crates.io as `sonos-sdk-event-manager` so that [`sonos-sdk`](../sonos-sdk) resolves as a dependency, but it is not intended for direct use and may change at any time without notice.

## Overview

`sonos-event-manager` provides reference-counted subscription management for Sonos device events. It is a **sync-first** facade over [`sonos-stream`](../sonos-stream): every async operation runs on a background worker thread with its own Tokio runtime, so nothing above this layer needs `async`/`await`.

## Architecture Role

```text
┌─────────────────┐     ┌──────────────────┐     ┌─────────────────────┐     ┌─────────────────┐
│   End Users     │────▶│    sonos-sdk     │────▶│ sonos-event-manager │────▶│  sonos-stream   │
│                 │     │  (Public API)    │     │     (Internal)      │     │   (Internal)    │
└─────────────────┘     └──────────────────┘     └─────────────────────┘     └─────────────────┘
                                 ▲                          ▲                          ▲
                                 │                          │                          │
                         Property Watchers          Reference Counting         Raw Event Processing
                         State Management           Subscription Lifecycle     UPnP/Polling Fallback
```

`sonos-state` sits alongside, owning the property store; this crate owns subscription lifetime.

## Responsibilities

- **🔢 Reference Counting**: Creates UPnP subscriptions only when needed and tears them down when the last consumer lets go
- **📡 Subscription Lifecycle**: Manages the full lifecycle of UPnP event subscriptions
- **🎯 Demand-Driven**: Only subscribes to services that are actively being watched
- **🛡️ Resource Efficiency**: Prevents subscription leaks and unnecessary network traffic
- **🔗 Clean Integration**: Presents a sync API to `sonos-state`
- **🧵 Background Processing**: All async work lives on one dedicated worker thread

## Internal API Overview

**For `sonos-state` integration only.** Every method below is synchronous:

```rust,ignore
use sonos_api::Service;
use sonos_event_manager::SonosEventManager;

// Create event manager (no .await)
let manager = SonosEventManager::new()?;

// Add devices from discovery
manager.add_devices(sonos_discovery::get())?;

// Reference-counted subscription management
manager.ensure_service_subscribed(device_ip, Service::RenderingControl)?;
manager.ensure_service_subscribed(device_ip, Service::RenderingControl)?; // Ref count: 2

// Multiplexed event stream
let events = manager.iter();
while let Some(enriched_event) = events.recv() {
    // sonos-state decodes these into property changes
}

// Cleanup as references drop
manager.release_service_subscription(device_ip, Service::RenderingControl)?; // Ref count: 1
manager.release_service_subscription(device_ip, Service::RenderingControl)?; // 0 -> teardown
```

### Event iteration

`manager.iter()` returns an `EventManagerIterator`:

- `recv() -> Option<EnrichedEvent>` — block until an event arrives
- `try_recv() -> Option<EnrichedEvent>` — non-blocking
- `recv_timeout(Duration) -> Option<EnrichedEvent>` — block with a deadline
- `try_iter()` — drain whatever is queued
- `timeout_iter(Duration)` — iterate with a per-item deadline

It also implements `Iterator`, so `for event in manager.iter()` blocks on each item. The
iterator is `Clone`; clones share one receiver.

### Watch guards

`acquire_watch(&speaker_id, property_key, ip, service)` returns a `WatchGuard`. This is the path
`sonos-sdk`'s `watch()` takes: the guard registers the `(speaker_id, key)` pair in the
`WatchRegistry` so changes are forwarded, and increments the `(ip, service)` reference count.
Dropping it schedules a release.

## Reference-Counted Observable Pattern

The crate implements a **Reference-Counted Observable** pattern similar to RxJS's `refCount()` operator:

1. **Device Registration**: Discovered devices are registered with the manager
2. **Demand-Driven Subscriptions**: UPnP subscriptions created only when first consumer requests them
3. **Reference Counting**: Each consumer increments a reference count for the `(device_ip, service)` pair
4. **Grace-Period Teardown**: When the count reaches zero, teardown is *scheduled*, not immediate
5. **Event Distribution**: Events are multiplexed from the single `sonos-stream` `EventBroker`

## Grace Period

Dropping the last reference does not unsubscribe straight away. The teardown is queued with a
**50 ms** delay, and re-acquiring the same `(ip, service)` within that window cancels it. This
exists because an immediate-mode TUI drops and re-acquires every handle each frame — nine
handles at 60fps is roughly 540 releases per second, and unsubscribing then resubscribing on
each one would be pure network churn.

All pending teardowns are serviced by one deadline-ordered queue on a dedicated thread, not on
the worker's Tokio runtime. That runtime is `new_current_thread` and the UPnP subscribe path
makes blocking calls inside `async fn`s, so a single SUBSCRIBE to an unreachable speaker wedges
it. Teardown timing has to stay independent of broker health.

## Subscription Reference Counting

```text
Timeline: Multiple Volume watchers for same device

T1: First Volume watcher created
    └─ acquire_watch(device, RenderingControl) [count: 0→1]
    └─ Creates UPnP subscription to device RenderingControl service

T2: Second Volume watcher created
    └─ acquire_watch(device, RenderingControl) [count: 1→2]
    └─ Reuses existing UPnP subscription (no network call)

T3: First watcher dropped
    └─ release [count: 2→1]
    └─ UPnP subscription remains active

T4: Second watcher dropped
    └─ release [count: 1→0]
    └─ Teardown scheduled; fires 50ms later unless a re-acquire claims it first
```

## Internal Components

- **`SonosEventManager`** (`manager.rs`): facade managing devices, subscriptions and watch guards
- **Reference Counting**: `Arc<RwLock<HashMap<(IpAddr, Service), usize>>>` using `parking_lot` locks, which do not poison — important because releases run from `Drop`
- **Device Registry**: `Arc<RwLock<HashMap<IpAddr, Device>>>`
- **Worker** (`worker.rs`): owns the Tokio runtime and the `sonos-stream` `EventBroker`, driven by a command channel
- **Teardown timer** (`timer.rs`): one thread servicing a `BinaryHeap` of pending teardowns
- **Event Stream** (`iter.rs`): `EventManagerIterator` over the worker's event channel

## Error Handling

`EventManagerError` covers:
- `BrokerInitialization` - `EventBroker` setup failures
- `DeviceRegistration` / `DeviceUnregistration` - UPnP subscribe and unsubscribe failures
- `ConsumerCreation` - event consumer setup failures
- `DeviceNotFound` - unknown device IP
- `SubscriptionNotFound` - reference counting inconsistencies
- `ChannelClosed` / `WorkerDisconnected` - event stream or worker thread gone
- `Discovery`, `InvalidIpAddress`, `Sync`, `LockPoisoned` - discovery and internal synchronization failures

## Performance Characteristics

- **Memory Efficient**: Reference counting prevents duplicate subscriptions
- **Network Efficient**: Only creates necessary UPnP subscriptions, and the grace period absorbs drop/re-acquire churn
- **CPU Efficient**: Single event stream with routing vs. multiple streams
- **Thread Safe**: `parking_lot` read-write locks around the ref-count and device maps

## Configuration

`BrokerConfig` from `sonos-stream` is passed straight through:

```rust,ignore
use sonos_event_manager::SonosEventManager;
use sonos_stream::BrokerConfig;
use std::time::Duration;

let config = BrokerConfig::default()
    .with_callback_ports(3400, 3500)
    .with_polling_interval(Duration::from_secs(5), Duration::from_secs(30));

let manager = SonosEventManager::with_config(config)?;
```

## Monitoring and Debugging

```rust,ignore
// Current reference counts
for ((device_ip, service), ref_count) in manager.service_subscription_stats() {
    println!("{device_ip} {service:?}: {ref_count} references");
}

// Single pair
let is_subscribed = manager.is_service_subscribed(device_ip, Service::AVTransport);
let count = manager.service_ref_count(device_ip, Service::AVTransport);
```

## Dependencies

This internal crate wraps:
- **[`sonos-stream`](../sonos-stream)** - Low-level event streaming and UPnP management
- **[`sonos-api`](../sonos-api)** - Service definitions and types
- **[`sonos-discovery`](../sonos-discovery)** - `Device` from discovery

## Limitations

- **Internal API**: Not designed for direct external use
- **Single Event Stream**: All events flow through one multiplexed iterator
- **No Consumer Isolation**: Events are not filtered per consumer (handled by `sonos-state`)

## Development Notes

**For sonos-sdk workspace maintainers**:

- Watch acquisition and the grace-period claim in `manager.rs:acquire_watch()`
- Reference counting in `manager.rs:ensure_service_subscribed()`
- Subscription release in `manager.rs:release_service_subscription()`
- Device management in `manager.rs:add_devices()` and `manager.rs:device_by_ip()`
- Event stream access via `manager.rs:iter()`
- Teardown scheduling and cancellation in `timer.rs`

This crate has no examples of its own. The integrated behavior is demonstrated from the public
API:

```bash
cargo run -p sonos-sdk --example smart_dashboard
cargo run -p sonos-sdk --example watch_grace_period_demo
```

## License

Licensed under either of [Apache License, Version 2.0](../LICENSE-APACHE) or
[MIT license](../LICENSE-MIT), at your option.

## See Also

- **[`sonos-sdk`](../sonos-sdk)** - the public API
- [`sonos-state`](../sonos-state) - reactive state management
- [`sonos-stream`](../sonos-stream) - Low-level event streaming and UPnP subscriptions
- [`sonos-api`](../sonos-api) - Core Sonos UPnP API definitions
