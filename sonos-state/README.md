# sonos-state

A sync-first state management system for Sonos devices with automatic change detection and blocking iteration.

## Overview

`sonos-state` provides a type-safe, synchronous approach to managing Sonos device state. It delivers property changes through a blocking iterator pattern, making it ideal for CLI tools, TUI applications (like ratatui), and any context where a simple synchronous API is preferred.

## Features

- **Sync API**: All operations are synchronous - no async/await required
- **Type-safe State**: Strongly typed properties with automatic change detection
- **Change Events**: Blocking iterator over property changes
- **Watch Pattern**: Register for property changes, iterate to receive them

## Architecture

```text
Devices → StateManager → ChangeIterator
              │                │
              │                └── Blocking iteration
              │
              └── state-store (generic storage)
                      │
                      ├── PropertyBag (type-erased storage)
                      ├── StateStore<SpeakerId> (entity-based)
                      └── ChangeIterator (blocking iteration)
```

The StateManager:
1. Stores property values for all registered speakers (via `state-store`)
2. Tracks which properties are being watched
3. Emits change events when watched properties update
4. Provides blocking iteration via `iter()`

**Layered Design**: Generic state management primitives are provided by the `state-store` crate. `sonos-state` adds Sonos-specific property types, UPnP event decoding, and speaker metadata.

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
sonos-state = { path = "../sonos-state" }
```

## Quick Start

### Basic Usage

```rust
use sonos_state::{StateManager, Volume, SpeakerId};
use sonos_discovery;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create state manager (sync - no async runtime needed)
    let manager = StateManager::new()?;

    // Discover and add devices
    let devices = sonos_discovery::get();
    manager.add_devices(devices)?;

    // Get speaker info
    for info in manager.speaker_infos() {
        println!("Found: {} at {}", info.name, info.ip_address);
    }

    Ok(())
}
```

### Property Access

```rust
use sonos_state::{StateManager, Volume, SpeakerId};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manager = StateManager::new()?;
    // ... add devices ...

    let speaker_id = SpeakerId::new("RINCON_123");

    // Read current cached value (instant, no network)
    if let Some(vol) = manager.get_property::<Volume>(&speaker_id) {
        println!("Volume: {}%", vol.0);
    }

    // Register for change events
    manager.register_watch(&speaker_id, "volume");

    Ok(())
}
```

### Blocking Iteration Over Changes

```rust
use sonos_state::{StateManager, Volume};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manager = StateManager::new()?;
    // ... add devices and watch properties ...

    // Blocking iteration over change events
    for event in manager.iter() {
        println!("{} changed on {}", event.property_key, event.speaker_id);

        // Get the new value
        if let Some(vol) = manager.get_property::<Volume>(&event.speaker_id) {
            println!("New volume: {}%", vol.0);
        }
    }

    Ok(())
}
```

### Non-Blocking Iteration

```rust
use std::time::Duration;

// Check for events without blocking
for event in manager.iter().try_iter() {
    println!("Event: {:?}", event);
}

// Wait with timeout
if let Some(event) = manager.iter().recv_timeout(Duration::from_secs(1)) {
    println!("Got event: {:?}", event);
}
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

## API Documentation

### `StateManager`

The main entry point for state management:

```rust
// Create with defaults
let manager = StateManager::new()?;

// Or use builder for custom configuration
let manager = StateManager::builder()
    .cleanup_timeout(Duration::from_secs(10))
    .build()?;
```

**Methods:**
- `new()` - Create a new state manager
- `builder()` - Create a builder for custom configuration
- `add_devices(devices)` - Register discovered devices
- `speaker_infos()` - Get all speaker metadata
- `speaker_info(id)` - Get specific speaker metadata
- `get_property<P>(speaker_id)` - Get current property value
- `set_property<P>(speaker_id, value)` - Set a property value
- `iter()` - Create a blocking iterator over change events
- `register_watch(speaker_id, property_key)` - Watch a property for changes
- `unregister_watch(speaker_id, property_key)` - Stop watching a property
- `is_watched(speaker_id, property_key)` - Check if a property is being watched

### `ChangeIterator`

Blocking iterator over property change events:

```rust
let iter = manager.iter();

// Block until next event
let event = iter.recv();

// Block with timeout
let event = iter.recv_timeout(Duration::from_secs(5));

// Non-blocking check
let event = iter.try_recv();

// Non-blocking iterator over available events
for event in iter.try_iter() {
    // Process events without blocking
}

// Iterator with timeout per event
for event in iter.timeout_iter(Duration::from_secs(1)) {
    // Stops when timeout expires without events
}
```

### `ChangeEvent`

Event emitted when a watched property changes:

```rust
pub struct ChangeEvent {
    pub speaker_id: SpeakerId,    // Which speaker changed
    pub property_key: &'static str, // Which property (e.g., "volume")
    pub service: Service,          // Which UPnP service
    pub timestamp: Instant,        // When it changed
}
```

### `Property` and `SonosProperty` Traits

Properties use a two-tier trait system:

```rust
// Generic trait from state-store crate
pub trait Property: Clone + Send + Sync + PartialEq + 'static {
    const KEY: &'static str;       // Unique identifier
}

// Sonos-specific extension trait
pub trait SonosProperty: Property {
    const SCOPE: Scope;            // Speaker, Group, or System
    const SERVICE: Service;        // UPnP service source
}
```

All Sonos properties implement both traits. The base `Property` trait comes from the generic `state-store` crate, while `SonosProperty` adds Sonos-specific metadata.

## Error Handling

The crate provides structured error types:

- `StateError::Init` - Initialization error
- `StateError::Parse` - Data parsing error
- `StateError::Api` - Error from sonos-api
- `StateError::SpeakerNotFound` - Invalid speaker ID
- `StateError::InvalidIpAddress` - IP address parsing failed
- `StateError::LockPoisoned` - Internal mutex error

## Dependencies

- **Core**: `serde`, `tracing`
- **Workspace**: `sonos-api`, `sonos-discovery`, `state-store`

The `state-store` crate provides generic state management primitives (PropertyBag, StateStore, ChangeIterator) that this crate builds upon.

## Performance Characteristics

- **Low Latency**: Property reads are instant from local cache
- **Thread-Safe**: All operations are internally synchronized
- **Memory Efficient**: Shared state across cloned managers

## Thread Safety

All public APIs are thread-safe:
- `StateManager` can be cloned and shared across threads
- State operations are internally synchronized with `RwLock`

## License

MIT License

## See Also

- [`state-store`](../state-store) - Generic state management primitives
- [`sonos-api`](../sonos-api) - Low-level Sonos UPnP API interactions
- [`sonos-discovery`](../sonos-discovery) - Device discovery utilities
