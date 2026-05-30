---
title: Monitor Volume
description: React to volume changes in real time.
---

## Fetch current volume

```rust
use sonos_sdk::prelude::*;

fn main() -> Result<(), SdkError> {
    let sonos = SonosSystem::new()?;
    let speaker = sonos.speaker("Kitchen").unwrap();

    let volume = speaker.volume.fetch()?;
    println!("Kitchen volume: {}%", volume);

    Ok(())
}
```

## Watch for volume changes

```rust
use sonos_sdk::prelude::*;

fn main() -> Result<(), SdkError> {
    let sonos = SonosSystem::new()?;
    let speaker = sonos.speaker("Kitchen").unwrap();

    for event in sonos.iter() {
        let volume = speaker.volume.watch()?;
        if let Some(vol) = volume.value() {
            println!("Volume is now: {}%", vol);
        }
    }

    Ok(())
}
```

## Set volume

```rust
// Absolute volume (0-100)
speaker.volume.set(50)?;

// Read back to confirm
let confirmed = speaker.volume.fetch()?;
assert_eq!(confirmed, 50);
```

## Monitor multiple speakers

```rust
use sonos_sdk::prelude::*;

fn main() -> Result<(), SdkError> {
    let sonos = SonosSystem::new()?;

    // Events from all speakers arrive here
    for event in sonos.iter() {
        if let Some(speaker) = sonos.speaker_by_id(&event.speaker_id) {
            let volume = speaker.volume.watch()?;
            println!("{}: {:?}", speaker.name(), volume.value());
        }
    }

    Ok(())
}
```

## Related properties

Volume often changes alongside these:

```rust
for event in sonos.iter() {
    let volume = speaker.volume.watch()?;
    let mute = speaker.mute.watch()?;
    let loudness = speaker.loudness.watch()?;

    println!("Volume: {:?}, Mute: {:?}, Loudness: {:?}",
        volume.value(), mute.value(), loudness.value());
}
```
