---
title: Bedtime Routine
description: Wind down with lowered volume, soft EQ, and a sleep timer.
---

A complete bedtime automation that lowers volume, softens the EQ, and sets a sleep timer so the music fades out naturally.

```rust
use sonos_sdk::prelude::*;

fn bedtime(sonos: &SonosSystem) -> Result<(), SdkError> {
    let bedroom = sonos.speaker("Bedroom").unwrap();

    // Soft EQ for nighttime listening
    bedroom.set_bass(-2)?;
    bedroom.set_treble(-1)?;
    bedroom.set_loudness(true)?;

    // Lower volume
    bedroom.set_volume(15)?;

    // Auto-stop after 45 minutes
    bedroom.configure_sleep_timer("00:45:00")?;

    // Start playing
    bedroom.play()?;

    Ok(())
}

fn cancel_bedtime(sonos: &SonosSystem) -> Result<(), SdkError> {
    let bedroom = sonos.speaker("Bedroom").unwrap();

    bedroom.cancel_sleep_timer()?;
    bedroom.stop()?;

    // Restore daytime EQ
    bedroom.set_bass(0)?;
    bedroom.set_treble(0)?;

    Ok(())
}
```

## With group fade-out

If the bedroom is part of a group, lower all speakers together:

```rust
fn bedtime_group(sonos: &SonosSystem) -> Result<(), SdkError> {
    let bedroom = sonos.speaker("Bedroom").unwrap();
    let group = bedroom.group().unwrap();

    // Snapshot current levels for potential restore
    group.snapshot_volume()?;

    // Lower entire group
    group.set_volume(15)?;

    // Timer applies to the coordinator
    if let Some(coordinator) = group.coordinator() {
        coordinator.configure_sleep_timer("00:45:00")?;
    }

    Ok(())
}
```
