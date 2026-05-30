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

println!("Bass: {}, Treble: {}, Loudness: {}", bass, treble, loudness);
```

## Group volume

Group volume adjusts all members proportionally. Controlled via the group handle:

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

## Snapshot and restore

Capture current group volume levels before a temporary change (e.g., an announcement):

```rust
let group = sonos.speaker("Living Room").unwrap().group().unwrap();

// Save current state
group.snapshot_volume()?;

// Temporarily lower for announcement
group.set_volume(20)?;

// Firmware restores after snapshot
```

## Watch for changes

All volume and EQ properties support the reactive `watch()` pattern:

```rust
for event in sonos.iter() {
    let volume = speaker.volume.watch()?;
    let mute = speaker.mute.watch()?;
    let bass = speaker.bass.watch()?;
    let treble = speaker.treble.watch()?;
    let loudness = speaker.loudness.watch()?;

    println!("Vol: {:?}, Mute: {:?}, Bass: {:?}, Treble: {:?}, Loudness: {:?}",
        volume.value(), mute.value(), bass.value(), treble.value(), loudness.value());
}
```

Group volume is also watchable:

```rust
let group = sonos.speaker("Living Room").unwrap().group().unwrap();

for event in sonos.iter() {
    let vol = group.volume.watch()?;
    let mute = group.mute.watch()?;
    println!("Group vol: {:?}, mute: {:?}", vol.value(), mute.value());
}
```
