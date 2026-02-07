//! Sonos State Management
//!
//! A sync-first state management system for Sonos devices.
//!
//! # Features
//!
//! - **Sync API**: All operations are synchronous - no async/await required
//! - **Type-safe State**: Strongly typed properties with automatic change detection
//! - **Change Events**: Blocking iterator over property changes
//! - **Watch Pattern**: Register for property changes, iterate to receive them
//!
//! # Quick Start
//!
//! ```rust,ignore
//! use sonos_state::{StateManager, Volume, SpeakerId};
//! use sonos_discovery;
//!
//! // Create state manager (sync - no .await!)
//! let manager = StateManager::new()?;
//!
//! // Add discovered devices
//! let devices = sonos_discovery::get();
//! manager.add_devices(devices)?;
//!
//! // Get current property value
//! let speaker_id = SpeakerId::new("RINCON_123");
//! if let Some(vol) = manager.get_property::<Volume>(&speaker_id) {
//!     println!("Current volume: {}%", vol.0);
//! }
//!
//! // Watch for changes
//! manager.register_watch(&speaker_id, "volume");
//!
//! // Blocking iteration over changes
//! for event in manager.iter() {
//!     println!("{} changed on {}", event.property_key, event.speaker_id);
//!     if let Some(vol) = manager.get_property::<Volume>(&event.speaker_id) {
//!         println!("New volume: {}%", vol.0);
//!     }
//! }
//! ```
//!
//! # Non-blocking Iteration
//!
//! ```rust,ignore
//! // Check for events without blocking
//! for event in manager.iter().try_iter() {
//!     println!("Event: {:?}", event);
//! }
//!
//! // Wait with timeout
//! if let Some(event) = manager.iter().recv_timeout(Duration::from_secs(1)) {
//!     println!("Got event: {:?}", event);
//! }
//! ```

// Core modules
pub mod model;
pub mod property;

// Event decoding
pub mod decoder;

// Event processing
pub(crate) mod event_worker;

// Sync-first API
pub mod state;
pub mod iter;
pub mod speaker;

// Error types
pub mod error;

// ============================================================================
// Re-exports - Main API
// ============================================================================

// State manager
pub use state::{StateManager, StateManagerBuilder, ChangeEvent};

// Change iterator
pub use iter::ChangeIterator;

// Properties
pub use property::{
    Bass, CurrentTrack, GroupInfo, GroupMembership, Loudness, Mute, PlaybackState, Position,
    Property, Scope, Topology, Treble, Volume,
};

// Model types
pub use model::{GroupId, SpeakerId, SpeakerInfo};

// Event decoder
pub use decoder::{decode_event, DecodedChanges, PropertyChange};

// Error types
pub use error::{Result, StateError};

// ============================================================================
// Prelude
// ============================================================================

/// Commonly used types for convenient importing
pub mod prelude {
    // Properties
    pub use crate::property::{
        Bass, CurrentTrack, GroupMembership, Loudness, Mute, PlaybackState, Position, Property,
        Scope, Topology, Treble, Volume,
    };

    // Model types
    pub use crate::model::{GroupId, SpeakerId, SpeakerInfo};

    // State management
    pub use crate::state::{StateManager, ChangeEvent};
    pub use crate::iter::ChangeIterator;

    // Error types
    pub use crate::error::{Result, StateError};
}
