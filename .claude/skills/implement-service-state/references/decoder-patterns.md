# Decoder Patterns

## Overview

Decoders convert sonos-stream events into typed property changes that can be applied to the state store. The main decoder is in `sonos-state/src/decoder.rs`.

## Core Types

### DecodedChanges

```rust
/// Decoded changes from a single event
#[derive(Debug)]
pub struct DecodedChanges {
    /// Speaker ID the changes apply to
    pub speaker_id: SpeakerId,
    /// List of property changes
    pub changes: Vec<PropertyChange>,
}
```

### PropertyChange Enum

```rust
/// A single property change
#[derive(Debug, Clone)]
pub enum PropertyChange {
    Volume(Volume),
    Mute(Mute),
    Bass(Bass),
    Treble(Treble),
    Loudness(Loudness),
    PlaybackState(PlaybackState),
    Position(Position),
    CurrentTrack(CurrentTrack),
    GroupMembership(GroupMembership),
    // Add new variants here
}
```

## Adding a New Property to PropertyChange

### Step 1: Add Enum Variant

```rust
pub enum PropertyChange {
    // ... existing variants ...
    NewProperty(NewProperty),
}
```

### Step 2: Update key() Method

```rust
impl PropertyChange {
    pub fn key(&self) -> &'static str {
        use crate::property::Property;
        match self {
            // ... existing matches ...
            PropertyChange::NewProperty(_) => NewProperty::KEY,
        }
    }
}
```

### Step 3: Update service() Method

```rust
impl PropertyChange {
    pub fn service(&self) -> Service {
        use crate::property::SonosProperty;
        match self {
            // ... existing matches ...
            PropertyChange::NewProperty(_) => NewProperty::SERVICE,
        }
    }
}
```

## Decoder Functions

### Main Entry Point

```rust
/// Decode an enriched event into typed property changes
pub fn decode_event(event: &EnrichedEvent, speaker_id: SpeakerId) -> DecodedChanges {
    let changes = match &event.event_data {
        EventData::RenderingControlEvent(rc) => decode_rendering_control(rc),
        EventData::AVTransportEvent(avt) => decode_av_transport(avt),
        EventData::ZoneGroupTopologyEvent(zgt) => decode_topology(zgt),
        EventData::DevicePropertiesEvent(_) => vec![],
        EventData::NewServiceEvent(ns) => decode_new_service(ns),  // Add new service
    };

    DecodedChanges { speaker_id, changes }
}
```

### Service-Specific Decoder Pattern

```rust
/// Decode NewService event data
fn decode_new_service(event: &NewServiceEvent) -> Vec<PropertyChange> {
    let mut changes = vec![];

    // Parse each field from the event
    // ...

    changes
}
```

## Field Parsing Patterns

### Integer Parsing (u8, u16, etc.)

```rust
if let Some(value_str) = &event.field_name {
    if let Ok(value) = value_str.parse::<u8>() {
        changes.push(PropertyChange::NewProperty(NewProperty(value.min(100))));
    }
}
```

### Signed Integer Parsing (i8, i16, etc.)

```rust
if let Some(value_str) = &event.field_name {
    if let Ok(value) = value_str.parse::<i8>() {
        changes.push(PropertyChange::NewProperty(NewProperty(value.clamp(-10, 10))));
    }
}
```

### Boolean Parsing

UPnP events represent booleans inconsistently ("1", "0", "true", "false"):

```rust
if let Some(bool_str) = &event.field_name {
    let enabled = bool_str == "1" || bool_str.eq_ignore_ascii_case("true");
    changes.push(PropertyChange::Enabled(Enabled(enabled)));
}
```

### Enum Parsing

```rust
if let Some(state_str) = &event.transport_state {
    let state = match state_str.to_uppercase().as_str() {
        "PLAYING" => PlaybackState::Playing,
        "PAUSED_PLAYBACK" | "PAUSED" => PlaybackState::Paused,
        "STOPPED" => PlaybackState::Stopped,
        _ => PlaybackState::Transitioning,
    };
    changes.push(PropertyChange::PlaybackState(state));
}
```

### Duration Parsing (HH:MM:SS to milliseconds)

```rust
/// Parse duration string (HH:MM:SS or H:MM:SS) to milliseconds
fn parse_duration_ms(duration: Option<&str>) -> Option<u64> {
    let d = duration?;

    // Handle NOT_IMPLEMENTED or empty strings
    if d.is_empty() || d == "NOT_IMPLEMENTED" {
        return None;
    }

    let parts: Vec<&str> = d.split(':').collect();
    if parts.len() != 3 {
        return None;
    }

    let hours: u64 = parts[0].parse().ok()?;
    let minutes: u64 = parts[1].parse().ok()?;

    // Handle potential milliseconds in seconds part (HH:MM:SS.mmm)
    let seconds_parts: Vec<&str> = parts[2].split('.').collect();
    let seconds: u64 = seconds_parts[0].parse().ok()?;
    let millis: u64 = seconds_parts.get(1).and_then(|m| m.parse().ok()).unwrap_or(0);

    Some((hours * 3600 + minutes * 60 + seconds) * 1000 + millis)
}
```

### XML Metadata Parsing

```rust
/// Parse DIDL-Lite track metadata XML
fn parse_track_metadata(
    metadata: Option<&str>,
) -> (Option<String>, Option<String>, Option<String>, Option<String>) {
    let xml = match metadata {
        Some(m) if !m.is_empty() && m != "NOT_IMPLEMENTED" => m,
        _ => return (None, None, None, None),
    };

    let title = extract_xml_element(xml, "dc:title");
    let artist = extract_xml_element(xml, "dc:creator")
        .or_else(|| extract_xml_element(xml, "r:albumArtist"));
    let album = extract_xml_element(xml, "upnp:album");
    let album_art_uri = extract_xml_element(xml, "upnp:albumArtURI");

    (title, artist, album, album_art_uri)
}

/// Extract content from an XML element
fn extract_xml_element(xml: &str, element: &str) -> Option<String> {
    let start_tag = format!("<{}>", element);
    let end_tag = format!("</{}>", element);

    let start_idx = xml.find(&start_tag)? + start_tag.len();
    let end_idx = xml[start_idx..].find(&end_tag)? + start_idx;

    let content = &xml[start_idx..end_idx];

    // Unescape basic XML entities
    let unescaped = content
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
        .replace("&apos;", "'")
        .replace("&quot;", "\"");

    if unescaped.is_empty() {
        None
    } else {
        Some(unescaped)
    }
}
```

### Complex Struct Construction

```rust
// Position (multiple fields)
if event.rel_time.is_some() || event.track_duration.is_some() {
    let position_ms = parse_duration_ms(event.rel_time.as_deref()).unwrap_or(0);
    let duration_ms = parse_duration_ms(event.track_duration.as_deref()).unwrap_or(0);

    let position = Position {
        position_ms,
        duration_ms,
    };
    changes.push(PropertyChange::Position(position));
}

// CurrentTrack (optional fields)
if event.current_track_uri.is_some() || event.track_metadata.is_some() {
    let (title, artist, album, album_art_uri) =
        parse_track_metadata(event.track_metadata.as_deref());

    let track = CurrentTrack {
        title,
        artist,
        album,
        album_art_uri,
        uri: event.current_track_uri.clone(),
    };
    changes.push(PropertyChange::CurrentTrack(track));
}
```

## Existing Decoder Examples

### RenderingControl Decoder

Handles audio settings from RenderingControlEvent:

```rust
fn decode_rendering_control(event: &RenderingControlEvent) -> Vec<PropertyChange> {
    let mut changes = vec![];

    // Volume
    if let Some(vol_str) = &event.master_volume {
        if let Ok(vol) = vol_str.parse::<u8>() {
            changes.push(PropertyChange::Volume(Volume(vol.min(100))));
        }
    }

    // Mute
    if let Some(mute_str) = &event.master_mute {
        let muted = mute_str == "1" || mute_str.eq_ignore_ascii_case("true");
        changes.push(PropertyChange::Mute(Mute(muted)));
    }

    // Bass
    if let Some(bass_str) = &event.bass {
        if let Ok(bass) = bass_str.parse::<i8>() {
            changes.push(PropertyChange::Bass(Bass(bass.clamp(-10, 10))));
        }
    }

    // Treble
    if let Some(treble_str) = &event.treble {
        if let Ok(treble) = treble_str.parse::<i8>() {
            changes.push(PropertyChange::Treble(Treble(treble.clamp(-10, 10))));
        }
    }

    // Loudness
    if let Some(loudness_str) = &event.loudness {
        let loudness = loudness_str == "1" || loudness_str.eq_ignore_ascii_case("true");
        changes.push(PropertyChange::Loudness(Loudness(loudness)));
    }

    changes
}
```

### AVTransport Decoder

Handles playback state from AVTransportEvent:

```rust
fn decode_av_transport(event: &AVTransportEvent) -> Vec<PropertyChange> {
    let mut changes = vec![];

    // Playback state
    if let Some(state) = &event.transport_state {
        let ps = match state.to_uppercase().as_str() {
            "PLAYING" => PlaybackState::Playing,
            "PAUSED_PLAYBACK" | "PAUSED" => PlaybackState::Paused,
            "STOPPED" => PlaybackState::Stopped,
            _ => PlaybackState::Transitioning,
        };
        changes.push(PropertyChange::PlaybackState(ps));
    }

    // Position
    if event.rel_time.is_some() || event.track_duration.is_some() {
        let position_ms = parse_duration_ms(event.rel_time.as_deref()).unwrap_or(0);
        let duration_ms = parse_duration_ms(event.track_duration.as_deref()).unwrap_or(0);

        let position = Position { position_ms, duration_ms };
        changes.push(PropertyChange::Position(position));
    }

    // CurrentTrack
    if event.current_track_uri.is_some() || event.track_metadata.is_some() {
        let (title, artist, album, album_art_uri) =
            parse_track_metadata(event.track_metadata.as_deref());

        let track = CurrentTrack {
            title, artist, album, album_art_uri,
            uri: event.current_track_uri.clone(),
        };
        changes.push(PropertyChange::CurrentTrack(track));
    }

    changes
}
```

## Error Handling

### Silent Skip Pattern

Parse errors are silently skipped - the decoder continues with other fields:

```rust
// If parsing fails, no change is emitted
if let Some(vol_str) = &event.volume {
    if let Ok(vol) = vol_str.parse::<u8>() {
        changes.push(PropertyChange::Volume(Volume(vol)));
    }
    // Silent skip if parse fails
}
```

### NOT_IMPLEMENTED Handling

UPnP returns "NOT_IMPLEMENTED" for unsupported fields:

```rust
// Handle NOT_IMPLEMENTED explicitly
if d.is_empty() || d == "NOT_IMPLEMENTED" {
    return None;
}
```

## Testing Decoders

### Unit Test for Decoder Function

```rust
#[test]
fn test_decode_new_service() {
    let event = NewServiceEvent {
        field1: Some("42".to_string()),
        field2: Some("true".to_string()),
    };

    let changes = decode_new_service(&event);

    assert!(changes.len() >= 1);
    if let PropertyChange::NewProperty(prop) = &changes[0] {
        assert_eq!(prop.value(), 42);
    } else {
        panic!("Expected NewProperty change");
    }
}
```

### Test PropertyChange Methods

```rust
#[test]
fn test_property_change_key() {
    use crate::property::Property;

    let change = PropertyChange::NewProperty(NewProperty(42));
    assert_eq!(change.key(), NewProperty::KEY);
}

#[test]
fn test_property_change_service() {
    use crate::property::SonosProperty;

    let change = PropertyChange::NewProperty(NewProperty(42));
    assert_eq!(change.service(), NewProperty::SERVICE);
}
```

### Test Parse Helpers

```rust
#[test]
fn test_parse_duration_ms() {
    assert_eq!(parse_duration_ms(Some("0:00:00")), Some(0));
    assert_eq!(parse_duration_ms(Some("0:01:00")), Some(60_000));
    assert_eq!(parse_duration_ms(Some("1:00:00")), Some(3_600_000));
    assert_eq!(parse_duration_ms(Some("0:03:45")), Some(225_000));
    assert_eq!(parse_duration_ms(Some("0:03:45.500")), Some(225_500));
    assert_eq!(parse_duration_ms(Some("NOT_IMPLEMENTED")), None);
    assert_eq!(parse_duration_ms(None), None);
}
```

## Checklist for New Decoder

- [ ] PropertyChange enum variant added
- [ ] key() match arm added
- [ ] service() match arm added
- [ ] decode_event() match arm added
- [ ] Service-specific decoder function created
- [ ] All event fields handled with appropriate parsing
- [ ] Parse errors silently skipped (no panics)
- [ ] NOT_IMPLEMENTED values handled
- [ ] Unit tests for decoder function
- [ ] Unit tests for PropertyChange methods
