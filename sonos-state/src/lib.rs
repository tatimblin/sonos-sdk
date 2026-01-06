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
//! use sonos_state::{StateManager, RawEvent, Volume, PlaybackState};
//!
//! // Create state manager
//! let mut manager = StateManager::new();
//!
//! // Process incoming events
//! manager.process(event);
//!
//! // Query current state
//! if let Some(vol) = manager.store().get::<Volume>(&speaker_id) {
//!     println!("Volume: {}%", vol.0);
//! }
//!
//! // Watch for changes (reactive)
//! let mut rx = manager.store().watch::<Volume>(&speaker_id);
//! tokio::spawn(async move {
//!     loop {
//!         rx.changed().await.unwrap();
//!         println!("Volume changed: {:?}", *rx.borrow());
//!     }
//! });
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
pub mod state_manager;
pub mod store;
pub mod watcher;

// Error types
pub mod error;

// ============================================================================
// Re-exports - New API
// ============================================================================

// State Manager (main entry point)
pub use state_manager::StateManager;

// Store
pub use store::{StateChange, StateStore};

// Properties (commonly used)
pub use property::{
    Bass, CurrentTrack, GroupInfo, GroupMembership, Loudness, Mute, PlaybackState, Position,
    Property, Scope, Topology, Treble, Volume,
};

// Model types
pub use model::{Group, GroupId, Speaker, SpeakerId, SpeakerInfo, SpeakerRef};

// Decoder types
pub use decoder::{
    AVTransportData, DevicePropertiesData, EventData, EventDecoder, PropertyUpdate, RawEvent,
    RenderingControlData, TopologyData, ZoneGroupData, ZoneMemberData,
};

// Decoders
pub use decoders::{AVTransportDecoder, RenderingControlDecoder, TopologyDecoder};

// Watcher utilities
pub use watcher::{SyncWatchExt, SyncWatcher};

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
    pub use crate::state_manager::StateManager;
    pub use crate::store::{StateChange, StateStore};
    pub use crate::model::{GroupId, SpeakerId, SpeakerInfo};
    pub use crate::decoder::RawEvent;
    pub use crate::watcher::{SyncWatchExt, SyncWatcher};
}
