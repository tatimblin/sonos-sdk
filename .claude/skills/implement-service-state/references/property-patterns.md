# Property Patterns

## Overview

Properties in sonos-state represent typed values that can be stored, tracked, and watched. Each property implements two traits: `Property` and `SonosProperty`.

## The Property Trait

```rust
/// Marker trait for all property types
pub trait Property: Clone + Send + Sync + 'static {
    /// Unique key identifying this property type
    const KEY: &'static str;
}
```

The `KEY` constant is used as the identifier in the state store.

## The SonosProperty Trait

```rust
/// Extended trait for Sonos-specific properties
pub trait SonosProperty: Property {
    /// Scope of this property (Speaker, Group, or System)
    const SCOPE: Scope;

    /// UPnP service this property comes from
    const SERVICE: Service;
}
```

## Scope Enum

```rust
pub enum Scope {
    /// Property applies to a single speaker
    Speaker,

    /// Property applies to a speaker group
    Group,

    /// Property is system-wide
    System,
}
```

### Scope Selection Guide

| Scope | Description | Examples |
|-------|-------------|----------|
| `Speaker` | Per-speaker value, independent of grouping | Volume, Mute, Bass, Treble |
| `Group` | Shared across grouped speakers | GroupVolume (via GroupRenderingControl) |
| `System` | System-wide, not tied to speakers | ZoneGroupTopology |

## Complete Property Implementation Pattern

### Simple Value Property

```rust
/// Speaker volume level (0-100)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Volume(pub u8);

impl Property for Volume {
    const KEY: &'static str = "volume";
}

impl SonosProperty for Volume {
    const SCOPE: Scope = Scope::Speaker;
    const SERVICE: Service = Service::RenderingControl;
}

impl Volume {
    /// Create a new Volume, clamped to valid range
    pub fn new(value: u8) -> Self {
        Self(value.min(100))
    }

    /// Get the volume value
    pub fn value(&self) -> u8 {
        self.0
    }
}
```

### Boolean Property

```rust
/// Speaker mute state
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Mute(pub bool);

impl Property for Mute {
    const KEY: &'static str = "mute";
}

impl SonosProperty for Mute {
    const SCOPE: Scope = Scope::Speaker;
    const SERVICE: Service = Service::RenderingControl;
}

impl Mute {
    pub fn new(muted: bool) -> Self {
        Self(muted)
    }

    pub fn is_muted(&self) -> bool {
        self.0
    }
}
```

### Signed Range Property

```rust
/// Bass EQ level (-10 to +10)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bass(pub i8);

impl Property for Bass {
    const KEY: &'static str = "bass";
}

impl SonosProperty for Bass {
    const SCOPE: Scope = Scope::Speaker;
    const SERVICE: Service = Service::RenderingControl;
}

impl Bass {
    pub fn new(value: i8) -> Self {
        Self(value.clamp(-10, 10))
    }

    pub fn value(&self) -> i8 {
        self.0
    }
}
```

### Enum Property

```rust
/// Current playback state
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PlaybackState {
    Playing,
    Paused,
    Stopped,
    Transitioning,
}

impl Property for PlaybackState {
    const KEY: &'static str = "playback_state";
}

impl SonosProperty for PlaybackState {
    const SCOPE: Scope = Scope::Speaker;
    const SERVICE: Service = Service::AVTransport;
}
```

### Complex Struct Property

```rust
/// Current track information
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CurrentTrack {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub album_art_uri: Option<String>,
    pub uri: Option<String>,
}

impl Property for CurrentTrack {
    const KEY: &'static str = "current_track";
}

impl SonosProperty for CurrentTrack {
    const SCOPE: Scope = Scope::Speaker;
    const SERVICE: Service = Service::AVTransport;
}

impl CurrentTrack {
    pub fn new() -> Self {
        Self {
            title: None,
            artist: None,
            album: None,
            album_art_uri: None,
            uri: None,
        }
    }

    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    // ... more builder methods
}

impl Default for CurrentTrack {
    fn default() -> Self {
        Self::new()
    }
}
```

### Position Property (with Parsing)

```rust
/// Playback position
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Position {
    pub position_ms: u64,
    pub duration_ms: u64,
}

impl Property for Position {
    const KEY: &'static str = "position";
}

impl SonosProperty for Position {
    const SCOPE: Scope = Scope::Speaker;
    const SERVICE: Service = Service::AVTransport;
}

impl Position {
    pub fn new(position_ms: u64, duration_ms: u64) -> Self {
        Self { position_ms, duration_ms }
    }

    /// Parse time string (H:MM:SS) to milliseconds
    pub fn parse_time_to_ms(time_str: &str) -> Option<u64> {
        let parts: Vec<&str> = time_str.split(':').collect();
        if parts.len() != 3 {
            return None;
        }

        let hours: u64 = parts[0].parse().ok()?;
        let minutes: u64 = parts[1].parse().ok()?;
        let seconds: u64 = parts[2].parse().ok()?;

        Some((hours * 3600 + minutes * 60 + seconds) * 1000)
    }

    /// Get position as percentage (0.0 - 1.0)
    pub fn progress(&self) -> f64 {
        if self.duration_ms == 0 {
            0.0
        } else {
            (self.position_ms as f64) / (self.duration_ms as f64)
        }
    }
}
```

## Required Derives

All properties should derive:

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
```

| Derive | Why |
|--------|-----|
| `Debug` | Logging and debugging |
| `Clone` | State store operations |
| `PartialEq` | Change detection |
| `Serialize, Deserialize` | Persistence, debugging |

## Key Naming Convention

Keys should be:
- Lowercase with underscores
- Descriptive but concise
- Unique across all properties

Examples:
- `"volume"` not `"vol"` or `"Volume"`
- `"playback_state"` not `"state"` or `"PlaybackState"`
- `"current_track"` not `"track"` or `"nowPlaying"`

## Checklist for New Properties

- [ ] Struct defined with appropriate inner type
- [ ] All required derives present
- [ ] `Property` trait implemented with unique KEY
- [ ] `SonosProperty` trait implemented with correct SCOPE and SERVICE
- [ ] Constructor method (`new()`)
- [ ] Accessor methods (`value()`, `is_*()`, etc.)
- [ ] Validation/clamping in constructor if needed
- [ ] Doc comment explaining the property
- [ ] Re-exported in `lib.rs`
