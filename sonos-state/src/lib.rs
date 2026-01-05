//! Sonos State Management
//!
//! This crate provides centralized state management for Sonos devices, tracking
//! speaker states, group topology, and emitting state change events.
//!
//! # Features
//!
//! - **State Tracking**: Track playback state, volume, mute, and group membership for all speakers
//! - **Auto-initialization**: Initialize from a single speaker IP using GetZoneGroupTopology
//! - **Change Detection**: Emit StateChange events when state changes
//! - **Pluggable Event Source**: Trait-based abstraction for event sources (not tied to sonos-stream)
//!
//! # Example
//!
//! ```rust,ignore
//! use sonos_state::{StateManager, StateManagerConfig, StateChange};
//! use std::net::IpAddr;
//!
//! // Initialize from a single speaker IP
//! let mut manager = StateManager::new(StateManagerConfig::default());
//! let ip: IpAddr = "192.168.1.100".parse().unwrap();
//! manager.initialize_from_ip(ip).unwrap();
//!
//! // Get current state
//! let snapshot = manager.snapshot();
//! for speaker in snapshot.speakers() {
//!     println!("{}: {:?}", speaker.speaker.name, speaker.playback_state);
//! }
//!
//! // Start processing events and consume state changes
//! let change_rx = manager.take_change_receiver().unwrap();
//! while let Ok(change) = change_rx.recv() {
//!     match change {
//!         StateChange::VolumeChanged { speaker_id, new_volume, .. } => {
//!             println!("Volume changed to {}", new_volume);
//!         }
//!         _ => {}
//!     }
//! }
//! ```

pub mod cache;
pub mod error;
pub mod event_receiver;
pub mod init;
pub mod manager;
pub mod model;
pub mod processor;

// Re-export main types
pub use cache::{StateCache, StateSnapshot};
pub use error::{Result, StateError};
pub use event_receiver::{
    EventReceiver, StateEvent, StateEventPayload, TopologyZoneGroup, TopologyZoneMember,
};
pub use manager::{StateManager, StateManagerConfig};
pub use model::{
    Group, GroupId, PlaybackState, Speaker, SpeakerId, SpeakerRef, SpeakerState, StateChange,
    TrackInfo,
};
pub use processor::StateProcessor;
