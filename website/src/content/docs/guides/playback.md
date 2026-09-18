---
title: Playback
description: Transport controls, seeking, play modes, crossfade, and URI sources.
---

All playback methods are synchronous SOAP calls that return `Result<(), SdkError>` unless otherwise noted.
`play()`, `pause()`, and `stop()` also write the resulting `PlaybackState` into the cache on success; the
other transport calls leave the cache to the device's own event.

## Transport controls

| Method | Description |
|--------|-------------|
| `play()` | Start or resume playback |
| `pause()` | Pause playback |
| `stop()` | Stop playback |
| `next()` | Skip to next track |
| `previous()` | Skip to previous track |

```rust
use sonos_sdk::prelude::*;

fn main() -> Result<(), SdkError> {
    let sonos = SonosSystem::new()?;
    let speaker = sonos.speaker("Living Room").unwrap();

    speaker.play()?;
    speaker.pause()?;
    speaker.stop()?;
    speaker.next()?;
    speaker.previous()?;

    Ok(())
}
```

## Seek

Seek accepts a `SeekTarget` enum:

| Variant | Example | Description |
|---------|---------|-------------|
| `SeekTarget::Time(String)` | `"0:02:30"` | Absolute position (H:MM:SS) |
| `SeekTarget::Track(u32)` | `3` | Jump to track number (1-based) |
| `SeekTarget::Delta(String)` | `"+0:00:30"` | Relative offset (prefix with +/-) |

```rust
// Seek to 2 minutes 30 seconds
speaker.seek(SeekTarget::Time("0:02:30".into()))?;

// Jump to track 3
speaker.seek(SeekTarget::Track(3))?;

// Skip forward 30 seconds
speaker.seek(SeekTarget::Delta("+0:00:30".into()))?;

// Skip backward 10 seconds
speaker.seek(SeekTarget::Delta("-0:00:10".into()))?;
```

## Play mode

| Mode | Description |
|------|-------------|
| `Normal` | Sequential playback |
| `RepeatAll` | Repeat entire queue |
| `RepeatOne` | Repeat current track |
| `Shuffle` | Shuffle with repeat |
| `ShuffleNoRepeat` | Shuffle without repeat |
| `ShuffleRepeatOne` | Shuffle, repeat one |

```rust
speaker.set_play_mode(PlayMode::Shuffle)?;
speaker.set_play_mode(PlayMode::RepeatAll)?;
speaker.set_play_mode(PlayMode::Normal)?;
```

## Crossfade

```rust
// Enable crossfade between tracks
speaker.set_crossfade_mode(true)?;

// Disable crossfade
speaker.set_crossfade_mode(false)?;

// Check current state
let crossfade = speaker.get_crossfade_mode()?;
println!("Crossfade: {}", crossfade.crossfade_mode);
```

## Set transport URI

Controls what the speaker plays. Used behind the scenes for queues, radio, line-in, and grouping.

```rust
// Play a radio stream
speaker.set_av_transport_uri(
    "x-rincon-mp3radio://streams.example.com/jazz",
    "", // metadata (DIDL-Lite XML, optional)
)?;
speaker.play()?;

// Play from the speaker's queue
speaker.set_av_transport_uri("x-rincon-queue:RINCON_XXX#0", "")?;
speaker.play()?;
```

## Gapless playback

Pre-load the next source so the speaker transitions seamlessly:

```rust
speaker.set_next_av_transport_uri(
    "x-rincon-mp3radio://streams.example.com/next",
    "",
)?;
```

## Query available actions

Check what actions are valid in the current state (depends on source type):

```rust
let actions = speaker.get_current_transport_actions()?;
println!("Available: {}", actions.actions);
// e.g., "Play,Pause,Stop,Next,Previous,Seek"
```

## Reactive playback state

The `playback_state` property is watchable. Acquire the handle before the loop —
`sonos.iter()` only emits events for properties already under watch:

```rust
let state = speaker.playback_state.watch()?;

for _event in sonos.iter() {
    println!("State: {:?}", state.value());
}
```

## Current track info

`CurrentTrack` fields are all `Option<String>`, because a stream may supply any
subset of them. `display()` formats whatever is present as `"artist - title"`,
falling back to whichever half exists.

```rust
let track = speaker.current_track.fetch()?;
println!("{}", track.display());
println!("album: {:?}, art: {:?}", track.album, track.album_art_uri);
```

Watch for track changes:

```rust
let track = speaker.current_track.watch()?;

for _event in sonos.iter() {
    if let Some(t) = track.value() {
        println!("Now playing: {}", t.display());
    }
}
```

## Position

`Position` reports milliseconds, and `progress()` turns it into a 0.0–1.0
fraction for a progress bar:

```rust
let pos = speaker.position.fetch()?;
println!("{}ms of {}ms ({:.0}%)", pos.position_ms, pos.duration_ms, pos.progress() * 100.0);
```
