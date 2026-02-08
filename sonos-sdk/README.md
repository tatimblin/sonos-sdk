# sonos-sdk

A sync-first, DOM-like SDK for controlling Sonos speakers. Access properties directly on speaker objects with a consistent three-method pattern.

## Features

- **Sync-First API**: All methods are synchronous - no async/await required
- **DOM-like Access**: Properties accessed directly on speaker objects (`speaker.volume.get()`)
- **Three Access Patterns**: `get()` for cached, `fetch()` for fresh, `watch()` for reactive
- **Automatic Subscriptions**: UPnP subscriptions managed automatically via watch/unwatch
- **Type Safety**: All properties are strongly typed
- **Blocking Iteration**: Event loop pattern for reactive applications

## Quick Start

```rust
use sonos_sdk::{SonosSystem, SdkError};

fn main() -> Result<(), SdkError> {
    // Create system with automatic device discovery (sync)
    let system = SonosSystem::new()?;

    // Get speaker by name
    let speaker = system.get_speaker_by_name("Living Room")
        .ok_or_else(|| SdkError::SpeakerNotFound("Living Room".to_string()))?;

    // Access properties directly on the speaker object
    let volume = speaker.volume.get();           // Cached value (instant)
    let fresh = speaker.volume.fetch()?;         // Fresh from device (API call)
    let current = speaker.volume.watch()?;       // Start watching for changes

    println!("Volume: {:?}", volume);
    Ok(())
}
```

## The Get/Fetch/Watch Pattern

Every property on a speaker provides three methods:

### `get()` - Cached Value (Instant)

Returns the cached value without any network calls. Fast and always available.

```rust
// Get cached volume - returns Option<Volume>
if let Some(vol) = speaker.volume.get() {
    println!("Volume: {}%", vol.0);
}

// Get cached playback state
if let Some(state) = speaker.playback_state.get() {
    println!("State: {:?}", state);
}
```

### `fetch()` - Fresh Value (API Call)

Makes a synchronous API call to the device and updates the cache.

```rust
// Fetch fresh volume from device
let volume = speaker.volume.fetch()?;
println!("Fresh volume: {}%", volume.0);

// Fetch fresh playback state
let state = speaker.playback_state.fetch()?;
println!("Fresh state: {:?}", state);
```

### `watch()` - Reactive Updates

Registers for change notifications. Changes appear in `system.iter()`.

```rust
// Start watching volume changes
speaker.volume.watch()?;

// Start watching playback state
speaker.playback_state.watch()?;

// Stop watching when done
speaker.volume.unwatch();
```

## Event Loop Pattern

Build reactive applications by iterating over property changes:

```rust
use sonos_sdk::{SonosSystem, SdkError};
use std::time::Duration;

fn main() -> Result<(), SdkError> {
    let system = SonosSystem::new()?;
    
    // Get a speaker
    let speaker = system.get_speaker_by_name("Living Room")
        .ok_or_else(|| SdkError::SpeakerNotFound("Living Room".to_string()))?;

    // Watch properties of interest
    speaker.volume.watch()?;
    speaker.playback_state.watch()?;
    speaker.current_track.watch()?;

    println!("Listening for changes... (Ctrl+C to exit)");

    // Event loop - blocks until changes occur
    for event in system.iter() {
        println!("Property '{}' changed on speaker {}", 
            event.property_key, event.speaker_id);

        // React to specific property changes
        match event.property_key {
            "volume" => {
                if let Some(vol) = speaker.volume.get() {
                    println!("  New volume: {}%", vol.0);
                }
            }
            "playback_state" => {
                if let Some(state) = speaker.playback_state.get() {
                    println!("  New state: {:?}", state);
                }
            }
            "current_track" => {
                if let Some(track) = speaker.current_track.get() {
                    println!("  Now playing: {} - {}", 
                        track.title.as_deref().unwrap_or("Unknown"),
                        track.artist.as_deref().unwrap_or("Unknown"));
                }
            }
            _ => {}
        }
    }

    Ok(())
}
```

### Non-Blocking Iteration

For applications that need to check for events without blocking:

```rust
// Check for events without blocking
for event in system.iter().try_iter() {
    println!("Event: {:?}", event);
}

// Wait with timeout
if let Some(event) = system.iter().recv_timeout(Duration::from_secs(1)) {
    println!("Got event: {:?}", event);
}
```

## Available Properties

### Audio Control (RenderingControl)
| Property | Type | Description |
|----------|------|-------------|
| `volume` | `Volume` (u8) | Master volume (0-100) |
| `mute` | `Mute` (bool) | Mute state |
| `bass` | `Bass` (i8) | Bass EQ (-10 to +10) |
| `treble` | `Treble` (i8) | Treble EQ (-10 to +10) |
| `loudness` | `Loudness` (bool) | Loudness compensation |

### Playback (AVTransport)
| Property | Type | Description |
|----------|------|-------------|
| `playback_state` | `PlaybackState` | Playing/Paused/Stopped/Transitioning |
| `position` | `Position` | Current position and duration |
| `current_track` | `CurrentTrack` | Track metadata (title, artist, album) |

### Grouping (ZoneGroupTopology)
| Property | Type | Description |
|----------|------|-------------|
| `group_membership` | `GroupMembership` | Group ID and coordinator status |

## Speaker Lookup

```rust
// Get speaker by friendly name
let speaker = system.get_speaker_by_name("Kitchen")?;

// Get speaker by unique ID
let speaker = system.get_speaker_by_id(&speaker_id)?;

// Get all speakers
for speaker in system.speakers() {
    println!("{}: {} ({})", speaker.name, speaker.model_name, speaker.ip);
}

// Get all speaker names
let names = system.speaker_names();
```

## Error Handling

The SDK provides structured error types:

```rust
use sonos_sdk::SdkError;

match speaker.volume.fetch() {
    Ok(vol) => println!("Volume: {}%", vol.0),
    Err(SdkError::ApiError(e)) => println!("API error: {}", e),
    Err(SdkError::SpeakerNotFound(name)) => println!("Speaker not found: {}", name),
    Err(e) => println!("Other error: {}", e),
}
```

## Architecture

```text
sonos-sdk (Sync-First DOM-like API)
    ↓
sonos-state (State Management) ←→ sonos-event-manager (Event Subscriptions)
    ↓                                    ↓
sonos-api (UPnP Operations)         sonos-stream (Event Processing)
```

## License

MIT License

## See Also

- [`sonos-api`](../sonos-api) - Low-level UPnP operations
- [`sonos-discovery`](../sonos-discovery) - Device discovery
- [`sonos-stream`](../sonos-stream) - Event streaming
