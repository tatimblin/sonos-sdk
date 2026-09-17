# Sonos API Usage Guide

This guide demonstrates how to use the sonos-api crate through the interactive CLI example and provides patterns for building your own applications.

## Quick Start with CLI Example

The fastest way to explore the sonos-api functionality is through the interactive CLI example:

```bash
cargo run -p sonos-api --example cli_example
```

### Prerequisites

Before running the example, ensure:

1. **Sonos speakers are powered on** and connected to your network
2. **Same network**: Your computer is on the same WiFi network as your Sonos speakers
3. **Firewall settings**: Network discovery is allowed (check firewall/antivirus settings)
4. **Speakers are responsive**: Try the official Sonos app to verify speakers work

### What the CLI Example Demonstrates

The CLI example showcases the crate's core workflow:

#### 🔍 Device Discovery
- Automatic network scanning for Sonos devices
- Timeout handling and retry logic
- Clear error messages for common issues

#### 📱 Interactive Operation Selection
- Grouped operations by UPnP service (AVTransport, RenderingControl)
- Dynamic parameter collection with validation
- Type-safe operation execution

#### 🎛️ Supported Operations

**AVTransport Service** (Playback Control):
- `Play` - Start playback with optional speed parameter
- `Pause` - Pause current playback
- `Stop` - Stop current playback
- `GetTransportInfo` - Get current playback state and information

**RenderingControl Service** (Audio Control):
- `GetVolume` / `SetVolume` - Read or set volume (0-100)
- `SetRelativeVolume` - Adjust volume by a relative amount (-128 to +127)
- `GetMute` / `SetMute` - Read or set mute state
- `GetBass` / `SetBass` - Read or set bass level (-10 to +10)
- `GetTreble` / `SetTreble` - Read or set treble level (-10 to +10)
- `GetLoudness` / `SetLoudness` - Read or set loudness compensation

#### ❌ Error Handling
- Network connectivity issues
- Device discovery timeouts
- Invalid user input validation
- SOAP operation failures
- Parameter validation errors

### Other Examples

| Example | Command | Purpose |
|---------|---------|---------|
| `cli_example` | `cargo run -p sonos-api --example cli_example` | Interactive control of a discovered speaker |
| `managed_subscription_example` | `cargo run -p sonos-api --example managed_subscription_example` | `ManagedSubscription` lifecycle: renew, expiry, unsubscribe |
| `validate_rendering_control` | `cargo run -p sonos-api --example validate_rendering_control` | Round-trips every RenderingControl operation against a real speaker |
| `test_operation` | `cargo run -p sonos-api --example test_operation -- <ip> <service> <action> [Param=value...]` | Sends an arbitrary raw SOAP action, for probing undocumented behavior |

## Building Your Own Applications

### 1. Basic Setup

```rust
use sonos_api::SonosClient;
use sonos_discovery::get_with_timeout;
use std::time::Duration;

// Create a client
let client = SonosClient::new();

// Discover devices
let devices = get_with_timeout(Duration::from_secs(5));
let device = &devices[0]; // Use first device
```

### 2. Execute Operations

Each operation has a snake_case constructor that returns an `OperationBuilder`. `build()`
validates the request; `execute_enhanced` sends it:

```rust
use sonos_api::services::av_transport;
use sonos_api::SonosClient;

let client = SonosClient::new();

let operation = av_transport::play_operation("1".to_string()).build()?;

match client.execute_enhanced("192.168.1.100", operation) {
    Ok(_) => println!("✓ Playback started"),
    Err(e) => eprintln!("Error: {e}"),
}
```

### 3. Handle Different Operation Types

```rust
use sonos_api::services::{av_transport, rendering_control};
use sonos_api::SonosClient;

let client = SonosClient::new();
let ip = "192.168.1.100";

// Get transport info (no parameters)
let operation = av_transport::get_transport_info_operation().build()?;
let transport = client.execute_enhanced(ip, operation)?;
println!("Current state: {}", transport.current_transport_state);

// Set volume (with parameters)
let operation = rendering_control::set_volume_operation("Master".to_string(), 75).build()?;
client.execute_enhanced(ip, operation)?;
println!("Volume set to 75%");
```

### 4. Error Handling Patterns

```rust
use sonos_api::services::av_transport;
use sonos_api::{ApiError, SonosClient};

let client = SonosClient::new();
let operation = av_transport::play_operation("1".to_string()).build()?;

match client.execute_enhanced("192.168.1.100", operation) {
    Ok(_response) => {
        println!("Operation completed successfully");
    }
    Err(ApiError::NetworkError(msg)) => {
        eprintln!("Network error: {msg}");
        // Maybe retry or switch to a different device
    }
    Err(ApiError::SoapFault(code)) => {
        eprintln!("Device returned error code: {code}");
        // Handle device-specific errors
    }
    Err(ApiError::ParseError(msg)) => {
        eprintln!("Failed to parse response: {msg}");
        // Handle malformed responses
    }
    Err(e) => {
        eprintln!("Other error: {e}");
    }
}
```

### 5. Parameter Validation

Range and format checks live on the request type's `Validate` implementation and run inside
`build()`, so an out-of-range value never reaches the network:

```rust
use sonos_api::services::rendering_control;

// Volume above 100 is rejected before any SOAP call is made.
let too_loud = rendering_control::set_volume_operation("Master".to_string(), 150).build();
assert!(too_loud.is_err());

let ok = rendering_control::set_volume_operation("Master".to_string(), 75).build();
assert!(ok.is_ok());
```

The same applies to channel names, bass and treble range (-10 to +10), and relative volume
adjustments (-128 to +127).

## Advanced Usage Patterns

### 1. Multiple Device Control

```rust
use sonos_api::services::rendering_control;
use sonos_api::SonosClient;
use sonos_discovery::get_with_timeout;
use std::collections::HashMap;
use std::time::Duration;

let client = SonosClient::new();
let devices = get_with_timeout(Duration::from_secs(5));
let mut results = HashMap::new();

for device in &devices {
    let operation = rendering_control::get_volume_operation("Master".to_string()).build()?;

    match client.execute_enhanced(&device.ip_address, operation) {
        Ok(response) => {
            results.insert(device.name.clone(), response.current_volume);
        }
        Err(e) => {
            eprintln!("Failed to get volume for {}: {e}", device.name);
        }
    }
}

for (name, volume) in results {
    println!("{name}: {volume}%");
}
```

### 2. Operation Sequences

Operations execute one at a time. Sequence them by executing in order and propagating errors:

```rust
use sonos_api::services::{av_transport, rendering_control};
use sonos_api::SonosClient;
use sonos_discovery::Device;

fn control_playback_sequence(
    client: &SonosClient,
    device: &Device,
) -> Result<(), Box<dyn std::error::Error>> {
    // 1. Get current state
    let operation = av_transport::get_transport_info_operation().build()?;
    let info = client.execute_enhanced(&device.ip_address, operation)?;
    println!("Current state: {}", info.current_transport_state);

    // 2. Set volume to a reasonable level
    let operation = rendering_control::set_volume_operation("Master".to_string(), 30).build()?;
    client.execute_enhanced(&device.ip_address, operation)?;
    println!("Volume set to 30%");

    // 3. Start playback
    let operation = av_transport::play_operation("1".to_string()).build()?;
    client.execute_enhanced(&device.ip_address, operation)?;
    println!("Playback started");

    Ok(())
}
```

### 3. Retry Logic

A `ComposableOperation` is consumed by `execute_enhanced`, so a retry loop rebuilds the
operation on each attempt:

```rust
use sonos_api::services::rendering_control;
use sonos_api::{ApiError, SonosClient};
use std::thread;
use std::time::Duration;

fn get_volume_with_retry(
    client: &SonosClient,
    device_ip: &str,
    max_retries: u32,
) -> Result<u8, Box<dyn std::error::Error>> {
    let mut last_error = None;

    for attempt in 1..=max_retries {
        let operation = rendering_control::get_volume_operation("Master".to_string()).build()?;

        match client.execute_enhanced(device_ip, operation) {
            Ok(response) => return Ok(response.current_volume),
            Err(ApiError::NetworkError(msg)) if attempt < max_retries => {
                println!("Network error on attempt {attempt}, retrying...");
                thread::sleep(Duration::from_millis(1000 * u64::from(attempt)));
                last_error = Some(ApiError::NetworkError(msg));
            }
            Err(e) => return Err(e.into()),
        }
    }

    Err(last_error.expect("loop ran at least once").into())
}
```

### 4. Event Subscriptions

Control operations and event subscriptions are independent. A subscription points a service's
`/Event` endpoint at a callback URL you host, and the returned `ManagedSubscription` tracks
expiry:

```rust
use sonos_api::{Service, SonosClient};

let client = SonosClient::new();

let subscription = client.subscribe(
    "192.168.1.100",
    Service::RenderingControl,
    "http://192.168.1.50:8080/callback",
)?;

println!("SID: {}", subscription.subscription_id());
println!("Expires at: {:?}", subscription.expires_at());

if subscription.needs_renewal() {
    subscription.renew()?;
}

subscription.unsubscribe()?;
```

Receiving the resulting NOTIFY bodies means running an HTTP server at that callback URL. If you
want that handled for you, along with polling fallback when a firewall blocks callbacks, use
[`sonos-sdk`](https://crates.io/crates/sonos-sdk) rather than building it on this crate.

## Integration with Other Crates

### With sonos-discovery

```rust
use sonos_discovery::{get_with_timeout, Device};
use std::time::Duration;

fn find_device_by_name(name: &str) -> Result<Device, Box<dyn std::error::Error>> {
    let devices = get_with_timeout(Duration::from_secs(10));

    devices
        .into_iter()
        .find(|d| d.name.contains(name) || d.room_name.contains(name))
        .ok_or_else(|| format!("Device '{name}' not found").into())
}
```

### With sonos-sdk

`sonos-api` is stateless and speaks to one speaker at a time. For device discovery, speaker
grouping, cached properties and a blocking iterator over live property changes, use
[`sonos-sdk`](../sonos-sdk/README.md), which builds all of that on top of this crate.

## Common Patterns and Best Practices

### 1. Device Selection UI

```rust
use sonos_discovery::Device;

fn select_device_interactive(devices: &[Device]) -> Result<&Device, Box<dyn std::error::Error>> {
    if devices.is_empty() {
        return Err("No devices found".into());
    }

    println!("Available devices:");
    for (i, device) in devices.iter().enumerate() {
        println!("{}. {} ({})", i + 1, device.name, device.room_name);
    }

    print!("Select device (1-{}): ", devices.len());
    // ... input handling logic

    Ok(&devices[0]) // Simplified
}
```

### 2. Operation Registry Pattern

```rust
use std::collections::HashMap;

struct OperationInfo {
    name: String,
    description: String,
    service: String,
}

fn build_operation_registry() -> HashMap<String, OperationInfo> {
    let mut registry = HashMap::new();

    registry.insert(
        "play".to_string(),
        OperationInfo {
            name: "Play".to_string(),
            description: "Start playback".to_string(),
            service: "AVTransport".to_string(),
        },
    );

    registry.insert(
        "pause".to_string(),
        OperationInfo {
            name: "Pause".to_string(),
            description: "Pause playback".to_string(),
            service: "AVTransport".to_string(),
        },
    );

    registry
}
```

### 3. Configuration Management

```rust
use std::time::Duration;

#[derive(Debug)]
struct SonosConfig {
    default_volume: u8,
    discovery_timeout: Duration,
    operation_timeout: Duration,
}

impl Default for SonosConfig {
    fn default() -> Self {
        Self {
            default_volume: 30,
            discovery_timeout: Duration::from_secs(5),
            operation_timeout: Duration::from_secs(10),
        }
    }
}
```

## Troubleshooting

### Common Issues and Solutions

#### "No devices found"
- **Check network**: Ensure computer and speakers are on same WiFi
- **Firewall**: Allow network discovery in firewall settings
- **Speaker status**: Verify speakers are powered on and responsive
- **Sonos app**: Test with official Sonos app first

#### "Network timeout" errors
- **Network stability**: Check WiFi connection quality
- **Speaker load**: Speakers might be busy with other operations
- **Retry logic**: Implement retry with exponential backoff

#### "SOAP fault" errors
- **Operation state**: Some operations only work in certain states
- **Coordinator-only actions**: AVTransport, GroupRenderingControl and GroupManagement actions must be sent to the group coordinator
- **Speaker capabilities**: Not all speakers support all operations

#### "Parse error" responses
- **Speaker firmware**: Ensure speakers have recent firmware
- **Network corruption**: Check for network packet corruption
- **Response format**: Some speakers may return non-standard responses

### Debug Logging

The crate emits `tracing` spans and events. Install a subscriber and set `RUST_LOG`:

```bash
RUST_LOG=sonos_api=debug,soap_client=debug cargo run -p sonos-api --example cli_example
```

To probe a single action without writing code, `test_operation` sends a raw SOAP body and
prints the response:

```bash
cargo run -p sonos-api --example test_operation -- 192.168.1.100 RenderingControl GetVolume Channel=Master
```

## Performance Considerations

### 1. Connection Reuse
`SonosClient::new()` takes a handle to a process-wide shared SOAP client, so every client
instance reuses the same HTTP connection pool.

### 2. Concurrent Operations

`SonosClient` is `Clone`, and cloning is cheap because the underlying transport is shared:

```rust
use sonos_api::services::rendering_control;
use sonos_api::SonosClient;
use sonos_discovery::get;
use std::thread;

let client = SonosClient::new();
let devices = get();

let handles: Vec<_> = devices
    .iter()
    .map(|device| {
        let client = client.clone();
        let device_ip = device.ip_address.clone();

        thread::spawn(move || {
            let operation = rendering_control::get_volume_operation("Master".to_string())
                .build()
                .expect("volume request is valid");
            client.execute_enhanced(&device_ip, operation)
        })
    })
    .collect();

for handle in handles {
    match handle.join().expect("worker thread panicked") {
        Ok(response) => println!("Volume: {}", response.current_volume),
        Err(e) => eprintln!("Error: {e}"),
    }
}
```

### 3. Caching Device Information

```rust
use sonos_discovery::{get_with_timeout, Device};
use std::collections::HashMap;
use std::time::{Duration, Instant};

struct DeviceCache {
    devices: HashMap<String, Device>,
    last_discovery: Instant,
    cache_duration: Duration,
}

impl DeviceCache {
    fn get_devices(&mut self) -> &HashMap<String, Device> {
        if self.last_discovery.elapsed() > self.cache_duration {
            self.refresh();
        }
        &self.devices
    }

    fn refresh(&mut self) {
        let discovered = get_with_timeout(Duration::from_secs(5));
        self.devices.clear();
        for device in discovered {
            self.devices.insert(device.ip_address.clone(), device);
        }
        self.last_discovery = Instant::now();
    }
}
```

## Next Steps

1. **Explore the CLI Example**: Run `cargo run -p sonos-api --example cli_example` to see all features
2. **Read the API Documentation**: Use `cargo doc -p sonos-api --open` to browse the full API
3. **Event Handling**: For live property updates without hosting your own callback server, use [`sonos-sdk`](../sonos-sdk/README.md)
4. **Build Your App**: Use these patterns to build your own Sonos applications

## Related Documentation

- [sonos-api README](README.md) - Crate overview and basic usage
- [Examples README](examples/README.md) - What each example does and how to run it
- [Services README](src/services/README.md) - Service layout and how to add a new UPnP service
