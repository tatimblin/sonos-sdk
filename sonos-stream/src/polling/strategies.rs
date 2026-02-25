//! Service-specific polling strategies
//!
//! This module implements polling strategies for different UPnP services,
//! providing the actual logic for querying device state and detecting changes.

use async_trait::async_trait;
use std::collections::HashMap;
use sonos_api::{SonosClient, Service};

use crate::error::{PollingError, PollingResult};
use crate::events::types::{GroupRenderingControlEvent, ZoneGroupTopologyEvent};
use crate::registry::SpeakerServicePair;

/// Represents a state change detected by polling
#[derive(Debug, Clone)]
pub enum StateChange {
    /// Transport state changed (PLAYING, PAUSED, STOPPED, etc.)
    TransportState {
        old_state: String,
        new_state: String,
    },
    /// Current track changed
    TrackChanged {
        old_uri: String,
        new_uri: String,
    },
    /// Volume level changed
    VolumeChanged {
        old_volume: String,
        new_volume: String,
    },
    /// Mute state changed
    MuteChanged {
        old_mute: bool,
        new_mute: bool,
    },
    /// Position in current track changed significantly
    PositionChanged {
        old_position: String,
        new_position: String,
    },
    /// Generic state change (fallback for bass, treble, loudness, etc.)
    GenericChange {
        field: String,
        old_value: String,
        new_value: String,
    },
    /// Complete topology change (carries full parsed topology event)
    TopologyChanged {
        event: ZoneGroupTopologyEvent,
    },
    /// Complete group rendering control change (carries full event)
    GroupRenderingControlChanged {
        event: GroupRenderingControlEvent,
    },
}

/// Trait for service-specific polling strategies
#[async_trait]
pub trait ServicePoller: Send + Sync {
    /// Poll the device state for this service and return a comparable string representation
    async fn poll_state(&self, client: &SonosClient, pair: &SpeakerServicePair) -> PollingResult<String>;

    /// Parse state differences and return list of changes
    async fn parse_for_changes(&self, old_state: &str, new_state: &str) -> Vec<StateChange>;

    /// Get the service type this poller handles
    fn service_type(&self) -> Service;
}

/// Polling strategy for AVTransport service
pub struct AVTransportPoller;

#[async_trait]
impl ServicePoller for AVTransportPoller {
    async fn poll_state(&self, client: &SonosClient, pair: &SpeakerServicePair) -> PollingResult<String> {
        use sonos_api::services::av_transport;
        let ip = pair.speaker_ip.to_string();

        let transport_op = av_transport::get_transport_info_operation()
            .build()
            .map_err(|e| PollingError::StateParsing(format!("Failed to build operation: {}", e)))?;
        let transport_info = client.execute_enhanced(&ip, transport_op)
            .map_err(|e| PollingError::Network(e.to_string()))?;

        let position_op = av_transport::get_position_info_operation()
            .build()
            .map_err(|e| PollingError::StateParsing(format!("Failed to build operation: {}", e)))?;
        let position_info = client.execute_enhanced(&ip, position_op)
            .map_err(|e| PollingError::Network(e.to_string()))?;

        let state = AVTransportState {
            transport_state: transport_info.current_transport_state,
            current_transport_status: transport_info.current_transport_status,
            current_speed: transport_info.current_speed,
            current_track_uri: position_info.track_uri,
            track_duration: position_info.track_duration,
            track_metadata: position_info.track_meta_data,
            rel_time: position_info.rel_time,
            abs_time: position_info.abs_time,
            rel_count: position_info.rel_count as u32,
            abs_count: position_info.abs_count as u32,
        };

        serde_json::to_string(&state)
            .map_err(|e| PollingError::StateParsing(format!("Failed to serialize state: {}", e)))
    }

    async fn parse_for_changes(&self, old_state: &str, new_state: &str) -> Vec<StateChange> {
        let old: AVTransportState = match serde_json::from_str(old_state) {
            Ok(state) => state,
            Err(_) => return vec![], // Can't parse old state
        };

        let new: AVTransportState = match serde_json::from_str(new_state) {
            Ok(state) => state,
            Err(_) => return vec![], // Can't parse new state
        };

        let mut changes = Vec::new();

        // Check transport state changes
        if old.transport_state != new.transport_state {
            changes.push(StateChange::TransportState {
                old_state: old.transport_state,
                new_state: new.transport_state,
            });
        }

        // Check transport status changes (for more granular monitoring)
        if old.current_transport_status != new.current_transport_status {
            changes.push(StateChange::GenericChange {
                field: "transport_status".to_string(),
                old_value: old.current_transport_status,
                new_value: new.current_transport_status,
            });
        }

        // Check speed changes
        if old.current_speed != new.current_speed {
            changes.push(StateChange::GenericChange {
                field: "speed".to_string(),
                old_value: old.current_speed,
                new_value: new.current_speed,
            });
        }

        // Check track changes
        if old.current_track_uri != new.current_track_uri {
            changes.push(StateChange::TrackChanged {
                old_uri: old.current_track_uri,
                new_uri: new.current_track_uri,
            });
        }

        // Check position changes (only report significant changes > 5 seconds)
        if old.rel_time != new.rel_time {
            if let (Ok(old_secs), Ok(new_secs)) = (
                Self::parse_time_string(&old.rel_time),
                Self::parse_time_string(&new.rel_time)
            ) {
                if old_secs.abs_diff(new_secs) > 5 {
                    changes.push(StateChange::PositionChanged {
                        old_position: old.rel_time,
                        new_position: new.rel_time,
                    });
                }
            }
        }

        changes
    }

    fn service_type(&self) -> Service {
        Service::AVTransport
    }
}

impl AVTransportPoller {
    /// Parse a time string in HH:MM:SS format to total seconds
    pub fn parse_time_string(time_str: &str) -> Result<u32, String> {
        let parts: Vec<&str> = time_str.split(':').collect();

        if parts.len() != 3 {
            return Err(format!("Invalid time format, expected HH:MM:SS, got: {}", time_str));
        }

        let hours: u32 = parts[0].parse()
            .map_err(|_| format!("Invalid hours: {}", parts[0]))?;
        let minutes: u32 = parts[1].parse()
            .map_err(|_| format!("Invalid minutes: {}", parts[1]))?;
        let seconds: u32 = parts[2].parse()
            .map_err(|_| format!("Invalid seconds: {}", parts[2]))?;

        if minutes >= 60 || seconds >= 60 {
            return Err("Minutes and seconds must be less than 60".to_string());
        }

        Ok(hours * 3600 + minutes * 60 + seconds)
    }
}

/// Serializable state for AVTransport service
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct AVTransportState {
    pub transport_state: String,
    pub current_transport_status: String,
    pub current_speed: String,
    pub current_track_uri: String,
    pub track_duration: String,
    pub track_metadata: String,
    pub rel_time: String,
    pub abs_time: String,
    pub rel_count: u32,
    pub abs_count: u32,
}

/// Polling strategy for RenderingControl service
pub struct RenderingControlPoller;

#[async_trait]
impl ServicePoller for RenderingControlPoller {
    async fn poll_state(&self, client: &SonosClient, pair: &SpeakerServicePair) -> PollingResult<String> {
        use sonos_api::services::rendering_control;
        let ip = pair.speaker_ip.to_string();

        let volume_op = rendering_control::get_volume_operation("Master".to_string())
            .build()
            .map_err(|e| PollingError::StateParsing(format!("Failed to build operation: {}", e)))?;
        let volume_response = client.execute_enhanced(&ip, volume_op)
            .map_err(|e| PollingError::Network(e.to_string()))?;

        let mute_op = rendering_control::get_mute_operation("Master".to_string())
            .build()
            .map_err(|e| PollingError::StateParsing(format!("Failed to build operation: {}", e)))?;
        let mute_response = client.execute_enhanced(&ip, mute_op)
            .map_err(|e| PollingError::Network(e.to_string()))?;

        let bass_op = rendering_control::get_bass_operation()
            .build()
            .map_err(|e| PollingError::StateParsing(format!("Failed to build operation: {}", e)))?;
        let bass_response = client.execute_enhanced(&ip, bass_op)
            .map_err(|e| PollingError::Network(e.to_string()))?;

        let treble_op = rendering_control::get_treble_operation()
            .build()
            .map_err(|e| PollingError::StateParsing(format!("Failed to build operation: {}", e)))?;
        let treble_response = client.execute_enhanced(&ip, treble_op)
            .map_err(|e| PollingError::Network(e.to_string()))?;

        let loudness_op = rendering_control::get_loudness_operation("Master".to_string())
            .build()
            .map_err(|e| PollingError::StateParsing(format!("Failed to build operation: {}", e)))?;
        let loudness_response = client.execute_enhanced(&ip, loudness_op)
            .map_err(|e| PollingError::Network(e.to_string()))?;

        let state = RenderingControlState {
            volume: volume_response.current_volume as u16,
            mute: mute_response.current_mute,
            bass: bass_response.current_bass,
            treble: treble_response.current_treble,
            loudness: loudness_response.current_loudness,
        };

        serde_json::to_string(&state)
            .map_err(|e| PollingError::StateParsing(format!("Failed to serialize state: {}", e)))
    }

    async fn parse_for_changes(&self, old_state: &str, new_state: &str) -> Vec<StateChange> {
        let old: RenderingControlState = match serde_json::from_str(old_state) {
            Ok(state) => state,
            Err(_) => return vec![],
        };

        let new: RenderingControlState = match serde_json::from_str(new_state) {
            Ok(state) => state,
            Err(_) => return vec![],
        };

        let mut changes = Vec::new();

        if old.volume != new.volume {
            changes.push(StateChange::VolumeChanged {
                old_volume: old.volume.to_string(),
                new_volume: new.volume.to_string(),
            });
        }

        if old.mute != new.mute {
            changes.push(StateChange::MuteChanged {
                old_mute: old.mute,
                new_mute: new.mute,
            });
        }

        if old.bass != new.bass {
            changes.push(StateChange::GenericChange {
                field: "bass".to_string(),
                old_value: old.bass.to_string(),
                new_value: new.bass.to_string(),
            });
        }

        if old.treble != new.treble {
            changes.push(StateChange::GenericChange {
                field: "treble".to_string(),
                old_value: old.treble.to_string(),
                new_value: new.treble.to_string(),
            });
        }

        if old.loudness != new.loudness {
            changes.push(StateChange::GenericChange {
                field: "loudness".to_string(),
                old_value: old.loudness.to_string(),
                new_value: new.loudness.to_string(),
            });
        }

        changes
    }

    fn service_type(&self) -> Service {
        Service::RenderingControl
    }
}

/// Serializable state for RenderingControl service
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct RenderingControlState {
    pub volume: u16,
    pub mute: bool,
    #[serde(default)]
    pub bass: i8,
    #[serde(default)]
    pub treble: i8,
    #[serde(default)]
    pub loudness: bool,
}

/// Polling strategy for ZoneGroupTopology service.
///
/// Calls GetZoneGroupState and compares the raw XML string for fast change
/// detection. Only parses the XML into structured data when a change is
/// detected, using the existing parser infrastructure from sonos-api.
pub struct ZoneGroupTopologyPoller;

#[async_trait]
impl ServicePoller for ZoneGroupTopologyPoller {
    async fn poll_state(&self, client: &SonosClient, pair: &SpeakerServicePair) -> PollingResult<String> {
        use sonos_api::services::zone_group_topology;

        let op = zone_group_topology::get_zone_group_state_operation()
            .build()
            .map_err(|e| PollingError::StateParsing(format!("Failed to build operation: {}", e)))?;
        let response = client.execute_enhanced(&pair.speaker_ip.to_string(), op)
            .map_err(|e| PollingError::Network(e.to_string()))?;

        let state = ZoneGroupTopologyState {
            zone_group_state_xml: response.zone_group_state,
        };

        serde_json::to_string(&state)
            .map_err(|e| PollingError::StateParsing(format!("Failed to serialize state: {}", e)))
    }

    async fn parse_for_changes(&self, old_state: &str, new_state: &str) -> Vec<StateChange> {
        let old: ZoneGroupTopologyState = match serde_json::from_str(old_state) {
            Ok(state) => state,
            Err(_) => return vec![],
        };

        let new: ZoneGroupTopologyState = match serde_json::from_str(new_state) {
            Ok(state) => state,
            Err(_) => return vec![],
        };

        // Fast path: raw string comparison
        if old.zone_group_state_xml == new.zone_group_state_xml {
            return vec![];
        }

        // XML changed — parse into structured topology event
        match Self::parse_topology_xml(&new.zone_group_state_xml) {
            Some(event) => vec![StateChange::TopologyChanged { event }],
            None => {
                // Parse failure — emit as generic change so it's not silently lost
                vec![StateChange::GenericChange {
                    field: "zone_group_state".to_string(),
                    old_value: old.zone_group_state_xml,
                    new_value: new.zone_group_state_xml,
                }]
            }
        }
    }

    fn service_type(&self) -> Service {
        Service::ZoneGroupTopology
    }
}

impl ZoneGroupTopologyPoller {
    /// Parse raw zone group state XML into a ZoneGroupTopologyEvent.
    ///
    /// Wraps the raw XML in a UPnP propertyset envelope and delegates to the
    /// existing parser in sonos-api, then converts to sonos-stream types using
    /// the same pattern as the event processor (processor.rs:264-297).
    fn parse_topology_xml(raw_xml: &str) -> Option<ZoneGroupTopologyEvent> {
        // The raw XML from GetZoneGroupState is entity-decoded by xmltree:
        //   <ZoneGroups><ZoneGroup ...>...</ZoneGroup></ZoneGroups>
        // The from_xml() parser expects the propertyset envelope with entity-encoded content.
        // We need to: wrap in <ZoneGroupState>, entity-encode, then wrap in propertyset.
        let with_wrapper = if raw_xml.trim_start().starts_with("<ZoneGroupState>") {
            raw_xml.to_string()
        } else {
            format!("<ZoneGroupState>{}</ZoneGroupState>", raw_xml)
        };

        let encoded = with_wrapper
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;");

        let envelope = format!(
            r#"<e:propertyset xmlns:e="urn:schemas-upnp-org:event-1-0"><e:property><ZoneGroupState>{}</ZoneGroupState></e:property></e:propertyset>"#,
            encoded
        );

        let api_event = sonos_api::services::zone_group_topology::ZoneGroupTopologyEvent::from_xml(&envelope).ok()?;

        // Convert from sonos-api types to sonos-stream types
        let zone_groups = api_event.zone_groups().into_iter().map(|group| {
            let members = group.members.into_iter().map(|member| {
                let satellites = member.satellites.into_iter().map(|sat| {
                    crate::events::types::SatelliteInfo {
                        uuid: sat.uuid,
                        location: sat.location,
                        zone_name: sat.zone_name,
                        ht_sat_chan_map_set: sat.ht_sat_chan_map_set,
                        invisible: sat.invisible,
                    }
                }).collect();

                crate::events::types::ZoneGroupMemberInfo {
                    uuid: member.uuid,
                    location: member.location,
                    zone_name: member.zone_name,
                    software_version: member.software_version,
                    network_info: crate::events::types::NetworkInfo {
                        wireless_mode: member.network_info.wireless_mode,
                        wifi_enabled: member.network_info.wifi_enabled,
                        eth_link: member.network_info.eth_link,
                        channel_freq: member.network_info.channel_freq,
                        behind_wifi_extender: member.network_info.behind_wifi_extender,
                    },
                    satellites,
                }
            }).collect();

            crate::events::types::ZoneGroupInfo {
                coordinator: group.coordinator,
                id: group.id,
                members,
            }
        }).collect();

        Some(ZoneGroupTopologyEvent {
            zone_groups,
            vanished_devices: vec![],
        })
    }
}

/// Serializable state for ZoneGroupTopology service
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct ZoneGroupTopologyState {
    pub zone_group_state_xml: String,
}

/// Polling strategy for GroupManagement service.
///
/// Intentional no-op: GroupManagement has no Get operations — it's action-only
/// (AddMember, RemoveMember). Group state changes are reflected via ZoneGroupTopology
/// events. Returns stable empty state to avoid triggering error escalation in the scheduler.
pub struct GroupManagementPoller;

#[async_trait]
impl ServicePoller for GroupManagementPoller {
    async fn poll_state(&self, _client: &SonosClient, _pair: &SpeakerServicePair) -> PollingResult<String> {
        Ok("{}".to_string())
    }

    async fn parse_for_changes(&self, _old_state: &str, _new_state: &str) -> Vec<StateChange> {
        vec![]
    }

    fn service_type(&self) -> Service {
        Service::GroupManagement
    }
}

/// Polling strategy for GroupRenderingControl service.
///
/// Polls group volume and mute from the group coordinator. Operations must be
/// sent to the group coordinator — the registration layer is responsible for
/// ensuring the correct speaker is targeted.
pub struct GroupRenderingControlPoller;

#[async_trait]
impl ServicePoller for GroupRenderingControlPoller {
    async fn poll_state(&self, client: &SonosClient, pair: &SpeakerServicePair) -> PollingResult<String> {
        use sonos_api::services::group_rendering_control;
        let ip = pair.speaker_ip.to_string();

        let volume_op = group_rendering_control::get_group_volume_operation()
            .build()
            .map_err(|e| PollingError::StateParsing(format!("Failed to build operation: {}", e)))?;
        let volume_response = client.execute_enhanced(&ip, volume_op)
            .map_err(|e| PollingError::Network(e.to_string()))?;

        let mute_op = group_rendering_control::get_group_mute_operation()
            .build()
            .map_err(|e| PollingError::StateParsing(format!("Failed to build operation: {}", e)))?;
        let mute_response = client.execute_enhanced(&ip, mute_op)
            .map_err(|e| PollingError::Network(e.to_string()))?;

        let state = GroupRenderingControlState {
            group_volume: volume_response.current_volume,
            group_mute: mute_response.current_mute,
        };

        serde_json::to_string(&state)
            .map_err(|e| PollingError::StateParsing(format!("Failed to serialize state: {}", e)))
    }

    async fn parse_for_changes(&self, old_state: &str, new_state: &str) -> Vec<StateChange> {
        let old: GroupRenderingControlState = match serde_json::from_str(old_state) {
            Ok(state) => state,
            Err(_) => return vec![],
        };

        let new: GroupRenderingControlState = match serde_json::from_str(new_state) {
            Ok(state) => state,
            Err(_) => return vec![],
        };

        if old.group_volume == new.group_volume && old.group_mute == new.group_mute {
            return vec![];
        }

        // Emit a complete GroupRenderingControlEvent for the downstream consumer
        vec![StateChange::GroupRenderingControlChanged {
            event: GroupRenderingControlEvent {
                group_volume: Some(new.group_volume),
                group_mute: Some(new.group_mute),
                group_volume_changeable: None,
            },
        }]
    }

    fn service_type(&self) -> Service {
        Service::GroupRenderingControl
    }
}

/// Serializable state for GroupRenderingControl service
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct GroupRenderingControlState {
    pub group_volume: u16,
    pub group_mute: bool,
}

/// Main device state poller that coordinates different service strategies
pub struct DeviceStatePoller {
    /// Service-specific polling strategies
    service_pollers: HashMap<Service, Box<dyn ServicePoller>>,
    /// SonosClient for making requests
    sonos_client: SonosClient,
}

impl DeviceStatePoller {
    /// Create a new device state poller with all supported strategies
    pub fn new() -> Self {
        let mut service_pollers: HashMap<Service, Box<dyn ServicePoller>> = HashMap::new();

        // Register supported service pollers
        service_pollers.insert(
            Service::AVTransport,
            Box::new(AVTransportPoller),
        );
        service_pollers.insert(
            Service::RenderingControl,
            Box::new(RenderingControlPoller),
        );
        service_pollers.insert(
            Service::ZoneGroupTopology,
            Box::new(ZoneGroupTopologyPoller),
        );
        service_pollers.insert(
            Service::GroupManagement,
            Box::new(GroupManagementPoller),
        );
        service_pollers.insert(
            Service::GroupRenderingControl,
            Box::new(GroupRenderingControlPoller),
        );

        Self {
            service_pollers,
            sonos_client: SonosClient::new(),
        }
    }

    /// Poll device state for a specific speaker/service pair
    pub async fn poll_device_state(&self, pair: &SpeakerServicePair) -> PollingResult<String> {
        match self.service_pollers.get(&pair.service) {
            Some(poller) => {
                poller.poll_state(&self.sonos_client, pair).await
            }
            None => Err(PollingError::UnsupportedService {
                service: pair.service,
            }),
        }
    }

    /// Parse state changes for a specific service
    pub async fn parse_state_changes(
        &self,
        service: &Service,
        old_state: Option<&str>,
        new_state: &str,
    ) -> Option<Vec<StateChange>> {
        if let Some(old_state) = old_state {
            if let Some(poller) = self.service_pollers.get(service) {
                let changes = poller.parse_for_changes(old_state, new_state).await;
                if !changes.is_empty() {
                    return Some(changes);
                }
            }
        }
        None
    }

    /// Get list of supported service types
    pub fn supported_services(&self) -> Vec<Service> {
        self.service_pollers.keys().cloned().collect()
    }

    /// Check if a service type is supported
    pub fn is_service_supported(&self, service: &Service) -> bool {
        self.service_pollers.contains_key(service)
    }

    /// Get statistics about the device poller
    pub fn stats(&self) -> DevicePollerStats {
        DevicePollerStats {
            supported_services: self.supported_services(),
            total_pollers: self.service_pollers.len(),
        }
    }
}

/// Statistics about the device poller
#[derive(Debug)]
pub struct DevicePollerStats {
    pub supported_services: Vec<Service>,
    pub total_pollers: usize,
}

impl std::fmt::Display for DevicePollerStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Device Poller Stats:")?;
        writeln!(f, "  Total pollers: {}", self.total_pollers)?;
        writeln!(f, "  Supported services:")?;
        for service in &self.supported_services {
            writeln!(f, "    {:?}", service)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_poller_creation() {
        let poller = DeviceStatePoller::new();
        let stats = poller.stats();

        assert_eq!(stats.total_pollers, 5); // AVTransport, RenderingControl, ZoneGroupTopology, GroupManagement (no-op), GroupRenderingControl
        assert!(poller.is_service_supported(&Service::AVTransport));
        assert!(poller.is_service_supported(&Service::RenderingControl));
        assert!(poller.is_service_supported(&Service::ZoneGroupTopology));
        assert!(poller.is_service_supported(&Service::GroupManagement));
        assert!(poller.is_service_supported(&Service::GroupRenderingControl));
    }

    #[test]
    fn test_av_transport_time_parsing() {
        assert_eq!(AVTransportPoller::parse_time_string("00:01:30"), Ok(90));
        assert_eq!(AVTransportPoller::parse_time_string("01:02:03"), Ok(3723));
        assert!(AVTransportPoller::parse_time_string("invalid").is_err());
        assert!(AVTransportPoller::parse_time_string("1:2").is_err());
    }

    #[tokio::test]
    async fn test_av_transport_change_detection() {
        let poller = AVTransportPoller;

        let old_state = serde_json::to_string(&AVTransportState {
            transport_state: "PLAYING".to_string(),
            current_transport_status: "OK".to_string(),
            current_speed: "1".to_string(),
            current_track_uri: "x-rincon-queue:RINCON_123#0".to_string(),
            track_duration: "0:03:45".to_string(),
            track_metadata: "metadata".to_string(),
            rel_time: "0:01:30".to_string(),
            abs_time: "0:01:30".to_string(),
            rel_count: 1,
            abs_count: 1,
        }).unwrap();

        let new_state = serde_json::to_string(&AVTransportState {
            transport_state: "PAUSED".to_string(),
            current_transport_status: "OK".to_string(),
            current_speed: "1".to_string(),
            current_track_uri: "x-rincon-queue:RINCON_123#0".to_string(),
            track_duration: "0:03:45".to_string(),
            track_metadata: "metadata".to_string(),
            rel_time: "0:01:30".to_string(),
            abs_time: "0:01:30".to_string(),
            rel_count: 1,
            abs_count: 1,
        }).unwrap();

        let changes = poller.parse_for_changes(&old_state, &new_state).await;
        assert_eq!(changes.len(), 1);

        match &changes[0] {
            StateChange::TransportState { old_state, new_state } => {
                assert_eq!(old_state, "PLAYING");
                assert_eq!(new_state, "PAUSED");
            }
            _ => panic!("Expected TransportState change"),
        }
    }

    #[tokio::test]
    async fn test_rendering_control_change_detection() {
        let poller = RenderingControlPoller;

        let old_state = serde_json::to_string(&RenderingControlState {
            volume: 50,
            mute: false,
            bass: 0,
            treble: 0,
            loudness: true,
        }).unwrap();

        let new_state = serde_json::to_string(&RenderingControlState {
            volume: 75,
            mute: true,
            bass: 5,
            treble: -3,
            loudness: false,
        }).unwrap();

        let changes = poller.parse_for_changes(&old_state, &new_state).await;
        assert_eq!(changes.len(), 5);

        assert!(changes.iter().any(|c| matches!(c, StateChange::VolumeChanged { .. })));
        assert!(changes.iter().any(|c| matches!(c, StateChange::MuteChanged { .. })));
        assert!(changes.iter().any(|c| matches!(c, StateChange::GenericChange { field, .. } if field == "bass")));
        assert!(changes.iter().any(|c| matches!(c, StateChange::GenericChange { field, .. } if field == "treble")));
        assert!(changes.iter().any(|c| matches!(c, StateChange::GenericChange { field, .. } if field == "loudness")));
    }

    #[tokio::test]
    async fn test_rendering_control_no_change() {
        let poller = RenderingControlPoller;

        let state = serde_json::to_string(&RenderingControlState {
            volume: 50,
            mute: false,
            bass: 0,
            treble: 0,
            loudness: true,
        }).unwrap();

        let changes = poller.parse_for_changes(&state, &state).await;
        assert!(changes.is_empty());
    }

    #[tokio::test]
    async fn test_group_rendering_control_change_detection() {
        let poller = GroupRenderingControlPoller;

        let old_state = serde_json::to_string(&GroupRenderingControlState {
            group_volume: 40,
            group_mute: false,
        }).unwrap();

        let new_state = serde_json::to_string(&GroupRenderingControlState {
            group_volume: 60,
            group_mute: true,
        }).unwrap();

        let changes = poller.parse_for_changes(&old_state, &new_state).await;
        assert_eq!(changes.len(), 1);
        assert!(matches!(&changes[0], StateChange::GroupRenderingControlChanged { event }
            if event.group_volume == Some(60) && event.group_mute == Some(true)));
    }

    #[tokio::test]
    async fn test_service_poller_types() {
        let av_poller = AVTransportPoller;
        let rc_poller = RenderingControlPoller;
        let zgt_poller = ZoneGroupTopologyPoller;
        let gm_poller = GroupManagementPoller;
        let grc_poller = GroupRenderingControlPoller;

        assert_eq!(av_poller.service_type(), Service::AVTransport);
        assert_eq!(rc_poller.service_type(), Service::RenderingControl);
        assert_eq!(zgt_poller.service_type(), Service::ZoneGroupTopology);
        assert_eq!(gm_poller.service_type(), Service::GroupManagement);
        assert_eq!(grc_poller.service_type(), Service::GroupRenderingControl);
    }

    #[tokio::test]
    async fn test_zone_group_topology_change_detection() {
        let poller = ZoneGroupTopologyPoller;

        let old_state = serde_json::to_string(&ZoneGroupTopologyState {
            zone_group_state_xml: "<ZoneGroups><ZoneGroup Coordinator=\"RINCON_A\" ID=\"group1\"><ZoneGroupMember UUID=\"RINCON_A\"/></ZoneGroup></ZoneGroups>".to_string(),
        }).unwrap();

        let new_state = serde_json::to_string(&ZoneGroupTopologyState {
            zone_group_state_xml: "<ZoneGroups><ZoneGroup Coordinator=\"RINCON_B\" ID=\"group2\"><ZoneGroupMember UUID=\"RINCON_B\"/></ZoneGroup></ZoneGroups>".to_string(),
        }).unwrap();

        // Same state = no changes
        let changes = poller.parse_for_changes(&old_state, &old_state).await;
        assert!(changes.is_empty(), "Same state should produce no changes");

        // Different state = change detected (falls back to GenericChange since minimal XML won't fully parse)
        let changes = poller.parse_for_changes(&old_state, &new_state).await;
        assert_eq!(changes.len(), 1, "Different state should produce one change");
    }

    #[test]
    fn test_zone_group_topology_parse_xml() {
        // Raw XML as returned by GetZoneGroupState after entity decoding by xmltree
        let raw_xml = r#"<ZoneGroups><ZoneGroup Coordinator="RINCON_000E58C3892C01400" ID="RINCON_000E58C3892C01400:3716085098"><ZoneGroupMember UUID="RINCON_000E58C3892C01400" Location="http://192.168.1.42:1400/xml/device_description.xml" ZoneName="Living Room" SoftwareVersion="79.1-53080" WirelessMode="0" WifiEnabled="1" EthLink="0" ChannelFreq="2437" BehindWifiExtender="0"/></ZoneGroup></ZoneGroups>"#;

        let result = ZoneGroupTopologyPoller::parse_topology_xml(raw_xml);
        assert!(result.is_some(), "Should parse valid topology XML");

        let event = result.unwrap();
        assert_eq!(event.zone_groups.len(), 1);
        assert_eq!(event.zone_groups[0].coordinator, "RINCON_000E58C3892C01400");
        assert_eq!(event.zone_groups[0].members.len(), 1);
        assert_eq!(event.zone_groups[0].members[0].zone_name, "Living Room");
    }

    #[tokio::test]
    async fn test_group_management_poller_noop() {
        let poller = GroupManagementPoller;
        let pair = SpeakerServicePair {
            speaker_ip: "192.168.1.100".parse().unwrap(),
            service: Service::GroupManagement,
        };

        // No-op poller returns Ok with stable empty state
        let result = poller.poll_state(&SonosClient::new(), &pair).await;
        assert!(result.is_ok(), "GroupManagement poller should return Ok");
        assert_eq!(result.unwrap(), "{}");

        // Identical state = no changes
        let changes = poller.parse_for_changes("{}", "{}").await;
        assert!(changes.is_empty());
    }
}
