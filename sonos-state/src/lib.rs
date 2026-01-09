//! Sonos State Management
//!
//! A lightweight, reactive state management system for Sonos devices.
//!
//! # Features
//!
//! - **Local State**: Type-safe unified store for all Sonos state
//! - **Reactive Updates**: Watch properties for changes using `tokio::sync::watch`
//! - **Event-driven**: Process events from sonos-stream or other sources
//! - **Extensible**: Add custom properties and decoders
//!
//! # Architecture
//!
//! ```text
//! External Events → Decoders → StateStore → Watchers
//!                              (queries)   (reactive)
//! ```
//!
//! # Quick Start
//!
//! ```rust,ignore
//! use sonos_state::{StateManager, Volume};
//! use sonos_discovery;
//!
//! // Create state manager with automatic event processing
//! let mut manager = StateManager::new().await?;
//! let devices = sonos_discovery::get();
//! manager.add_devices(devices).await?;
//!
//! // Watch for property changes - automatic subscription management
//! let mut volume_watcher = manager.watch_property::<Volume>(speaker_id).await?;
//! while volume_watcher.changed().await.is_ok() {
//!     if let Some(volume) = volume_watcher.current() {
//!         println!("Volume: {}%", volume.0);
//!     }
//! }
//!
//! // Or get current value without watching
//! if let Some(vol) = manager.get_property::<Volume>(&speaker_id) {
//!     println!("Current volume: {}%", vol.0);
//! }
//! ```
//!
//! # Sync Usage (CLI)
//!
//! ```rust,ignore
//! use sonos_state::{SyncWatcher, SyncWatchExt};
//!
//! let rt = tokio::runtime::Handle::current();
//! let watcher = store.sync_watch::<Volume>(&speaker_id, rt);
//!
//! // Blocking wait for changes
//! while let Some(vol) = watcher.wait() {
//!     println!("Volume: {}%", vol.0);
//! }
//! ```

// Core modules
pub mod decoder;
pub mod decoders;
pub mod model;
pub mod property;
pub mod store;
pub mod watcher;

// Internal state manager (used by reactive system)
mod state_manager;

// Reactive state manager (main interface)
pub mod reactive;

// Error types
pub mod error;

// ============================================================================
// Re-exports - New API
// ============================================================================

// Main reactive state manager
pub use reactive::StateManager;

// Store
pub use store::{StateChange, StateStore};

// Properties (commonly used)
pub use property::{
    Bass, CurrentTrack, GroupInfo, GroupMembership, Loudness, Mute, PlaybackState, Position,
    Property, Scope, Topology, Treble, Volume,
};

// Model types
pub use model::{GroupId, Speaker, SpeakerId, SpeakerInfo};

// Decoder types
pub use decoder::{
    AVTransportData, DevicePropertiesData, EventData, EventDecoder, PropertyUpdate, RawEvent,
    RenderingControlData, TopologyData, ZoneGroupData, ZoneMemberData,
};

// Decoders
pub use decoders::{AVTransportDecoder, RenderingControlDecoder, TopologyDecoder};

// Watcher utilities
pub use watcher::{SyncWatchExt, SyncWatcher};

// Reactive property watcher
pub use reactive::PropertyWatcher;

// ============================================================================
// Re-exports - Error types
// ============================================================================

pub use error::{Result, StateError};

// ============================================================================
// Prelude
// ============================================================================

/// Commonly used types for convenient importing
pub mod prelude {
    pub use crate::property::{
        Bass, CurrentTrack, GroupMembership, Loudness, Mute, PlaybackState, Position, Property,
        Scope, Topology, Treble, Volume,
    };
    pub use crate::reactive::{StateManager, PropertyWatcher};
    pub use crate::store::{StateChange, StateStore};
    pub use crate::model::{GroupId, SpeakerId, SpeakerInfo};
    pub use crate::decoder::RawEvent;
    pub use crate::watcher::{SyncWatchExt, SyncWatcher};
}
