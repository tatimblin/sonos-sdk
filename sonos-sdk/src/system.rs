//! SonosSystem - Main entry point for the SDK
//!
//! Provides a sync-first, DOM-like API for controlling Sonos devices.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use sonos_api::SonosClient;
use sonos_discovery::{self, Device};
use sonos_event_manager::SonosEventManager;
use sonos_state::{SpeakerId, StateManager};

use crate::{SdkError, Speaker};

/// Main system entry point - provides DOM-like API
///
/// SonosSystem is fully synchronous - no async/await required.
///
/// # Example
///
/// ```rust,ignore
/// use sonos_sdk::SonosSystem;
///
/// fn main() -> Result<(), sonos_sdk::SdkError> {
///     let system = SonosSystem::new()?;
///
///     // Get speaker by name
///     let speaker = system.get_speaker_by_name("Living Room")
///         .ok_or_else(|| sonos_sdk::SdkError::SpeakerNotFound("Living Room".to_string()))?;
///
///     // Three methods on each property:
///     let volume = speaker.volume.get();              // Get cached value
///     let fresh_volume = speaker.volume.fetch()?;     // API call + update cache
///     let current = speaker.volume.watch()?;          // Start watching for changes
///
///     // Iterate over changes
///     for event in system.iter() {
///         println!("Property changed: {:?}", event);
///     }
///
///     Ok(())
/// }
/// ```
pub struct SonosSystem {
    /// State manager for property values
    state_manager: Arc<StateManager>,

    /// Event manager for UPnP subscriptions (kept alive)
    _event_manager: Arc<SonosEventManager>,

    /// API client for direct operations (kept for future use)
    _api_client: SonosClient,

    /// Speaker handles by name
    speakers: RwLock<HashMap<String, Speaker>>,
}

impl SonosSystem {
    /// Create a new SonosSystem with automatic device discovery (sync)
    ///
    /// This will:
    /// 1. Discover Sonos devices on the network
    /// 2. Create event manager for UPnP subscriptions
    /// 3. Create state manager for property tracking
    /// 4. Create speaker handles for each device
    pub fn new() -> Result<Self, SdkError> {
        // Discover devices first
        let devices = sonos_discovery::get();

        Self::from_discovered_devices(devices)
    }

    /// Create a new SonosSystem from pre-discovered devices (sync)
    ///
    /// Use this when you already have a list of devices from `sonos_discovery::get()`.
    pub fn from_discovered_devices(devices: Vec<Device>) -> Result<Self, SdkError> {
        // Create event manager (sync)
        let event_manager =
            Arc::new(SonosEventManager::new().map_err(|e| SdkError::EventManager(e.to_string()))?);

        // Create state manager with event manager wired up (sync)
        let state_manager = Arc::new(
            StateManager::builder()
                .with_event_manager(Arc::clone(&event_manager))
                .build()
                .map_err(SdkError::StateError)?,
        );

        // Add devices
        state_manager
            .add_devices(devices.clone())
            .map_err(SdkError::StateError)?;

        let api_client = SonosClient::new();

        // Create speaker handles
        let mut speakers = HashMap::new();
        for device in devices {
            let speaker_id = SpeakerId::new(&device.id);
            let ip = device
                .ip_address
                .parse()
                .map_err(|_| SdkError::InvalidIpAddress)?;

            let speaker = Speaker::new(
                speaker_id,
                device.name.clone(),
                ip,
                device.model_name.clone(),
                Arc::clone(&state_manager),
                api_client.clone(),
            );

            speakers.insert(device.name, speaker);
        }

        Ok(Self {
            state_manager,
            _event_manager: event_manager,
            _api_client: api_client,
            speakers: RwLock::new(speakers),
        })
    }

    /// Get speaker by name (sync)
    ///
    /// Returns `None` if no speaker with that name exists.
    pub fn get_speaker_by_name(&self, name: &str) -> Option<Speaker> {
        self.speakers.read().ok()?.get(name).cloned()
    }

    /// Get all speakers (sync)
    pub fn speakers(&self) -> Vec<Speaker> {
        self.speakers
            .read()
            .map(|s| s.values().cloned().collect())
            .unwrap_or_default()
    }

    /// Get speaker by ID (sync)
    pub fn get_speaker_by_id(&self, speaker_id: &SpeakerId) -> Option<Speaker> {
        let speakers = self.speakers.read().ok()?;
        speakers.values().find(|s| s.id == *speaker_id).cloned()
    }

    /// Get all speaker names (sync)
    pub fn speaker_names(&self) -> Vec<String> {
        self.speakers
            .read()
            .map(|s| s.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// Get the state manager for advanced usage
    pub fn state_manager(&self) -> &Arc<StateManager> {
        &self.state_manager
    }

    /// Get a blocking iterator over property change events
    ///
    /// Only emits events for properties that have been `watch()`ed.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // First, watch some properties
    /// speaker.volume.watch()?;
    /// speaker.playback_state.watch()?;
    ///
    /// // Then iterate over changes (blocking)
    /// for event in system.iter() {
    ///     println!("Changed: {} on {}", event.property_key, event.speaker_id);
    /// }
    /// ```
    pub fn iter(&self) -> sonos_state::ChangeIterator {
        self.state_manager.iter()
    }
}
