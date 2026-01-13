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
//! # Global Change Iterator (for Applications)
//!
//! For applications that need to detect when to rerender (like TUIs), use the global change iterator:
//!
//! ```rust,ignore
//! use sonos_state::{StateManager, ChangeFilter, RerenderScope};
//!
//! let manager = StateManager::new().await?;
//! let mut changes = manager.changes_filtered(ChangeFilter::rerender_only());
//!
//! while let Some(change) = changes.next().await {
//!     match change.context.rerender_scope {
//!         RerenderScope::Full => refresh_entire_ui(),
//!         RerenderScope::Device(speaker_id) => refresh_device_ui(&speaker_id),
//!         RerenderScope::Group(group_id) => refresh_group_ui(&group_id),
//!         RerenderScope::System => refresh_status_bar(),
//!     }
//! }
//! ```
//!
//! # Ratatui Integration
//!
//! For ratatui TUI applications, use `WidgetStateManager` for efficient widget-level property watching:
//!
//! ```rust,ignore
//! use sonos_state::{StateManager, WidgetStateManager, Volume};
//!
//! let state_manager = Arc::new(StateManager::new().await?);
//! let mut widget_state = WidgetStateManager::new(Arc::clone(&state_manager)).await?;
//!
//! // In widget render functions:
//! async fn render_volume_bar(widget_state: &mut WidgetStateManager, speaker_id: &SpeakerId) {
//!     let (volume, changed) = widget_state.watch_property::<Volume>(speaker_id).await?;
//!     if changed {
//!         // Only render when volume actually changed
//!         let gauge = Gauge::default().percent(volume.unwrap_or_default().0 as u16);
//!         frame.render_widget(gauge, area);
//!     }
//! }
//!
//! // In main event loop:
//! loop {
//!     widget_state.process_global_changes(); // Process all Sonos changes
//!     if widget_state.has_any_changes() {
//!         terminal.draw(|frame| render_ui(frame, &mut widget_state))?;
//!     }
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

// Global change iterator for application rerender triggering
pub mod change_iterator;

// Error types
pub mod error;

// Logging infrastructure
pub mod logging;

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

// Global change iterator types
pub use change_iterator::{
    BlockingChangeIterator, ChangeContext, ChangeEvent, ChangeFilter, ChangeStream, ChangeType,
    ChangeTypeFilter, RerenderScope, TryRecvError, WidgetStateManager,
};

// ============================================================================
// Re-exports - Error types
// ============================================================================

pub use error::{Result, StateError};

// ============================================================================
// Re-exports - Logging
// ============================================================================

pub use logging::{LoggingError, LoggingMode, init_logging, init_logging_from_env, init_silent};

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
    pub use crate::change_iterator::{
        ChangeEvent, ChangeFilter, ChangeStream, ChangeType, RerenderScope, WidgetStateManager,
    };
}
