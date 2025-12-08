# sonos-stream Examples

## simple_event_consumer

A minimal, working example demonstrating how to consume events from sonos-stream.

### What it does

- Creates an event broker with a simple mock strategy
- Subscribes to a mock speaker
- **Simulates receiving events** by sending HTTP POST requests to the callback server
- Parses and prints events to the terminal
- Demonstrates the complete event consumption pattern

### Running the example

```bash
cargo run --example simple_event_consumer
```

### Example output

```
Starting simple event consumer example...

Subscribing to speaker: Living Room
Listening for events...

✓ Subscription established
  ID: sub-RINCON_EXAMPLE123

📤 Simulating event: Playing music
→ Event received:
  Speaker: RINCON_EXAMPLE123
  Service: AVTransport
  Type: playing
  Data: {"speaker": "RINCON_EXAMPLE123", "state": "playing"}

📤 Simulating event: Paused
→ Event received:
  Speaker: RINCON_EXAMPLE123
  Service: AVTransport
  Type: paused
  Data: {"state": "paused", "speaker": "RINCON_EXAMPLE123"}

📤 Simulating event: Resumed playing
→ Event received:
  Speaker: RINCON_EXAMPLE123
  Service: AVTransport
  Type: playing
  Data: {"speaker": "RINCON_EXAMPLE123", "state": "playing"}

Received 3 events, exiting...

Demo complete! Cleaning up...
✓ Unsubscribed successfully
✓ Broker shut down

Goodbye!
```

### Key concepts

1. **Create a broker** with a strategy that handles subscription and event parsing
2. **Subscribe** to speakers and services you want to monitor
3. **Get the event stream** and process events in a loop
4. **Handle different event types** (subscription lifecycle, service events, errors)
5. **Unsubscribe and shutdown** when done

### How event simulation works

The example spawns a background task that sends HTTP POST requests to the callback server:

```rust
// POST to http://127.0.0.1:3400/notify/{subscription_id}
// Headers: SID, NT, NTS (UPnP event headers)
// Body: XML event data
```

This mimics how real Sonos speakers send event notifications to your callback server.

### Event types you'll see

- `SubscriptionEstablished` - Subscription was created successfully
- `ServiceEvent` - An event from the speaker (e.g., playback state change)
- `SubscriptionRemoved` - Subscription was removed
- `SubscriptionFailed` - Subscription creation failed
- `SubscriptionRenewed` - Subscription was automatically renewed
- `SubscriptionExpired` - Subscription expired after renewal failures
- `ParseError` - Failed to parse an event

### Adapting for real use

To use with real Sonos speakers, you'll need to:

1. Implement a proper `SubscriptionStrategy` that:
   - Makes real UPnP SUBSCRIBE requests to speaker endpoints
   - Parses actual UPnP event XML from speakers
   
2. Use real speaker information from `sonos-discovery`

3. Keep the event loop running to continuously process events

The event consumption pattern shown here remains exactly the same!
