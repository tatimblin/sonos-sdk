//! Service-specific polling strategies
//!
//! This module implements polling strategies for different UPnP services,
//! providing the actual logic for querying device state and detecting changes.

use async_trait::async_trait;
use std::collections::HashMap;
use sonos_api::{SonosClient, Service};

use crate::error::{PollingError, PollingResult};
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
    /// Generic state change (fallback)
    GenericChange {
        field: String,
        old_value: String,
        new_value: String,
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

        // Get transport info (current state, speed, etc.)
        let transport_op = av_transport::get_transport_info_operation()
            .build()
            .map_err(|e| PollingError::StateParsing(format!("Failed to build operation: {}", e)))?;

        let transport_info = client
            .execute_enhanced(&pair.speaker_ip.to_string(), transport_op)
            .map_err(|e| PollingError::Network(e.to_string()))?;

        // Create a comparable state representation
        let state = AVTransportState {
            transport_state: transport_info.current_transport_state,
            current_transport_status: transport_info.current_transport_status,
            current_speed: transport_info.current_speed,
            current_track_uri: "".to_string(), // TODO: Get from get_position_info_operation
            track_duration: "".to_string(),     // TODO: Get from get_position_info_operation
            track_metadata: "".to_string(),     // TODO: Get from get_position_info_operation
            rel_time: "".to_string(),           // TODO: Get from get_position_info_operation
            abs_time: "".to_string(),           // TODO: Get from get_position_info_operation
            rel_count: 0,                       // TODO: Get from get_position_info_operation
            abs_count: 0,                       // TODO: Get from get_position_info_operation
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

        // Get volume level
        let volume_op = rendering_control::get_volume_operation("Master".to_string())
            .build()
            .map_err(|e| PollingError::StateParsing(format!("Failed to build operation: {}", e)))?;

        let volume_response = client
            .execute_enhanced(&pair.speaker_ip.to_string(), volume_op)
            .map_err(|e| PollingError::Network(e.to_string()))?;

        // Create comparable state representation
        let state = RenderingControlState {
            volume: volume_response.current_volume as u16,
            mute: false, // TODO: Get from get_mute_operation
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

        // Check volume changes
        if old.volume != new.volume {
            changes.push(StateChange::VolumeChanged {
                old_volume: old.volume.to_string(),
                new_volume: new.volume.to_string(),
            });
        }

        // Check mute changes
        if old.mute != new.mute {
            changes.push(StateChange::MuteChanged {
                old_mute: old.mute,
                new_mute: new.mute,
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
}

/// Polling strategy for ZoneGroupTopology service (STUB - not yet implemented)
pub struct ZoneGroupTopologyPoller;

#[async_trait]
impl ServicePoller for ZoneGroupTopologyPoller {
    async fn poll_state(&self, _client: &SonosClient, _pair: &SpeakerServicePair) -> PollingResult<String> {
        // TODO: Implement ZoneGroupTopology polling
        // This would require:
        // 1. Adding ZoneGroupTopology operations to sonos-api crate
        // 2. Querying GetZoneGroupState operation
        // 3. Serializing the topology state for comparison
        Err(PollingError::UnsupportedService {
            service: Service::ZoneGroupTopology,
        })
    }

    async fn parse_for_changes(&self, _old_state: &str, _new_state: &str) -> Vec<StateChange> {
        // TODO: Implement topology change detection
        // This would detect:
        // - Speakers joining/leaving groups
        // - Coordinator changes
        // - Network configuration changes
        // - New/vanished devices
        vec![]
    }

    fn service_type(&self) -> Service {
        Service::ZoneGroupTopology
    }
}

/// Polling strategy for GroupManagement service.
///
/// Stub — GroupManagement doesn't currently emit events, but the poller is
/// registered so the service is included in service enumeration and will work
/// automatically if Sonos adds event support in a future firmware update.
pub struct GroupManagementPoller;

#[async_trait]
impl ServicePoller for GroupManagementPoller {
    async fn poll_state(&self, _client: &SonosClient, _pair: &SpeakerServicePair) -> PollingResult<String> {
        Err(PollingError::UnsupportedService {
            service: Service::GroupManagement,
        })
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
/// Stub — GroupRenderingControl events are received via UPnP subscriptions,
/// but the poller is registered so the service is included in service enumeration.
pub struct GroupRenderingControlPoller;

#[async_trait]
impl ServicePoller for GroupRenderingControlPoller {
    async fn poll_state(&self, _client: &SonosClient, _pair: &SpeakerServicePair) -> PollingResult<String> {
        Err(PollingError::UnsupportedService {
            service: Service::GroupRenderingControl,
        })
    }

    async fn parse_for_changes(&self, _old_state: &str, _new_state: &str) -> Vec<StateChange> {
        vec![]
    }

    fn service_type(&self) -> Service {
        Service::GroupRenderingControl
    }
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

        assert_eq!(stats.total_pollers, 5); // AVTransport, RenderingControl, ZoneGroupTopology (stub), GroupManagement (stub), GroupRenderingControl (stub)
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
        }).unwrap();

        let new_state = serde_json::to_string(&RenderingControlState {
            volume: 75,
            mute: true,
        }).unwrap();

        let changes = poller.parse_for_changes(&old_state, &new_state).await;
        assert_eq!(changes.len(), 2);

        // Check that we got both volume and mute changes
        let has_volume_change = changes.iter().any(|c| matches!(c, StateChange::VolumeChanged { .. }));
        let has_mute_change = changes.iter().any(|c| matches!(c, StateChange::MuteChanged { .. }));

        assert!(has_volume_change);
        assert!(has_mute_change);
    }

    #[tokio::test]
    async fn test_service_poller_types() {
        let av_poller = AVTransportPoller;
        let rc_poller = RenderingControlPoller;
        let zgt_poller = ZoneGroupTopologyPoller;
        let gm_poller = GroupManagementPoller;

        assert_eq!(av_poller.service_type(), Service::AVTransport);
        assert_eq!(rc_poller.service_type(), Service::RenderingControl);
        assert_eq!(zgt_poller.service_type(), Service::ZoneGroupTopology);
        assert_eq!(gm_poller.service_type(), Service::GroupManagement);
    }

    #[tokio::test]
    async fn test_zone_group_topology_poller_stub() {
        let poller = ZoneGroupTopologyPoller;
        let pair = SpeakerServicePair {
            speaker_ip: "192.168.1.100".parse().unwrap(),
            service: Service::ZoneGroupTopology,
        };

        // Test that polling returns an unsupported service error (stubbed behavior)
        let result = poller.poll_state(&SonosClient::new(), &pair).await;
        assert!(result.is_err());

        match result.unwrap_err() {
            PollingError::UnsupportedService { service } => {
                assert_eq!(service, Service::ZoneGroupTopology);
            }
            _ => panic!("Expected UnsupportedService error for stubbed ZoneGroupTopology poller"),
        }

        // Test that change parsing returns empty vec (no-op for stub)
        let changes = poller.parse_for_changes("old_state", "new_state").await;
        assert!(changes.is_empty());
    }

    #[tokio::test]
    async fn test_group_management_poller_stub() {
        let poller = GroupManagementPoller;
        let pair = SpeakerServicePair {
            speaker_ip: "192.168.1.100".parse().unwrap(),
            service: Service::GroupManagement,
        };

        let result = poller.poll_state(&SonosClient::new(), &pair).await;
        assert!(result.is_err());

        match result.unwrap_err() {
            PollingError::UnsupportedService { service } => {
                assert_eq!(service, Service::GroupManagement);
            }
            _ => panic!("Expected UnsupportedService error for stubbed GroupManagement poller"),
        }

        let changes = poller.parse_for_changes("old_state", "new_state").await;
        assert!(changes.is_empty());
    }
}
