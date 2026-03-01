# sonos-sdk

[![Crates.io](https://img.shields.io/crates/v/sonos-sdk.svg)](https://crates.io/crates/sonos-sdk)
[![docs.rs](https://docs.rs/sonos-sdk/badge.svg)](https://docs.rs/sonos-sdk)
[![License](https://img.shields.io/crates/l/sonos-sdk.svg)](LICENSE-MIT)

A Rust SDK for controlling Sonos speakers over the local network via UPnP/SOAP. Provides a sync-first, DOM-like API with reactive state management — no async/await required.

## Quick Start

Add `sonos-sdk` to your project:

```toml
[dependencies]
sonos-sdk = "0.1"
```

Discover speakers and start controlling them:

```rust
use sonos_sdk::{SonosSystem, SdkError};

fn main() -> Result<(), SdkError> {
    let system = SonosSystem::new()?;

    let speaker = system.get_speaker_by_name("Living Room")
        .ok_or_else(|| SdkError::SpeakerNotFound("Living Room".to_string()))?;

    // Read properties
    let volume = speaker.volume.get();           // Cached value (instant)
    let fresh = speaker.volume.fetch()?;         // Fresh from device
    speaker.volume.watch()?;                     // Start watching for changes

    // Control playback
    speaker.play()?;
    speaker.set_volume(40)?;

    // React to changes
    for event in system.iter() {
        println!("{} changed on {}", event.property_key, event.speaker_id);
    }

    Ok(())
}
```

## Features

- **Sync-first API** — all methods are synchronous, no async/await required
- **DOM-like property access** — `speaker.volume.get()`, `speaker.playback_state.fetch()`
- **Three access patterns** — `get()` for cached, `fetch()` for fresh, `watch()` for reactive
- **Automatic UPnP subscriptions** — managed via watch/unwatch lifecycle
- **Speaker actions** — play, pause, stop, seek, volume, EQ, queue management, sleep timers
- **Group management** — create groups, dissolve, join, leave, group volume/mute
- **Firewall fallback** — automatic polling when UPnP events are blocked
- **Type safety** — strongly typed properties and operations

## Crates

| Crate | Description |
|-------|-------------|
| [`sonos-sdk`](https://crates.io/crates/sonos-sdk) | High-level sync-first SDK (start here) |
| [`sonos-api`](https://crates.io/crates/sonos-api) | Low-level type-safe UPnP operations |

Internal crates (`sonos-sdk-*`) are published as transitive dependencies and are not intended for direct use.

## Architecture

```text
sonos-sdk          Sync-first DOM-like API
    |
sonos-state        Reactive state management
    |         \
sonos-api      sonos-stream → callback-server
    |                            |
soap-client    sonos-discovery   sonos-event-manager
```

## Contributing

```bash
# Build
cargo build

# Test
cargo test

# Lint
cargo clippy --workspace -- -D warnings

# Format
cargo fmt --all -- --check

# Generate docs
cargo doc --workspace --no-deps
```

Requires Rust 1.80+ (MSRV).

Sonos speakers must be on the local network. Discovery uses SSDP multicast on port 1400.

## License

Licensed under either of

- [MIT License](LICENSE-MIT)
- [Apache License, Version 2.0](LICENSE-APACHE)

at your option.
