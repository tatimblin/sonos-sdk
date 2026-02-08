# sonos-state

Internal state management crate for the Sonos SDK. Provides property storage, change detection, and event iteration.

> **Note**: This is an internal crate. For the public API, use [`sonos-sdk`](../sonos-sdk) which provides a DOM-like interface for accessing speaker properties.

## Overview

`sonos-state` provides the backing state management infrastructure for `sonos-sdk`. It handles:

- Property value storage and caching
- Change detection for watched properties
- Blocking iteration over change events
- UPnP event decoding and property updates

## Architecture

```text
sonos-sdk (Public API)
    │
    └── Speaker.volume.get() / fetch() / watch()
            │
            ▼
sonos-state (Internal State Management)
    │
    ├── StateManager (property storage + watch tracking)
    ├── ChangeIterator (blocking event iteration)
    └── Property types (Volume, Mute, PlaybackState, etc.)
            │
            ▼
state-store (Generic Storage Primitives)
```

## Role in the SDK

The `sonos-sdk` crate provides the public-facing DOM-like API:

```rust
// Public API via sonos-sdk
let speaker = system.get_speaker_by_name("Living Room")?;
let volume = speaker.volume.get();      // Uses sonos-state internally
speaker.volume.watch()?;                 // Registers watch in sonos-state
```

Internally, `sonos-sdk` delegates to `sonos-state`:

- `speaker.volume.get()` → `state_manager.get_property::<Volume>(speaker_id)`
- `speaker.volume.watch()` → `state_manager.register_watch(speaker_id, "volume")`
- `system.iter()` → `state_manager.iter()`

## Key Components

### StateManager

Central state management with property storage and watch tracking:

```rust
impl StateManager {
    // Property access
    fn get_property<P: Property>(&self, speaker_id: &SpeakerId) -> Option<P>;
    fn set_property<P: SonosProperty>(&self, speaker_id: &SpeakerId, value: P);
    
    // Watch management
    fn register_watch(&self, speaker_id: &SpeakerId, property_key: &'static str);
    fn unregister_watch(&self, speaker_id: &SpeakerId, property_key: &'static str);
    fn is_watched(&self, speaker_id: &SpeakerId, property_key: &'static str) -> bool;
    
    // Event iteration
    fn iter(&self) -> ChangeIterator;
}
```

### ChangeIterator

Blocking iterator over property change events:

```rust
impl ChangeIterator {
    fn recv(&self) -> Option<ChangeEvent>;                    // Block until event
    fn recv_timeout(&self, timeout: Duration) -> Option<ChangeEvent>;
    fn try_recv(&self) -> Option<ChangeEvent>;                // Non-blocking
    fn try_iter(&self) -> TryIter<'_>;                        // Non-blocking iterator
}
```

### Property Types

Sonos-specific property types with UPnP service metadata:

| Property | Service | Description |
|----------|---------|-------------|
| `Volume` | RenderingControl | Master volume (0-100) |
| `Mute` | RenderingControl | Mute state |
| `Bass`, `Treble` | RenderingControl | EQ settings |
| `Loudness` | RenderingControl | Loudness compensation |
| `PlaybackState` | AVTransport | Playing/Paused/Stopped |
| `Position` | AVTransport | Track position and duration |
| `CurrentTrack` | AVTransport | Track metadata |
| `GroupMembership` | ZoneGroupTopology | Group info |

### Property Traits

```rust
// Generic trait from state-store
pub trait Property: Clone + Send + Sync + PartialEq + 'static {
    const KEY: &'static str;
}

// Sonos-specific extension
pub trait SonosProperty: Property {
    const SCOPE: Scope;      // Speaker, Group, or System
    const SERVICE: Service;  // UPnP service source
}
```

## Change Event Flow

1. UPnP event received by `sonos-event-manager`
2. Event decoded and property value extracted
3. `StateManager::set_property()` called with new value
4. If property is watched, `ChangeEvent` emitted to channel
5. `ChangeIterator` delivers event to consumer

## Dependencies

- `state-store` - Generic state management primitives
- `sonos-api` - UPnP operations and types
- `sonos-discovery` - Device discovery types

## License

MIT License

## See Also

- [`sonos-sdk`](../sonos-sdk) - Public DOM-like API (use this for applications)
- [`state-store`](../state-store) - Generic state management primitives
- [`sonos-api`](../sonos-api) - Low-level UPnP operations
