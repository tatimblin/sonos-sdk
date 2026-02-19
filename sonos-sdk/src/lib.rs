//! # Sonos SDK - Sync-First API for Sonos Control
//!
//! Provides a clean, property-centric API for controlling Sonos devices.
//! All operations are **synchronous** - no async/await required.
//!
//! ## Quick Start
//!
//! ```rust,ignore
//! use sonos_sdk::SonosSystem;
//!
//! fn main() -> Result<(), sonos_sdk::SdkError> {
//!     // Create system with automatic device discovery (sync)
//!     let system = SonosSystem::new()?;
//!
//!     // Get speaker by name
//!     let speaker = system.get_speaker_by_name("Living Room")
//!         .ok_or_else(|| sonos_sdk::SdkError::SpeakerNotFound("Living Room".to_string()))?;
//!
//!     // Three methods on each property:
//!     let volume = speaker.volume.get();             // Get cached value
//!     let fresh_volume = speaker.volume.fetch()?;    // API call + update cache
//!     let current = speaker.volume.watch()?;         // Start watching for changes
//!
//!     // Iterate over changes (blocking)
//!     for event in system.iter() {
//!         println!("Changed: {} on {}", event.property_key, event.speaker_id);
//!         if event.property_key == "volume" {
//!             let new_vol = speaker.volume.get();
//!             println!("New volume: {:?}", new_vol);
//!         }
//!     }
//!
//!     Ok(())
//! }
//! ```
//!
//! ## Key Features
//!
//! - **Sync-First API**: All methods are synchronous - no async/await required
//! - **DOM-like API**: Access properties directly on speaker objects
//! - **Three access patterns**: `get()` for cached, `fetch()` for fresh, `watch()` for reactive
//! - **Automatic event management**: UPnP subscriptions managed automatically via watch/unwatch
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
pub use group::Group;
pub use speaker::Speaker;
pub use system::SonosSystem;

// Re-export the generic PropertyHandle, SpeakerContext, and watch types
pub use property::{PropertyHandle, SpeakerContext, WatchMode, WatchStatus};

// Re-export commonly used types from sonos-state
pub use sonos_state::{ChangeEvent, ChangeIterator, GroupId, PlaybackState, SpeakerId, Volume};

// Internal modules
mod error;
mod group;
pub mod property;
mod speaker;
mod system;
