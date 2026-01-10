# sonos-state

A lightweight, reactive state management system for Sonos devices with automatic UPnP subscription management.

## Overview

`sonos-state` provides a unified, type-safe, and reactive approach to managing Sonos device state. It automatically handles UPnP event subscriptions and delivers real-time property updates through an elegant Rust API.

## Features

- **🏪 Local State**: Type-safe unified store for all Sonos device properties
- **⚡ Reactive Updates**: Watch properties for changes using `tokio::sync::watch` channels
- **🔄 Event-driven**: Processes events from sonos-stream with automatic decoding
- **📡 Automatic Subscriptions**: Demand-driven UPnP service subscriptions with reference counting
- **🛠️ Extensible**: Add custom properties and decoders
- **🔧 Zero Configuration**: No manual service management required

## Architecture

```text
External Events → Decoders → StateStore → PropertyWatchers
                              (queries)    (reactive)
```

The StateManager automatically:
1. Subscribes to UPnP services when properties are first watched
2. Shares subscriptions across multiple property watchers (reference counting)
3. Processes events and updates the state store
4. Notifies watchers of property changes
5. Cleans up subscriptions when no longer needed

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
sonos-state = { path = "../sonos-state" }
```

## Quick Start

### Basic Reactive Usage

```rust
use sonos_state::{StateManager, Volume, Mute, PlaybackState};
use sonos_discovery;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create state manager with automatic event processing
    let manager = StateManager::new().await?;

    // Discover and add devices
    let devices = sonos_discovery::get();
    manager.add_devices(devices).await?;

    let speaker_id = sonos_state::model::SpeakerId::new(&devices[0].id);

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

### Non-Reactive Property Access

```rust
// Get current property values without watching
if let Some(vol) = manager.get_property::<Volume>(&speaker_id) {
    println!("Current volume: {}%", vol.0);
}

if let Some(state) = manager.get_property::<PlaybackState>(&speaker_id) {
    match state {
        PlaybackState::Playing => println!("Currently playing"),
        PlaybackState::Paused => println!("Paused"),
        PlaybackState::Stopped => println!("Stopped"),
        PlaybackState::Transitioning => println!("Changing state"),
    }
}
```

### Multiple Property Watching

```rust
// Multiple properties sharing the same service subscription
let volume_watcher = manager.watch_property::<Volume>(speaker_id.clone()).await?;
let mute_watcher = manager.watch_property::<Mute>(speaker_id.clone()).await?;
// Both watchers share the same RenderingControl subscription!

let playback_watcher = manager.watch_property::<PlaybackState>(speaker_id.clone()).await?;
// This creates a separate AVTransport subscription
```

## Available Properties

### Audio Control (RenderingControl Service)
- `Volume` - Master volume (0-100)
- `Mute` - Master mute state (bool)
- `Bass` - Bass EQ setting (-10 to +10)
- `Treble` - Treble EQ setting (-10 to +10)
- `Loudness` - Loudness compensation (bool)

### Playback (AVTransport Service)
- `PlaybackState` - Current playback state (Playing, Paused, Stopped, Transitioning)
- `Position` - Current position and total duration with progress calculation
- `CurrentTrack` - Track metadata (title, artist, album, art URI)

### Grouping (ZoneGroupTopology Service)
- `GroupMembership` - Speaker's group membership and coordinator status
- `Topology` - System-wide speaker and group topology

## Examples

The crate includes two comprehensive examples:

### Live Dashboard
```bash
cargo run -p sonos-state --example live_dashboard
```
A real-time dashboard showing current state of all discovered Sonos devices.

### Reactive Dashboard
```bash
cargo run -p sonos-state --example reactive_dashboard
```
Demonstrates demand-driven subscriptions and automatic subscription management.

## API Documentation

### Core Types

#### `StateManager`
The main entry point for reactive state management:
- `new()` - Create a new state manager
- `add_devices(devices)` - Register discovered devices
- `watch_property<P>(speaker_id)` - Watch a property with automatic subscriptions
- `get_property<P>(speaker_id)` - Get current property value (non-reactive)

#### `PropertyWatcher<P>`
Handle for watching property changes:
- `changed()` - Wait for the property to change
- `current()` - Get the current property value
- `speaker_id()` - Get the speaker ID being watched

#### `Property` Trait
All properties implement this trait with:
- `KEY` - Unique string identifier
- `SCOPE` - Where the property is stored (Speaker, Group, System)
- `SERVICE` - Which UPnP service provides this property

### Subscription Management

Subscriptions are automatically managed:
- **Reference Counting**: Multiple watchers share the same service subscription
- **Automatic Cleanup**: Subscriptions are removed when no watchers remain
- **Demand-Driven**: Services are only subscribed when properties are actively watched

## Synchronous Usage (CLI/Non-async)

```rust
use sonos_state::{SyncWatchExt, SyncWatcher};

let rt = tokio::runtime::Handle::current();
let sync_watcher = store.sync_watch::<Volume>(&speaker_id, rt);

// Blocking wait for changes
while let Some(volume) = sync_watcher.wait() {
    println!("Volume: {}%", volume.0);
}
```

## Dependencies

- **Core**: `tokio`, `serde`, `quick-xml`, `url`, `tracing`
- **Workspace**: `sonos-api`, `sonos-stream`, `sonos-event-manager`, `sonos-discovery`

## Error Handling

The crate provides structured error types:
- `StateError::InitializationFailed` - EventManager or state setup failed
- `StateError::SubscriptionFailed` - UPnP subscription error
- `StateError::SpeakerNotFound` - Invalid speaker ID
- `StateError::DeviceRegistrationFailed` - Device setup error

## Performance Characteristics

- **Memory Efficient**: Shared HTTP connection pools and event processing
- **Low Latency**: Direct event streaming with minimal processing overhead
- **Resource Management**: Automatic cleanup prevents resource leaks
- **Scalable**: Single event processor handles all devices and services

## Limitations

- Requires active network connection to Sonos devices
- Initial state discovery may take 2-3 seconds for subscriptions to establish
- Best suited for async Rust applications (sync support available but limited)
- UPnP events depend on device network configuration

## Thread Safety

All public APIs are thread-safe:
- `StateManager` can be shared across threads with `Arc`
- Property watchers are `Send + Sync`
- State store operations are internally synchronized

## Contributing

This crate is part of the larger `sonos-sdk` workspace. See the main project for contribution guidelines.

## License

MIT License

## See Also

- [`sonos-api`](../sonos-api) - Low-level Sonos UPnP API interactions
- [`sonos-stream`](../sonos-stream) - Event streaming for Sonos devices
- [`sonos-event-manager`](../sonos-event-manager) - UPnP event subscription management
- [`sonos-discovery`](../sonos-discovery) - Device discovery utilities