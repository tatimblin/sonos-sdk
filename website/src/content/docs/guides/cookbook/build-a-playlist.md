---
title: Build a Playlist
description: Programmatically build a queue and save it as a Sonos playlist.
---

Build a queue from scratch, reorder tracks, and save the result as a reusable Sonos playlist.

```rust
use sonos_sdk::prelude::*;

fn build_playlist(sonos: &SonosSystem) -> Result<(), SdkError> {
    let speaker = sonos.speaker("Living Room").unwrap();

    // Start with an empty queue
    speaker.remove_all_tracks_from_queue()?;

    // Add tracks
    speaker.add_uri_to_queue("x-rincon-mp3radio://example.com/track1", "", 0, false)?;
    speaker.add_uri_to_queue("x-rincon-mp3radio://example.com/track2", "", 0, false)?;
    speaker.add_uri_to_queue("x-rincon-mp3radio://example.com/track3", "", 0, false)?;
    speaker.add_uri_to_queue("x-rincon-mp3radio://example.com/track4", "", 0, false)?;
    speaker.add_uri_to_queue("x-rincon-mp3radio://example.com/track5", "", 0, false)?;

    // Save as a Sonos playlist
    let result = speaker.save_queue("Friday Night Mix", "")?;
    println!("Saved playlist: {}", result.assigned_object_id);

    // Start playing
    speaker.set_av_transport_uri("x-rincon-queue:RINCON_XXX#0", "")?;
    speaker.set_play_mode(PlayMode::Shuffle)?;
    speaker.play()?;

    Ok(())
}
```

## Add "play next" while listening

Insert a track after the currently playing one without disrupting playback:

```rust
// This goes right after whatever is playing now
speaker.add_uri_to_queue(
    "x-rincon-mp3radio://example.com/great-song",
    "",
    0,
    true, // enqueue_as_next
)?;
```

## Remove tracks

```rust
// Remove a specific track by its queue object ID
speaker.remove_track_from_queue("Q:0/3", 0)?;

// Remove a range: 2 tracks starting at index 1
speaker.remove_track_range_from_queue(0, 1, 2)?;

// Nuclear option: clear everything
speaker.remove_all_tracks_from_queue()?;
```

## Create a playlist from a single seed track

```rust
let result = speaker.create_saved_queue(
    "Discover Weekly",
    "x-rincon-mp3radio://example.com/seed-track",
    "",
)?;
println!("Created playlist: {}", result.assigned_object_id);
```
