//! Convenience re-exports for common types.
//!
//! ```rust,ignore
//! use sonos_sdk::prelude::*;
//! ```

pub use crate::error::SdkError;
pub use crate::group::Group;
pub use crate::speaker::{PlayMode, SeekTarget, Speaker};
pub use crate::system::SonosSystem;

// Property value types
pub use sonos_state::{GroupId, GroupMute, GroupVolume, PlaybackState, SpeakerId, Volume};
