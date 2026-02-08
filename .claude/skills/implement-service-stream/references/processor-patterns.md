# Event Processor Patterns

## Overview

The event processor in `sonos-stream/src/events/processor.rs` converts raw UPnP events from sonos-api into enriched sonos-stream events. This translation layer is necessary because:

1. sonos-api events use accessor methods (getters)
2. sonos-stream events use public fields
3. Some data transformations may be needed

## The convert_api_event_data() Method

This method is the key integration point for new services:

```rust
fn convert_api_event_data(
    &self,
    service: &sonos_api::Service,
    api_event_data: Box<dyn std::any::Any + Send + Sync>,
) -> EventProcessingResult<EventData> {
    match service {
        sonos_api::Service::AVTransport => {
            // Handle AVTransport...
        }
        sonos_api::Service::RenderingControl => {
            // Handle RenderingControl...
        }
        // Add new services here
    }
}
```

## Implementation Pattern

### Step 1: Downcast the API Event

```rust
sonos_api::Service::NewService => {
    let api_event = api_event_data
        .downcast::<sonos_api::services::new_service::NewServiceEvent>()
        .map_err(|_| EventProcessingError::Parsing(
            "Failed to downcast NewService event".to_string()
        ))?;
```

The `Box<dyn Any>` must be downcast to the concrete event type from sonos-api.

### Step 2: Map Fields to Stream Event

```rust
    let stream_event = crate::events::types::NewServiceEvent {
        // String fields - use map() to clone
        field1: api_event.field1().map(|s| s.to_string()),

        // Already owned types - direct access
        field2: api_event.field2(),

        // Boolean as string
        enabled: api_event.enabled().map(|b| b.to_string()),

        // Nested Option - flatten or preserve
        nested_field: api_event.nested().and_then(|n| n.inner()),
    };
```

### Step 3: Return Wrapped Event

```rust
    Ok(EventData::NewServiceEvent(stream_event))
}
```

## Field Mapping Patterns

### String Fields (Most Common)

```rust
// API returns &str, stream needs String
field: api_event.field().map(|s| s.to_string()),
```

### Numeric Fields

```rust
// Direct copy for Copy types
count: api_event.count(),

// Or if API returns reference
volume: api_event.volume().copied(),
```

### Boolean Fields

```rust
// If API returns bool, convert to String for consistency
enabled: api_event.enabled().map(|b| if b { "1" } else { "0" }.to_string()),

// Or if API returns string already
mute: api_event.mute().map(|s| s.to_string()),
```

### Complex Nested Types

```rust
// For complex types, create nested stream structs
members: api_event.members().map(|members| {
    members.iter().map(|m| ZoneGroupMemberInfo {
        uuid: m.uuid().to_string(),
        location: m.location().to_string(),
        zone_name: m.zone_name().to_string(),
    }).collect()
}).unwrap_or_default(),
```

### HashMap Fields

```rust
// Copy HashMap contents
other_channels: api_event.other_channels()
    .map(|h| h.iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect())
    .unwrap_or_default(),
```

## Complete Example: RenderingControl

```rust
sonos_api::Service::RenderingControl => {
    let api_event = api_event_data
        .downcast::<sonos_api::services::rendering_control::RenderingControlEvent>()
        .map_err(|_| EventProcessingError::Parsing(
            "Failed to downcast RenderingControl event".to_string()
        ))?;

    let stream_event = RenderingControlEvent {
        master_volume: api_event.master_volume().map(|s| s.to_string()),
        lf_volume: api_event.lf_volume().map(|s| s.to_string()),
        rf_volume: api_event.rf_volume().map(|s| s.to_string()),
        master_mute: api_event.master_mute().map(|s| s.to_string()),
        lf_mute: api_event.lf_mute().map(|s| s.to_string()),
        rf_mute: api_event.rf_mute().map(|s| s.to_string()),
        bass: api_event.bass().map(|s| s.to_string()),
        treble: api_event.treble().map(|s| s.to_string()),
        loudness: api_event.loudness().map(|s| s.to_string()),
        balance: api_event.balance().map(|s| s.to_string()),
        other_channels: api_event.other_channels()
            .map(|h| h.iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect())
            .unwrap_or_default(),
    };

    Ok(EventData::RenderingControlEvent(stream_event))
}
```

## Error Handling

### Downcast Failures

If the event type doesn't match, return a parsing error:

```rust
.map_err(|_| EventProcessingError::Parsing(
    format!("Failed to downcast {} event", service_name)
))?;
```

### Missing Required Fields

If a field is truly required, handle it:

```rust
let required_field = api_event.required_field()
    .ok_or_else(|| EventProcessingError::Parsing(
        "Missing required field".to_string()
    ))?
    .to_string();
```

### Graceful Degradation

For optional fields, just use `None`:

```rust
// This is fine - field will be None if not present
optional_field: api_event.optional_field().map(|s| s.to_string()),
```

## Testing the Processor

Since the processor handles `Box<dyn Any>`, testing requires creating real event objects:

```rust
#[test]
fn test_convert_new_service_event() {
    // Create a mock API event (if possible)
    // Or test through integration with real events
}
```

More practical testing happens through:
1. Integration tests with real speakers
2. Testing the full event pipeline

## Checklist

- [ ] Match arm added for new Service variant
- [ ] Downcast to correct sonos-api event type
- [ ] All fields mapped with appropriate transformations
- [ ] Error message includes service name
- [ ] Integration tested with real UPnP events
