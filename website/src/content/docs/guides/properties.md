---
title: Properties
description: "The three access patterns: get, fetch, and watch."
---

Every speaker property in the SDK supports three access patterns. This page explains when to use each one.

## Speaker properties

| Property | Type | Description |
|----------|------|-------------|
| `volume` | `Volume(u8)` | Master volume (0–100) |
| `mute` | `Mute(bool)` | Mute state |
| `bass` | `Bass(i8)` | Bass EQ (-10 to +10) |
| `treble` | `Treble(i8)` | Treble EQ (-10 to +10) |
| `loudness` | `Loudness(bool)` | Loudness compensation |
| `playback_state` | `PlaybackState` | Playing, Paused, Stopped, Transitioning |
| `position` | `Position` | Current position and duration in ms |
| `current_track` | `CurrentTrack` | Title, artist, album, album art URI, track URI |
| `group_membership` | `GroupMembership` | Group ID and whether this speaker is coordinator |

## Group properties

Accessed via `group.volume`, `group.mute`, etc.:

| Property | Type | Description |
|----------|------|-------------|
| `volume` | `GroupVolume(u16)` | Group master volume (0–100) |
| `mute` | `GroupMute(bool)` | Group mute state |
| `volume_changeable` | `GroupVolumeChangeable(bool)` | Whether group volume can be adjusted |

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
// Acquire once, outside the loop, and hold the handles.
let volume = speaker.volume.watch()?;
let mute = speaker.mute.watch()?;

for _event in sonos.iter() {
    println!("Volume: {:?}, Mute: {:?}", volume.value(), mute.value());
}
```

`WatchHandle` is a live view, not a snapshot: `value()` reads the current value on every call, so one handle held across a whole loop reports every change. It returns `Option<P>` by value — `None` if nothing has been observed yet.

Hold the handle for as long as you want updates. The subscription starts on the first `watch()` and is torn down 50ms after the last handle drops, so a handle that genuinely must be dropped and reacquired does not churn the subscription.

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

// Then react to changes through the one handle
let volume = speaker.volume.watch()?;
for _event in sonos.iter() {
    println!("Volume changed to: {:?}", volume.value());
}
```

`watch_or_fetch()` does both in one call: it subscribes, and fetches once if nothing has been observed yet.

## Setting properties

Writable properties also have a `set()` method:

```rust
speaker.volume.set(50)?;
speaker.mute.set(true)?;
speaker.bass.set(5)?;
```

`set()` is always a synchronous SOAP call. It updates the cache on success.
