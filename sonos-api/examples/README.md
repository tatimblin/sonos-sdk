# Sonos API CLI Example

This directory contains interactive examples demonstrating the functionality of the sonos-api crate.

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
   cargo run --example cli_example
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
✓ Operation registry loaded with 7 operations
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
  7. SetRelativeVolume - Adjust volume relatively

0. ← Return to device selection

💡 Tip: Select an operation to execute on Living Room
Enter your choice (0-7): 6

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

### Troubleshooting

#### No Devices Found
- Ensure Sonos speakers are powered on
- Check network connectivity (same WiFi network)
- Verify firewall settings allow network discovery
- Try the official Sonos app to confirm speakers are responsive

#### Operation Failures
- Check if the speaker is currently playing music from another source
- Ensure the speaker isn't grouped with other speakers in a way that prevents control
- Try a simpler operation like `GetTransportInfo` first
- Restart the speaker if issues persist

#### Network Issues
- Check your network connection
- Ensure no VPN is interfering with local network discovery
- Try running the example from a different network location
- Verify the speaker's IP address hasn't changed

### Code Structure

The CLI example demonstrates several key patterns:

#### Type-Safe Operations
```rust
// Each operation has strongly-typed request/response structures
let request = SetVolumeRequest {
    instance_id: 0,
    channel: "Master".to_string(),
    desired_volume: 50,
};

let response = client.execute::<SetVolumeOperation>(device_ip, &request).await?;
```

#### Dynamic Parameter Collection
```rust
// Parameters are collected dynamically based on operation metadata
let params = collect_parameters(operation)?;
let volume: u8 = params.get("volume").unwrap().parse()?;
```

#### Comprehensive Error Handling
```rust
match client.execute::<PlayOperation>(device_ip, &request).await {
    Ok(_) => println!("✓ Playback started"),
    Err(ApiError::NetworkError(msg)) => eprintln!("Network error: {}", msg),
    Err(ApiError::SoapFault(code)) => eprintln!("Device error: {}", code),
    Err(e) => eprintln!("Other error: {}", e),
}
```

### Integration with Other Crates

This example demonstrates integration with:

- **`sonos-discovery`**: For finding devices on the network
- **`sonos-api`**: For type-safe operation execution
- **`soap-client`**: (Used internally by sonos-api for SOAP communication)

### Next Steps

After exploring this example:

1. **Read the Source**: Check `cli_example.rs` to understand the implementation
2. **Explore the API**: Look at the `sonos-api` crate documentation
3. **Build Your Own**: Use these patterns in your own applications
4. **Add Operations**: Extend the example with additional Sonos operations
5. **Event Handling**: Explore the `sonos-stream` crate for real-time events

### Related Examples

- **Integration Example**: See `../../integration-example/` for a more comprehensive example with event handling
- **Stream Examples**: Check `../../sonos-stream/examples/` for event streaming examples