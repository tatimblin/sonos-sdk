# sonos-sdk

A sync-first, DOM-like SDK for controlling Sonos speakers. Access properties directly on speaker objects with a consistent three-method pattern.

## Features

- **Sync-First API**: All methods are synchronous - no async/await required
- **Cheap constructor**: `SonosSystem::new()` does discovery only; event infrastructure starts lazily on the first `watch()`
- **DOM-like Access**: Properties accessed directly on speaker objects (`speaker.volume.get()`)
- **Three Access Patterns**: `get()` for cached, `fetch()` for fresh, `watch()` for reactive
- **RAII Subscriptions**: UPnP subscriptions managed automatically via `WatchHandle` (drop to unsubscribe)
- **Fluent navigation**: `speaker.group()`, `group.speaker("name")`
- **Type Safety**: All properties are strongly typed
- **Blocking Iteration**: Event loop pattern for reactive applications

## Quick Start

```rust
use sonos_sdk::{SdkError, SonosSystem};

fn main() -> Result<(), SdkError> {
    // Create system with automatic device discovery (sync)
    let system = SonosSystem::new()?;

    // Get speaker by name
    let speaker = system
        .speaker("Living Room")
        .ok_or_else(|| SdkError::SpeakerNotFound("Living Room".to_string()))?;

    // Access properties directly on the speaker object
    let cached = speaker.volume.get();      // Cached value (instant)
    let fresh = speaker.volume.fetch()?;    // Fresh from device (SOAP call)
    let handle = speaker.volume.watch()?;   // Start watching for changes

    println!("Volume: {:?}, now {}%", cached, fresh.0);
    println!("Live value: {:?}", handle.value());
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
    println!("State: {state:?}");
}
```

### `fetch()` - Fresh Value (SOAP Call)

Makes a synchronous call to the device and updates the cache.

```rust
// Fetch fresh volume from device
let volume = speaker.volume.fetch()?;
println!("Fresh volume: {}%", volume.0);

// Fetch fresh playback state
let state = speaker.playback_state.fetch()?;
println!("Fresh state: {state:?}");
```

### `watch()` - Reactive Updates

Returns a `WatchHandle` that keeps the subscription alive. Changes appear in `system.iter()`.
`handle.value()` reads the property live from the store, so the handle is a view rather than a
snapshot. Dropping the handle starts a 50 ms grace period before unsubscribing, so code that
drops and re-acquires handles every frame does not churn UPnP subscriptions.

```rust
// Start watching volume — hold the handle to keep the subscription alive
let vol_handle = speaker.volume.watch()?;

// Start watching playback state
let _playback_handle = speaker.playback_state.watch()?;

// Access the current value via the handle
if let Some(vol) = vol_handle.value() {
    println!("Volume: {}%", vol.0);
}

// `mode()` reports whether the value is backed by UPnP events, polling, or cache only
println!("Watch mode: {:?}", vol_handle.mode());

// Dropping the handle starts the grace period; the subscription persists for 50ms
drop(vol_handle);
```

## Event Loop Pattern

Build reactive applications by iterating over property changes. A `ChangeEvent` carries the new
value in its `change` field, so you match on the change itself instead of re-reading the cache:

```rust
use sonos_sdk::{PropertyChange, SdkError, SonosSystem};

fn main() -> Result<(), SdkError> {
    let system = SonosSystem::new()?;

    let speaker = system
        .speaker("Living Room")
        .ok_or_else(|| SdkError::SpeakerNotFound("Living Room".to_string()))?;

    // Watch properties of interest — hold handles to keep subscriptions alive
    let _vol = speaker.volume.watch()?;
    let _playback = speaker.playback_state.watch()?;
    let _track = speaker.current_track.watch()?;

    println!("Listening for changes... (Ctrl+C to exit)");

    // Event loop - blocks until changes occur
    for event in system.iter() {
        println!(
            "Property '{}' changed on speaker {}",
            event.property_key(),
            event.speaker_id
        );

        match &event.change {
            PropertyChange::Volume(vol) => {
                println!("  New volume: {}%", vol.value());
            }
            PropertyChange::PlaybackState(state) => {
                println!("  New state: {state:?}");
            }
            PropertyChange::CurrentTrack(track) => {
                println!(
                    "  Now playing: {} - {}",
                    track.title.as_deref().unwrap_or("Unknown"),
                    track.artist.as_deref().unwrap_or("Unknown")
                );
            }
            other => println!("  {} changed", other.key()),
        }
    }

    Ok(())
}
```

`PropertyChange` is `#[non_exhaustive]`: it gains a variant whenever a property is added, so a
`match` over it needs a catch-all arm.

`event.property_key()` and `event.service()` are derived from the payload. The event's own
fields are `speaker_id`, `change`, `source` and `timestamp`.

### Non-Blocking Iteration

For applications that need to check for events without blocking. Bind the iterator first —
`try_iter()` and `timeout_iter()` borrow it:

```rust
use std::time::Duration;

let events = system.iter();

// Drain whatever is queued, without blocking
for event in events.try_iter() {
    println!("Event: {:?}", event.change);
}

// Wait with timeout
if let Some(event) = events.recv_timeout(Duration::from_secs(1)) {
    println!("Got event: {:?}", event.change);
}

// Or iterate with a per-item timeout
for event in events.timeout_iter(Duration::from_millis(250)) {
    println!("Event: {:?}", event.change);
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
| `position` | `Position` | Current position and duration, in milliseconds |
| `current_track` | `CurrentTrack` | Track metadata (title, artist, album, art, uri) |

### Grouping (ZoneGroupTopology)
| Property | Type | Description |
|----------|------|-------------|
| `group_membership` | `GroupMembership` | Group ID and coordinator status |

### Group Properties (GroupRenderingControl)

These live on `Group`, not `Speaker`, and follow the same get/fetch/watch pattern:

| Property | Type | Description |
|----------|------|-------------|
| `volume` | `GroupVolume` (u16) | Group master volume |
| `mute` | `GroupMute` (bool) | Group mute state |
| `volume_changeable` | `GroupVolumeChangeable` (bool) | Whether group volume can be set |

## Speaker Lookup

```rust
use sonos_sdk::SpeakerId;

// Get speaker by friendly name. Misses trigger a rate-limited SSDP rediscovery.
let kitchen = system.speaker("Kitchen");

// Get speaker by unique ID
let by_id = system.speaker_by_id(&SpeakerId::new("RINCON_123"));

// Get all speakers
for speaker in system.speakers() {
    println!("{}: {} ({})", speaker.name, speaker.model_name, speaker.ip);
}

// Get all speaker names
let names = system.speaker_names();
```

## Groups

`Group` exposes its members, its own audio properties, and membership changes:

```rust
// Every speaker belongs to a group, even when standalone
let group = speaker.group().expect("speaker is in a group");

println!("group {} has {} member(s)", group.id, group.member_count());
println!("standalone: {}", group.is_standalone());

// Navigate within the group
let coordinator = group.coordinator();
let members = group.members();

// Group-wide audio
group.set_volume(25)?;
group.set_mute(false)?;

// Membership changes
speaker.join_group(&group)?;
speaker.leave_group()?;
```

System-level group access mirrors speaker lookup: `system.groups()`, `system.group(name)`,
`system.group_by_id(&id)`, `system.group_for_speaker(&speaker_id)` and
`system.create_group(...)`.

## Speaker Actions

Beyond properties, `Speaker` exposes the AVTransport and RenderingControl write path directly:

```rust
use sonos_sdk::{PlayMode, SeekTarget};

speaker.play()?;
speaker.pause()?;
speaker.stop()?;
speaker.next()?;
speaker.previous()?;

speaker.seek(SeekTarget::Time("0:02:30".to_string()))?;
speaker.set_play_mode(PlayMode::ShuffleNoRepeat)?;
speaker.set_volume(40)?;
speaker.set_mute(false)?;
speaker.set_bass(2)?;
speaker.set_treble(-1)?;
speaker.set_loudness(true)?;
```

Writes update the state cache optimistically once the SOAP call succeeds, so
`speaker.volume.get()` reflects the new value immediately. If the speaker silently rejects a
command, the cache stays stale until the next UPnP event corrects it — `watch()` is the
authoritative view.

## Error Handling

The SDK provides structured error types:

```rust
use sonos_sdk::SdkError;

match speaker.volume.fetch() {
    Ok(vol) => println!("Volume: {}%", vol.0),
    Err(SdkError::ApiError(e)) => println!("API error: {e}"),
    Err(SdkError::SpeakerNotFound(name)) => println!("Speaker not found: {name}"),
    Err(SdkError::FetchFailed(msg)) => println!("Fetch failed: {msg}"),
    Err(e) => println!("Other error: {e}"),
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

`sonos-sdk` and `sonos-api` are the user-facing crates. Everything below them is an
implementation detail, published only so this crate resolves as a dependency.

## Examples

```bash
cargo run -p sonos-sdk --example basic_usage_sdk
cargo run -p sonos-sdk --example smart_dashboard
cargo run -p sonos-sdk --example property_observer
cargo run -p sonos-sdk --example sdk_demo
cargo run -p sonos-sdk --example watch_grace_period_demo
cargo run -p sonos-sdk --example group_lifecycle_test
```

## License

Licensed under either of [Apache License, Version 2.0](../LICENSE-APACHE) or
[MIT license](../LICENSE-MIT), at your option.

## See Also

- [`sonos-api`](../sonos-api) - Stateless UPnP operations against a single speaker
