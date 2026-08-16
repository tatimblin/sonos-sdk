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

// Change event payload — needed to match on `event.change` from `system.iter()`
pub use sonos_state::{ChangeSource, PropertyChange};
