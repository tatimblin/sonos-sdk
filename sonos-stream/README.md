# sonos-stream

> ⚠️ **INTERNAL CRATE - NOT FOR DIRECT USE**
> This crate is an **internal implementation detail** of the sonos-sdk workspace. It is published to crates.io as `sonos-sdk-stream` so that [`sonos-sdk`](../sonos-sdk) resolves as a dependency, but it is not intended for direct use and may change at any time without notice.

## Overview

`sonos-stream` provides low-level event streaming and subscription management for Sonos devices with automatic fallback between UPnP events and polling. It is the event pipeline underneath [`sonos-state`](../sonos-state), which in turn backs the public [`sonos-sdk`](../sonos-sdk) API.

## Architecture Role

```text
┌─────────────────┐     ┌──────────────────┐     ┌─────────────────┐     ┌─────────────────┐
│   End Users     │────▶│    sonos-sdk     │────▶│   sonos-state   │────▶│  sonos-stream   │
│                 │     │  (Public API)    │     │   (Internal)    │     │   (Internal)    │
└─────────────────┘     └──────────────────┘     └─────────────────┘     └─────────────────┘
                                                          ▲                       ▲
                                                          │                       │
                                                  Property Watchers        Event Streaming
                                                  State Management         UPnP Subscriptions
```

## Responsibilities

- **Event Processing**: Raw UPnP event handling, parsing, and enrichment
- **Subscription Lifecycle**: UPnP service subscription management with fallback
- **Network Resilience**: Firewall detection and polling fallback
- **Event Iteration**: Sync and async event iteration interfaces

## Key Features

- **🔄 Transparent Event/Polling Switching**: Automatically switches between UPnP events and polling based on network conditions
- **🔥 Proactive Firewall Detection**: Detects firewall blocking and starts polling without waiting for a timeout
- **📡 Complete Event Enrichment**: Full event data with source attribution and timing information
- **⚡ Optimized Iteration**: Both sync and async iterator patterns for different use cases
- **🛡️ Intelligent Fallback**: Automatic fallback to polling when UPnP events become unavailable
- **🔧 Resource Efficient**: Shared HTTP clients and connection pools

## Internal API Overview

`EventBroker::new` and `register_speaker_service` are async and must run on a Tokio runtime.
Consumers above this layer (`sonos-event-manager`) own that runtime on a background worker
thread and expose a sync API upward.

```rust,ignore
use sonos_stream::{BrokerConfig, EventBroker, Service};

let mut broker = EventBroker::new(BrokerConfig::default()).await?;
let reg = broker
    .register_speaker_service("192.168.1.100".parse()?, Service::AVTransport)
    .await?;

// Async consumption
let mut events = broker.event_iterator()?;
while let Some(enriched_event) = events.next_async().await {
    process_enriched_event(enriched_event);
}
```

`EventIterator` offers, in addition to `next_async()`:

- `next_timeout(Duration)` — async, with a deadline
- `try_next()` — non-blocking
- `iter()` — a blocking sync iterator that drives the async receiver on the ambient runtime
- `filter_by_registration(...)`, `filter_by_service(...)`, `filter_by_source_type(...)` — consuming filters
- `stats()` — delivery counts and timeouts

It also implements `futures::Stream`.

## Event Types

The crate produces `EnrichedEvent` values carrying:

- **Event Data**: a service state snapshot, as one `EventData` variant
  - `AVTransport(AVTransportState)` - Transport state, track info, position, metadata
  - `RenderingControl(RenderingControlState)` - Volume, mute, bass, treble, loudness
  - `ZoneGroupTopology(ZoneGroupTopologyState)` - Group membership and network topology
  - `GroupRenderingControl(GroupRenderingControlState)` - Group volume and mute
  - `GroupManagement(GroupManagementState)` - Group coordinator and member state
- **Event Source**: `EventSource::UPnPNotification { subscription_id }` or `EventSource::PollingDetection { poll_interval }`
- **Context**: `registration_id`, `speaker_ip`, `service`, `timestamp` (wall clock, for display) and `observed_at` (monotonic, for ordering)

Order two events by `observed_at`, never by `timestamp`: wall-clock time can step backwards
when NTP corrects the clock.

## Network Resilience

The crate handles various network conditions transparently:

- **UPnP Events Available**: Real-time event notifications (preferred)
- **Firewall Blocked**: Detection and immediate polling fallback
- **Event Timeout**: Graceful switching to polling when events stop arriving
- **Subscription Failures**: Robust error handling with polling as safety net

## Dependencies

This internal crate depends on:

- [`callback-server`](../callback-server) - HTTP server for UPnP event callbacks and firewall detection
- [`sonos-api`](../sonos-api) - Service definitions, operations and event parsers

`sonos-discovery` is a dev-dependency only, used by the examples.

## Performance Characteristics

- **Low Latency**: Direct UPnP event processing when available
- **Adaptive**: Polling intervals back off between `base_polling_interval` and `max_polling_interval`
- **Memory Efficient**: Shared HTTP connection pools and event processors
- **CPU Efficient**: Event-driven architecture with polling only as fallback

## Configuration

`BrokerConfig` is a plain struct with a `Default`, plus builder-style setters:

```rust,ignore
use sonos_stream::BrokerConfig;
use std::time::Duration;

let config = BrokerConfig::default()
    .with_callback_ports(3400, 3500)
    .with_polling_interval(Duration::from_secs(5), Duration::from_secs(30))
    .with_event_timeout(Duration::from_secs(30))
    .with_buffer_size(1000)
    .with_firewall_detection(true)
    .with_force_polling(false);
```

| Field | Default | Meaning |
|-------|---------|---------|
| `callback_port_range` | `(3400, 3500)` | Port range the callback server binds within |
| `event_timeout` | 30s | How long without events before falling back to polling |
| `base_polling_interval` | 5s | Starting poll interval |
| `max_polling_interval` | 30s | Ceiling for adaptive backoff |
| `event_buffer_size` | 1000 | Event channel buffer |
| `max_concurrent_polls` | 50 | Cap on simultaneous polling tasks |
| `enable_proactive_firewall_detection` | `true` | Detect blocked callbacks up front |
| `firewall_event_wait_timeout` | 15s | How long to wait for a first event when deciding firewall status |
| `enable_firewall_caching` | `true` | Cache per-device firewall state |
| `max_cached_device_states` | 100 | Cap on that cache |
| `max_registrations` | 1000 | Cap on speaker/service registrations |
| `adaptive_polling` | `true` | Scale poll interval with change frequency |
| `renewal_threshold` | 5min | How far ahead of expiry to renew a subscription |
| `force_polling_mode` | `false` | Skip UPnP entirely; simulates a blocking firewall |

`BrokerConfig::fast_polling()` is a preset with shorter intervals for tests.

## Examples (For Development/Testing Only)

The published package name is `sonos-sdk-stream`, which is what `-p` takes:

```bash
# Basic event streaming example
cargo run -p sonos-sdk-stream --example basic_usage

# Async real-time processing
cargo run -p sonos-sdk-stream --example async_realtime

# Firewall handling demonstration
cargo run -p sonos-sdk-stream --example firewall_handling

# Filtering and batch processing
cargo run -p sonos-sdk-stream --example filtering_and_batch

# Live end-to-end demo against real speakers
cargo run -p sonos-sdk-stream --example live_demo
```

## Integration with sonos-state

The `sonos-state` crate consumes this crate through `sonos-event-manager`:

1. **Event Processing**: the event manager owns an `EventBroker` on a background worker thread
2. **Event Conversion**: `sonos-state`'s decoder converts `EnrichedEvent` → `PropertyChange`
3. **State Updates**: changes are applied to the store and watchers are notified
4. **Subscription Management**: subscriptions are created on demand, as properties are watched

## Error Handling

The crate provides structured error types:
- `BrokerError` - Event broker operational errors
- `RegistryError` - Speaker/service registration errors
- `SubscriptionError` - UPnP subscription management errors
- `PollingError` - Polling fallback errors

## Thread Safety

- `EventBroker` can be shared across threads with `Arc`
- Internal state is protected with appropriate synchronization primitives
- `EventIterator` must be created inside a Tokio runtime; it captures a runtime handle so its sync `iter()` can block on the async receiver

## Development Notes

**For sonos-sdk workspace maintainers**:

- Event enrichment happens in `events/processor.rs`
- Subscription lifecycle in `subscription/manager.rs`
- Polling strategies in `polling/strategies.rs`
- Firewall detection integration in `broker.rs`

## License

Licensed under either of [Apache License, Version 2.0](../LICENSE-APACHE) or
[MIT license](../LICENSE-MIT), at your option.

## See Also

- [`sonos-sdk`](../sonos-sdk) - the public API
- [`sonos-state`](../sonos-state) - reactive state management
- [`callback-server`](../callback-server) - UPnP event callback infrastructure
- [`sonos-api`](../sonos-api) - Core Sonos UPnP API definitions
