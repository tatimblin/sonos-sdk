---
title: Volume & EQ
description: Volume control, relative adjustments, bass, treble, loudness, and group volume.
---

All setter methods are synchronous SOAP calls that update the internal state cache on success.

## Speaker volume

| Method | Signature | Description |
|--------|-----------|-------------|
| `set_volume` | `set_volume(volume: u8)` | Absolute volume (0–100) |
| `set_relative_volume` | `set_relative_volume(adjustment: i8)` | Relative adjustment, returns new level |
| `set_mute` | `set_mute(muted: bool)` | Mute or unmute |

```rust
use sonos_sdk::prelude::*;

fn main() -> Result<(), SdkError> {
    let sonos = SonosSystem::new()?;
    let speaker = sonos.speaker("Kitchen").unwrap();

    // Set absolute volume
    speaker.set_volume(40)?;

    // Adjust relative to current
    let result = speaker.set_relative_volume(5)?;
    println!("New volume: {}", result.new_volume);

    // Mute
    speaker.set_mute(true)?;

    Ok(())
}
```

## Equalizer

| Method | Signature | Range | Description |
|--------|-----------|-------|-------------|
| `set_bass` | `set_bass(level: i8)` | -10 to +10 | Bass EQ |
| `set_treble` | `set_treble(level: i8)` | -10 to +10 | Treble EQ |
| `set_loudness` | `set_loudness(enabled: bool)` | — | Loudness compensation |

```rust
speaker.set_bass(5)?;
speaker.set_treble(-3)?;
speaker.set_loudness(true)?;
```

Loudness boosts bass and treble at low volumes to compensate for human hearing characteristics.

## Read EQ values

```rust
let bass = speaker.bass.fetch()?;
let treble = speaker.treble.fetch()?;
let loudness = speaker.loudness.fetch()?;

println!(
    "Bass: {}, Treble: {}, Loudness: {}",
    bass.value(),
    treble.value(),
    loudness.is_enabled(),
);
```

## Group volume

Group volume adjusts all members proportionally. Controlled via the group
handle, which addresses the group's coordinator — sending GroupRenderingControl
operations to a non-coordinator is a UPnP error 701, and `Group` routes around
that for you:

| Method | Signature | Description |
|--------|-----------|-------------|
| `set_volume` | `set_volume(volume: u16)` | Absolute group volume (0–100) |
| `set_relative_volume` | `set_relative_volume(adjustment: i16)` | Relative adjustment, returns new level |
| `set_mute` | `set_mute(muted: bool)` | Mute/unmute entire group |
| `snapshot_volume` | `snapshot_volume()` | Save current levels for restore |

```rust
let group = sonos.speaker("Living Room").unwrap().group().unwrap();

// Absolute
group.set_volume(40)?;

// Relative
let result = group.set_relative_volume(-5)?;
println!("New group volume: {}", result.new_volume);

// Mute all
group.set_mute(true)?;
```

## Snapshot group volume

`snapshot_volume()` sends `SnapshotGroupVolume` to the coordinator, which
records the members' current volumes as the ratios that subsequent group volume
changes scale against. Call it after changing an individual member's volume so
the next `group.set_volume()` keeps the new balance:

```rust
let group = sonos.speaker("Living Room").unwrap().group().unwrap();

// Re-baseline the members' relative levels
group.snapshot_volume()?;

// Members now scale from that baseline
group.set_volume(20)?;
```

The SDK exposes no restore call. To put levels back, read each member's volume
first and set them again afterwards:

```rust
let group = sonos.speaker("Living Room").unwrap().group().unwrap();

let before: Vec<_> = group
    .members()
    .into_iter()
    .map(|s| s.volume.fetch().map(|v| (s, v)))
    .collect::<Result<_, _>>()?;

group.set_volume(20)?;

for (speaker, volume) in before {
    speaker.set_volume(volume.value())?;
}
```

## Watch for changes

All volume and EQ properties support the reactive `watch()` pattern:

```rust
// Acquire every handle before the loop: iter() only emits events for
// properties that are already being watched.
let volume = speaker.volume.watch()?;
let mute = speaker.mute.watch()?;
let bass = speaker.bass.watch()?;
let treble = speaker.treble.watch()?;
let loudness = speaker.loudness.watch()?;

for _event in sonos.iter() {
    println!("Vol: {:?}, Mute: {:?}, Bass: {:?}, Treble: {:?}, Loudness: {:?}",
        volume.value(), mute.value(), bass.value(), treble.value(), loudness.value());
}
```

Group volume is also watchable:

```rust
let group = sonos.speaker("Living Room").unwrap().group().unwrap();

let vol = group.volume.watch()?;
let mute = group.mute.watch()?;

for _event in sonos.iter() {
    println!("Group vol: {:?}, mute: {:?}", vol.value(), mute.value());
}
```
