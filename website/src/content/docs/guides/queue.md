---
title: Queue
description: Add, remove, reorder, and save queue tracks.
---

The speaker queue is a playlist of tracks that plays sequentially. Use `set_av_transport_uri` to point at the queue before calling `play()`.

## Methods

| Method | Signature | Description |
|--------|-----------|-------------|
| `add_uri_to_queue` | `add_uri_to_queue(uri, metadata, position, enqueue_as_next)` | Add track to queue |
| `remove_track_from_queue` | `remove_track_from_queue(object_id, update_id)` | Remove one track |
| `remove_track_range_from_queue` | `remove_track_range_from_queue(update_id, start, count)` | Remove a range |
| `remove_all_tracks_from_queue` | `remove_all_tracks_from_queue()` | Clear the queue |
| `save_queue` | `save_queue(title, object_id)` | Save queue as Sonos playlist |
| `create_saved_queue` | `create_saved_queue(title, uri, metadata)` | Create new playlist with track |
| `backup_queue` | `backup_queue()` | Backup queue state |

## Add a track

```rust
use sonos_sdk::prelude::*;

fn main() -> Result<(), SdkError> {
    let sonos = SonosSystem::new()?;
    let speaker = sonos.speaker("Living Room").unwrap();

    let result = speaker.add_uri_to_queue(
        "x-rincon-mp3radio://example.com/track",
        "",    // metadata (DIDL-Lite XML)
        0,     // position: 0 = end of queue, 1+ = specific slot
        false, // enqueue_as_next: if true, plays after current track
    )?;

    println!("Queued at position {}", result.first_track_number_enqueued);

    Ok(())
}
```

## Add as "play next"

```rust
speaker.add_uri_to_queue(
    "x-rincon-mp3radio://example.com/next",
    "",
    0,
    true, // inserts after currently playing track
)?;
```

## Remove a track

```rust
// Object IDs come from browsing the queue content (e.g., "Q:0/3")
speaker.remove_track_from_queue("Q:0/3", 0)?;
```

## Remove a range

```rust
// Remove 3 tracks starting at index 5 (0-based)
let result = speaker.remove_track_range_from_queue(0, 5, 3)?;
println!("New update ID: {}", result.new_update_id);
```

## Clear the queue

```rust
speaker.remove_all_tracks_from_queue()?;
```

## Save as playlist

```rust
// Save current queue as a named Sonos playlist
let result = speaker.save_queue("Weekend Favorites", "")?;
println!("Playlist ID: {}", result.assigned_object_id);
```

## Create a new playlist

```rust
// Create a playlist and seed it with a track
let result = speaker.create_saved_queue(
    "Morning Mix",
    "x-rincon-mp3radio://example.com/track",
    "",
)?;
println!("New playlist: {}", result.assigned_object_id);
```

## Backup

```rust
// Backup queue state for recovery after interruption
speaker.backup_queue()?;
```

## Playing from the queue

After populating the queue, point the transport at it:

```rust
// The URI format is x-rincon-queue:{speaker_rincon_id}#0
speaker.set_av_transport_uri("x-rincon-queue:RINCON_XXX#0", "")?;
speaker.play()?;

// Jump to track 3 in the queue
speaker.seek(SeekTarget::Track(3))?;
```
