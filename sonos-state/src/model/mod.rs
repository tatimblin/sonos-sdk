//! Model types for sonos-state
//!
//! This module contains the core data types for representing Sonos devices,
//! groups, and their identifiers.

mod group;
mod group_id;
mod playback_state;
mod speaker;
mod speaker_id;
mod speaker_state;
mod state_change;
mod track_info;

pub use group::{Group, SpeakerRef};
pub use group_id::GroupId;
pub use playback_state::PlaybackState;
pub use speaker::Speaker;
pub use speaker_id::SpeakerId;
pub use speaker_state::SpeakerState;
pub use state_change::StateChange;
pub use track_info::TrackInfo;

/// Alias for Speaker - used in the new property system
/// Contains static device information (ID, name, IP, model, etc.)
pub type SpeakerInfo = Speaker;
