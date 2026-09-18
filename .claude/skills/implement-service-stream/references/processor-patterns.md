# Event Processor Patterns

## Overview

Two files share the work of turning a raw UPnP event into an `EventData`:

| File | Responsibility |
|------|----------------|
| `sonos-api/src/services/{service}/events.rs` | Parse the UPnP XML into `{Service}Event`, then map it to `{Service}State` in `into_state()` |
| `sonos-stream/src/events/processor.rs` | Downcast the type-erased event and call `into_state()` |

All field-level mapping lives in `into_state()`, in sonos-api. The processor arm does no
field mapping at all — that is what lets the polling path, which never sees an event, reach
the identical `{Service}State` through `state::poll()`.

## The convert_api_event_data() Method

This method is the integration point in sonos-stream:

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

The match is exhaustive over `Service`, so a new `Service` variant without an arm is a
compile error, not a silently dropped event.

## Implementation Pattern

### Step 1: Add the Processor Arm

The whole arm is a downcast plus `into_state()`:

```rust
sonos_api::Service::NewService => {
    let event = api_event_data
        .downcast::<sonos_api::services::new_service::NewServiceEvent>()
        .map_err(|_| {
            EventProcessingError::Parsing(
                "Failed to downcast NewService event".to_string(),
            )
        })?;
    Ok(EventData::NewService(event.into_state()))
}
```

`Box<dyn Any>` must be downcast to the concrete event type from sonos-api. If a field
needs converting, that belongs in `into_state()` below — not here.

### Step 2: Write the Accessors and into_state()

In `sonos-api/src/services/new_service/events.rs`. `{Service}Event` deserializes the
`<e:propertyset>` payload with serde; accessors normalize each field; `into_state()`
assembles the canonical type:

```rust
impl NewServiceEvent {
    /// Get field1
    pub fn field1(&self) -> Option<String> {
        self.properties
            .iter()
            .find_map(|p| p.field1.as_ref())
            .cloned()
    }

    /// Get whether the feature is enabled.
    ///
    /// UPnP spells booleans as "1"/"0" on some firmwares and "true"/"false" on
    /// others, so both are accepted.
    pub fn enabled(&self) -> Option<bool> {
        self.properties
            .iter()
            .find_map(|p| p.enabled.as_ref())
            .map(|s| s == "1" || s.to_lowercase() == "true")
    }

    /// Convert parsed UPnP event to canonical state representation.
    pub fn into_state(&self) -> super::state::NewServiceState {
        super::state::NewServiceState {
            field1: self.field1(),
            enabled: self.enabled(),
        }
    }

    /// Parse from UPnP event XML using serde
    pub fn from_xml(xml: &str) -> Result<Self> {
        quick_xml::de::from_str(xml)
            .map_err(|e| ApiError::ParseError(format!("Failed to parse NewService XML: {e}")))
    }
}
```

A UPnP event carries only the properties that changed, so `find_map` over the property
list — rather than indexing a fixed position — is what makes partial events work.

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

## Testing

The processor itself handles `Box<dyn Any>` and is awkward to unit test. Test
`into_state()` instead — that is where every field decision lives — using XML captured
from a real speaker:

```rust
#[test]
fn test_parse_real_event_xml() {
    // Captured from a real Sonos Amp (Living Room)
    let xml = r#"<e:propertyset xmlns:e="urn:schemas-upnp-org:event-1-0"><e:property><Field1>abc</Field1></e:property><e:property><Enabled>1</Enabled></e:property></e:propertyset>"#;

    let event = NewServiceEvent::from_xml(xml).unwrap();
    let state = event.into_state();

    assert_eq!(state.field1.as_deref(), Some("abc"));
    assert_eq!(state.enabled, Some(true));
}
```

Add a second case with the payload pretty-printed across lines. Real firmwares send both,
and whitespace handling is the most common parse regression.

The end-to-end path is covered by integration tests against real speakers.

## Checklist

- [ ] `{Service}State` defined in `sonos-api/src/services/{service}/state.rs`
- [ ] `{Service}Event::from_xml()` parses the propertyset with serde
- [ ] `{Service}Event::into_state()` maps every field, accepting both boolean spellings
- [ ] Match arm added in `convert_api_event_data()` for the new `Service` variant
- [ ] Arm downcasts to the correct sonos-api event type and calls `into_state()`
- [ ] Error message includes the service name
- [ ] `into_state()` tested against captured real-speaker XML, compact and pretty-printed
- [ ] Integration tested with real UPnP events
