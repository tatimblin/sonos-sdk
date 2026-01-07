//! Model types for sonos-state
//!
//! This module contains the core data types for representing Sonos devices,
//! groups, and their identifiers.

mod id_types;
mod speaker;

pub use id_types::{GroupId, SpeakerId};
pub use speaker::Speaker;

/// Alias for Speaker - used in the new property system
/// Contains static device information (ID, name, IP, model, etc.)
pub type SpeakerInfo = Speaker;
