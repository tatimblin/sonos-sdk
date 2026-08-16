---
title: Quick Start
description: Control a Sonos speaker in under a minute.
---

## Your first program

This example discovers your Sonos network, plays music on a speaker, and reads its volume:

```rust
use sonos_sdk::prelude::*;

fn main() -> Result<(), SdkError> {
    // Discovers all speakers on your network
    let sonos = SonosSystem::new()?;

    // Get a speaker by name
    let speaker = sonos.speaker("Kitchen").unwrap();

    // Control playback
    speaker.play()?;

    // Read properties directly from the device
    let volume = speaker.volume.fetch()?;
    println!("Kitchen is playing at {}%", volume);

    Ok(())
}
```

## Three ways to read properties

Every property on a speaker supports three access patterns:

```rust
// 1. get() — instant, returns cached value (None if never fetched)
let cached = speaker.volume.get();

// 2. fetch() — makes a live SOAP call to the device
let live = speaker.volume.fetch()?;

// 3. watch() — subscribes to real-time events; the handle reads live
let volume = speaker.volume.watch()?;
println!("Volume right now: {:?}", volume.value());

// Hold that one handle: value() re-reads on every call, so there is no
// need to watch again to refresh it.
for _event in sonos.iter() {
    println!("Volume: {:?}", volume.value());
}
```

| Method | Speed | Freshness | Use when |
|--------|-------|-----------|----------|
| `get()` | Instant | May be stale | Displaying cached state in a UI |
| `fetch()` | ~10ms | Always fresh | Need the current value right now |
| `watch()` | Real-time | Live stream | Reacting to changes as they happen |

## Navigate between speakers and groups

```rust
let sonos = SonosSystem::new()?;

// Speakers → Groups
let kitchen = sonos.speaker("Kitchen").unwrap();
let group = kitchen.group().unwrap();
println!("Kitchen is in: {}", group.name);

// Groups → Speakers
for speaker in group.speakers() {
    println!("  - {}", speaker.name());
}
```

## Next steps

- [Architecture](/sonos-sdk/guides/architecture/) — understand the layered design
- [Properties](/sonos-sdk/guides/properties/) — deep dive into get/fetch/watch
- [Cookbook](/sonos-sdk/guides/cookbook/control-playback/) — common recipes
