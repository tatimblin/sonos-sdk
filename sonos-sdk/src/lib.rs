//! # Sonos SDK - Sync-First API for Sonos Control
//!
//! Provides a clean, property-centric API for controlling Sonos devices.
//! All operations are **synchronous** - no async/await required.
//!
//! ## Quick Start
//!
//! ```rust,ignore
//! use sonos_sdk::prelude::*;
//!
//! fn main() -> Result<(), SdkError> {
//!     let sonos = SonosSystem::new()?;
//!
//!     // Direct SOAP calls — no event infrastructure created
//!     let kitchen = sonos.speaker("Kitchen").unwrap();
//!     kitchen.play()?;
//!     let vol = kitchen.volume.fetch()?;
//!
//!     // Fluent navigation
//!     let group = kitchen.group().unwrap();
//!     println!("Kitchen is in group {}", group.id);
//!
//!     // ONLY NOW does the event manager lazily initialize
//!     kitchen.volume.watch()?;
//!     for event in sonos.iter() {
//!         println!("Changed: {} on {}", event.property_key, event.speaker_id);
//!     }
//!
//!     Ok(())
//! }
//! ```
//!
//! ## Key Features
//!
//! - **Sync-First API**: All methods are synchronous - no async/await required
//! - **Cheap constructor**: `SonosSystem::new()` does discovery only — event infrastructure is lazy
//! - **DOM-like API**: Access properties directly on speaker objects
//! - **Three access patterns**: `get()` for cached, `fetch()` for fresh, `watch()` for reactive
//! - **Fluent navigation**: `speaker.group()`, `group.speaker("name")`
//! - **Type safety**: All properties are strongly typed
//! - **Resource efficiency**: Shared state management and HTTP connections
//!
//! ## Available Properties
//!
//! Currently implemented:
//! - `volume` - Speaker volume (0-100)
//! - `playback_state` - Current playback state (Playing/Paused/Stopped/Transitioning)
//! - `mute` - Mute state
//! - `bass`, `treble`, `loudness` - EQ settings
//! - `position` - Current track position
//! - `current_track` - Track metadata
//!
//! ## Architecture
//!
//! ```text
//! sonos-sdk (Sync-First DOM-like API)
//!     ↓
//! sonos-state (State Management) ←→ sonos-event-manager (Event Subscriptions)
//!     ↓                                    ↓
//! sonos-api (UPnP Operations)         sonos-stream (Event Processing)
//! ```

// Main exports
pub use error::SdkError;
pub use group::{Group, GroupChangeResult};
pub use speaker::{PlayMode, SeekTarget, Speaker};
pub use system::SonosSystem;

// Re-export the generic PropertyHandle, SpeakerContext, and watch types
pub use property::{PropertyHandle, SpeakerContext, WatchMode, WatchStatus};

// Re-export group property handle types
pub use property::{
    GroupContext, GroupFetchable, GroupMuteHandle, GroupPropertyHandle,
    GroupVolumeChangeableHandle, GroupVolumeHandle,
};

// Re-export response types for action methods
pub use sonos_api::services::av_transport::{
    AddURIToQueueResponse, BecomeCoordinatorOfStandaloneGroupResponse, CreateSavedQueueResponse,
    GetCrossfadeModeResponse, GetCurrentTransportActionsResponse, GetDeviceCapabilitiesResponse,
    GetMediaInfoResponse, GetRemainingSleepTimerDurationResponse,
    GetRunningAlarmPropertiesResponse, GetTransportSettingsResponse,
    RemoveTrackRangeFromQueueResponse, SaveQueueResponse,
};
pub use sonos_api::services::group_rendering_control::SetRelativeGroupVolumeResponse;
pub use sonos_api::services::rendering_control::SetRelativeVolumeResponse;

// sonos_discovery is internal — consumers use SonosSystem::new()
// Re-exported under test-support for integration tests that need Device
#[cfg(feature = "test-support")]
pub use sonos_discovery;

// Re-export commonly used types from sonos-state
pub use sonos_state::{
    ChangeEvent, ChangeIterator, GroupId, GroupMute, GroupVolume, GroupVolumeChangeable,
    PlaybackState, SpeakerId, Volume,
};

// Public modules
pub mod prelude;

// Internal modules
mod cache;
mod error;
mod group;
pub mod property;
mod speaker;
mod system;
