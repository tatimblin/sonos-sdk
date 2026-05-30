---
title: Installation
description: Add sonos-sdk to your Rust project.
---

## Add the dependency

```toml
# Cargo.toml
[dependencies]
sonos-sdk = "0.5"
```

## Requirements

- **Rust** 1.75 or later (2024 edition recommended)
- **Network access** to Sonos devices on your local network (port 1400)
- Sonos devices must be on the same subnet as the machine running your code

## Feature flags

The SDK works out of the box with no feature flags required. Optional features:

| Feature | Description |
|---------|-------------|
| `test-support` | Enables mock discovery and test utilities |

```toml
[dev-dependencies]
sonos-sdk = { version = "0.5", features = ["test-support"] }
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
