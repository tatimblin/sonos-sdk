---
title: Multi-Room Announcement
description: Play an announcement across all speakers, then resume.
---

Interrupt all speakers with an announcement, then restore their previous state.

```rust
use sonos_sdk::prelude::*;

fn announce(sonos: &SonosSystem, audio_uri: &str) -> Result<(), SdkError> {
    let speakers: Vec<_> = sonos.speakers().collect();
    let main = &speakers[0];
    let main_group = main.group().unwrap();

    // Snapshot group volume before changing anything
    main_group.snapshot_volume()?;

    // Group all speakers together for synchronized playback
    for speaker in &speakers[1..] {
        speaker.join_group(&main_group)?;
    }

    // Set announcement volume
    let group = main.group().unwrap();
    group.set_volume(35)?;

    // Play the announcement
    main.set_av_transport_uri(audio_uri, "")?;
    main.play()?;

    Ok(())
}
```

## Wait for completion and restore

Use reactive state to detect when the announcement finishes:

```rust
fn announce_and_restore(sonos: &SonosSystem, audio_uri: &str) -> Result<(), SdkError> {
    let main = sonos.speakers().next().unwrap();

    // Start announcement (as above)
    main.set_av_transport_uri(audio_uri, "")?;
    main.play()?;

    // Wait for playback to stop
    for event in sonos.iter() {
        let state = main.playback_state.watch()?;
        if let Some(PlaybackState::Stopped) = state.value() {
            break;
        }
    }

    // Dissolve the announcement group
    let group = main.group().unwrap();
    group.dissolve();

    Ok(())
}
```

## TTS announcement pattern

If you have a text-to-speech service that returns an audio URL:

```rust
fn tts_announce(sonos: &SonosSystem, message: &str) -> Result<(), SdkError> {
    // Your TTS service generates an audio file
    let audio_url = format!("http://your-tts-server/speak?text={}", message);

    announce(sonos, &audio_url)?;

    Ok(())
}
```
