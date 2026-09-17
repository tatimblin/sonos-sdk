# Sonos SDK

[![CI](https://github.com/tatimblin/sonos-sdk/workflows/CI/badge.svg)](https://github.com/tatimblin/sonos-sdk/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/sonos-sdk.svg)](https://crates.io/crates/sonos-sdk)
[![Documentation](https://docs.rs/sonos-sdk/badge.svg)](https://docs.rs/sonos-sdk)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](https://github.com/tatimblin/sonos-sdk#license)

Modern Rust SDK for Sonos device control via UPnP/SOAP with a DOM-like API and sync-first design.

## Quick Start

**Requirements:** Sonos speakers on the local network. Discovery uses SSDP multicast (`239.255.255.250:1900`); once a device is found, control requests go to its HTTP/SOAP endpoint on port 1400.

```rust
use sonos_sdk::{SdkError, SonosSystem};

fn main() -> Result<(), SdkError> {
    // Discover devices and create system (sync)
    let system = SonosSystem::new()?;

    // Get first available speaker
    let speaker_names = system.speaker_names();
    if speaker_names.is_empty() {
        println!("No Sonos speakers found on the network");
        return Ok(());
    }

    let speaker = system
        .speaker(&speaker_names[0])
        .ok_or_else(|| SdkError::SpeakerNotFound(speaker_names[0].clone()))?;

    // Control playback and properties
    speaker.play()?;
    let volume = speaker.volume.fetch()?;
    println!("Playing on {} at {}%", speaker.name, volume.0);

    Ok(())
}
```

Add to your `Cargo.toml`:
```toml
[dependencies]
sonos-sdk = "0.8"
```

The minimum supported Rust version is **1.98**, and the workspace builds on edition 2021.

## Crates

Two crates are user-facing:

| Crate | Purpose |
|-------|---------|
| [`sonos-sdk`](sonos-sdk/) | Sync-first, DOM-like API over a whole Sonos system. Start here. |
| [`sonos-api`](sonos-api/) | Stateless, type-safe UPnP/SOAP operations against a single speaker. |

The rest — `sonos-state`, `sonos-stream`, `sonos-event-manager`, `sonos-discovery`, `callback-server`, `soap-client` — are implementation details. They are published to crates.io so `sonos-sdk` resolves as a transitive dependency, and their APIs carry no stability promise.

## Key Concepts

### DOM-like API
Access properties directly on speaker objects using familiar syntax:
```rust
let cached = speaker.volume.get();    // cached value, no network call
let fresh = speaker.volume.fetch()?;  // fresh SOAP call, updates the cache
let handle = speaker.volume.watch()?; // reactive updates while the handle lives
```

### Three-Method Pattern
Every property provides three access methods:
- **`get()`** - Returns the cached value as an `Option`, no network calls (instant)
- **`fetch()`** - Makes a SOAP call to the device, updates the cache (fresh data)
- **`watch()`** - Returns a `WatchHandle` that keeps a subscription alive (reactive)

### Change Events Carry Values
`system.iter()` yields a `ChangeEvent` that includes the new value, so you match on it
directly instead of re-reading the cache:

```rust
use sonos_sdk::PropertyChange;

let _watch = speaker.volume.watch()?; // hold the handle to keep receiving

for event in system.iter() {
    match &event.change {
        PropertyChange::Volume(v) => println!("volume -> {}%", v.value()),
        PropertyChange::PlaybackState(s) => println!("playback -> {s:?}"),
        other => println!("{} changed", other.key()),
    }
}
```

This matters when events queue up: the cache only holds the *latest* value, so a
`Playing → Transitioning → Playing` sequence reads as `Playing` three times if you re-read the
cache per event. The event payload preserves the full sequence. Use `get()` when you want
current state (a full UI repaint); use `event.change` when you want what changed.

### Sync-First Design
All operations are synchronous - no `async`/`await` required. The SDK handles the complexity of UPnP subscriptions and event processing internally while presenting a simple, blocking API to your application.

## Examples & Documentation

### 📚 Learn More
- **[API Documentation](https://docs.rs/sonos-sdk)** - Complete API reference and detailed examples
- **[Project Status](https://github.com/tatimblin/sonos-sdk/blob/main/docs/STATUS.md)** - Service completion matrix and roadmap
- **[Architecture Overview](https://github.com/tatimblin/sonos-sdk/blob/main/docs/SUMMARY.md)** - System design and component relationships

### 🔧 Development
- **[Contributing Guide](https://github.com/tatimblin/sonos-sdk/blob/main/docs/CONTRIBUTING.md)** - Development workflow and CI requirements
- **[Developer Guide](https://github.com/tatimblin/sonos-sdk/blob/main/CLAUDE.md)** - Comprehensive development documentation

### 💡 Examples
- **[Basic Usage](https://github.com/tatimblin/sonos-sdk/blob/main/sonos-sdk/examples/basic_usage_sdk.rs)** - DOM-like API demonstration
- **[Smart Dashboard](https://github.com/tatimblin/sonos-sdk/blob/main/sonos-sdk/examples/smart_dashboard.rs)** - Reactive property monitoring
- **[All Examples](https://github.com/tatimblin/sonos-sdk/tree/main/sonos-sdk/examples)** - Complete examples collection

## Community & Projects

### 🛠️ Built with sonos-sdk
- **[sonos-cli](https://github.com/tatimblin/sonos-cli)** - Command-line interface for Sonos speaker control and automation
- **[oto](https://github.com/Oszkar/oto)** - Fast, local-first multi-platform Sonos controller with a Flutter UI on a Rust core (Android, Windows)

### 🤝 Contributing
We welcome contributions! Whether you're building applications, finding bugs, or improving documentation - every contribution helps make the SDK better.

**Building something with sonos-sdk?** We'd love to feature your project here. Open an issue or submit a PR to add it to this list!

## License

Licensed under either of
- Apache License, Version 2.0 ([LICENSE-APACHE](https://github.com/tatimblin/sonos-sdk/blob/main/LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](https://github.com/tatimblin/sonos-sdk/blob/main/LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
