//! Property handles for DOM-like property access
//!
//! This module provides the `PropertyHandle<P>` generic type and type aliases
//! for all supported property types.

mod handles;

// Re-export the generic PropertyHandle, SpeakerContext, and Fetchable traits
pub use handles::{Fetchable, FetchableWithContext, PropertyHandle, SpeakerContext};

// Re-export group property handle types
pub use handles::{GroupContext, GroupFetchable, GroupPropertyHandle};

// Re-export watch handle types
pub use handles::{WatchHandle, WatchMode};

// Re-export type aliases for all property handles
pub use handles::{
    BassHandle, CurrentTrackHandle, GroupMembershipHandle, GroupMuteHandle,
    GroupVolumeChangeableHandle, GroupVolumeHandle, LoudnessHandle, MuteHandle,
    PlaybackStateHandle, PositionHandle, TrebleHandle, VolumeHandle,
};
