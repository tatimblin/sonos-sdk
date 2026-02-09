//! Property trait and built-in properties for Sonos state management
//!
//! Properties are the fundamental unit of state in sonos-state. Each property:
//! - Has a unique key for identification (from state-store::Property)
//! - Belongs to a scope (Speaker, Group, or System)
//! - Is associated with a UPnP service (for subscription hints)
//! - Can be watched for changes

use serde::{Deserialize, Serialize};
use sonos_api::Service;

use crate::model::{GroupId, SpeakerInfo};

// Re-export the base Property trait from state-store
pub use state_store::Property;

// ============================================================================
// Sonos-specific Extensions
// ============================================================================

/// Scope of a property - determines where it's stored and how it's queried
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Scope {
    /// Property belongs to individual speakers (e.g., volume, mute)
    Speaker,
    /// Property belongs to groups/zones (e.g., group playback state)
    Group,
    /// Property is system-wide (e.g., topology, alarms)
    System,
}

/// Extension trait for Sonos-specific property metadata
///
/// Extends the base `state_store::Property` trait with Sonos-specific
/// information about scope and UPnP service.
///
/// # Example
///
/// ```rust,ignore
/// #[derive(Clone, PartialEq, Debug)]
/// pub struct Volume(pub u8);
///
/// impl Property for Volume {
///     const KEY: &'static str = "volume";
/// }
///
/// impl SonosProperty for Volume {
///     const SCOPE: Scope = Scope::Speaker;
///     const SERVICE: Service = Service::RenderingControl;
/// }
/// ```
pub trait SonosProperty: Property {
    /// Scope of this property
    const SCOPE: Scope;

    /// UPnP service this property comes from
    ///
    /// Used for subscription hints - to know which services need subscriptions
    /// when this property is being watched.
    const SERVICE: Service;
}

// ============================================================================
// Speaker-scoped Properties (from RenderingControl)
// ============================================================================

/// Master volume level (0-100)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Volume(pub u8);

impl Property for Volume {
    const KEY: &'static str = "volume";
}

impl SonosProperty for Volume {
    const SCOPE: Scope = Scope::Speaker;
    const SERVICE: Service = Service::RenderingControl;
}

impl Volume {
    pub fn new(value: u8) -> Self {
        Self(value.min(100))
    }

    pub fn value(&self) -> u8 {
        self.0
    }
}

/// Master mute state
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Mute(pub bool);

impl Property for Mute {
    const KEY: &'static str = "mute";
}

impl SonosProperty for Mute {
    const SCOPE: Scope = Scope::Speaker;
    const SERVICE: Service = Service::RenderingControl;
}

impl Mute {
    pub fn new(muted: bool) -> Self {
        Self(muted)
    }

    pub fn is_muted(&self) -> bool {
        self.0
    }
}

/// Bass EQ setting (-10 to +10)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bass(pub i8);

impl Property for Bass {
    const KEY: &'static str = "bass";
}

impl SonosProperty for Bass {
    const SCOPE: Scope = Scope::Speaker;
    const SERVICE: Service = Service::RenderingControl;
}

impl Bass {
    pub fn new(value: i8) -> Self {
        Self(value.clamp(-10, 10))
    }

    pub fn value(&self) -> i8 {
        self.0
    }
}

/// Treble EQ setting (-10 to +10)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Treble(pub i8);

impl Property for Treble {
    const KEY: &'static str = "treble";
}

impl SonosProperty for Treble {
    const SCOPE: Scope = Scope::Speaker;
    const SERVICE: Service = Service::RenderingControl;
}

impl Treble {
    pub fn new(value: i8) -> Self {
        Self(value.clamp(-10, 10))
    }

    pub fn value(&self) -> i8 {
        self.0
    }
}

/// Loudness compensation setting
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Loudness(pub bool);

impl Property for Loudness {
    const KEY: &'static str = "loudness";
}

impl SonosProperty for Loudness {
    const SCOPE: Scope = Scope::Speaker;
    const SERVICE: Service = Service::RenderingControl;
}

impl Loudness {
    pub fn new(enabled: bool) -> Self {
        Self(enabled)
    }

    pub fn is_enabled(&self) -> bool {
        self.0
    }
}

// ============================================================================
// Speaker-scoped Properties (from AVTransport)
// ============================================================================

/// Current playback state
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PlaybackState {
    Playing,
    Paused,
    Stopped,
    Transitioning,
}

impl Property for PlaybackState {
    const KEY: &'static str = "playback_state";
}

impl SonosProperty for PlaybackState {
    const SCOPE: Scope = Scope::Speaker;
    const SERVICE: Service = Service::AVTransport;
}

impl PlaybackState {
    /// Parse from UPnP transport state string
    pub fn from_transport_state(state: &str) -> Self {
        match state.to_uppercase().as_str() {
            "PLAYING" => PlaybackState::Playing,
            "PAUSED_PLAYBACK" | "PAUSED" => PlaybackState::Paused,
            "STOPPED" => PlaybackState::Stopped,
            "TRANSITIONING" => PlaybackState::Transitioning,
            _ => PlaybackState::Stopped,
        }
    }

    pub fn is_playing(&self) -> bool {
        matches!(self, PlaybackState::Playing)
    }

    pub fn is_paused(&self) -> bool {
        matches!(self, PlaybackState::Paused)
    }

    pub fn is_stopped(&self) -> bool {
        matches!(self, PlaybackState::Stopped)
    }
}

/// Current playback position and duration
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Position {
    /// Current position in milliseconds
    pub position_ms: u64,
    /// Total duration in milliseconds
    pub duration_ms: u64,
}

impl Property for Position {
    const KEY: &'static str = "position";
}

impl SonosProperty for Position {
    const SCOPE: Scope = Scope::Speaker;
    const SERVICE: Service = Service::AVTransport;
}

impl Position {
    pub fn new(position_ms: u64, duration_ms: u64) -> Self {
        Self {
            position_ms,
            duration_ms,
        }
    }

    /// Get position as a fraction (0.0 to 1.0)
    pub fn progress(&self) -> f64 {
        if self.duration_ms == 0 {
            0.0
        } else {
            (self.position_ms as f64) / (self.duration_ms as f64)
        }
    }

    /// Parse time string (HH:MM:SS or HH:MM:SS.mmm) to milliseconds
    pub fn parse_time_to_ms(time_str: &str) -> Option<u64> {
        if !time_str.contains(':') {
            return None;
        }

        let parts: Vec<&str> = time_str.split(':').collect();
        if parts.len() != 3 {
            return None;
        }

        let hours: u64 = parts[0].parse().ok()?;
        let minutes: u64 = parts[1].parse().ok()?;

        let seconds_parts: Vec<&str> = parts[2].split('.').collect();
        let seconds: u64 = seconds_parts[0].parse().ok()?;
        let millis: u64 = seconds_parts.get(1).and_then(|m| m.parse().ok()).unwrap_or(0);

        Some((hours * 3600 + minutes * 60 + seconds) * 1000 + millis)
    }
}

/// Information about the currently playing track
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CurrentTrack {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub album_art_uri: Option<String>,
    pub uri: Option<String>,
}

impl Property for CurrentTrack {
    const KEY: &'static str = "current_track";
}

impl SonosProperty for CurrentTrack {
    const SCOPE: Scope = Scope::Speaker;
    const SERVICE: Service = Service::AVTransport;
}

impl CurrentTrack {
    pub fn new() -> Self {
        Self {
            title: None,
            artist: None,
            album: None,
            album_art_uri: None,
            uri: None,
        }
    }

    /// Check if the track has any meaningful content
    pub fn is_empty(&self) -> bool {
        self.title.is_none() && self.artist.is_none() && self.uri.is_none()
    }

    /// Get a display string for the track
    pub fn display(&self) -> String {
        match (&self.artist, &self.title) {
            (Some(artist), Some(title)) => format!("{} - {}", artist, title),
            (None, Some(title)) => title.clone(),
            (Some(artist), None) => artist.clone(),
            (None, None) => "Unknown".to_string(),
        }
    }
}

impl Default for CurrentTrack {
    fn default() -> Self {
        Self::new()
    }
}

/// Speaker's group membership
///
/// Every speaker is always in a group - a single speaker forms a group of one.
/// The group_id is always present and valid.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GroupMembership {
    /// ID of the group this speaker belongs to (always present)
    pub group_id: GroupId,
    /// Whether this speaker is the coordinator (master) of its group
    pub is_coordinator: bool,
}

impl Property for GroupMembership {
    const KEY: &'static str = "group_membership";
}

impl SonosProperty for GroupMembership {
    const SCOPE: Scope = Scope::Speaker;
    const SERVICE: Service = Service::ZoneGroupTopology;
}

impl GroupMembership {
    /// Create a new GroupMembership with the given group ID and coordinator status
    pub fn new(group_id: GroupId, is_coordinator: bool) -> Self {
        Self {
            group_id,
            is_coordinator,
        }
    }
}

// ============================================================================
// System-scoped Properties
// ============================================================================

/// System-wide topology of all speakers and groups
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Topology {
    pub speakers: Vec<SpeakerInfo>,
    pub groups: Vec<GroupInfo>,
}

impl Property for Topology {
    const KEY: &'static str = "topology";
}

impl SonosProperty for Topology {
    const SCOPE: Scope = Scope::System;
    const SERVICE: Service = Service::ZoneGroupTopology;
}

impl Topology {
    pub fn new(speakers: Vec<SpeakerInfo>, groups: Vec<GroupInfo>) -> Self {
        Self { speakers, groups }
    }

    pub fn empty() -> Self {
        Self {
            speakers: vec![],
            groups: vec![],
        }
    }

    pub fn speaker_count(&self) -> usize {
        self.speakers.len()
    }

    pub fn group_count(&self) -> usize {
        self.groups.len()
    }
}

impl Default for Topology {
    fn default() -> Self {
        Self::empty()
    }
}

/// Group information for topology
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GroupInfo {
    pub id: GroupId,
    pub coordinator_id: crate::model::SpeakerId,
    pub member_ids: Vec<crate::model::SpeakerId>,
}

impl GroupInfo {
    pub fn new(
        id: GroupId,
        coordinator_id: crate::model::SpeakerId,
        member_ids: Vec<crate::model::SpeakerId>,
    ) -> Self {
        Self {
            id,
            coordinator_id,
            member_ids,
        }
    }

    pub fn is_standalone(&self) -> bool {
        self.member_ids.len() == 1
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_volume_clamping() {
        assert_eq!(Volume::new(50).value(), 50);
        assert_eq!(Volume::new(150).value(), 100);
        assert_eq!(Volume::new(0).value(), 0);
    }

    #[test]
    fn test_bass_clamping() {
        assert_eq!(Bass::new(0).value(), 0);
        assert_eq!(Bass::new(-15).value(), -10);
        assert_eq!(Bass::new(15).value(), 10);
    }

    #[test]
    fn test_playback_state_parsing() {
        assert_eq!(
            PlaybackState::from_transport_state("PLAYING"),
            PlaybackState::Playing
        );
        assert_eq!(
            PlaybackState::from_transport_state("PAUSED_PLAYBACK"),
            PlaybackState::Paused
        );
        assert_eq!(
            PlaybackState::from_transport_state("STOPPED"),
            PlaybackState::Stopped
        );
        assert_eq!(
            PlaybackState::from_transport_state("unknown"),
            PlaybackState::Stopped
        );
    }

    #[test]
    fn test_position_progress() {
        let pos = Position::new(30_000, 180_000); // 30s / 3min
        assert!((pos.progress() - 0.1667).abs() < 0.001);

        let zero_duration = Position::new(1000, 0);
        assert_eq!(zero_duration.progress(), 0.0);
    }

    #[test]
    fn test_position_time_parsing() {
        assert_eq!(Position::parse_time_to_ms("0:00:00"), Some(0));
        assert_eq!(Position::parse_time_to_ms("0:01:00"), Some(60_000));
        assert_eq!(Position::parse_time_to_ms("1:00:00"), Some(3_600_000));
        assert_eq!(Position::parse_time_to_ms("0:03:45"), Some(225_000));
        assert_eq!(Position::parse_time_to_ms("0:03:45.500"), Some(225_500));
        assert_eq!(Position::parse_time_to_ms("NOT_IMPLEMENTED"), None);
    }

    #[test]
    fn test_current_track_display() {
        let track = CurrentTrack {
            title: Some("Song".to_string()),
            artist: Some("Artist".to_string()),
            album: None,
            album_art_uri: None,
            uri: None,
        };
        assert_eq!(track.display(), "Artist - Song");

        let title_only = CurrentTrack {
            title: Some("Song".to_string()),
            artist: None,
            album: None,
            album_art_uri: None,
            uri: None,
        };
        assert_eq!(title_only.display(), "Song");
    }

    #[test]
    fn test_property_constants() {
        assert_eq!(Volume::KEY, "volume");
        assert_eq!(<Volume as SonosProperty>::SCOPE, Scope::Speaker);

        assert_eq!(Topology::KEY, "topology");
        assert_eq!(<Topology as SonosProperty>::SCOPE, Scope::System);
    }

    #[test]
    fn test_group_membership_always_has_valid_group_id() {
        // GroupMembership always requires a valid GroupId
        let group_id = GroupId::new("RINCON_12345:1");
        let membership = GroupMembership::new(group_id.clone(), true);
        
        // Verify group_id is always present and matches what was provided
        assert_eq!(membership.group_id, group_id);
        assert!(!membership.group_id.as_str().is_empty());
    }

    #[test]
    fn test_group_membership_is_coordinator_flag() {
        let group_id = GroupId::new("RINCON_12345:1");
        
        // Test coordinator
        let coordinator = GroupMembership::new(group_id.clone(), true);
        assert!(coordinator.is_coordinator);
        
        // Test non-coordinator (member)
        let member = GroupMembership::new(group_id.clone(), false);
        assert!(!member.is_coordinator);
    }

    #[test]
    fn test_group_membership_equality() {
        let group_id = GroupId::new("RINCON_12345:1");
        
        let membership1 = GroupMembership::new(group_id.clone(), true);
        let membership2 = GroupMembership::new(group_id.clone(), true);
        let membership3 = GroupMembership::new(group_id.clone(), false);
        let membership4 = GroupMembership::new(GroupId::new("RINCON_67890:1"), true);
        
        // Same group_id and is_coordinator should be equal
        assert_eq!(membership1, membership2);
        
        // Different is_coordinator should not be equal
        assert_ne!(membership1, membership3);
        
        // Different group_id should not be equal
        assert_ne!(membership1, membership4);
    }

    #[test]
    fn test_group_membership_property_metadata() {
        assert_eq!(GroupMembership::KEY, "group_membership");
        assert_eq!(<GroupMembership as SonosProperty>::SCOPE, Scope::Speaker);
        assert_eq!(<GroupMembership as SonosProperty>::SERVICE, Service::ZoneGroupTopology);
    }
}
