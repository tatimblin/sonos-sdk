# sonos-discovery

A Rust library for discovering Sonos devices on your local network using SSDP (Simple Service Discovery Protocol).

## Features

- Simple API for one-time device discovery
- Iterator-based streaming for flexible processing
- Automatic deduplication of devices
- Filters out non-Sonos devices
- Configurable timeout
- Resource cleanup on early termination

## Usage

Add to your `Cargo.toml`:

```toml
[dependencies]
sonos-discovery = { path = "../sonos-discovery" }
```

### Quick Start

Discover all Sonos devices with default settings:

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

Specify a custom timeout for discovery:

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

Use the iterator API for more control:

```rust
use sonos_discovery::{get_iter, DeviceEvent};

fn main() {
    for event in get_iter() {
        match event {
            DeviceEvent::Found(device) => {
                println!("Found: {}", device.name);
                // Can break early if you only need the first device
                break;
            }
        }
    }
}
```

## Device Information

Each discovered device includes:

- `id`: Unique device identifier (UDN)
- `name`: Friendly name
- `room_name`: Room where the device is located
- `ip_address`: IP address on the network
- `port`: Port number (typically 1400)
- `model_name`: Model name (e.g., "Sonos One")

## How It Works

1. Sends SSDP M-SEARCH multicast request for Sonos ZonePlayer devices
2. Receives SSDP responses from devices on the network
3. Filters responses to identify likely Sonos devices
4. Fetches device description XML via HTTP
5. Parses and validates device information
6. Yields discovered devices as events

## License

This crate is part of a larger Sonos control project.
