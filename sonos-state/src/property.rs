//! Property trait and built-in properties for Sonos state management
//!
//! Properties are the fundamental unit of state in sonos-state. Each property:
//! - Has a unique key for identification (`Property::KEY`)
//! - Belongs to a scope (Speaker, Group, or System)
//! - Is associated with a UPnP service (for subscription hints)
//! - Can be watched for changes

use serde::{Deserialize, Serialize};
use sonos_api::Service;

use crate::model::{GroupId, SpeakerInfo};

// ============================================================================
// Base Property trait
// ============================================================================

/// Marker trait for values that can be stored and watched
///
/// Properties must be:
/// - `Clone`: for copying values to watchers
/// - `Send + Sync`: for thread-safe access
/// - `PartialEq`: for change detection (only emit when the value actually changes)
/// - `'static`: for type-erased storage keyed by `TypeId`
pub trait Property: Clone + Send + Sync + PartialEq + 'static {
    /// Unique key identifying this property type
    ///
    /// Used for debugging, logging, and filtering change events. Must be unique
    /// across the properties registered in one store — e.g. `"volume"`, `"mute"`.
    const KEY: &'static str;
}

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
/// Extends the base [`Property`] trait with Sonos-specific
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

    /// Wrap this value in its [`PropertyChange`] variant, for the change event
    /// payload.
    ///
    /// Returns `None` for properties that have no variant — currently only
    /// [`Topology`], which is written wholesale through `initialize()` rather
    /// than per-property and is not watchable through the SDK. Such a property
    /// still updates the store; it just cannot be carried in a `ChangeEvent`,
    /// so no event is emitted for it.
    ///
    /// Defaulted to `None` so adding a property does not silently gain a wrong
    /// payload — a new watchable property must opt in explicitly, and the
    /// missing-variant path logs.
    ///
    /// [`PropertyChange`]: crate::decoder::PropertyChange
    fn to_change(&self) -> Option<crate::decoder::PropertyChange> {
        None
    }
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

    fn to_change(&self) -> Option<crate::decoder::PropertyChange> {
        Some(crate::decoder::PropertyChange::Volume(self.clone()))
    }
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

    fn to_change(&self) -> Option<crate::decoder::PropertyChange> {
        Some(crate::decoder::PropertyChange::Mute(self.clone()))
    }
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

    fn to_change(&self) -> Option<crate::decoder::PropertyChange> {
        Some(crate::decoder::PropertyChange::Bass(self.clone()))
    }
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

    fn to_change(&self) -> Option<crate::decoder::PropertyChange> {
        Some(crate::decoder::PropertyChange::Treble(self.clone()))
    }
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

    fn to_change(&self) -> Option<crate::decoder::PropertyChange> {
        Some(crate::decoder::PropertyChange::Loudness(self.clone()))
    }
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
// Group-scoped Properties (from GroupRenderingControl)
// ============================================================================

/// Group master volume level (0-100)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GroupVolume(pub u16);

impl Property for GroupVolume {
    const KEY: &'static str = "group_volume";
}

impl SonosProperty for GroupVolume {
    const SCOPE: Scope = Scope::Group;
    const SERVICE: Service = Service::GroupRenderingControl;

    fn to_change(&self) -> Option<crate::decoder::PropertyChange> {
        Some(crate::decoder::PropertyChange::GroupVolume(self.clone()))
    }
}

impl GroupVolume {
    pub fn new(value: u16) -> Self {
        Self(value.min(100))
    }

    pub fn value(&self) -> u16 {
        self.0
    }
}

/// Group master mute state
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GroupMute(pub bool);

impl Property for GroupMute {
    const KEY: &'static str = "group_mute";
}

impl SonosProperty for GroupMute {
    const SCOPE: Scope = Scope::Group;
    const SERVICE: Service = Service::GroupRenderingControl;

    fn to_change(&self) -> Option<crate::decoder::PropertyChange> {
        Some(crate::decoder::PropertyChange::GroupMute(self.clone()))
    }
}

impl GroupMute {
    pub fn new(muted: bool) -> Self {
        Self(muted)
    }

    pub fn is_muted(&self) -> bool {
        self.0
    }
}

/// Whether the group volume can be changed
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GroupVolumeChangeable(pub bool);

impl Property for GroupVolumeChangeable {
    const KEY: &'static str = "group_volume_changeable";
}

impl SonosProperty for GroupVolumeChangeable {
    const SCOPE: Scope = Scope::Group;
    const SERVICE: Service = Service::GroupRenderingControl;

    fn to_change(&self) -> Option<crate::decoder::PropertyChange> {
        Some(crate::decoder::PropertyChange::GroupVolumeChangeable(
            self.clone(),
        ))
    }
}

impl GroupVolumeChangeable {
    pub fn new(changeable: bool) -> Self {
        Self(changeable)
    }

    pub fn is_changeable(&self) -> bool {
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

    fn to_change(&self) -> Option<crate::decoder::PropertyChange> {
        Some(crate::decoder::PropertyChange::PlaybackState(self.clone()))
    }
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

    fn to_change(&self) -> Option<crate::decoder::PropertyChange> {
        Some(crate::decoder::PropertyChange::Position(self.clone()))
    }
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
        let millis: u64 = seconds_parts
            .get(1)
            .and_then(|m| m.parse().ok())
            .unwrap_or(0);

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

    fn to_change(&self) -> Option<crate::decoder::PropertyChange> {
        Some(crate::decoder::PropertyChange::CurrentTrack(self.clone()))
    }
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

    /// Classify where this track is coming from, based on its URI.
    ///
    /// Returns `None` when the track has no URI. See [`PlaybackSource`].
    pub fn source(&self) -> Option<PlaybackSource> {
        PlaybackSource::from_uri(self.uri.as_deref()?)
    }

    /// Get a display string for the track
    pub fn display(&self) -> String {
        match (&self.artist, &self.title) {
            (Some(artist), Some(title)) => format!("{artist} - {title}"),
            (None, Some(title)) => title.clone(),
            (Some(artist), None) => artist.clone(),
            (None, None) => "Unknown".to_string(),
        }
    }

    /// Album art as an absolute URL, resolved against the speaker that
    /// reported the track.
    ///
    /// Speakers report `album_art_uri` either as an absolute URL (typically
    /// from a streaming service) or as a path on the speaker itself, such as
    /// `/getaa?s=1&u=...`. A path is joined to `http://{speaker_ip}:1400`;
    /// an absolute URL is returned unchanged. IPv6 addresses are bracketed.
    ///
    /// ```
    /// # use sonos_state::CurrentTrack;
    /// let track = CurrentTrack {
    ///     album_art_uri: Some("/getaa?s=1&u=x-sonos-spotify".to_string()),
    ///     ..CurrentTrack::new()
    /// };
    /// assert_eq!(
    ///     track.album_art_url("192.168.1.20".parse().unwrap()).as_deref(),
    ///     Some("http://192.168.1.20:1400/getaa?s=1&u=x-sonos-spotify"),
    /// );
    /// ```
    pub fn album_art_url(&self, speaker_ip: std::net::IpAddr) -> Option<String> {
        let uri = self.album_art_uri.as_deref()?.trim();
        if uri.is_empty() {
            return None;
        }
        if uri.contains("://") {
            return Some(uri.to_string());
        }
        let origin = std::net::SocketAddr::new(speaker_ip, 1400);
        let separator = if uri.starts_with('/') { "" } else { "/" };
        Some(format!("http://{origin}{separator}{uri}"))
    }
}

impl Default for CurrentTrack {
    fn default() -> Self {
        Self::new()
    }
}

/// Where a speaker's audio is coming from, classified from a transport URI
///
/// Sonos encodes the source in the URI scheme: a grouped follower reports
/// `x-rincon:{coordinator}`, a TV input `x-sonos-htastream:`, and so on.
/// [`CurrentTrack::source`] classifies the current track's URI; pass the
/// transport URI (`GetMediaInfo`'s `CurrentURI`) to [`PlaybackSource::from_uri`]
/// to distinguish queue playback, which the track URI does not show.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PlaybackSource {
    /// Following a group coordinator (`x-rincon:`). The coordinator holds the
    /// real track, so read playback state from it instead.
    GroupFollower,
    /// Playing the queue (`x-rincon-queue:`). Only appears in transport URIs.
    Queue,
    /// Home theater input: TV, HDMI ARC or optical (`x-sonos-htastream:`).
    Tv,
    /// Analog line-in, local or from another speaker (`x-rincon-stream:`).
    LineIn,
    /// Pushed by an external app, such as AirPlay or Spotify Connect
    /// (`x-sonos-vli:`).
    VirtualLineIn,
    /// Live radio or a continuous stream (`x-sonosapi-stream:`,
    /// `x-sonosapi-radio:`, `x-sonosapi-hls:`, `x-rincon-mp3radio:`,
    /// `hls-radio:`, `aac:`).
    Stream,
    /// A single track from a music service or library (`x-sonos-spotify:`,
    /// `x-sonos-http:`, `x-sonosapi-hls-static:`, `x-file-cifs:`).
    Track,
    /// A scheme not listed above, including plain `http(s)://` URLs, which
    /// can be either a file or a stream.
    Other,
}

impl PlaybackSource {
    /// Classify a transport or track URI by its scheme.
    ///
    /// Returns `None` for an empty URI, which is what a speaker with nothing
    /// loaded reports.
    ///
    /// ```
    /// # use sonos_state::PlaybackSource;
    /// assert_eq!(
    ///     PlaybackSource::from_uri("x-rincon:RINCON_000E58A0123401400"),
    ///     Some(PlaybackSource::GroupFollower),
    /// );
    /// assert_eq!(PlaybackSource::from_uri(""), None);
    /// ```
    pub fn from_uri(uri: &str) -> Option<Self> {
        let uri = uri.trim();
        if uri.is_empty() {
            return None;
        }
        let scheme = uri.split_once(':').map_or("", |(scheme, _)| scheme);
        let source = match scheme.to_ascii_lowercase().as_str() {
            "x-rincon" => Self::GroupFollower,
            "x-rincon-queue" => Self::Queue,
            "x-sonos-htastream" => Self::Tv,
            "x-rincon-stream" => Self::LineIn,
            "x-sonos-vli" => Self::VirtualLineIn,
            "x-sonosapi-stream" | "x-sonosapi-radio" | "x-sonosapi-hls" | "x-rincon-mp3radio"
            | "hls-radio" | "aac" => Self::Stream,
            "x-sonos-spotify" | "x-sonos-http" | "x-sonosapi-hls-static" | "x-file-cifs" => {
                Self::Track
            }
            _ => Self::Other,
        };
        Some(source)
    }
}

/// Repeat and shuffle mode of the speaker's queue
///
/// Carried as `CurrentPlayMode` in AVTransport `LastChange` events and read by
/// `GetTransportSettings`. `Display` produces the UPnP string that
/// `SetPlayMode` expects, and [`PlayMode::from_upnp`] parses it back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PlayMode {
    /// Normal sequential playback
    Normal,
    /// Repeat all tracks
    RepeatAll,
    /// Repeat current track
    RepeatOne,
    /// Shuffle without repeat
    ShuffleNoRepeat,
    /// Shuffle with repeat
    Shuffle,
    /// Shuffle and repeat current track
    ShuffleRepeatOne,
}

impl Property for PlayMode {
    const KEY: &'static str = "play_mode";
}

impl SonosProperty for PlayMode {
    const SCOPE: Scope = Scope::Speaker;
    const SERVICE: Service = Service::AVTransport;

    fn to_change(&self) -> Option<crate::decoder::PropertyChange> {
        Some(crate::decoder::PropertyChange::PlayMode(*self))
    }
}

impl PlayMode {
    /// Parse a UPnP play mode string (`"SHUFFLE_NOREPEAT"`, ...)
    ///
    /// Returns `None` for a value outside the six modes Sonos documents, so a
    /// caller can skip it rather than guess.
    pub fn from_upnp(mode: &str) -> Option<Self> {
        match mode.to_ascii_uppercase().as_str() {
            "NORMAL" => Some(PlayMode::Normal),
            "REPEAT_ALL" => Some(PlayMode::RepeatAll),
            "REPEAT_ONE" => Some(PlayMode::RepeatOne),
            "SHUFFLE_NOREPEAT" => Some(PlayMode::ShuffleNoRepeat),
            "SHUFFLE" => Some(PlayMode::Shuffle),
            "SHUFFLE_REPEAT_ONE" => Some(PlayMode::ShuffleRepeatOne),
            _ => None,
        }
    }

    /// Whether tracks play in shuffled order
    pub fn is_shuffle(&self) -> bool {
        matches!(
            self,
            PlayMode::ShuffleNoRepeat | PlayMode::Shuffle | PlayMode::ShuffleRepeatOne
        )
    }

    /// Whether the queue or the current track repeats
    pub fn is_repeat(&self) -> bool {
        !matches!(self, PlayMode::Normal | PlayMode::ShuffleNoRepeat)
    }
}

impl std::fmt::Display for PlayMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlayMode::Normal => write!(f, "NORMAL"),
            PlayMode::RepeatAll => write!(f, "REPEAT_ALL"),
            PlayMode::RepeatOne => write!(f, "REPEAT_ONE"),
            PlayMode::ShuffleNoRepeat => write!(f, "SHUFFLE_NOREPEAT"),
            PlayMode::Shuffle => write!(f, "SHUFFLE"),
            PlayMode::ShuffleRepeatOne => write!(f, "SHUFFLE_REPEAT_ONE"),
        }
    }
}

/// Whether crossfade between tracks is enabled
///
/// Carried as `CurrentCrossfadeMode` (`"0"` / `"1"`) in AVTransport `LastChange`
/// events and read by `GetCrossfadeMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrossfadeMode(pub bool);

impl Property for CrossfadeMode {
    const KEY: &'static str = "crossfade_mode";
}

impl SonosProperty for CrossfadeMode {
    const SCOPE: Scope = Scope::Speaker;
    const SERVICE: Service = Service::AVTransport;

    fn to_change(&self) -> Option<crate::decoder::PropertyChange> {
        Some(crate::decoder::PropertyChange::CrossfadeMode(*self))
    }
}

impl CrossfadeMode {
    pub fn new(enabled: bool) -> Self {
        Self(enabled)
    }

    /// Parse the UPnP boolean Sonos uses (`"1"` / `"0"`, or `"true"` / `"false"`)
    ///
    /// Returns `None` for anything else.
    pub fn from_upnp(value: &str) -> Option<Self> {
        match value.trim() {
            "1" => Some(Self(true)),
            "0" => Some(Self(false)),
            v if v.eq_ignore_ascii_case("true") => Some(Self(true)),
            v if v.eq_ignore_ascii_case("false") => Some(Self(false)),
            _ => None,
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.0
    }
}

/// Sleep timer state, as of the last time it was read or set
///
/// Not evented: AVTransport events carry only `SleepTimerGeneration`, a counter
/// that changes when a timer is set or cleared, never the remaining time. The
/// value therefore comes from `GetRemainingSleepTimerDuration` (a fetch) or from
/// the SDK's own `configure_sleep_timer()`, and `remaining_ms` is a snapshot —
/// it does not count down in the cache.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SleepTimer {
    /// Remaining time in milliseconds when read, or `None` if no timer is set
    pub remaining_ms: Option<u64>,
}

impl Property for SleepTimer {
    const KEY: &'static str = "sleep_timer";
}

impl SonosProperty for SleepTimer {
    const SCOPE: Scope = Scope::Speaker;
    const SERVICE: Service = Service::AVTransport;

    fn to_change(&self) -> Option<crate::decoder::PropertyChange> {
        Some(crate::decoder::PropertyChange::SleepTimer(self.clone()))
    }
}

impl SleepTimer {
    /// No sleep timer set
    pub fn off() -> Self {
        Self { remaining_ms: None }
    }

    /// A timer with `remaining_ms` left
    pub fn remaining(remaining_ms: u64) -> Self {
        Self {
            remaining_ms: Some(remaining_ms),
        }
    }

    /// Parse the `HH:MM:SS` duration Sonos uses for sleep timers
    ///
    /// An empty or unparseable string means no timer, which is how
    /// `GetRemainingSleepTimerDuration` reports one that is not set.
    pub fn from_duration_str(duration: &str) -> Self {
        Self {
            remaining_ms: Position::parse_time_to_ms(duration),
        }
    }

    /// Whether a sleep timer is set
    pub fn is_active(&self) -> bool {
        self.remaining_ms.is_some()
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

    fn to_change(&self) -> Option<crate::decoder::PropertyChange> {
        Some(crate::decoder::PropertyChange::GroupMembership(
            self.clone(),
        ))
    }
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
    fn test_current_track_album_art_url() {
        let track = |art: Option<&str>| CurrentTrack {
            album_art_uri: art.map(str::to_string),
            ..CurrentTrack::new()
        };
        let v4 = "192.168.1.20".parse().unwrap();

        assert_eq!(
            track(Some("/getaa?s=1&u=x")).album_art_url(v4).as_deref(),
            Some("http://192.168.1.20:1400/getaa?s=1&u=x")
        );
        assert_eq!(
            track(Some("getaa?s=1")).album_art_url(v4).as_deref(),
            Some("http://192.168.1.20:1400/getaa?s=1")
        );
        assert_eq!(
            track(Some("https://i.scdn.co/image/abc"))
                .album_art_url(v4)
                .as_deref(),
            Some("https://i.scdn.co/image/abc")
        );
        assert_eq!(
            track(Some("/getaa?s=1"))
                .album_art_url("fe80::1".parse().unwrap())
                .as_deref(),
            Some("http://[fe80::1]:1400/getaa?s=1")
        );
        assert_eq!(track(None).album_art_url(v4), None);
        assert_eq!(track(Some("  ")).album_art_url(v4), None);
    }

    #[test]
    fn test_playback_source_from_uri() {
        use PlaybackSource::*;
        let cases = [
            ("x-rincon:RINCON_000E58A0123401400", GroupFollower),
            ("x-rincon-queue:RINCON_000E58A0123401400#0", Queue),
            ("x-sonos-htastream:RINCON_000E58A0123401400:spdif", Tv),
            ("x-rincon-stream:RINCON_000E58A0123401400", LineIn),
            (
                "x-sonos-vli:RINCON_000E58A0123401400:1,airplay:abc",
                VirtualLineIn,
            ),
            ("x-sonosapi-stream:s1?sid=254&flags=32", Stream),
            ("x-sonosapi-radio:ST%3aabc?sid=236", Stream),
            ("x-sonosapi-hls:Api%3atune?sid=204", Stream),
            ("x-rincon-mp3radio://stream.example.com/live", Stream),
            ("aac://http://stream.example.com/live.aac", Stream),
            ("x-sonos-spotify:spotify%3atrack%3aabc?sid=12", Track),
            (
                "x-sonosapi-hls-static:catalog%2ftracks%2fabc?sid=201",
                Track,
            ),
            ("x-file-cifs://nas/music/song.mp3", Track),
            ("X-RINCON:RINCON_000E58A0123401400", GroupFollower),
            ("https://stream.example.com/radio.mp3", Other),
            ("no-scheme", Other),
        ];
        for (uri, expected) in cases {
            assert_eq!(PlaybackSource::from_uri(uri), Some(expected), "{uri}");
        }
        assert_eq!(PlaybackSource::from_uri(""), None);
        assert_eq!(PlaybackSource::from_uri("  "), None);
    }

    #[test]
    fn test_current_track_source() {
        let follower = CurrentTrack {
            uri: Some("x-rincon:RINCON_000E58A0123401400".to_string()),
            ..CurrentTrack::new()
        };
        assert_eq!(follower.source(), Some(PlaybackSource::GroupFollower));
        assert_eq!(CurrentTrack::new().source(), None);
    }

    #[test]
    fn test_play_mode_round_trips_through_upnp_string() {
        for mode in [
            PlayMode::Normal,
            PlayMode::RepeatAll,
            PlayMode::RepeatOne,
            PlayMode::ShuffleNoRepeat,
            PlayMode::Shuffle,
            PlayMode::ShuffleRepeatOne,
        ] {
            assert_eq!(PlayMode::from_upnp(&mode.to_string()), Some(mode));
        }
        assert_eq!(PlayMode::from_upnp("shuffle"), Some(PlayMode::Shuffle));
        assert_eq!(PlayMode::from_upnp("PARTY"), None);
    }

    #[test]
    fn test_play_mode_shuffle_and_repeat_flags() {
        assert!(!PlayMode::Normal.is_shuffle() && !PlayMode::Normal.is_repeat());
        assert!(PlayMode::ShuffleNoRepeat.is_shuffle() && !PlayMode::ShuffleNoRepeat.is_repeat());
        assert!(PlayMode::Shuffle.is_shuffle() && PlayMode::Shuffle.is_repeat());
        assert!(!PlayMode::RepeatOne.is_shuffle() && PlayMode::RepeatOne.is_repeat());
    }

    #[test]
    fn test_crossfade_mode_from_upnp() {
        assert_eq!(CrossfadeMode::from_upnp("1"), Some(CrossfadeMode(true)));
        assert_eq!(CrossfadeMode::from_upnp("0"), Some(CrossfadeMode(false)));
        assert_eq!(CrossfadeMode::from_upnp("TRUE"), Some(CrossfadeMode(true)));
        assert_eq!(CrossfadeMode::from_upnp(""), None);
    }

    #[test]
    fn test_sleep_timer_from_duration_str() {
        assert_eq!(
            SleepTimer::from_duration_str("01:30:00"),
            SleepTimer::remaining(5_400_000)
        );
        assert_eq!(SleepTimer::from_duration_str(""), SleepTimer::off());
        assert!(!SleepTimer::off().is_active());
        assert!(SleepTimer::remaining(1).is_active());
    }

    #[test]
    fn test_property_constants() {
        assert_eq!(Volume::KEY, "volume");
        assert_eq!(<Volume as SonosProperty>::SCOPE, Scope::Speaker);

        assert_eq!(Topology::KEY, "topology");
        assert_eq!(<Topology as SonosProperty>::SCOPE, Scope::System);
    }

    #[test]
    fn test_group_volume_clamping() {
        assert_eq!(GroupVolume::new(50).value(), 50);
        assert_eq!(GroupVolume::new(200).value(), 100);
        assert_eq!(GroupVolume::new(0).value(), 0);
        assert_eq!(GroupVolume::new(100).value(), 100);
    }

    #[test]
    fn test_group_volume_property_metadata() {
        assert_eq!(GroupVolume::KEY, "group_volume");
        assert_eq!(<GroupVolume as SonosProperty>::SCOPE, Scope::Group);
        assert_eq!(
            <GroupVolume as SonosProperty>::SERVICE,
            Service::GroupRenderingControl
        );
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
        assert_eq!(
            <GroupMembership as SonosProperty>::SERVICE,
            Service::ZoneGroupTopology
        );
    }

    #[test]
    fn test_group_mute_property_metadata() {
        assert_eq!(GroupMute::KEY, "group_mute");
        assert_eq!(<GroupMute as SonosProperty>::SCOPE, Scope::Group);
        assert_eq!(
            <GroupMute as SonosProperty>::SERVICE,
            Service::GroupRenderingControl
        );
    }

    #[test]
    fn test_group_volume_changeable_property_metadata() {
        assert_eq!(GroupVolumeChangeable::KEY, "group_volume_changeable");
        assert_eq!(
            <GroupVolumeChangeable as SonosProperty>::SCOPE,
            Scope::Group
        );
        assert_eq!(
            <GroupVolumeChangeable as SonosProperty>::SERVICE,
            Service::GroupRenderingControl
        );
    }
}
