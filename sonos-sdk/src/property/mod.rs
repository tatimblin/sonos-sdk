//! Property handles for DOM-like property access
//!
//! This module provides the `PropertyHandle<P>` generic type and type aliases
//! for all supported property types.

mod handles;

// Re-export the generic PropertyHandle, SpeakerContext, and Fetchable trait
pub use handles::{Fetchable, PropertyHandle, SpeakerContext};

// Re-export type aliases for all property handles
pub use handles::{
    BassHandle, CurrentTrackHandle, GroupMembershipHandle, LoudnessHandle, MuteHandle,
    PlaybackStateHandle, PositionHandle, TrebleHandle, VolumeHandle,
};
