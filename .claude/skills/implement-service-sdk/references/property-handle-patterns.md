# Property Handle Patterns

## Overview

The sonos-sdk provides a DOM-like API for accessing speaker properties through `PropertyHandle<P>`. Each handle provides:

- `get()` - Get cached value (instant, no network)
- `fetch()` - Fetch fresh value from device (if Fetchable)
- `watch()` - Register for change notifications
- `unwatch()` - Unregister from change notifications

## Core Types

### PropertyHandle<P>

```rust
/// Generic property handle providing get/fetch/watch/unwatch pattern
#[derive(Clone)]
pub struct PropertyHandle<P: SonosProperty> {
    context: Arc<SpeakerContext>,
    _phantom: PhantomData<P>,
}

impl<P: SonosProperty> PropertyHandle<P> {
    /// Create a new PropertyHandle from a shared SpeakerContext
    pub fn new(context: Arc<SpeakerContext>) -> Self;

    /// Get cached property value (sync, instant, no network call)
    pub fn get(&self) -> Option<P>;

    /// Start watching this property for changes (sync)
    pub fn watch(&self) -> Result<WatchStatus<P>, SdkError>;

    /// Stop watching this property (sync)
    pub fn unwatch(&self);

    /// Check if this property is currently being watched
    pub fn is_watched(&self) -> bool;
}

// fetch() is only available for Fetchable properties
impl<P: Fetchable> PropertyHandle<P> {
    /// Fetch fresh value from device + update cache (sync)
    pub fn fetch(&self) -> Result<P, SdkError>;
}
```

### SpeakerContext

Shared context for all property handles on a speaker:

```rust
#[derive(Clone)]
pub struct SpeakerContext {
    pub(crate) speaker_id: SpeakerId,
    pub(crate) speaker_ip: IpAddr,
    pub(crate) state_manager: Arc<StateManager>,
    pub(crate) api_client: SonosClient,
}

impl SpeakerContext {
    pub fn new(
        speaker_id: SpeakerId,
        speaker_ip: IpAddr,
        state_manager: Arc<StateManager>,
        api_client: SonosClient,
    ) -> Arc<Self>;
}
```

### WatchStatus<P>

Result of watch() operation:

```rust
pub struct WatchStatus<P> {
    /// Current cached value of the property (if any)
    pub current: Option<P>,

    /// How updates will be delivered
    pub mode: WatchMode,
}

pub enum WatchMode {
    /// UPnP event subscription is active - real-time updates
    Events,
    /// UPnP subscription failed, polling fallback active
    Polling,
    /// No event manager - cache-only mode
    CacheOnly,
}
```

## The Fetchable Trait

Properties that can be fetched directly via UPnP operations implement `Fetchable`:

```rust
pub trait Fetchable: SonosProperty {
    /// The UPnP operation type used to fetch this property
    type Operation: UPnPOperation;

    /// Build the operation to fetch this property
    fn build_operation() -> Result<ComposableOperation<Self::Operation>, SdkError>;

    /// Convert the operation response to the property value
    fn from_response(response: <Self::Operation as UPnPOperation>::Response) -> Self;
}
```

## Fetchable Implementation Examples

### Simple Value Property (Volume)

```rust
impl Fetchable for Volume {
    type Operation = GetVolumeOperation;

    fn build_operation() -> Result<ComposableOperation<Self::Operation>, SdkError> {
        rendering_control::get_volume_operation("Master".to_string())
            .build()
            .map_err(|e| build_error("GetVolume", e))
    }

    fn from_response(response: GetVolumeResponse) -> Self {
        Volume::new(response.current_volume)
    }
}
```

### Enum Property (PlaybackState)

```rust
impl Fetchable for PlaybackState {
    type Operation = GetTransportInfoOperation;

    fn build_operation() -> Result<ComposableOperation<Self::Operation>, SdkError> {
        av_transport::get_transport_info_operation()
            .build()
            .map_err(|e| build_error("GetTransportInfo", e))
    }

    fn from_response(response: GetTransportInfoResponse) -> Self {
        match response.current_transport_state.as_str() {
            "PLAYING" => PlaybackState::Playing,
            "PAUSED" | "PAUSED_PLAYBACK" => PlaybackState::Paused,
            "STOPPED" => PlaybackState::Stopped,
            _ => PlaybackState::Transitioning,
        }
    }
}
```

### Complex Property with Parsing (Position)

```rust
impl Fetchable for Position {
    type Operation = GetPositionInfoOperation;

    fn build_operation() -> Result<ComposableOperation<Self::Operation>, SdkError> {
        av_transport::get_position_info_operation()
            .build()
            .map_err(|e| build_error("GetPositionInfo", e))
    }

    fn from_response(response: GetPositionInfoResponse) -> Self {
        let position_ms = Position::parse_time_to_ms(&response.rel_time).unwrap_or(0);
        let duration_ms = Position::parse_time_to_ms(&response.track_duration).unwrap_or(0);
        Position::new(position_ms, duration_ms)
    }
}
```

## Non-Fetchable Properties

Properties without dedicated Get operations can't implement `Fetchable`:

```rust
// Note: The following properties don't have dedicated GetXxx operations in the
// Sonos UPnP API. Their values are typically obtained through:
// 1. UPnP event subscriptions (RenderingControl LastChange events)
// 2. Parsing the LastChange XML from events
//
// For now, these properties can only be read from cache (via get()) after
// being populated by the event system. The fetch() method is not available
// for these property types.
//
// Properties without fetch():
// - Mute: Obtained from RenderingControl events
// - Bass: Obtained from RenderingControl events
// - Treble: Obtained from RenderingControl events
// - Loudness: Obtained from RenderingControl events
// - CurrentTrack: Obtained from AVTransport events (track metadata)
// - GroupMembership: Obtained from ZoneGroupTopology events
```

## Type Alias Pattern

Create type aliases for cleaner API:

```rust
// ============================================================================
// Type aliases
// ============================================================================

/// Handle for speaker volume (0-100)
pub type VolumeHandle = PropertyHandle<Volume>;

/// Handle for playback state (Playing/Paused/Stopped)
pub type PlaybackStateHandle = PropertyHandle<PlaybackState>;

/// Handle for mute state
pub type MuteHandle = PropertyHandle<Mute>;

/// Handle for bass EQ setting (-10 to +10)
pub type BassHandle = PropertyHandle<Bass>;

/// Handle for treble EQ setting (-10 to +10)
pub type TrebleHandle = PropertyHandle<Treble>;

/// Handle for loudness compensation setting
pub type LoudnessHandle = PropertyHandle<Loudness>;

/// Handle for current playback position
pub type PositionHandle = PropertyHandle<Position>;

/// Handle for current track information
pub type CurrentTrackHandle = PropertyHandle<CurrentTrack>;

/// Handle for group membership information
pub type GroupMembershipHandle = PropertyHandle<GroupMembership>;
```

## Speaker Struct Pattern

Add handles as public fields on Speaker:

```rust
pub struct Speaker {
    /// Unique speaker identifier
    pub id: SpeakerId,
    /// Friendly name of the speaker
    pub name: String,
    /// IP address of the speaker
    pub ip: IpAddr,
    /// Model name of the speaker
    pub model_name: String,

    // ========================================================================
    // RenderingControl properties
    // ========================================================================
    /// Volume property (0-100)
    pub volume: VolumeHandle,
    /// Mute state (true = muted)
    pub mute: MuteHandle,
    /// Bass EQ setting (-10 to +10)
    pub bass: BassHandle,
    /// Treble EQ setting (-10 to +10)
    pub treble: TrebleHandle,
    /// Loudness compensation setting
    pub loudness: LoudnessHandle,

    // ========================================================================
    // AVTransport properties
    // ========================================================================
    /// Playback state (Playing/Paused/Stopped/Transitioning)
    pub playback_state: PlaybackStateHandle,
    /// Current playback position and duration
    pub position: PositionHandle,
    /// Current track information (title, artist, album, etc.)
    pub current_track: CurrentTrackHandle,

    // ========================================================================
    // ZoneGroupTopology properties
    // ========================================================================
    /// Group membership information (group_id, is_coordinator)
    pub group_membership: GroupMembershipHandle,
}
```

## Speaker::new() Initialization Pattern

```rust
impl Speaker {
    pub fn new(
        id: SpeakerId,
        name: String,
        ip: IpAddr,
        model_name: String,
        state_manager: Arc<StateManager>,
        api_client: SonosClient,
    ) -> Self {
        // Create shared context (wrapped in Arc)
        let context = SpeakerContext::new(id.clone(), ip, state_manager, api_client);

        Self {
            id,
            name,
            ip,
            model_name,
            // All handles share the same context via Arc::clone
            volume: PropertyHandle::new(Arc::clone(&context)),
            mute: PropertyHandle::new(Arc::clone(&context)),
            bass: PropertyHandle::new(Arc::clone(&context)),
            treble: PropertyHandle::new(Arc::clone(&context)),
            loudness: PropertyHandle::new(Arc::clone(&context)),
            playback_state: PropertyHandle::new(Arc::clone(&context)),
            position: PropertyHandle::new(Arc::clone(&context)),
            current_track: PropertyHandle::new(Arc::clone(&context)),
            // Last handle doesn't need clone
            group_membership: PropertyHandle::new(context),
        }
    }
}
```

## Required Imports

### In handles.rs

```rust
use std::fmt;
use std::marker::PhantomData;
use std::net::IpAddr;
use std::sync::Arc;

use sonos_api::operation::{ComposableOperation, UPnPOperation};
use sonos_api::SonosClient;
use sonos_api::services::{
    av_transport::{self, GetPositionInfoOperation, GetTransportInfoOperation},
    rendering_control::{self, GetVolumeOperation},
    // Add new service imports here
};
use sonos_state::{property::SonosProperty, SpeakerId, StateManager};
use sonos_state::{
    Bass, CurrentTrack, GroupMembership, Loudness, Mute,
    PlaybackState, Position, Treble, Volume,
    // Add new property imports here
};

use crate::SdkError;
```

### In speaker.rs

```rust
use std::net::IpAddr;
use std::sync::Arc;

use sonos_api::SonosClient;
use sonos_discovery::Device;
use sonos_state::{SpeakerId, StateManager};

use crate::SdkError;
use crate::property::{
    BassHandle, CurrentTrackHandle, GroupMembershipHandle, LoudnessHandle,
    MuteHandle, PlaybackStateHandle, PositionHandle, PropertyHandle,
    SpeakerContext, TrebleHandle, VolumeHandle,
    // Add new handle type aliases here
};
```

## Error Handling

### Build Error Helper

```rust
/// Helper to create consistent error messages for operation build failures
fn build_error<E: std::fmt::Display>(operation_name: &str, e: E) -> SdkError {
    SdkError::FetchFailed(format!("Failed to build {operation_name} operation: {e}"))
}
```

### Using build_error

```rust
fn build_operation() -> Result<ComposableOperation<Self::Operation>, SdkError> {
    service::get_operation()
        .build()
        .map_err(|e| build_error("GetOperation", e))
}
```

## Test Patterns

### Test Handle Creation

```rust
#[test]
fn test_new_property_handle_creation() {
    let state_manager = create_test_state_manager();
    let context = create_test_context(state_manager);

    let handle: NewPropertyHandle = PropertyHandle::new(context);

    assert_eq!(handle.speaker_id().as_str(), "RINCON_TEST123");
}
```

### Test get() Returns Cached Value

```rust
#[test]
fn test_new_property_get_cached() {
    let state_manager = create_test_state_manager();
    let speaker_id = SpeakerId::new("RINCON_TEST123");

    // Pre-populate cache
    state_manager.set_property(&speaker_id, NewProperty::new(42));

    let context = create_test_context(Arc::clone(&state_manager));
    let handle: NewPropertyHandle = PropertyHandle::new(context);

    assert_eq!(handle.get(), Some(NewProperty::new(42)));
}
```

### Test watch() Registers Property

```rust
#[test]
fn test_new_property_watch() {
    let state_manager = create_test_state_manager();
    let context = create_test_context(Arc::clone(&state_manager));

    let handle: NewPropertyHandle = PropertyHandle::new(context);

    assert!(!handle.is_watched());
    handle.watch().unwrap();
    assert!(handle.is_watched());
}
```

## Checklist for New Property Handle

- [ ] Property type defined in sonos-state with Property + SonosProperty traits
- [ ] Fetchable impl added (if property has Get operation)
- [ ] Type alias created (e.g., `pub type NewPropertyHandle = PropertyHandle<NewProperty>`)
- [ ] Required imports added to handles.rs
- [ ] Field added to Speaker struct with doc comment
- [ ] Handle import added to speaker.rs
- [ ] Field initialized in Speaker::new() with `PropertyHandle::new(Arc::clone(&context))`
- [ ] Re-export added to lib.rs (if needed)
- [ ] Unit tests added for handle creation, get(), watch()
