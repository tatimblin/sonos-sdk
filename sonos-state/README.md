# sonos-state

Internal implementation detail of [`sonos-sdk`](https://crates.io/crates/sonos-sdk). Published to crates.io as a transitive dependency; not intended for direct use, and its API carries no stability promise.

Provides property storage, change detection, and event iteration for the SDK.

> **Using this from an application?** Use [`sonos-sdk`](../sonos-sdk), which wraps everything here behind a DOM-like interface for accessing speaker properties.

## Overview

`sonos-state` provides the backing state management infrastructure for `sonos-sdk`. It handles:

- Property value storage and caching, scoped per speaker and per group
- Change detection for watched properties, with last-write-wins ordering
- Blocking iteration over change events
- UPnP event decoding and property updates
- Topology tracking: which speakers exist, which group each belongs to, and which are satellites

## Architecture

```text
sonos-sdk (Public API)
    │
    └── Speaker.volume.get() / fetch() / watch()
            │
            ▼
sonos-state (Internal State Management)
    │
    ├── StateManager (property storage + watch tracking + topology)
    ├── ChangeIterator (blocking event iteration)
    ├── decoder (EnrichedEvent -> PropertyChange)
    └── Property types (Volume, Mute, PlaybackState, etc.)
```

## Role in the SDK

The `sonos-sdk` crate provides the public-facing DOM-like API, and delegates to this crate:

- `speaker.volume.get()` → `state_manager.get_property::<Volume>(speaker_id)`
- `speaker.volume.watch()` → an event-manager `WatchGuard` plus a watch registration here
- `system.iter()` → `state_manager.iter()`

## Key Components

### StateManager

Central state management with property storage, watch tracking and topology. Each `iter()` gets
its own channel, so multiple consumers each see the full change stream.

Property access:
- `get_property::<P: SonosProperty>(&SpeakerId) -> Option<P>`
- `get_group_property::<P: Property>(&GroupId) -> Option<P>`
- `set_property::<P: SonosProperty>(&SpeakerId, P)`
- `set_property_stamped::<P: SonosProperty>(&SpeakerId, P, WriteStamp) -> WriteOutcome`
- `set_group_property` / `set_group_property_stamped`

Watch management:
- `register_watch(&SpeakerId, property_key)` / `unregister_watch(&SpeakerId, property_key)`
- `is_watched(&SpeakerId, property_key) -> bool`
- `watch_property_with_subscription::<P>(...)` / `unwatch_property_with_subscription::<P>(...)`

Topology:
- `add_devices(Vec<Device>)`, `initialize(Topology)`, `is_initialized()`
- `speaker_infos()`, `speaker_info(&SpeakerId)`, `get_speaker_ip(&SpeakerId)`
- `groups()`, `get_group(&GroupId)`, `get_group_for_speaker(&SpeakerId)`
- `get_satellite_ids()` / `set_satellite_ids(...)` for bonded surrounds and subs

Event iteration:
- `iter() -> ChangeIterator`

### ChangeIterator

Blocking iterator over property change events:

- `recv() -> Option<ChangeEvent>` — block until an event arrives
- `recv_timeout(Duration) -> Option<ChangeEvent>` — block with a deadline
- `try_recv() -> Option<ChangeEvent>` — non-blocking
- `try_iter() -> TryIter<'_>` — drain what is already queued
- `timeout_iter(Duration) -> TimeoutIter<'_>` — iterate with a per-item deadline

`ChangeIterator` also implements `Iterator`, so `for event in manager.iter()` blocks on each
item until the channel closes.

### ChangeEvent

```rust,ignore
pub struct ChangeEvent {
    pub speaker_id: SpeakerId,
    pub change: PropertyChange,
    pub source: ChangeSource,
    pub timestamp: Instant,
}
```

`property_key()` and `service()` are methods, derived from `change` rather than stored beside
it, so the two cannot drift apart. `change` carries the new value, which is what lets a
consumer draining a backlog observe every value the property passed through — a
`Playing -> Transitioning -> Playing` sequence is three queued events but only one final store
value.

### Property Types

Sonos-specific property types with UPnP service metadata:

| Property | Scope | Service | Description |
|----------|-------|---------|-------------|
| `Volume` (u8) | Speaker | RenderingControl | Master volume (0-100) |
| `Mute` (bool) | Speaker | RenderingControl | Mute state |
| `Bass`, `Treble` (i8) | Speaker | RenderingControl | EQ settings (-10 to +10) |
| `Loudness` (bool) | Speaker | RenderingControl | Loudness compensation |
| `PlaybackState` | Speaker | AVTransport | Playing/Paused/Stopped/Transitioning |
| `Position` | Speaker | AVTransport | Track position and duration, in milliseconds |
| `CurrentTrack` | Speaker | AVTransport | Track metadata |
| `GroupMembership` | Speaker | ZoneGroupTopology | Group ID and coordinator flag |
| `GroupVolume` (u16) | Group | GroupRenderingControl | Group master volume |
| `GroupMute` (bool) | Group | GroupRenderingControl | Group mute state |
| `GroupVolumeChangeable` (bool) | Group | GroupRenderingControl | Whether group volume is settable |

Speaker-scoped properties are stored against a `SpeakerId`; group-scoped ones resolve
speaker → group first and are stored against a `GroupId`.

### Property Traits

```rust,ignore
// Base trait (src/property.rs)
pub trait Property: Clone + Send + Sync + PartialEq + 'static {
    const KEY: &'static str;
}

// Sonos-specific extension
pub trait SonosProperty: Property {
    const SCOPE: Scope;      // Speaker, Group, or System
    const SERVICE: Service;  // UPnP service source

    fn to_change(&self) -> Option<PropertyChange>;
}
```

Both traits are re-exported from `sonos-sdk`, so downstream code can be generic over properties
without depending on this crate directly.

## Change Event Flow

1. UPnP event (or polling result) arrives from `sonos-event-manager` as an `EnrichedEvent`
2. `decoder` turns it into one or more `PropertyChange` values
3. Each change is applied to the store with a `WriteStamp`, yielding a `WriteOutcome`
4. A write that is `Changed` — a real difference, and not older than the stored observation — on a watched property emits a `ChangeEvent`
5. `ChangeIterator` delivers the event to each consumer

## Dependencies

- `sonos-api` - UPnP operations, service definitions and event types
- `sonos-stream` - event streaming primitives
- `sonos-event-manager` - subscription lifecycle and the watch registry
- `sonos-discovery` - `Device` from discovery

## License

Licensed under either of [Apache License, Version 2.0](../LICENSE-APACHE) or
[MIT license](../LICENSE-MIT), at your option.

## See Also

- [`sonos-sdk`](../sonos-sdk) - Public DOM-like API (use this for applications)
- [`sonos-api`](../sonos-api) - Stateless UPnP operations
