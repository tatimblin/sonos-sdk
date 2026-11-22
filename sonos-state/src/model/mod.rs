//! Model types for sonos-state
//!
//! This module contains the core data types for representing Sonos devices,
//! groups, and their identifiers.

mod home_theater;
mod id_types;
mod speaker;

pub(crate) use home_theater::channels_for;
pub use home_theater::{SatelliteMember, SatelliteRole};
pub use id_types::{GroupId, SpeakerId};
pub use speaker::Speaker;

/// Alias for Speaker - used in the new property system
/// Contains static device information (ID, name, IP, model, etc.)
pub type SpeakerInfo = Speaker;
