---
title: Properties
description: "The three access patterns: get, fetch, and watch."
---

Every speaker property in the SDK supports three access patterns. This page explains when to use each one.

## Speaker properties

| Property | Type | Read the value with |
|----------|------|---------------------|
| `volume` | `Volume(u8)` | `.value() -> u8` |
| `mute` | `Mute(bool)` | `.is_muted() -> bool` |
| `bass` | `Bass(i8)` | `.value() -> i8`, −10 to +10 |
| `treble` | `Treble(i8)` | `.value() -> i8`, −10 to +10 |
| `loudness` | `Loudness(bool)` | `.is_enabled() -> bool` |
| `playback_state` | `PlaybackState` | `Playing`, `Paused`, `Stopped`, `Transitioning`; plus `.is_playing()`, `.is_paused()`, `.is_stopped()` |
| `position` | `Position` | `.position_ms`, `.duration_ms`, `.progress() -> f64` |
| `current_track` | `CurrentTrack` | `.title`, `.artist`, `.album`, `.album_art_uri`, `.uri` — each `Option<String>`; plus `.display() -> String` |
| `group_membership` | `GroupMembership` | `.group_id`, `.is_coordinator` |

Property values are typed newtypes and enums, not bare integers and strings.
They derive `Debug`, `Clone`, `PartialEq`, `Serialize`, and `Deserialize` — no
`Display`, so format them with `{:?}` or unwrap them with the accessor above.

## Group properties

Accessed via `group.volume`, `group.mute`, and `group.volume_changeable`:

| Property | Type | Read the value with |
|----------|------|---------------------|
| `volume` | `GroupVolume(u16)` | `.value() -> u16` |
| `mute` | `GroupMute(bool)` | `.is_muted() -> bool` |
| `volume_changeable` | `GroupVolumeChangeable(bool)` | `.is_changeable() -> bool` |

`volume` and `mute` support all three access patterns. `volume_changeable` has
`get()` and `watch()` but no `fetch()` — its value arrives with
GroupRenderingControl events.

## get() — cached value

Returns the last known value from the internal cache. Instant, never makes a network call. Returns `None` if the property hasn't been fetched or received via events yet.

```rust
let cached: Option<Volume> = speaker.volume.get();
```

**Use when:**
- Displaying state in a UI that doesn't need to be perfectly fresh
- Checking a value you already fetched earlier
- Performance-critical loops where network latency is unacceptable

## fetch() — live network read

Makes a synchronous SOAP request to the device and returns the current value. Also updates the internal cache.

```rust
let volume: Volume = speaker.volume.fetch()?;
println!("{}%", volume.value());
```

**Use when:**
- You need the guaranteed-current value
- Initial page load or first display of a property
- After performing an action (e.g., confirming a write took effect)

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

`sonos.iter()` only emits events for properties that are already being watched,
so acquire every handle you care about before entering the loop.

`WatchHandle` is a live view, not a snapshot: `value()` reads the current value on every call, so one handle held across a whole loop reports every change. It returns `Option<P>` by value — `None` if nothing has been observed yet. `has_value()` is the same live read as a bool, and `mode()` reports what is behind this watch: `WatchMode::Events` for real-time UPnP events, `WatchMode::Polling` when the subscription failed and polling is standing in, `WatchMode::CacheOnly` when no event manager is configured and only `fetch()` will move the value.

Hold the handle for as long as you want updates. The subscription starts on the first `watch()` and is torn down 50ms after the last handle drops, so a handle that genuinely must be dropped and reacquired does not churn the subscription.

**Use when:**
- Building a live dashboard or TUI
- Reacting to physical button presses on the speaker
- Keeping a UI in sync without polling

### Reading the event instead of the handle

Each `ChangeEvent` carries the new value as a typed `PropertyChange`, so a
consumer draining a backlog observes every value the property passed through
rather than only whatever the store holds by the time it looks. `PropertyChange`
is `#[non_exhaustive]`, so a `match` needs a catch-all arm.

```rust
let _volume = speaker.volume.watch()?;

for event in sonos.iter() {
    match &event.change {
        PropertyChange::Volume(v) => println!("volume -> {}%", v.value()),
        other => println!("{} changed on {}", other.key(), event.speaker_id),
    }
}
```

`event.source` tells you where the value came from: `ChangeSource::Event` for a
device-pushed NOTIFY (or a poll standing in for one), `ChangeSource::LocalAction`
for a value this process wrote after a successful control call, and
`ChangeSource::Fetch` for an explicit `fetch()` read.

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
println!("Current volume: {}%", initial.value());

// Then react to changes through the one handle
let volume = speaker.volume.watch()?;
for _event in sonos.iter() {
    println!("Volume changed to: {:?}", volume.value());
}
```

`watch_or_fetch()` does both in one call: it subscribes, and fetches once if nothing has been observed yet.

```rust
let volume = speaker.volume.watch_or_fetch()?;
println!("Starts populated: {:?}", volume.value());
```

## Writing properties

Writes are methods on `Speaker` and `Group`, not on the property handle:

```rust
speaker.set_volume(50)?;
speaker.set_mute(true)?;
speaker.set_bass(5)?;

group.set_volume(40)?;
group.set_mute(false)?;
```

Each is a synchronous SOAP call that updates the cache on success, recorded with
`ChangeSource::LocalAction`.
