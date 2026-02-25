//! Canonical GroupManagement service state type.
//!
//! Used by UPnP event streaming (via `into_state()`).
//! No `poll()` function — GroupManagement is an action-only service with no Get operations.

use serde::{Deserialize, Serialize};

/// Complete GroupManagement service state.
///
/// Canonical type used by UPnP event streaming.
/// GroupManagement has no Get operations, so polling returns a stable empty state.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GroupManagementState {
    /// Whether the group coordinator is local to this device
    pub group_coordinator_is_local: Option<bool>,

    /// UUID of the local group
    pub local_group_uuid: Option<String>,

    /// Whether volume should be reset after ungrouping
    pub reset_volume_after: Option<bool>,

    /// Virtual line-in group identifier
    pub virtual_line_in_group_id: Option<String>,

    /// Volume AV transport URI for the group
    pub volume_av_transport_uri: Option<String>,
}
