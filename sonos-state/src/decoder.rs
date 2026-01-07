//! Event decoder trait and types
//!
//! Decoders convert raw UPnP events into property updates. Each decoder handles
//! events from specific services (RenderingControl, AVTransport, etc.).
//!
//! # Architecture
//!
//! ```text
//! RawEvent (from sonos-stream or other source)
//!     │
//!     ▼
//! EventDecoder::decode()
//!     │
//!     ▼
//! Vec<PropertyUpdate> (closures that update StateStore)
//! ```
//!
//! Decoders are stateless pure functions - all context comes from the event
//! and the StateStore.

use std::net::IpAddr;
use std::time::SystemTime;

use sonos_api::Service;

use crate::store::StateStore;

// ============================================================================
// RawEvent
// ============================================================================

/// A raw event from an external source (sonos-stream, mock, etc.)
///
/// This is the unified event format that decoders consume. Adapters convert
/// from source-specific formats (like EnrichedEvent from sonos-stream).
#[derive(Debug, Clone)]
pub struct RawEvent {
    /// IP address of the speaker that generated this event
    pub speaker_ip: IpAddr,

    /// UPnP service this event came from
    pub service: Service,

    /// When this event occurred
    pub timestamp: SystemTime,

    /// Event payload (service-specific data)
    pub data: EventData,
}

impl RawEvent {
    /// Create a new RawEvent
    pub fn new(speaker_ip: IpAddr, service: Service, data: EventData) -> Self {
        Self {
            speaker_ip,
            service,
            timestamp: SystemTime::now(),
            data,
        }
    }

    /// Create a RawEvent with a specific timestamp
    pub fn with_timestamp(
        speaker_ip: IpAddr,
        service: Service,
        timestamp: SystemTime,
        data: EventData,
    ) -> Self {
        Self {
            speaker_ip,
            service,
            timestamp,
            data,
        }
    }
}

// ============================================================================
// EventData
// ============================================================================

/// Service-specific event data
#[derive(Debug, Clone)]
pub enum EventData {
    /// RenderingControl event (volume, mute, EQ)
    RenderingControl(RenderingControlData),

    /// AVTransport event (playback, track, position)
    AVTransport(AVTransportData),

    /// ZoneGroupTopology event (groups, speakers)
    ZoneGroupTopology(TopologyData),

    /// DeviceProperties event (name changes, etc.)
    DeviceProperties(DevicePropertiesData),
}

/// Data from RenderingControl events
#[derive(Debug, Clone, Default)]
pub struct RenderingControlData {
    /// Master volume (0-100)
    pub master_volume: Option<u8>,

    /// Master mute state
    pub master_mute: Option<bool>,

    /// Left front volume
    pub lf_volume: Option<u8>,

    /// Right front volume
    pub rf_volume: Option<u8>,

    /// Bass EQ (-10 to +10)
    pub bass: Option<i8>,

    /// Treble EQ (-10 to +10)
    pub treble: Option<i8>,

    /// Loudness compensation
    pub loudness: Option<bool>,
}

impl RenderingControlData {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_volume(mut self, volume: u8) -> Self {
        self.master_volume = Some(volume);
        self
    }

    pub fn with_mute(mut self, muted: bool) -> Self {
        self.master_mute = Some(muted);
        self
    }
}

/// Data from AVTransport events
#[derive(Debug, Clone, Default)]
pub struct AVTransportData {
    /// Transport state (PLAYING, PAUSED_PLAYBACK, STOPPED, etc.)
    pub transport_state: Option<String>,

    /// Current track URI
    pub current_track_uri: Option<String>,

    /// Track duration (HH:MM:SS format)
    pub track_duration: Option<String>,

    /// Relative time position (HH:MM:SS format)
    pub rel_time: Option<String>,

    /// Track metadata (DIDL-Lite XML)
    pub track_metadata: Option<String>,

    /// Play mode (NORMAL, REPEAT_ALL, SHUFFLE, etc.)
    pub play_mode: Option<String>,

    /// Next track URI
    pub next_track_uri: Option<String>,

    /// Next track metadata
    pub next_track_metadata: Option<String>,
}

impl AVTransportData {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_transport_state(mut self, state: impl Into<String>) -> Self {
        self.transport_state = Some(state.into());
        self
    }

    /// Check if this event has track information
    pub fn has_track_info(&self) -> bool {
        self.track_metadata.is_some() || self.current_track_uri.is_some()
    }
}

/// Data from ZoneGroupTopology events
#[derive(Debug, Clone, Default)]
pub struct TopologyData {
    /// Zone groups
    pub zone_groups: Vec<ZoneGroupData>,

    /// Vanished (offline) device UUIDs
    pub vanished_devices: Vec<String>,
}

/// A zone group from topology data
#[derive(Debug, Clone)]
pub struct ZoneGroupData {
    /// Group ID
    pub id: String,

    /// Coordinator UUID
    pub coordinator: String,

    /// Group members
    pub members: Vec<ZoneMemberData>,
}

/// A zone member from topology data
#[derive(Debug, Clone)]
pub struct ZoneMemberData {
    /// Speaker UUID
    pub uuid: String,

    /// Speaker location URL (for device description)
    pub location: String,

    /// Zone/room name
    pub zone_name: String,

    /// Software version
    pub software_version: String,

    /// IP address (parsed from location)
    pub ip_address: Option<IpAddr>,

    /// Satellite UUIDs
    pub satellites: Vec<String>,
}

/// Data from DeviceProperties events
#[derive(Debug, Clone, Default)]
pub struct DevicePropertiesData {
    /// Zone/room name
    pub zone_name: Option<String>,

    /// Icon URL
    pub icon: Option<String>,

    /// Whether invisible (hidden from UIs)
    pub invisible: Option<bool>,
}

// ============================================================================
// PropertyUpdate
// ============================================================================

/// A property update produced by a decoder
///
/// Contains a closure that applies the update to a StateStore.
/// This allows decoders to be decoupled from the store implementation.
pub struct PropertyUpdate {
    /// Description for debugging
    pub description: String,

    /// Service this update came from
    pub service: Service,

    /// Closure that applies the update
    updater: Box<dyn FnOnce(&StateStore) + Send>,
}

impl PropertyUpdate {
    /// Create a property update
    pub fn new<F>(description: impl Into<String>, service: Service, updater: F) -> Self
    where
        F: FnOnce(&StateStore) + Send + 'static,
    {
        Self {
            description: description.into(),
            service,
            updater: Box::new(updater),
        }
    }

    /// Apply this update to a store
    pub fn apply(self, store: &StateStore) {
        (self.updater)(store);
    }
}

impl std::fmt::Debug for PropertyUpdate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PropertyUpdate")
            .field("description", &self.description)
            .field("service", &self.service)
            .finish()
    }
}

// ============================================================================
// EventDecoder trait
// ============================================================================

/// Trait for decoders that convert raw events into property updates
///
/// Decoders are stateless - all context comes from the event and store.
/// Each decoder typically handles one service type.
///
/// # Example
///
/// ```rust,ignore
/// pub struct RenderingControlDecoder;
///
/// impl EventDecoder for RenderingControlDecoder {
///     fn services(&self) -> &[Service] {
///         &[Service::RenderingControl]
///     }
///
///     fn decode(&self, event: &RawEvent, store: &StateStore) -> Vec<PropertyUpdate> {
///         // Extract data, create property updates
///     }
/// }
/// ```
pub trait EventDecoder: Send + Sync {
    /// Services this decoder handles
    ///
    /// The decoder's `decode` method will only be called for events
    /// matching these services.
    fn services(&self) -> &[Service];

    /// Decode an event into property updates
    ///
    /// # Arguments
    ///
    /// * `event` - The raw event to decode
    /// * `store` - The state store (for looking up speaker IDs, etc.)
    ///
    /// # Returns
    ///
    /// A list of property updates to apply. May be empty if the event
    /// contains no relevant data.
    fn decode(&self, event: &RawEvent, store: &StateStore) -> Vec<PropertyUpdate>;

    /// Name of this decoder (for debugging/logging)
    fn name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }
}

// ============================================================================
// DIDL-Lite XML Parsing Helpers
// ============================================================================

/// Parse title from DIDL-Lite metadata
pub fn parse_didl_title(metadata: &str) -> Option<String> {
    extract_xml_value(metadata, "dc:title")
}

/// Parse artist from DIDL-Lite metadata
pub fn parse_didl_artist(metadata: &str) -> Option<String> {
    extract_xml_value(metadata, "dc:creator").or_else(|| extract_xml_value(metadata, "upnp:artist"))
}

/// Parse album from DIDL-Lite metadata
pub fn parse_didl_album(metadata: &str) -> Option<String> {
    extract_xml_value(metadata, "upnp:album")
}

/// Parse album art URI from DIDL-Lite metadata
pub fn parse_didl_album_art(metadata: &str) -> Option<String> {
    extract_xml_value(metadata, "upnp:albumArtURI")
}

/// Extract a value from an XML element
fn extract_xml_value(xml: &str, tag: &str) -> Option<String> {
    // Try standard format: <tag>value</tag>
    let start_tag = format!("<{}>", tag);
    let end_tag = format!("</{}>", tag);

    if let Some(start) = xml.find(&start_tag) {
        let start = start + start_tag.len();
        if let Some(end) = xml[start..].find(&end_tag) {
            let value = &xml[start..start + end];
            return Some(unescape_xml(value));
        }
    }

    // Try with namespace prefix variations
    let tag_name = tag.split(':').last().unwrap_or(tag);
    for prefix in &["dc:", "r:", "upnp:", ""] {
        let alt_start = format!("<{}{}>", prefix, tag_name);
        let alt_end = format!("</{}{}>", prefix, tag_name);

        if let Some(start) = xml.find(&alt_start) {
            let start = start + alt_start.len();
            if let Some(end) = xml[start..].find(&alt_end) {
                let value = &xml[start..start + end];
                return Some(unescape_xml(value));
            }
        }
    }

    None
}

/// Unescape XML entities
fn unescape_xml(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_raw_event_creation() {
        let event = RawEvent::new(
            "192.168.1.100".parse().unwrap(),
            Service::RenderingControl,
            EventData::RenderingControl(RenderingControlData::new().with_volume(50)),
        );

        assert_eq!(event.speaker_ip.to_string(), "192.168.1.100");
        assert_eq!(event.service, Service::RenderingControl);
    }

    #[test]
    fn test_rendering_control_data_builder() {
        let data = RenderingControlData::new().with_volume(75).with_mute(false);

        assert_eq!(data.master_volume, Some(75));
        assert_eq!(data.master_mute, Some(false));
    }

    #[test]
    fn test_av_transport_data_builder() {
        let data = AVTransportData::new().with_transport_state("PLAYING");

        assert_eq!(data.transport_state, Some("PLAYING".to_string()));
        assert!(!data.has_track_info());
    }

    #[test]
    fn test_didl_parsing() {
        let metadata = r#"<DIDL-Lite><item><dc:title>Test Song</dc:title><dc:creator>Test Artist</dc:creator><upnp:album>Test Album</upnp:album></item></DIDL-Lite>"#;

        assert_eq!(parse_didl_title(metadata), Some("Test Song".to_string()));
        assert_eq!(parse_didl_artist(metadata), Some("Test Artist".to_string()));
        assert_eq!(parse_didl_album(metadata), Some("Test Album".to_string()));
    }

    #[test]
    fn test_xml_unescape() {
        let metadata = r#"<dc:title>Rock &amp; Roll</dc:title>"#;
        assert_eq!(parse_didl_title(metadata), Some("Rock & Roll".to_string()));
    }

    #[test]
    fn test_property_update_debug() {
        let update = PropertyUpdate::new(
            "Set volume to 50",
            Service::RenderingControl,
            |_store| {},
        );

        let debug = format!("{:?}", update);
        assert!(debug.contains("Set volume to 50"));
    }
}
