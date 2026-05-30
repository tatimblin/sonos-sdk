---
title: Properties
description: "The three access patterns: get, fetch, and watch."
---

Every speaker property in the SDK supports three access patterns. This page explains when to use each one.

## Available properties

| Property | Type | Description |
|----------|------|-------------|
| `volume` | `u8` (0–100) | Speaker volume level |
| `mute` | `bool` | Whether the speaker is muted |
| `bass` | `i8` (-10–10) | Bass EQ setting |
| `treble` | `i8` (-10–10) | Treble EQ setting |
| `loudness` | `bool` | Loudness compensation |
| `playback_state` | `PlaybackState` | Playing, Paused, Stopped, Transitioning |
| `position` | `Duration` | Current track position |
| `current_track` | `TrackMetadata` | Title, artist, album, URI |

## get() — cached value

Returns the last known value from the internal cache. Instant, never makes a network call. Returns `None` if the property hasn't been fetched or received via events yet.

```rust
let cached: Option<u8> = speaker.volume.get();
```

**Use when:**
- Displaying state in a UI that doesn't need to be perfectly fresh
- Checking a value you already fetched earlier
- Performance-critical loops where network latency is unacceptable

## fetch() — live network read

Makes a synchronous SOAP request to the device and returns the current value. Also updates the internal cache.

```rust
let volume: u8 = speaker.volume.fetch()?;
```

**Use when:**
- You need the guaranteed-current value
- Initial page load or first display of a property
- After performing an action (e.g., confirming a set() took effect)

**Performance:** ~5-15ms on a local network. Each call is independent — no connection state or session.

## watch() — reactive event stream

Subscribes to real-time changes via UPnP events. Returns a `WatchHandle` that keeps the subscription alive. Events arrive through `sonos.iter()`.

```rust
for event in sonos.iter() {
    let volume = speaker.volume.watch()?;
    let mute = speaker.mute.watch()?;

    println!("Volume: {:?}, Mute: {:?}", volume.value(), mute.value());
}
```

`watch()` returns a `WatchHandle` containing the current snapshot. The subscription initializes on the first call and persists across iterations via a 50ms grace period.

**Use when:**
- Building a live dashboard or TUI
- Reacting to physical button presses on the speaker
- Keeping a UI in sync without polling

### Subscription lifecycle

Subscriptions are reference-counted:

```rust
{
    let w1 = speaker.volume.watch()?;  // subscription starts (refcount: 1)
    let w2 = speaker.volume.watch()?;  // reuses subscription (refcount: 2)
    drop(w1);                           // refcount: 1, subscription stays
}
// w2 drops here → refcount: 0 → subscription cleaned up
```

The event system initializes lazily on the first `watch()` call. If you never watch, no background threads are spawned.

## Combining patterns

A common pattern is to `fetch()` for initial state, then `watch()` for updates:

```rust
// Get the current value immediately
let initial = speaker.volume.fetch()?;
println!("Current volume: {}%", initial);

// Then react to changes — re-watch to get updated snapshots
for event in sonos.iter() {
    let volume = speaker.volume.watch()?;
    println!("Volume changed to: {:?}", volume.value());
}
```

## Setting properties

Writable properties also have a `set()` method:

```rust
speaker.volume.set(50)?;
speaker.mute.set(true)?;
speaker.bass.set(5)?;
```

`set()` is always a synchronous SOAP call. It updates the cache on success.
