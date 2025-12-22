# sonos-stream Examples

This directory contains examples demonstrating how to use the sonos-stream crate with the provider pattern and TypedEvent system for type-safe event handling.

## Examples

### 1. simple_event_consumer.rs

**Purpose**: Basic event consumption using the provider pattern

**What it demonstrates**:
- Creating a broker with AVTransportProvider (real provider implementation)
- Subscribing to a speaker for AVTransport events
- Receiving and processing events using TypedEvent
- Type-safe downcasting to AVTransportParser
- Accessing strongly-typed event data fields
- Proper cleanup and shutdown

**Key concepts**:
```rust
// Using real provider
let av_provider = AVTransportProvider::new();
let mut broker = EventBrokerBuilder::new()
    .with_strategy(Box::new(av_provider))
    .build()
    .await?;

// Type-safe downcasting
if let Some(av_event) = event.downcast_ref::<AVTransportParser>() {
    println!("Transport State: {}", av_event.transport_state());
    if let Some(track_uri) = av_event.current_track_uri() {
        println!("Track URI: {}", track_uri);
    }
}
```

**Run with**: `cargo run --example simple_event_consumer`

### 2. typed_event_handling.rs

**Purpose**: Advanced type-safe event processing with providers

**What it demonstrates**:
- Using AVTransportProvider for real service strategy implementation
- Handling multiple event types with a unified processor
- Type-safe event routing based on service types
- Conditional processing based on event content
- Error handling for unsupported event types
- Event processing statistics and monitoring
- Safe handling of optional fields in typed events

**Key concepts**:
```rust
// Service-type-based routing with real provider
match event.service_type() {
    ServiceType::AVTransport => {
        if let Some(av_event) = event.downcast_ref::<AVTransportParser>() {
            // Process AVTransport event with full type safety
            match av_event.transport_state() {
                "PLAYING" => println!("Music is playing"),
                "PAUSED_PLAYBACK" => println!("Music is paused"),
                _ => println!("Other state: {}", av_event.transport_state()),
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

### 3. multiple_providers.rs

**Purpose**: Demonstrating multiple service providers registration

**What it demonstrates**:
- Registering multiple service providers (AVTransport, RenderingControl, ZoneGroupTopology)
- Handling events from different service types in a unified stream
- Provider pattern's extensibility and pluggability
- Each provider encapsulating its service-specific logic
- Different subscription scopes (per-speaker vs global)
- Real provider alongside mock providers for demonstration

**Key concepts**:
```rust
// Register multiple providers
let mut broker = EventBrokerBuilder::new()
    .with_strategy(Box::new(AVTransportProvider::new()))           // Real implementation
    .with_strategy(Box::new(MockRenderingControlProvider::new()))  // Demo implementation
    .with_strategy(Box::new(MockZoneGroupTopologyProvider::new())) // Demo implementation
    .build()
    .await?;

// Each provider handles its own service type automatically
// Events are routed to the appropriate provider based on service type
```

**Run with**: `cargo run --example multiple_providers`

## Provider Pattern Overview

The provider pattern provides several key benefits over the previous strategy approach:

### 1. Service Provider Encapsulation

Each provider encapsulates all service-specific logic:

```rust
#[derive(Debug, Clone)]
pub struct AVTransportProvider;

#[async_trait]
impl ServiceStrategy for AVTransportProvider {
    fn service_type(&self) -> ServiceType {
        ServiceType::AVTransport
    }

    fn subscription_scope(&self) -> SubscriptionScope {
        SubscriptionScope::PerSpeaker
    }

    fn service_endpoint_path(&self) -> &'static str {
        "/MediaRenderer/AVTransport/Event"
    }

    fn parse_event(&self, _speaker_id: &SpeakerId, event_xml: &str) -> Result<TypedEvent, StrategyError> {
        let parser = AVTransportParser::from_xml(event_xml)?;
        Ok(TypedEvent::new_parser(parser, "av_transport_event", ServiceType::AVTransport))
    }

    // Default subscription creation implementation provided by trait
}
```

### 2. Pluggable Architecture

Providers can be easily registered and swapped:

```rust
// Register any provider that implements ServiceStrategy
let broker = EventBrokerBuilder::new()
    .with_strategy(Box::new(AVTransportProvider::new()))
    .with_strategy(Box::new(RenderingControlProvider::new()))
    .with_strategy(Box::new(CustomServiceProvider::new()))
    .build()
    .await?;
```

### 3. Type-Safe Event Parsing

Each provider returns strongly-typed events:

```rust
// AVTransportProvider returns TypedEvent containing AVTransportParser
if let Some(av_parser) = event.downcast_ref::<AVTransportParser>() {
    // Access parsed fields with full type safety
    println!("State: {}", av_parser.transport_state());
    println!("URI: {:?}", av_parser.current_track_uri());
}
```

### 4. Extensibility Without Core Changes

New services can be added by implementing the ServiceStrategy trait:

```rust
#[derive(Debug, Clone)]
pub struct MyCustomProvider;

#[async_trait]
impl ServiceStrategy for MyCustomProvider {
    fn service_type(&self) -> ServiceType {
        ServiceType::Custom
    }

    fn service_endpoint_path(&self) -> &'static str {
        "/Custom/Event"
    }

    fn parse_event(&self, _speaker_id: &SpeakerId, event_xml: &str) -> Result<TypedEvent, StrategyError> {
        let custom_parser = MyCustomParser::from_xml(event_xml)?;
        Ok(TypedEvent::new_parser(custom_parser, "custom_event", ServiceType::Custom))
    }

    // Other required methods...
}
```

## TypedEvent System with Providers

The TypedEvent system works seamlessly with the provider pattern:

### 1. Provider-Owned Parsers

Each provider uses its own parser type:

```rust
// AVTransportProvider uses AVTransportParser
let av_parser = AVTransportParser::from_xml(xml)?;
Ok(TypedEvent::new_parser(av_parser, "av_transport_event", ServiceType::AVTransport))

// RenderingControlProvider would use RenderingControlParser
let rc_parser = RenderingControlParser::from_xml(xml)?;
Ok(TypedEvent::new_parser(rc_parser, "rendering_control_event", ServiceType::RenderingControl))
```

### 2. Type-Safe Downcasting

Events can be safely downcast to their provider's parser type:

```rust
// Safe downcasting - returns None if types don't match
if let Some(av_parser) = typed_event.downcast_ref::<AVTransportParser>() {
    // Work with strongly-typed parser methods
    println!("State: {}", av_parser.transport_state());
} else {
    // Handle unknown or incompatible event types
    println!("Unknown event type: {}", typed_event.event_type());
}
```

### 3. Uniform Interface

All events provide a consistent interface regardless of provider:

```rust
// These methods work for any event type from any provider
println!("Event type: {}", event.event_type());
println!("Service: {:?}", event.service_type());
```

## Best Practices

### 1. Use Real Providers When Available

```rust
// ✅ Good - use real provider implementations
let broker = EventBrokerBuilder::new()
    .with_strategy(Box::new(AVTransportProvider::new()))
    .build()
    .await?;

// ❌ Avoid - creating mock strategies when real providers exist
// (unless specifically needed for testing)
```

### 2. Always Use Type-Safe Downcasting

```rust
// ✅ Good - safe downcasting with error handling
if let Some(av_parser) = event.downcast_ref::<AVTransportParser>() {
    // Process typed parser
} else {
    // Handle unknown event type gracefully
    eprintln!("Unexpected event type: {}", event.event_type());
}

// ❌ Avoid - assuming event types without checking
let av_parser = event.downcast_ref::<AVTransportParser>().unwrap(); // Can panic!
```

### 3. Handle Optional Fields Safely

```rust
// ✅ Good - safe optional field access
if let Some(track_uri) = av_parser.current_track_uri() {
    println!("Playing: {}", track_uri);
} else {
    println!("No track URI available");
}

// ✅ Also good - using unwrap_or for defaults
let duration = av_parser.current_track_duration()
    .unwrap_or("Unknown");
```

### 4. Use Service Type for Event Routing

```rust
// ✅ Good - route by service type first, then downcast
match event.service_type() {
    ServiceType::AVTransport => {
        if let Some(av_parser) = event.downcast_ref::<AVTransportParser>() {
            process_av_transport(av_parser);
        }
    }
    ServiceType::RenderingControl => {
        // Handle rendering control events
    }
    // ... other service types
}
```

## Testing Your Event Handlers

The examples demonstrate different testing approaches:

1. **Real providers with simulated events** (simple_event_consumer.rs, typed_event_handling.rs)
   - Uses real AVTransportProvider
   - Simulates events by posting to callback server
   - Tests real parsing logic with mock event data

2. **Mixed real and mock providers** (multiple_providers.rs)
   - Uses real AVTransportProvider alongside mock providers
   - Demonstrates how providers can be mixed and matched
   - Shows the pluggable nature of the provider pattern

This allows you to:
- **Test provider integration** without requiring real Sonos hardware
- **Simulate different event scenarios** (playing, paused, stopped, etc.)
- **Verify type-safe downcasting** works correctly with real parsers
- **Test error handling** for malformed or unexpected events

## Migration from Strategy Pattern

If you're migrating from the old strategy enum system:

1. **Replace strategy enums** with provider structs implementing ServiceStrategy
2. **Update strategy registration** to use `.with_strategy(Box::new(Provider::new()))`
3. **Encapsulate service logic** in provider implementations
4. **Use real providers** like AVTransportProvider instead of custom strategies
5. **Leverage provider extensibility** to add new services without core changes

The new provider pattern provides better encapsulation, extensibility, and maintainability while maintaining the same event processing capabilities.