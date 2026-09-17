# Sonos API Examples

Four examples ship with `sonos-api`. All are synchronous and talk to real hardware on the local network.

| Example | Purpose |
|---------|---------|
| [`cli_example`](cli_example.rs) | Interactive menu for discovering a speaker and running operations against it |
| [`validate_rendering_control`](validate_rendering_control.rs) | Round-trips every RenderingControl operation against the first discovered speaker |
| [`managed_subscription_example`](managed_subscription_example.rs) | `ManagedSubscription` lifecycle: create, inspect expiry, renew, unsubscribe |
| [`test_operation`](test_operation.rs) | Sends an arbitrary raw SOAP action, for probing behavior with no operation defined yet |

## CLI Example (`cli_example.rs`)

An interactive command-line interface that demonstrates device discovery, operation selection, and execution.

### Features

- **🔍 Device Discovery**: Automatically finds Sonos speakers on your network
- **📱 Interactive Menus**: Simple numbered menus for device and operation selection
- **🎛️ Operation Execution**: Execute AVTransport and RenderingControl operations
- **📝 Parameter Collection**: Dynamic parameter collection with validation
- **❌ Error Handling**: Comprehensive error handling with user-friendly messages
- **🔄 Graceful Recovery**: Continue operation even when individual commands fail

### Quick Start

1. **Ensure Prerequisites**:
   - Sonos speakers are powered on and connected to your network
   - Your computer is on the same network as the speakers
   - Firewall allows network discovery

2. **Run the Example**:
   ```bash
   cargo run -p sonos-api --example cli_example
   ```

3. **Follow the Interactive Prompts**:
   - The CLI will discover devices automatically
   - Select a device from the numbered list
   - Choose an operation to execute
   - Provide any required parameters
   - View the results

### Supported Operations

#### AVTransport Service
| Operation | Description | Parameters |
|-----------|-------------|------------|
| `Play` | Start playback | `speed` (optional, default: "1") |
| `Pause` | Pause current playback | None |
| `Stop` | Stop current playback | None |
| `GetTransportInfo` | Get current playback state | None |

#### RenderingControl Service
| Operation | Description | Parameters |
|-----------|-------------|------------|
| `GetVolume` | Get current volume level | `channel` (optional, default: "Master") |
| `SetVolume` | Set volume to specific level | `volume` (required: 0-100), `channel` (optional) |
| `SetRelativeVolume` | Adjust volume relatively | `adjustment` (required: -128 to +127), `channel` (optional) |
| `GetMute` | Get mute state | `channel` (optional) |
| `SetMute` | Set mute state | `mute` (required: bool), `channel` (optional) |
| `GetBass` | Get bass level | None |
| `SetBass` | Set bass level (-10 to +10) | `bass` (required: i8) |
| `GetTreble` | Get treble level | None |
| `SetTreble` | Set treble level (-10 to +10) | `treble` (required: i8) |
| `GetLoudness` | Get loudness compensation state | `channel` (optional) |
| `SetLoudness` | Set loudness compensation | `loudness` (required: bool), `channel` (optional) |

### Example Session

```
🎵 Sonos API CLI Example
========================

This interactive CLI demonstrates the sonos-api crate functionality.
You can discover Sonos devices and execute various control operations.

📋 What you can do:
   • Discover Sonos speakers on your network
   • Control playback (play, pause, stop)
   • Adjust volume settings
   • Get device status information

🔧 Requirements:
   • Sonos speakers must be powered on
   • Connected to the same network as this computer
   • Network discovery allowed (check firewall)

✓ Sonos API client initialized
✓ Signal handling configured (Ctrl+C to exit)

🔍 Discovering Sonos devices... (attempt 1/3)
✓ Found 2 Sonos device(s)

Discovered Sonos Devices:
=========================
1. Living Room (Living Room)
   IP: 192.168.1.100 | Model: Sonos One

2. Kitchen (Kitchen)
   IP: 192.168.1.101 | Model: Sonos Play:1

🎵 Ready to control your Sonos speakers!
   Use Ctrl+C at any time to exit gracefully

📱 Select a Sonos Device to Control
===================================
1. Living Room (Living Room)
   📍 192.168.1.100 | 🔧 Sonos One

2. Kitchen (Kitchen)
   📍 192.168.1.101 | 🔧 Sonos Play:1

0. Exit application

💡 Tip: Choose the device you want to control
Enter your choice (0-2): 1

✓ Selected device: Living Room (Living Room)
  IP Address: 192.168.1.100
  Model: Sonos One

🎛️  Available Operations for Living Room (Living Room)
============================================================

📂 AVTransport:
  1. Play - Start playback
  2. Pause - Pause playback
  3. Stop - Stop playback
  4. GetTransportInfo - Get current playback state

📂 RenderingControl:
  5. GetVolume - Get current volume
  6. SetVolume - Set volume level
  ...

0. ← Return to device selection

💡 Tip: Select an operation to execute on Living Room
Enter your choice: 6

🚀 Executing: SetVolume - Set volume level
   Target device: Living Room (Living Room)

📝 Parameter Collection for: SetVolume
==================================================
Please provide the following parameters:

Parameter 1 of 2:
  📋 Parameter: volume
     Type: u8
     Range: 0-255 (e.g., volume: 0-100)
     ⚠️  Required parameter
     Enter volume (u8): 50
     ✓ Valid u8 value: 50

Parameter 2 of 2:
  🔧 Optional Parameter: channel
     Type: String (default: Master)
     Provide custom value? (y/n): n
     → Will use default value: Master

✓ All parameters collected successfully!

⚡ Executing operation...
   Operation: SetVolume
   Service: RenderingControl
   Target: Living Room (192.168.1.100)
   Parameters:
     volume: 50
     channel: Master

✅ Operation Completed Successfully!
===================================

✓ Volume set to 50 on Living Room (Master)

Press Enter to continue...
```

### Error Handling Examples

The CLI handles various error conditions gracefully:

#### No Devices Found
```
❌ Device Discovery Failed
=========================

No Sonos devices were found on your network.

💡 Troubleshooting tips:
   1. Ensure your Sonos speakers are powered on
   2. Check that you're on the same WiFi network as your speakers
   3. Verify your firewall allows network discovery
   4. Try opening the Sonos app to ensure speakers are responsive
   5. Wait a moment and try running the example again
```

#### Invalid Parameter
```
❌ Operation Failed
==================

Parameter Error: Volume must be between 0-100, got 150

💡 Please check your parameter values and try again.

Press Enter to continue...
```

#### Network Error
```
❌ Operation Failed
==================

SOAP API Error: Network timeout after 30 seconds

💡 This might be because:
   • The device is busy with another operation
   • The requested operation is not supported in current state
   • Network connectivity issues
   • The device needs to be restarted

Press Enter to continue...
```

### Code Structure

The CLI example demonstrates the crate's core patterns.

#### Type-Safe Operations

Every operation has a generated request struct and is executed through `OperationBuilder`:

```rust
use sonos_api::services::rendering_control::{SetVolumeOperation, SetVolumeOperationRequest};
use sonos_api::{OperationBuilder, SonosClient};

let client = SonosClient::new();

let request = SetVolumeOperationRequest {
    instance_id: 0,
    channel: "Master".to_string(),
    desired_volume: 50,
};

let operation = OperationBuilder::<SetVolumeOperation>::new(request).build()?;
client.execute_enhanced("192.168.1.100", operation)?;
```

#### Dynamic Parameter Collection

Parameters are collected from an `OperationInfo` registry the example builds itself, then
parsed into the typed request fields:

```rust,ignore
let params = collect_parameters(operation)?;
let volume: u8 = params.get("volume").unwrap().parse()?;
```

#### Comprehensive Error Handling

```rust
use sonos_api::services::av_transport;
use sonos_api::{ApiError, SonosClient};

let client = SonosClient::new();
let operation = av_transport::play_operation("1".to_string()).build()?;

match client.execute_enhanced("192.168.1.100", operation) {
    Ok(_) => println!("✓ Playback started"),
    Err(ApiError::NetworkError(msg)) => eprintln!("Network error: {msg}"),
    Err(ApiError::SoapFault(code)) => eprintln!("Device error: {code}"),
    Err(e) => eprintln!("Other error: {e}"),
}
```

## `validate_rendering_control.rs`

Discovers speakers, picks the first one, and exercises every RenderingControl operation —
GetVolume, GetMute, GetBass, GetTreble, GetLoudness, then a write/read/restore round trip for
each setter. Useful after changing operation definitions or XML parsing.

```bash
cargo run -p sonos-api --example validate_rendering_control
```

## `managed_subscription_example.rs`

Creates a `ManagedSubscription` against a hard-coded device IP and callback URL, then shows the
lifecycle methods: `subscription_id()`, `expires_at()`, `needs_renewal()`, `renew()` and
`unsubscribe()`. Edit the IP at the top before running.

```bash
cargo run -p sonos-api --example managed_subscription_example
```

## `test_operation.rs`

Sends a raw SOAP body for any service and action, with arbitrary parameters, and prints the
response. This bypasses the typed operation layer entirely, which is what makes it useful for
probing actions that have no operation defined yet.

```bash
cargo run -p sonos-api --example test_operation -- 192.168.1.100 AVTransport GetTransportInfo
cargo run -p sonos-api --example test_operation -- 192.168.1.100 RenderingControl GetVolume Channel=Master
cargo run -p sonos-api --example test_operation -- 192.168.1.100 AVTransport Play Speed=1
```

`InstanceID=0` is supplied by default; any `Name=value` argument overrides or adds to it.

## Troubleshooting

#### No Devices Found
- Ensure Sonos speakers are powered on
- Check network connectivity (same WiFi network)
- Verify firewall settings allow network discovery
- Try the official Sonos app to confirm speakers are responsive

#### Operation Failures
- Check if the speaker is currently playing music from another source
- AVTransport, GroupRenderingControl and GroupManagement actions must target the group coordinator
- Try a simpler operation like `GetTransportInfo` first
- Restart the speaker if issues persist

#### Network Issues
- Check your network connection
- Ensure no VPN is interfering with local network discovery
- Try running the example from a different network location
- Verify the speaker's IP address hasn't changed

## Related Documentation

- [sonos-api README](../README.md) - Crate overview and basic usage
- [Usage Guide](../USAGE.md) - Patterns for building your own applications
- [Services README](../src/services/README.md) - Service layout and how to add a new UPnP service
- [sonos-stream examples](../../sonos-stream/examples/) - Event streaming examples
