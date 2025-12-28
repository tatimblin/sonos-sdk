# Sonos API Usage Guide

This guide demonstrates how to use the sonos-api crate through the interactive CLI example and provides patterns for building your own applications.

## Quick Start with CLI Example

The fastest way to explore the sonos-api functionality is through the interactive CLI example:

```bash
# Navigate to the sonos-api directory
cd sonos-api

# Run the interactive CLI example
cargo run --example cli_example
```

### Prerequisites

Before running the example, ensure:

1. **Sonos speakers are powered on** and connected to your network
2. **Same network**: Your computer is on the same WiFi network as your Sonos speakers
3. **Firewall settings**: Network discovery is allowed (check firewall/antivirus settings)
4. **Speakers are responsive**: Try the official Sonos app to verify speakers work

### What the CLI Example Demonstrates

The CLI example showcases all major sonos-api features:

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

**RenderingControl Service** (Volume Control):
- `GetVolume` - Get current volume level for a channel
- `SetVolume` - Set volume to specific level (0-100)
- `SetRelativeVolume` - Adjust volume by relative amount (-128 to +127)

#### ❌ Error Handling
- Network connectivity issues
- Device discovery timeouts  
- Invalid user input validation
- SOAP operation failures
- Parameter validation errors

## Building Your Own Applications

The CLI example demonstrates key patterns for building Sonos applications:

### 1. Basic Setup

```rust
use sonos_api::{SonosClient, operations::av_transport::*};
use sonos_discovery::{get_with_timeout, Device};
use std::time::Duration;

// Create a client
let client = SonosClient::new();

// Discover devices
let devices = get_with_timeout(Duration::from_secs(5));
let device = &devices[0]; // Use first device
```

### 2. Execute Operations

```rust
use sonos_api::operations::av_transport::{PlayOperation, PlayRequest};

// Create a request
let request = PlayRequest {
    instance_id: 0,
    speed: "1".to_string(),
};

// Execute the operation
match client.execute::<PlayOperation>(&device.ip_address, &request) {
    Ok(_) => println!("✓ Playback started"),
    Err(e) => eprintln!("Error: {}", e),
}
```

### 3. Handle Different Operation Types

```rust
use sonos_api::operations::{
    av_transport::{GetTransportInfoOperation, GetTransportInfoRequest},
    rendering_control::{SetVolumeOperation, SetVolumeRequest},
};

// Get transport info (no parameters)
let transport_request = GetTransportInfoRequest { instance_id: 0 };
let transport_response = client.execute::<GetTransportInfoOperation>(
    &device.ip_address, 
    &transport_request
)?;
println!("Current state: {:?}", transport_response.current_transport_state);

// Set volume (with parameters)
let volume_request = SetVolumeRequest {
    instance_id: 0,
    channel: "Master".to_string(),
    desired_volume: 75,
};
client.execute::<SetVolumeOperation>(&device.ip_address, &volume_request)?;
println!("Volume set to 75%");
```

### 4. Error Handling Patterns

```rust
use sonos_api::ApiError;

match client.execute::<PlayOperation>(&device.ip_address, &request) {
    Ok(response) => {
        // Handle successful response
        println!("Operation completed successfully");
    }
    Err(ApiError::NetworkError(msg)) => {
        eprintln!("Network error: {}", msg);
        // Maybe retry or switch to different device
    }
    Err(ApiError::SoapFault(code)) => {
        eprintln!("Device returned error code: {}", code);
        // Handle device-specific errors
    }
    Err(ApiError::ParseError(msg)) => {
        eprintln!("Failed to parse response: {}", msg);
        // Handle malformed responses
    }
    Err(e) => {
        eprintln!("Other error: {}", e);
    }
}
```

### 5. Parameter Validation

```rust
fn validate_volume(volume: u8) -> Result<(), String> {
    if volume > 100 {
        return Err(format!("Volume must be 0-100, got {}", volume));
    }
    Ok(())
}

fn set_volume_safely(client: &SonosClient, device: &Device, volume: u8) -> Result<(), Box<dyn std::error::Error>> {
    // Validate before sending
    validate_volume(volume)?;
    
    let request = SetVolumeRequest {
        instance_id: 0,
        channel: "Master".to_string(),
        desired_volume: volume,
    };
    
    client.execute::<SetVolumeOperation>(&device.ip_address, &request)?;
    Ok(())
}
```

## Advanced Usage Patterns

### 1. Multiple Device Control

```rust
use std::collections::HashMap;

// Control multiple devices
let devices = get_with_timeout(Duration::from_secs(5));
let mut results = HashMap::new();

for device in &devices {
    let request = GetVolumeRequest {
        instance_id: 0,
        channel: "Master".to_string(),
    };
    
    match client.execute::<GetVolumeOperation>(&device.ip_address, &request) {
        Ok(response) => {
            results.insert(&device.name, response.current_volume);
        }
        Err(e) => {
            eprintln!("Failed to get volume for {}: {}", device.name, e);
        }
    }
}

// Print all volumes
for (name, volume) in results {
    println!("{}: {}%", name, volume);
}
```

### 2. Operation Batching

```rust
// Execute multiple operations in sequence
fn control_playback_sequence(
    client: &SonosClient, 
    device: &Device
) -> Result<(), Box<dyn std::error::Error>> {
    
    // 1. Get current state
    let info_request = GetTransportInfoRequest { instance_id: 0 };
    let info = client.execute::<GetTransportInfoOperation>(&device.ip_address, &info_request)?;
    println!("Current state: {:?}", info.current_transport_state);
    
    // 2. Set volume to reasonable level
    let volume_request = SetVolumeRequest {
        instance_id: 0,
        channel: "Master".to_string(),
        desired_volume: 30,
    };
    client.execute::<SetVolumeOperation>(&device.ip_address, &volume_request)?;
    println!("Volume set to 30%");
    
    // 3. Start playback
    let play_request = PlayRequest {
        instance_id: 0,
        speed: "1".to_string(),
    };
    client.execute::<PlayOperation>(&device.ip_address, &play_request)?;
    println!("Playback started");
    
    Ok(())
}
```

### 3. Retry Logic

```rust
use std::thread;
use std::time::Duration;

fn execute_with_retry<Op: SonosOperation>(
    client: &SonosClient,
    device_ip: &str,
    request: &Op::Request,
    max_retries: u32,
) -> Result<Op::Response, ApiError> {
    let mut last_error = None;
    
    for attempt in 1..=max_retries {
        match client.execute::<Op>(device_ip, request) {
            Ok(response) => return Ok(response),
            Err(ApiError::NetworkError(_)) if attempt < max_retries => {
                println!("Network error on attempt {}, retrying...", attempt);
                thread::sleep(Duration::from_millis(1000 * attempt as u64));
                last_error = Some(ApiError::NetworkError("Network error".to_string()));
            }
            Err(e) => return Err(e),
        }
    }
    
    Err(last_error.unwrap())
}
```

## Integration with Other Crates

### With sonos-discovery

```rust
use sonos_discovery::{get_with_timeout, Device, DiscoveryError};
use std::time::Duration;

fn find_device_by_name(name: &str) -> Result<Device, Box<dyn std::error::Error>> {
    let devices = get_with_timeout(Duration::from_secs(10));
    
    devices.into_iter()
        .find(|d| d.name.contains(name) || d.room_name.contains(name))
        .ok_or_else(|| format!("Device '{}' not found", name).into())
}

// Usage
let kitchen_speaker = find_device_by_name("Kitchen")?;
```

### With sonos-stream (Event Handling)

```rust
// This would typically be in a separate application using sonos-stream
// The sonos-api crate focuses on stateless operations
// For event handling, see the sonos-stream crate examples
```

## Common Patterns and Best Practices

### 1. Device Selection UI

```rust
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
    
    registry.insert("play".to_string(), OperationInfo {
        name: "Play".to_string(),
        description: "Start playback".to_string(),
        service: "AVTransport".to_string(),
    });
    
    registry.insert("pause".to_string(), OperationInfo {
        name: "Pause".to_string(),
        description: "Pause playback".to_string(),
        service: "AVTransport".to_string(),
    });
    
    // ... more operations
    
    registry
}
```

### 3. Configuration Management

```rust
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
- **Parameter validation**: Check parameter values and types
- **Speaker capabilities**: Not all speakers support all operations

#### "Parse error" responses
- **Speaker firmware**: Ensure speakers have recent firmware
- **Network corruption**: Check for network packet corruption
- **Response format**: Some speakers may return non-standard responses

### Debug Mode

Enable debug logging to troubleshoot issues:

```rust
// This would depend on the logging setup in your application
env_logger::init(); // If using env_logger

// Set RUST_LOG=debug when running
// RUST_LOG=debug cargo run --example cli_example
```

## Performance Considerations

### 1. Connection Reuse
The SonosClient reuses HTTP connections internally for better performance.

### 2. Concurrent Operations
```rust
use std::thread;

// Execute operations on multiple devices concurrently
let handles: Vec<_> = devices.iter().map(|device| {
    let client = client.clone(); // SonosClient is Clone
    let device_ip = device.ip_address.clone();
    
    thread::spawn(move || {
        let request = GetVolumeRequest {
            instance_id: 0,
            channel: "Master".to_string(),
        };
        client.execute::<GetVolumeOperation>(&device_ip, &request)
    })
}).collect();

// Collect results
for handle in handles {
    match handle.join().unwrap() {
        Ok(response) => println!("Volume: {}", response.current_volume),
        Err(e) => eprintln!("Error: {}", e),
    }
}
```

### 3. Caching Device Information
```rust
use std::collections::HashMap;
use std::time::{Instant, Duration};

struct DeviceCache {
    devices: HashMap<String, Device>,
    last_discovery: Instant,
    cache_duration: Duration,
}

impl DeviceCache {
    fn get_devices(&mut self) -> Result<&HashMap<String, Device>, DiscoveryError> {
        if self.last_discovery.elapsed() > self.cache_duration {
            self.refresh()?;
        }
        Ok(&self.devices)
    }
    
    fn refresh(&mut self) -> Result<(), DiscoveryError> {
        let discovered = get_with_timeout(Duration::from_secs(5));
        self.devices.clear();
        for device in discovered {
            self.devices.insert(device.ip_address.clone(), device);
        }
        self.last_discovery = Instant::now();
        Ok(())
    }
}
```

## Next Steps

1. **Explore the CLI Example**: Run `cargo run --example cli_example` to see all features
2. **Read the API Documentation**: Use `cargo doc --open` to browse the full API
3. **Check Integration Example**: See `../../integration-example/` for more advanced usage
4. **Event Handling**: Explore `../../sonos-stream/` for real-time event subscriptions
5. **Build Your App**: Use these patterns to build your own Sonos applications

## Related Documentation

- [sonos-api README](README.md) - Crate overview and basic usage
- [CLI Example README](examples/README.md) - Detailed CLI example documentation  
- [Integration Example](../../integration-example/README.md) - Advanced integration patterns
- [sonos-stream Examples](../../sonos-stream/examples/README.md) - Event handling examples