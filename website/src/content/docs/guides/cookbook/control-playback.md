---
title: Control Playback
description: Play, pause, skip, seek, and manage the queue.
---

## Basic transport controls

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

## Seek to a position

```rust
use std::time::Duration;
use sonos_sdk::prelude::*;

let speaker = sonos.speaker("Living Room").unwrap();

// Seek to 1 minute 30 seconds
speaker.seek(SeekTarget::Time(Duration::from_secs(90)))?;
```

## Set play mode

```rust
speaker.set_play_mode(PlayMode::Shuffle)?;
speaker.set_play_mode(PlayMode::RepeatOne)?;
speaker.set_play_mode(PlayMode::Normal)?;
```

Available modes: `Normal`, `Repeat`, `RepeatOne`, `Shuffle`, `ShuffleNoRepeat`.

## Read current track info

```rust
let track = speaker.current_track.fetch()?;
println!("Now playing: {} by {}", track.title, track.artist);
println!("Album: {}", track.album);
println!("Duration: {:?}", track.duration);
```

## Monitor playback state reactively

```rust
for event in sonos.iter() {
    let state = speaker.playback_state.watch()?;
    println!("State: {:?}", state.value());
}
```

## Sleep timer

```rust
use std::time::Duration;

// Set a 30-minute sleep timer
speaker.set_sleep_timer(Duration::from_secs(30 * 60))?;

// Cancel the sleep timer
speaker.set_sleep_timer(Duration::ZERO)?;
```
