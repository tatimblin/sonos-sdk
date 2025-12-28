# Sonos SDK Examples

This directory contains examples demonstrating how to use the Sonos SDK with the new architecture where subscription management has been moved to `sonos-api`.

## discover_and_stream.rs

A complete example showing the integration of all three main crates:

- **sonos-discovery**: Find Sonos speakers on the network
- **sonos-api**: Create and manage UPnP subscriptions  
- **sonos-stream**: Process events from all speakers in a unified stream

### Usage

```bash
cargo run --example discover_and_stream -p sonos-stream
```

### What it does

1. **Sets up event streaming**: Creates an EventBroker with AVTransport strategy
2. **Discovers speakers**: Uses sonos-discovery to find all Sonos devices on the network
3. **Creates subscriptions**: Uses sonos-api's SonosClient to create managed subscriptions
4. **Processes events**: Listens for events from all speakers in a unified stream
5. **Handles lifecycle**: Manages subscription renewal and graceful shutdown

### Architecture

The example demonstrates the new separation of concerns:

```
┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐
│  sonos-discovery│    │   sonos-api     │    │  sonos-stream   │
│                 │    │                 │    │                 │
│ • Find speakers │───▶│ • Create subs   │───▶│ • Process events│
│ • Network scan  │    │ • Manage renewal│    │ • Callback server│
│ • Device info   │    │ • Device control│    │ • Event parsing │
└─────────────────┘    └─────────────────┘    └─────────────────┘
```

### Expected Output

When Sonos speakers are found:
```
🎵 Sonos SDK Example: Discover and Stream Events
================================================

📡 Setting up event streaming...
✅ Event broker ready, callback URL: http://192.168.1.100:3400

🔍 Discovering Sonos speakers...
✅ Found 2 Sonos speaker(s):
   📻 Living Room (Living Room) at 192.168.1.101
   📻 Kitchen (Kitchen) at 192.168.1.102

🔗 Creating subscriptions for AVTransport service...
✅ Subscribed to Living Room (192.168.1.101)
✅ Subscribed to Kitchen (192.168.1.102)
✅ Created 2 subscription(s)

🎧 Listening for events from all speakers...
💡 Try playing, pausing, or changing tracks on your Sonos speakers
⏹️  Press Ctrl+C to stop

🎵 [14:30:15.123] Living Room (AVTransport): Event type: transport_state_changed
🎵 [14:30:18.456] Kitchen (AVTransport): Event type: track_changed
```

When no speakers are found:
```
🔍 Discovering Sonos speakers...
❌ Discovery timed out after 10 seconds
💡 This might happen if no speakers are available or network issues
```

### Requirements

- Sonos speakers on the same network
- Network connectivity for UPnP discovery and subscriptions
- Available port in range 3400-3500 for callback server

### Troubleshooting

- **No speakers found**: Ensure Sonos speakers are powered on and connected to the same network
- **Subscription failures**: Check firewall settings and network connectivity
- **No events received**: Verify speakers are actively playing or changing state