# sonos-stream

> ⚠️ **INTERNAL CRATE - NOT FOR DIRECT USE**
> This crate is an **internal implementation detail** of the sonos-sdk workspace, specifically designed to be used exclusively by [`sonos-state`](../sonos-state). It is not intended for direct use by end-users and may change at any time without notice.

## Overview

`sonos-stream` provides low-level event streaming and subscription management for Sonos devices with automatic fallback between UPnP events and polling. It serves as the event pipeline foundation for the higher-level [`sonos-state`](../sonos-state) crate's reactive state management system.

## Architecture Role

```text
┌─────────────────┐     ┌──────────────────┐     ┌─────────────────┐
│   End Users     │────▶│   sonos-state    │────▶│  sonos-stream   │
│                 │     │  (Public API)    │     │   (Internal)    │
└─────────────────┘     └──────────────────┘     └─────────────────┘
                               ▲                         ▲
                               │                         │
                       Property Watchers          Event Streaming
                       State Management           UPnP Subscriptions
```

## Why This Crate Exists

This crate was extracted from `sonos-state` to separate concerns:

- **Event Processing**: Raw UPnP event handling, parsing, and enrichment
- **Subscription Lifecycle**: UPnP service subscription management with fallback
- **Network Resilience**: Automatic firewall detection and polling fallback
- **Event Iteration**: Optimized sync/async event iteration interfaces

## Key Features

- **🔄 Transparent Event/Polling Switching**: Automatically switches between UPnP events and polling based on network conditions
- **🔥 Proactive Firewall Detection**: Immediately detects firewall blocking and starts polling without delay
- **📡 Complete Event Enrichment**: Full event data with source attribution and timing information
- **⚡ Optimized Iteration**: Both sync and async iterator patterns for different use cases
- **🛡️ Intelligent Fallback**: Automatic fallback to polling when UPnP events become unavailable
- **🔧 Resource Efficient**: Shared HTTP clients and connection pools

## Internal API Overview

**For `sonos-state` integration only**:

```rust
// Main broker interface (used by sonos-state)
let mut broker = EventBroker::new(BrokerConfig::default()).await?;
let reg = broker.register_speaker_service(device_ip, Service::AVTransport).await?;

// Event consumption (used by sonos-state's StateManager)
let mut events = broker.event_iterator()?;
while let Some(enriched_event) = events.next_async().await {
    // sonos-state converts EnrichedEvent -> RawEvent -> PropertyUpdate
    process_enriched_event(enriched_event);
}
```

## Event Types

The crate produces `EnrichedEvent` instances containing:

- **Event Data**: Complete state information for each UPnP service
  - `AVTransportEvent` - Transport state, track info, position, metadata
  - `RenderingControlEvent` - Volume, mute, bass, treble, loudness
  - `DevicePropertiesEvent` - Zone name, model info, software version
  - `ZoneGroupTopologyEvent` - Group membership and network topology

- **Event Source**: Whether the event came from UPnP notifications or polling
- **Context**: Registration ID, speaker IP, service type, timestamp

## Network Resilience

The crate handles various network conditions transparently:

- **UPnP Events Available**: Real-time event notifications (preferred)
- **Firewall Blocked**: Automatic detection and immediate polling fallback
- **Event Timeout**: Graceful switching to polling when events stop arriving
- **Subscription Failures**: Robust error handling with polling as safety net

## Dependencies

This internal crate depends on several other internal crates:

- [`callback-server`](../callback-server) - HTTP server for UPnP event callbacks
- [`sonos-api`](../sonos-api) - Core Sonos UPnP API definitions
- [`soap-client`](../soap-client) - Low-level SOAP communication
- [`sonos-discovery`](../sonos-discovery) - Device discovery utilities

## Performance Characteristics

- **Low Latency**: Direct UPnP event processing when available
- **Adaptive**: Automatically adjusts polling intervals based on activity
- **Memory Efficient**: Shared HTTP connection pools and event processors
- **CPU Efficient**: Event-driven architecture with polling only as fallback

## Configuration

The broker supports internal configuration through `BrokerConfig`:

```rust
let config = BrokerConfig::default()
    .with_callback_port_range(8000..8100)
    .with_polling_interval(Duration::from_secs(30))
    .with_firewall_detection(true);
```

## Examples (For Development/Testing Only)

While not intended for end-user consumption, the crate includes examples for development and testing:

```bash
# Basic event streaming example
cargo run -p sonos-stream --example basic_usage

# Async real-time processing
cargo run -p sonos-stream --example async_realtime

# Firewall handling demonstration
cargo run -p sonos-stream --example firewall_handling

# Filtering and batch processing
cargo run -p sonos-stream --example filtering_and_batch
```

## Integration with sonos-state

The `sonos-state` crate uses this crate as follows:

1. **Event Processing**: `StateManager` creates an `EventBroker` internally
2. **Event Conversion**: Converts `EnrichedEvent` → `RawEvent` → `PropertyUpdate`
3. **State Updates**: Processes property updates and notifies watchers
4. **Subscription Management**: Automatic service subscriptions based on property demands

## Error Handling

The crate provides structured error types:
- `BrokerError` - Event broker operational errors
- `RegistryError` - Speaker/service registration errors
- `SubscriptionError` - UPnP subscription management errors
- `PollingError` - Polling fallback errors

## Thread Safety

All public APIs are thread-safe:
- `EventBroker` can be shared across threads with `Arc`
- Event iterators are `Send + Sync`
- Internal state is protected with appropriate synchronization primitives

## Development Notes

**For sonos-sdk workspace maintainers**:

- Event enrichment happens in `events/processor.rs`
- Subscription lifecycle in `subscription/manager.rs`
- Polling strategies in `polling/strategies.rs`
- Firewall detection integration in `broker.rs`

## License

MIT OR Apache-2.0

## See Also

- **[`sonos-state`](../sonos-state)** - reactive state management
- [`callback-server`](../callback-server) - UPnP event callback infrastructure
- [`sonos-api`](../sonos-api) - Core Sonos UPnP API definitions