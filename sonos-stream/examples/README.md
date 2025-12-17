# sonos-stream Examples

This directory contains examples demonstrating how to use the sonos-stream crate with the new TypedEvent system for type-safe event handling.

## Examples

### 1. simple_event_consumer.rs

**Purpose**: Basic event consumption with type-safe downcasting

**What it demonstrates**:
- Creating a broker with a mock strategy
- Subscribing to a speaker for AVTransport events
- Receiving and processing events using TypedEvent
- Type-safe downcasting to AVTransportEvent
- Accessing strongly-typed event data fields
- Proper cleanup and shutdown

**Key concepts**:
```rust
// Type-safe downcasting
if let Some(av_event) = event.downcast_ref::<AVTransportEvent>() {
    println!("Transport State: {}", av_event.transport_state);
    if let Some(track_uri) = &av_event.track_uri {
        println!("Track URI: {}", track_uri);
    }
}
```

**Run with**: `cargo run --example simple_event_consumer`

### 2. typed_event_handling.rs

**Purpose**: Advanced type-safe event processing patterns

**What it demonstrates**:
- Handling multiple event types with a unified processor
- Type-safe event routing based on service types
- Conditional processing based on event content
- Error handling for unsupported event types
- Event processing statistics and monitoring
- Safe handling of optional fields in typed events

**Key concepts**:
```rust
// Service-type-based routing
match event.service_type() {
    ServiceType::AVTransport => {
        if let Some(av_event) = event.downcast_ref::<AVTransportEvent>() {
            // Process AVTransport event with full type safety
            match av_event.transport_state.as_str() {
                "PLAYING" => println!("Music is playing"),
                "PAUSED_PLAYBACK" => println!("Music is paused"),
                _ => println!("Other state: {}", av_event.transport_state),
            }
        }
    }
    ServiceType::RenderingControl => {
        // Handle RenderingControl events (when implemented)
    }
    // ... other service types
}
```

**Run with**: `cargo run --example typed_event_handling`

## TypedEvent System Overview

The new TypedEvent system provides several key benefits over the previous ParsedEvent enum approach:

### 1. Strategy-Owned Event Types

Each strategy can define its own strongly-typed event data structures:

```rust
#[derive(Debug, Clone)]
pub struct AVTransportEvent {
    pub transport_state: String,
    pub track_uri: Option<String>,
    pub track_metadata: Option<DidlLite>,
    pub current_track_duration: Option<String>,
    // ... other fields
}
```

### 2. Type-Safe Downcasting

Events can be safely downcast to their specific types:

```rust
// Safe downcasting - returns None if types don't match
if let Some(av_event) = typed_event.downcast_ref::<AVTransportEvent>() {
    // Work with strongly-typed data
    println!("State: {}", av_event.transport_state);
} else {
    // Handle unknown or incompatible event types
    println!("Unknown event type: {}", typed_event.event_type());
}
```

### 3. Extensibility Without Core Changes

New strategies can define their own event types without modifying core event enums:

```rust
// In a new strategy crate
#[derive(Debug, Clone)]
pub struct CustomServiceEvent {
    pub custom_field: String,
    // ... strategy-specific fields
}

impl EventData for CustomServiceEvent {
    fn event_type(&self) -> &str { "custom_event" }
    fn service_type(&self) -> ServiceType { ServiceType::Custom }
    // ... other trait methods
}
```

### 4. Uniform Interface

All events implement the `EventData` trait, providing a consistent interface:

```rust
// These methods work for any event type
println!("Event type: {}", event.event_type());
println!("Service: {:?}", event.service_type());
println!("Debug: {:?}", event.debug());
```

## Best Practices

### 1. Always Use Type-Safe Downcasting

```rust
// ✅ Good - safe downcasting with error handling
if let Some(av_event) = event.downcast_ref::<AVTransportEvent>() {
    // Process typed event
} else {
    // Handle unknown event type gracefully
    eprintln!("Unexpected event type: {}", event.event_type());
}

// ❌ Avoid - assuming event types without checking
let av_event = event.downcast_ref::<AVTransportEvent>().unwrap(); // Can panic!
```

### 2. Handle Optional Fields Safely

```rust
// ✅ Good - safe optional field access
if let Some(track_uri) = &av_event.track_uri {
    println!("Playing: {}", track_uri);
} else {
    println!("No track URI available");
}

// ✅ Also good - using unwrap_or for defaults
let duration = av_event.current_track_duration
    .as_deref()
    .unwrap_or("Unknown");
```

### 3. Use Service Type for Event Routing

```rust
// ✅ Good - route by service type first, then downcast
match event.service_type() {
    ServiceType::AVTransport => {
        if let Some(av_event) = event.downcast_ref::<AVTransportEvent>() {
            process_av_transport(av_event);
        }
    }
    ServiceType::RenderingControl => {
        // Handle rendering control events
    }
    // ... other service types
}
```

### 4. Implement Graceful Fallbacks

```rust
// ✅ Good - graceful handling of unknown events
match event.service_type() {
    ServiceType::AVTransport => { /* handle */ }
    ServiceType::RenderingControl => { /* handle */ }
    ServiceType::ZoneGroupTopology => { /* handle */ }
    // Note: No catch-all pattern needed since ServiceType is exhaustive
}
```

## Testing Your Event Handlers

Both examples use mock strategies that simulate real UPnP events. This allows you to:

1. **Test event processing logic** without requiring real Sonos hardware
2. **Simulate different event scenarios** (playing, paused, stopped, etc.)
3. **Verify type-safe downcasting** works correctly
4. **Test error handling** for malformed or unexpected events

The mock strategies create realistic `AVTransportEvent` instances that match what you'd receive from real Sonos speakers, making the examples useful for understanding real-world usage patterns.

## Migration from ParsedEvent

If you're migrating from the old `ParsedEvent` enum system:

1. **Replace enum matching** with service type matching + downcasting
2. **Update event field access** to use strongly-typed fields instead of HashMap lookups
3. **Add error handling** for failed downcasts
4. **Leverage type safety** to catch errors at compile time instead of runtime

The new system provides better type safety, extensibility, and performance while maintaining backward compatibility through the common `EventData` interface.