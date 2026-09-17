# sonos-discovery

Internal implementation detail of [sonos-sdk](https://crates.io/crates/sonos-sdk). Published to crates.io as `sonos-sdk-discovery` so that `sonos-sdk` resolves as a dependency; not intended for direct use, and its API carries no stability promise. Applications should call `SonosSystem::new()`, which runs discovery for them.

Discovers Sonos devices on the local network using SSDP (Simple Service Discovery Protocol) and UPnP device descriptions.

## Features

- Simple API for one-time device discovery
- Iterator-based streaming for flexible processing
- Automatic deduplication of devices
- Filters out non-Sonos devices
- Configurable timeout
- Resource cleanup on early termination

## Usage

Within the workspace, the crate is depended on by its published name and bound to the
`sonos_discovery` lib name:

```toml
[dependencies]
sonos-discovery = { package = "sonos-sdk-discovery", path = "../sonos-discovery", version = "0.8.0" }
```

### Quick Start

Discover all Sonos devices with the default 3-second timeout:

```rust
use sonos_discovery::get;

fn main() {
    let devices = get();
    for device in devices {
        println!("Found {} at {}", device.name, device.ip_address);
    }
}
```

### Custom Timeout

```rust
use sonos_discovery::get_with_timeout;
use std::time::Duration;

fn main() {
    let devices = get_with_timeout(Duration::from_secs(5));
    for device in devices {
        println!("{} - {} ({})", device.name, device.model_name, device.ip_address);
    }
}
```

### Iterator API

`get_iter()` returns a `DiscoveryIterator`, which yields devices as they respond rather than
waiting for the full timeout. Dropping it early releases the socket:

```rust
use sonos_discovery::{get_iter, DeviceEvent};

fn main() {
    for event in get_iter() {
        match event {
            DeviceEvent::Found(device) => {
                println!("Found: {}", device.name);
                // Break early if you only need the first device
                break;
            }
        }
    }
}
```

`get_iter_with_timeout(Duration)` is the same thing with an explicit timeout.

## Device Information

Each discovered `Device` includes:

- `id`: Unique device identifier (UDN), e.g. `uuid:RINCON_000E58A0123456`
- `name`: Friendly name
- `room_name`: Room where the device is located
- `ip_address`: IP address on the network
- `port`: Port number (typically 1400)
- `model_name`: Model name (e.g. "Sonos One")

`Device` is `Serialize`/`Deserialize`, so a discovery result can be cached to disk.

## How It Works

1. Sends SSDP M-SEARCH multicast request for Sonos ZonePlayer devices
2. Receives SSDP responses from devices on the network
3. Filters responses to identify likely Sonos devices
4. Fetches device description XML via HTTP
5. Parses and validates device information
6. Yields discovered devices as events

`DiscoveryError` covers socket, HTTP and XML-parsing failures. `get()` and `get_with_timeout()`
swallow per-device errors and return whatever was found; the iterator surfaces them.

## Example

```bash
cargo run -p sonos-sdk-discovery --example discover_json
```

## Testing

```bash
cargo test -p sonos-sdk-discovery
```

Tests are excluded from the published package. Fixture-backed tests parse XML captured from
real hardware — see [`tests/fixtures/README.md`](tests/fixtures/README.md).

## License

Licensed under either of [Apache License, Version 2.0](../LICENSE-APACHE) or
[MIT license](../LICENSE-MIT), at your option.
