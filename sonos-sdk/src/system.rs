use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use sonos_state::{StateManager, SpeakerId};
use sonos_api::SonosClient;
use sonos_discovery::{self, Device};
use crate::{Speaker, SdkError};

/// Main system entry point - provides DOM-like API
pub struct SonosSystem {
    state_manager: Arc<StateManager>,
    api_client: SonosClient,
    speakers: Arc<RwLock<HashMap<String, Speaker>>>, // name -> speaker
}

impl SonosSystem {
    /// Create a new SonosSystem with automatic device discovery
    pub async fn new() -> Result<Self, SdkError> {
        // Initialize core reactive system
        let state_manager = Arc::new(StateManager::new().await.map_err(SdkError::StateError)?);
        let api_client = SonosClient::new();

        // Discover devices
        let devices = sonos_discovery::get();
        state_manager.add_devices(devices.clone()).await.map_err(SdkError::StateError)?;

        Self::from_devices(devices, state_manager, api_client).await
    }

    /// Create a new SonosSystem from pre-discovered devices (avoids runtime panic)
    pub async fn from_discovered_devices(devices: Vec<Device>) -> Result<Self, SdkError> {
        // Initialize core reactive system
        let state_manager = Arc::new(StateManager::new().await.map_err(SdkError::StateError)?);
        let api_client = SonosClient::new();

        // Use pre-discovered devices
        state_manager.add_devices(devices.clone()).await.map_err(SdkError::StateError)?;

        Self::from_devices(devices, state_manager, api_client).await
    }

    /// Common implementation for creating SonosSystem from devices
    async fn from_devices(
        devices: Vec<Device>,
        state_manager: Arc<StateManager>,
        api_client: SonosClient,
    ) -> Result<Self, SdkError> {
        // Create speaker handles
        let mut speakers = HashMap::new();
        for device in devices {
            let speaker_id = SpeakerId::new(&device.id);
            let ip = device.ip_address.parse().map_err(|_| SdkError::InvalidIpAddress)?;

            let speaker = Speaker::new(
                speaker_id,
                device.name.clone(),
                ip,
                Arc::clone(&state_manager),
                api_client.clone(),
            );

            speakers.insert(device.name, speaker);
        }

        Ok(Self {
            state_manager,
            api_client,
            speakers: Arc::new(RwLock::new(speakers)),
        })
    }

    /// Get speaker by name for DOM-like API
    pub async fn get_speaker_by_name(&self, name: &str) -> Option<Speaker> {
        self.speakers.read().await.get(name).cloned()
    }

    /// Get all speakers
    pub async fn speakers(&self) -> Vec<Speaker> {
        self.speakers.read().await.values().cloned().collect()
    }

    /// Get speaker by ID
    pub async fn get_speaker_by_id(&self, speaker_id: &SpeakerId) -> Option<Speaker> {
        let speakers = self.speakers.read().await;
        speakers.values().find(|s| s.id == *speaker_id).cloned()
    }

    /// Get speaker names (useful for discovery)
    pub async fn speaker_names(&self) -> Vec<String> {
        self.speakers.read().await.keys().cloned().collect()
    }
}