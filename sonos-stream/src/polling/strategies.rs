//! Service-specific polling strategies
//!
//! Each poller delegates to sonos-api's `poll()` function, producing the canonical
//! State type as JSON. The scheduler compares JSON snapshots for change detection
//! and emits full-state events (same type as UPnP event streaming).
//!
//! Blocking I/O: sonos-api uses `ureq` (blocking HTTP). All real pollers wrap
//! `poll()` calls in `tokio::task::spawn_blocking` to avoid starving the async runtime.

use async_trait::async_trait;
use sonos_api::{Service, SonosClient};
use std::collections::HashMap;

use crate::error::{PollingError, PollingResult};
use crate::events::types::EventData;
use crate::registry::SpeakerServicePair;

/// Trait for service-specific polling strategies.
///
/// Each poller returns a JSON snapshot string and can convert it back to EventData.
#[async_trait]
pub trait ServicePoller: Send + Sync {
    /// Poll the device state and return a JSON snapshot for comparison.
    async fn poll_state(
        &self,
        client: &SonosClient,
        pair: &SpeakerServicePair,
    ) -> PollingResult<String>;

    /// Convert a JSON state snapshot back into an EventData variant.
    fn state_to_event_data(&self, json_state: &str) -> PollingResult<EventData>;

    /// Get the service type this poller handles.
    fn service_type(&self) -> Service;
}

/// Polling strategy for AVTransport service.
///
/// Delegates to `sonos_api::services::av_transport::state::poll()`.
pub struct AVTransportPoller;

#[async_trait]
impl ServicePoller for AVTransportPoller {
    async fn poll_state(
        &self,
        client: &SonosClient,
        pair: &SpeakerServicePair,
    ) -> PollingResult<String> {
        let client = client.clone();
        let ip = pair.speaker_ip.to_string();

        let state = tokio::task::spawn_blocking(move || {
            sonos_api::services::av_transport::state::poll(&client, &ip)
        })
        .await
        .map_err(|e| PollingError::Network(format!("Polling task panicked: {e}")))?
        .map_err(|e| PollingError::Network(e.to_string()))?;

        serde_json::to_string(&state)
            .map_err(|e| PollingError::StateParsing(format!("Failed to serialize state: {e}")))
    }

    fn state_to_event_data(&self, json_state: &str) -> PollingResult<EventData> {
        let state: sonos_api::services::av_transport::state::AVTransportState =
            serde_json::from_str(json_state).map_err(|e| {
                PollingError::StateParsing(format!(
                    "Failed to deserialize AVTransport state: {e}"
                ))
            })?;
        Ok(EventData::AVTransport(state))
    }

    fn service_type(&self) -> Service {
        Service::AVTransport
    }
}

/// Polling strategy for RenderingControl service.
///
/// Delegates to `sonos_api::services::rendering_control::state::poll()`.
pub struct RenderingControlPoller;

#[async_trait]
impl ServicePoller for RenderingControlPoller {
    async fn poll_state(
        &self,
        client: &SonosClient,
        pair: &SpeakerServicePair,
    ) -> PollingResult<String> {
        let client = client.clone();
        let ip = pair.speaker_ip.to_string();

        let state = tokio::task::spawn_blocking(move || {
            sonos_api::services::rendering_control::state::poll(&client, &ip)
        })
        .await
        .map_err(|e| PollingError::Network(format!("Polling task panicked: {e}")))?
        .map_err(|e| PollingError::Network(e.to_string()))?;

        serde_json::to_string(&state)
            .map_err(|e| PollingError::StateParsing(format!("Failed to serialize state: {e}")))
    }

    fn state_to_event_data(&self, json_state: &str) -> PollingResult<EventData> {
        let state: sonos_api::services::rendering_control::state::RenderingControlState =
            serde_json::from_str(json_state).map_err(|e| {
                PollingError::StateParsing(format!(
                    "Failed to deserialize RenderingControl state: {e}"
                ))
            })?;
        Ok(EventData::RenderingControl(state))
    }

    fn service_type(&self) -> Service {
        Service::RenderingControl
    }
}

/// Polling strategy for ZoneGroupTopology service.
///
/// Delegates to `sonos_api::services::zone_group_topology::state::poll()`.
pub struct ZoneGroupTopologyPoller;

#[async_trait]
impl ServicePoller for ZoneGroupTopologyPoller {
    async fn poll_state(
        &self,
        client: &SonosClient,
        pair: &SpeakerServicePair,
    ) -> PollingResult<String> {
        let client = client.clone();
        let ip = pair.speaker_ip.to_string();

        let state = tokio::task::spawn_blocking(move || {
            sonos_api::services::zone_group_topology::state::poll(&client, &ip)
        })
        .await
        .map_err(|e| PollingError::Network(format!("Polling task panicked: {e}")))?
        .map_err(|e| PollingError::Network(e.to_string()))?;

        serde_json::to_string(&state)
            .map_err(|e| PollingError::StateParsing(format!("Failed to serialize state: {e}")))
    }

    fn state_to_event_data(&self, json_state: &str) -> PollingResult<EventData> {
        let state: sonos_api::services::zone_group_topology::state::ZoneGroupTopologyState =
            serde_json::from_str(json_state).map_err(|e| {
                PollingError::StateParsing(format!(
                    "Failed to deserialize ZoneGroupTopology state: {e}"
                ))
            })?;
        Ok(EventData::ZoneGroupTopology(state))
    }

    fn service_type(&self) -> Service {
        Service::ZoneGroupTopology
    }
}

/// Polling strategy for GroupManagement service.
///
/// GroupManagement is an action-only service with no Get operations.
/// Returns a stable empty JSON state (`"{}"`), so the scheduler never detects
/// changes after the initial poll.
///
/// **Startup behavior:** The first poll emits a single `GroupManagement` event
/// with all fields set to `None` (since `"{}"` deserializes to an all-`None`
/// `GroupManagementState`). This is benign — the sonos-state decoder ignores
/// `GroupManagement` events (returns an empty change set).
pub struct GroupManagementPoller;

#[async_trait]
impl ServicePoller for GroupManagementPoller {
    async fn poll_state(
        &self,
        _client: &SonosClient,
        _pair: &SpeakerServicePair,
    ) -> PollingResult<String> {
        // No Get operations — return stable empty state
        Ok("{}".to_string())
    }

    fn state_to_event_data(&self, json_state: &str) -> PollingResult<EventData> {
        let state: sonos_api::services::group_management::state::GroupManagementState =
            serde_json::from_str(json_state).map_err(|e| {
                PollingError::StateParsing(format!(
                    "Failed to deserialize GroupManagement state: {e}"
                ))
            })?;
        Ok(EventData::GroupManagement(state))
    }

    fn service_type(&self) -> Service {
        Service::GroupManagement
    }
}

/// Polling strategy for GroupRenderingControl service.
///
/// Delegates to `sonos_api::services::group_rendering_control::state::poll()`.
pub struct GroupRenderingControlPoller;

#[async_trait]
impl ServicePoller for GroupRenderingControlPoller {
    async fn poll_state(
        &self,
        client: &SonosClient,
        pair: &SpeakerServicePair,
    ) -> PollingResult<String> {
        let client = client.clone();
        let ip = pair.speaker_ip.to_string();

        let state = tokio::task::spawn_blocking(move || {
            sonos_api::services::group_rendering_control::state::poll(&client, &ip)
        })
        .await
        .map_err(|e| PollingError::Network(format!("Polling task panicked: {e}")))?
        .map_err(|e| PollingError::Network(e.to_string()))?;

        serde_json::to_string(&state)
            .map_err(|e| PollingError::StateParsing(format!("Failed to serialize state: {e}")))
    }

    fn state_to_event_data(&self, json_state: &str) -> PollingResult<EventData> {
        let state: sonos_api::services::group_rendering_control::state::GroupRenderingControlState =
            serde_json::from_str(json_state).map_err(|e| {
                PollingError::StateParsing(format!(
                    "Failed to deserialize GroupRenderingControl state: {e}"
                ))
            })?;
        Ok(EventData::GroupRenderingControl(state))
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

impl Default for DeviceStatePoller {
    fn default() -> Self {
        Self::new()
    }
}

impl DeviceStatePoller {
    /// Create a new device state poller with all supported strategies
    pub fn new() -> Self {
        let mut service_pollers: HashMap<Service, Box<dyn ServicePoller>> = HashMap::new();

        service_pollers.insert(Service::AVTransport, Box::new(AVTransportPoller));
        service_pollers.insert(Service::RenderingControl, Box::new(RenderingControlPoller));
        service_pollers.insert(
            Service::ZoneGroupTopology,
            Box::new(ZoneGroupTopologyPoller),
        );
        service_pollers.insert(Service::GroupManagement, Box::new(GroupManagementPoller));
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
            Some(poller) => poller.poll_state(&self.sonos_client, pair).await,
            None => Err(PollingError::UnsupportedService {
                service: pair.service,
            }),
        }
    }

    /// Convert a JSON state snapshot to EventData for a given service.
    pub fn state_to_event_data(
        &self,
        service: &Service,
        json_state: &str,
    ) -> PollingResult<EventData> {
        match self.service_pollers.get(service) {
            Some(poller) => poller.state_to_event_data(json_state),
            None => Err(PollingError::UnsupportedService { service: *service }),
        }
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
            writeln!(f, "    {service:?}")?;
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

        assert_eq!(stats.total_pollers, 5);
        assert!(poller.is_service_supported(&Service::AVTransport));
        assert!(poller.is_service_supported(&Service::RenderingControl));
        assert!(poller.is_service_supported(&Service::ZoneGroupTopology));
        assert!(poller.is_service_supported(&Service::GroupManagement));
        assert!(poller.is_service_supported(&Service::GroupRenderingControl));
    }

    #[test]
    fn test_service_poller_types() {
        assert_eq!(AVTransportPoller.service_type(), Service::AVTransport);
        assert_eq!(
            RenderingControlPoller.service_type(),
            Service::RenderingControl
        );
        assert_eq!(
            ZoneGroupTopologyPoller.service_type(),
            Service::ZoneGroupTopology
        );
        assert_eq!(
            GroupManagementPoller.service_type(),
            Service::GroupManagement
        );
        assert_eq!(
            GroupRenderingControlPoller.service_type(),
            Service::GroupRenderingControl
        );
    }

    #[tokio::test]
    async fn test_group_management_poller_returns_stable_state() {
        let poller = GroupManagementPoller;
        let pair =
            SpeakerServicePair::new("192.168.1.100".parse().unwrap(), Service::GroupManagement);

        let state1 = poller.poll_state(&SonosClient::new(), &pair).await.unwrap();
        let state2 = poller.poll_state(&SonosClient::new(), &pair).await.unwrap();
        assert_eq!(state1, state2, "GroupManagement should return stable state");
        assert_eq!(state1, "{}");
    }

    #[test]
    fn test_state_to_event_data_round_trip_all_services() {
        let poller = DeviceStatePoller::new();

        // AVTransport round-trip
        let avt_state = sonos_api::services::av_transport::state::AVTransportState {
            transport_state: Some("PLAYING".to_string()),
            transport_status: Some("OK".to_string()),
            speed: None,
            current_track_uri: None,
            track_duration: None,
            track_metadata: None,
            rel_time: None,
            abs_time: None,
            rel_count: None,
            abs_count: None,
            play_mode: None,
            next_track_uri: None,
            next_track_metadata: None,
            queue_length: None,
        };
        let json = serde_json::to_string(&avt_state).unwrap();
        let event_data = poller
            .state_to_event_data(&Service::AVTransport, &json)
            .unwrap();
        match event_data {
            EventData::AVTransport(state) => {
                assert_eq!(state.transport_state, Some("PLAYING".to_string()));
                assert_eq!(state.transport_status, Some("OK".to_string()));
            }
            _ => panic!("Expected AVTransport EventData"),
        }

        // RenderingControl round-trip
        let rc_state = sonos_api::services::rendering_control::state::RenderingControlState {
            master_volume: Some("75".to_string()),
            master_mute: Some("0".to_string()),
            bass: None,
            treble: None,
            loudness: None,
            balance: None,
            lf_volume: None,
            rf_volume: None,
            lf_mute: None,
            rf_mute: None,
            other_channels: std::collections::HashMap::new(),
        };
        let json = serde_json::to_string(&rc_state).unwrap();
        let event_data = poller
            .state_to_event_data(&Service::RenderingControl, &json)
            .unwrap();
        match event_data {
            EventData::RenderingControl(state) => {
                assert_eq!(state.master_volume, Some("75".to_string()));
            }
            _ => panic!("Expected RenderingControl EventData"),
        }

        // GroupRenderingControl round-trip
        let grc_state =
            sonos_api::services::group_rendering_control::state::GroupRenderingControlState {
                group_volume: Some(42),
                group_mute: Some(false),
                group_volume_changeable: Some(true),
            };
        let json = serde_json::to_string(&grc_state).unwrap();
        let event_data = poller
            .state_to_event_data(&Service::GroupRenderingControl, &json)
            .unwrap();
        match event_data {
            EventData::GroupRenderingControl(state) => {
                assert_eq!(state.group_volume, Some(42));
                assert_eq!(state.group_mute, Some(false));
            }
            _ => panic!("Expected GroupRenderingControl EventData"),
        }

        // ZoneGroupTopology round-trip
        let zgt_state = sonos_api::services::zone_group_topology::state::ZoneGroupTopologyState {
            zone_groups: vec![],
            vanished_devices: vec![],
        };
        let json = serde_json::to_string(&zgt_state).unwrap();
        let event_data = poller
            .state_to_event_data(&Service::ZoneGroupTopology, &json)
            .unwrap();
        match event_data {
            EventData::ZoneGroupTopology(state) => {
                assert!(state.zone_groups.is_empty());
            }
            _ => panic!("Expected ZoneGroupTopology EventData"),
        }

        // GroupManagement round-trip
        let gm_state = sonos_api::services::group_management::state::GroupManagementState {
            group_coordinator_is_local: Some(true),
            local_group_uuid: None,
            reset_volume_after: None,
            virtual_line_in_group_id: None,
            volume_av_transport_uri: None,
        };
        let json = serde_json::to_string(&gm_state).unwrap();
        let event_data = poller
            .state_to_event_data(&Service::GroupManagement, &json)
            .unwrap();
        match event_data {
            EventData::GroupManagement(state) => {
                assert_eq!(state.group_coordinator_is_local, Some(true));
            }
            _ => panic!("Expected GroupManagement EventData"),
        }
    }
}
