---
title: Installation
description: Add sonos-sdk to your Rust project.
---

## Add the dependency

```toml
# Cargo.toml
[dependencies]
sonos-sdk = "0.8"
```

## Requirements

- **Rust** 1.98 or later. That is the crate's MSRV, enforced by CI; the crate itself is edition 2021.
- **Network access** to Sonos devices on your local network (port 1400)
- Sonos devices must be on the same subnet as the machine running your code

## Feature flags

The SDK works out of the box with no feature flags required. Optional features:

| Feature | Description |
|---------|-------------|
| `test-support` | Builds a `SonosSystem` from synthetic devices instead of the network |

Enable `test-support` in dev-dependencies to drive the SDK from tests with no
speakers present. It unlocks `SonosSystem::with_speakers()`,
`SonosSystem::with_groups()`, `SonosSystem::from_discovered_devices()`, and the
offline constructors.

```toml
[dev-dependencies]
sonos-sdk = { version = "0.8", features = ["test-support"] }
```

```rust
// Two speakers, each in a standalone group. No SSDP, no sockets, no cache reads.
let system = SonosSystem::with_groups(&["Kitchen", "Bedroom"]);
assert_eq!(system.speakers().len(), 2);
assert!(system.speaker("Kitchen").is_some());
```

## Verify installation

```rust
use sonos_sdk::prelude::*;

fn main() -> Result<(), SdkError> {
    let sonos = SonosSystem::new()?;
    println!("Found {} speakers", sonos.speakers().len());
    Ok(())
}
```

```bash
cargo run
```

If you see your speakers listed, you're ready to go. If not, check the [troubleshooting guide](/sonos-sdk/troubleshooting/discovery/).
