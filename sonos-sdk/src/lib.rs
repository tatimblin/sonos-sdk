//! # Sonos SDK - DOM-like API for Sonos Control
//!
//! Provides a clean, property-centric API for controlling Sonos devices:
//!
//! ```rust,no_run
//! use sonos_sdk::SonosSystem;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), sonos_sdk::SdkError> {
//!     let system = SonosSystem::new().await?;
//!     let speaker = system.get_speaker_by_name("Living Room").await
//!         .ok_or_else(|| sonos_sdk::SdkError::SpeakerNotFound("Living Room".to_string()))?;
//!
//!     // Three methods on each property:
//!     let volume = speaker.volume.get();                    // Get cached value
//!     let fresh_volume = speaker.volume.fetch().await?;     // API call + update cache
//!     let mut watcher = speaker.volume.watch().await?;      // UPnP event stream
//!
//!     // PropertyWatcher provides reactive updates:
//!     println!("Current volume: {:?}", watcher.current());
//!     watcher.changed().await.ok(); // Wait for next change
//!     println!("Volume changed to: {:?}", watcher.current());
//!
//!     Ok(())
//! }
//! ```
//!
//! ## Key Features
//!
//! - **DOM-like API**: Access properties directly on speaker objects
//! - **Three access patterns**: `get()` for cached values, `fetch()` for fresh API calls, `watch()` for reactive updates
//! - **Automatic state management**: `fetch()` updates the reactive state system automatically
//! - **UPnP event streaming**: `watch()` returns PropertyWatcher with automatic subscription management
//! - **Type safety**: All properties are strongly typed with compile-time correctness
//! - **Resource efficiency**: Shared state management and HTTP connections
//!
//! ## Available Properties
//!
//! Currently implemented:
//! - `volume` - Speaker volume (0-100)
//! - `playback_state` - Current playback state (Playing/Paused/Stopped/Transitioning)
//!
//! Coming soon:
//! - `mute` - Mute state
//! - `position` - Current track position
//! - `current_track` - Track metadata
//! - `bass`, `treble`, `loudness` - EQ settings
//!
//! ## Architecture
//!
//! This SDK builds on top of the existing sonos-state reactive architecture:
//!
//! ```text
//! sonos-sdk (DOM-like API)
//!     ↓
//! Property Handles (get/fetch/watch)
//!     ↓
//! sonos-state (Reactive State Management)
//!     ↓
//! sonos-api (Direct UPnP Operations)
//! ```

// Main exports
pub use system::SonosSystem;
pub use speaker::Speaker;
pub use error::SdkError;

// Re-export commonly used types from sonos-state
pub use sonos_state::{Volume, PlaybackState, PropertyWatcher, SpeakerId};

// Internal modules
mod error;
mod system;
mod speaker;
mod property;