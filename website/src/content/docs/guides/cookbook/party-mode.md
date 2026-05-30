---
title: Party Mode
description: Group all speakers together with boosted audio.
---

Group every speaker in the house, boost the EQ, and start playing from a single source.

```rust
use sonos_sdk::prelude::*;

fn party_mode(sonos: &SonosSystem) -> Result<(), SdkError> {
    let speakers: Vec<_> = sonos.speakers().collect();
    let main = &speakers[0];
    let main_group = main.group().unwrap();

    // Group all speakers together
    for speaker in &speakers[1..] {
        speaker.join_group(&main_group)?;
    }

    // Boost EQ on each speaker individually (EQ is per-speaker, not per-group)
    for speaker in &speakers {
        speaker.set_bass(7)?;
        speaker.set_treble(3)?;
        speaker.set_loudness(false)?;
    }

    // Set group volume
    let group = main.group().unwrap();
    group.set_volume(50)?;

    // Shuffle the queue
    main.set_play_mode(PlayMode::Shuffle)?;
    main.set_crossfade_mode(true)?;
    main.play()?;

    Ok(())
}
```

## Wind down

Restore speakers to individual groups:

```rust
fn end_party(sonos: &SonosSystem) -> Result<(), SdkError> {
    let main = sonos.speakers().next().unwrap();
    let group = main.group().unwrap();

    // Stop playback
    main.stop()?;

    // Dissolve the group
    let result = group.dissolve();

    if !result.is_success() {
        for (id, err) in &result.failed {
            eprintln!("Could not ungroup {}: {}", id, err);
        }
    }

    // Reset EQ on all speakers
    for speaker in sonos.speakers() {
        speaker.set_bass(0)?;
        speaker.set_treble(0)?;
        speaker.set_loudness(true)?;
    }

    Ok(())
}
```
