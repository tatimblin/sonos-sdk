//! Reactive State Management for Sonos devices
//!
//! Provides demand-driven property subscriptions with automatic UPnP subscription management.
//!
//! # Example
//!
//! ```rust,ignore
//! use sonos_state::{StateManager, Volume};
//! use sonos_discovery;
//!
//! // Create state manager and add devices
//! let mut state_manager = StateManager::new().await?;
//! let devices = sonos_discovery::get();
//! state_manager.add_devices(devices).await?;
//!
//! // Watch a property - automatic subscription management
//! let mut volume_watcher = state_manager.watch_property::<Volume>(speaker_id).await?;
//! while volume_watcher.changed().await.is_ok() {
//!     if let Some(volume) = volume_watcher.current() {
//!         println!("Volume: {}%", volume.0);
//!     }
//! }
//! ```

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use tokio::sync::{watch, RwLock};
use tokio::task::JoinHandle;
use tracing::{debug, info, trace, warn};

use sonos_api::Service;
use sonos_discovery::Device;
use sonos_event_manager::SonosEventManager;
use sonos_stream::events::types as stream_types;

use crate::change_iterator::{BlockingChangeIterator, ChangeFilter, ChangeStream};
use crate::model::{SpeakerId, SpeakerInfo};
use crate::property::Property;
use crate::state_manager::StateManager as CoreStateManager;
use crate::{Result, StateError};

/// Key for tracking service subscriptions (speaker IP + service)
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct SubscriptionKey {
    pub speaker_ip: IpAddr,
    pub service: Service,
}

/// A handle for watching a specific property with automatic subscription management
pub struct PropertyWatcher<P: Property> {
    /// tokio::sync::watch receiver for this property from StateStore
    property_receiver: watch::Receiver<Option<P>>,
    /// Speaker ID being watched
    speaker_id: SpeakerId,
    /// Service for this property (for subscription info)
    service: Service,
    /// Reference to subscription manager for cleanup
    subscription_manager: Arc<PropertySubscriptionManager>,
}

impl<P: Property> PropertyWatcher<P> {
    fn new(
        property_receiver: watch::Receiver<Option<P>>,
        speaker_id: SpeakerId,
        subscription_manager: Arc<PropertySubscriptionManager>,
    ) -> Self {
        Self {
            property_receiver,
            speaker_id,
            service: P::SERVICE,
            subscription_manager,
        }
    }

    /// Wait for the property to change
    pub async fn changed(&mut self) -> std::result::Result<(), watch::error::RecvError> {
        self.property_receiver.changed().await
    }

    /// Get the current value of the property
    pub fn current(&self) -> Option<P> {
        self.property_receiver.borrow().clone()
    }

    /// Get the speaker ID being watched
    pub fn speaker_id(&self) -> &SpeakerId {
        &self.speaker_id
    }
}

impl<P: Property> Drop for PropertyWatcher<P> {
    fn drop(&mut self) {
        let subscription_manager = Arc::clone(&self.subscription_manager);
        let speaker_id = self.speaker_id.clone();
        let service = self.service;

        // Spawn cleanup task
        tokio::spawn(async move {
            if let Some(speaker_ip) = subscription_manager.get_speaker_ip(&speaker_id) {
                let key = SubscriptionKey { speaker_ip, service };
                subscription_manager.release_subscription(&key).await;
            }
        });
    }
}

/// Manages property subscriptions and the single event processor
struct PropertySubscriptionManager {
    /// The underlying event manager
    event_manager: Arc<RwLock<SonosEventManager>>,
    /// Map from speaker ID to IP address
    speaker_ips: Arc<RwLock<HashMap<SpeakerId, IpAddr>>>,
    /// Reference counting for service subscriptions
    subscription_refs: Arc<RwLock<HashMap<SubscriptionKey, usize>>>,
}

impl PropertySubscriptionManager {
    /// Create a new property subscription manager
    fn new(event_manager: Arc<RwLock<SonosEventManager>>) -> Self {
        Self {
            event_manager,
            speaker_ips: Arc::new(RwLock::new(HashMap::new())),
            subscription_refs: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register speaker IP addresses
    async fn register_speaker(&self, speaker_id: SpeakerId, ip: IpAddr) {
        let mut ips = self.speaker_ips.write().await;
        ips.insert(speaker_id, ip);
    }

    /// Get speaker IP by ID
    fn get_speaker_ip(&self, speaker_id: &SpeakerId) -> Option<IpAddr> {
        // This is a blocking operation, which is not ideal in async context
        // but needed for synchronous Drop implementation
        if let Ok(ips) = self.speaker_ips.try_read() {
            ips.get(speaker_id).copied()
        } else {
            None
        }
    }

    /// Ensure a service subscription exists (with reference counting)
    async fn ensure_subscription(&self, key: &SubscriptionKey) -> Result<()> {
        let mut refs = self.subscription_refs.write().await;
        let current_count = refs.get(key).copied().unwrap_or(0);

        if current_count == 0 {
            // First reference - create the subscription
            let mut event_manager = self.event_manager.write().await;
            event_manager.ensure_service_subscribed(key.speaker_ip, key.service).await
                .map_err(|e| StateError::SubscriptionFailed(format!("Failed to subscribe to {:?}: {}", key.service, e)))?;
        }

        refs.insert(key.clone(), current_count + 1);
        Ok(())
    }

    /// Release a service subscription (with reference counting)
    async fn release_subscription(&self, key: &SubscriptionKey) {
        let mut refs = self.subscription_refs.write().await;

        if let Some(current_count) = refs.get(key).copied() {
            let new_count = current_count.saturating_sub(1);

            if new_count == 0 {
                // Last reference - remove subscription
                refs.remove(key);

                // Release from event manager
                let mut event_manager = self.event_manager.write().await;
                if let Err(e) = event_manager.release_service_subscription(key.speaker_ip, key.service).await {
                    tracing::warn!("Failed to release service subscription {:?}: {}", key.service, e);
                }
            } else {
                refs.insert(key.clone(), new_count);
            }
        }
    }

    /// Get reference count for a subscription
    fn get_subscription_ref_count(&self, key: &SubscriptionKey) -> usize {
        if let Ok(refs) = self.subscription_refs.try_read() {
            refs.get(key).copied().unwrap_or(0)
        } else {
            0
        }
    }

    /// Get subscription statistics
    async fn subscription_stats(&self) -> HashMap<SubscriptionKey, usize> {
        let refs = self.subscription_refs.read().await;
        refs.clone()
    }
}

/// Reactive state manager with automatic UPnP subscription management
pub struct StateManager {
    /// Underlying core state manager
    core_state_manager: CoreStateManager,
    /// Event manager for UPnP subscriptions
    event_manager: Arc<RwLock<SonosEventManager>>,
    /// Property subscription manager
    subscription_manager: Arc<PropertySubscriptionManager>,
    /// Single event processor task
    _event_processor: JoinHandle<()>,
}

impl StateManager {
    /// Create a new reactive state manager with single event processor
    pub async fn new() -> Result<Self> {
        let core_state_manager = CoreStateManager::new();
        let event_manager = SonosEventManager::new()
            .await
            .map_err(|e| StateError::InitializationFailed(format!("EventManager failed: {}", e)))?;

        let event_manager = Arc::new(RwLock::new(event_manager));
        let subscription_manager = Arc::new(PropertySubscriptionManager::new(Arc::clone(&event_manager)));

        // Start the single event processor
        let event_processor = Self::start_event_processor(
            core_state_manager.clone(),
            Arc::clone(&event_manager),
        ).await?;

        Ok(Self {
            core_state_manager,
            event_manager,
            subscription_manager,
            _event_processor: event_processor,
        })
    }

    /// Start the single event processor that handles ALL events
    async fn start_event_processor(
        mut core_state_manager: CoreStateManager,
        event_manager: Arc<RwLock<SonosEventManager>>,
    ) -> Result<JoinHandle<()>> {
        // Get the single event iterator
        let mut event_iterator = {
            let mut em = event_manager.write().await;
            em.get_event_iterator()
                .map_err(|e| StateError::InitializationFailed(format!("Failed to get event iterator: {}", e)))?
        };

        let task = tokio::spawn(async move {
            info!("Event processor started - handling events from all speakers and services");

            // Add timeout to detect if we're getting ANY events
            let mut event_count = 0;
            loop {
                tokio::select! {
                    maybe_event = event_iterator.next_async() => {
                        match maybe_event {
                            Some(enriched_event) => {
                                event_count += 1;
                                debug!(
                                    event_count,
                                    speaker_ip = %enriched_event.speaker_ip,
                                    service = ?enriched_event.service,
                                    "Received event from speaker"
                                );

                                // Store fields before moving enriched_event
                                let speaker_ip = enriched_event.speaker_ip;
                                let service = enriched_event.service;

                                let raw_event = Self::convert_enriched_to_raw_event(enriched_event);
                                core_state_manager.process(raw_event);

                                debug!(
                                    speaker_ip = %speaker_ip,
                                    service = ?service,
                                    "State change processed"
                                );
                            }
                            None => {
                                warn!("EventIterator ended - channel closed");
                                break;
                            }
                        }
                    }
                    _ = tokio::time::sleep(std::time::Duration::from_secs(5)) => {
                        trace!(
                            events_received = event_count,
                            "Event processor waiting for events"
                        );
                    }
                }
            }

            info!("Event processor ended - event iterator closed");
        });

        Ok(task)
    }

    /// Convert sonos_stream::EnrichedEvent to sonos_state::RawEvent
    fn convert_enriched_to_raw_event(enriched: stream_types::EnrichedEvent) -> crate::decoder::RawEvent {
        use stream_types::EventData as StreamEventData;
        use crate::decoder::{EventData as StateEventData, RawEvent};

        // Convert EventData from sonos-stream format to sonos-state format
        let converted_data = match enriched.event_data {
            StreamEventData::AVTransportEvent(av_event) => {
                // Convert AVTransportEvent to AVTransportData - only map compatible fields
                StateEventData::AVTransport(crate::decoder::AVTransportData {
                    transport_state: av_event.transport_state,
                    current_track_uri: av_event.current_track_uri,
                    track_duration: av_event.track_duration,
                    rel_time: av_event.rel_time,
                    track_metadata: av_event.track_metadata,
                    play_mode: av_event.play_mode,
                    next_track_uri: av_event.next_track_uri,
                    next_track_metadata: av_event.next_track_metadata,
                })
            }
            StreamEventData::RenderingControlEvent(rc_event) => {
                // Convert RenderingControlEvent to RenderingControlData
                // Parse string values to proper types
                let master_volume = rc_event.master_volume
                    .and_then(|v| v.parse::<u8>().ok());
                let lf_volume = rc_event.lf_volume
                    .and_then(|v| v.parse::<u8>().ok());
                let rf_volume = rc_event.rf_volume
                    .and_then(|v| v.parse::<u8>().ok());
                let master_mute = rc_event.master_mute
                    .and_then(|v| v.parse::<bool>().ok().or_else(|| {
                        // Handle "0"/"1" format common in UPnP
                        match v.as_str() {
                            "0" => Some(false),
                            "1" => Some(true),
                            _ => None,
                        }
                    }));
                let bass = rc_event.bass
                    .and_then(|v| v.parse::<i8>().ok());
                let treble = rc_event.treble
                    .and_then(|v| v.parse::<i8>().ok());
                let loudness = rc_event.loudness
                    .and_then(|v| v.parse::<bool>().ok().or_else(|| {
                        match v.as_str() {
                            "0" => Some(false),
                            "1" => Some(true),
                            _ => None,
                        }
                    }));

                StateEventData::RenderingControl(crate::decoder::RenderingControlData {
                    master_volume,
                    master_mute,
                    lf_volume,
                    rf_volume,
                    bass,
                    treble,
                    loudness,
                })
            }
            StreamEventData::DevicePropertiesEvent(dp_event) => {
                // Convert DevicePropertiesEvent to DevicePropertiesData - map available fields
                StateEventData::DeviceProperties(crate::decoder::DevicePropertiesData {
                    zone_name: dp_event.zone_name,
                    icon: dp_event.zone_icon, // Note: zone_icon in stream, icon in state
                    invisible: None, // Not available in stream event
                })
            }
            StreamEventData::ZoneGroupTopologyEvent(topo_event) => {
                // Convert ZoneGroupTopologyEvent to TopologyData - simplified mapping
                // Note: This is a placeholder as the topology structures are quite different
                StateEventData::ZoneGroupTopology(crate::decoder::TopologyData {
                    zone_groups: vec![], // TODO: Implement proper topology conversion
                    vanished_devices: vec![],
                })
            }
        };

        RawEvent {
            speaker_ip: enriched.speaker_ip,
            service: enriched.service,
            timestamp: enriched.timestamp,
            data: converted_data,
        }
    }

    /// Add devices for property subscriptions
    pub async fn add_devices(&self, devices: Vec<Device>) -> Result<()> {
        // Add to event manager
        {
            let event_manager = self.event_manager.read().await;
            event_manager.add_devices(devices.clone()).await
                .map_err(|e| StateError::DeviceRegistrationFailed(format!("EventManager error: {}", e)))?;
        }

        // Register with subscription manager and state store
        for device in &devices {
            let speaker_id = SpeakerId::new(&device.id);
            let ip: IpAddr = device.ip_address.parse()
                .map_err(|_| StateError::InvalidIpAddress(device.ip_address.clone()))?;

            // Register IP with subscription manager
            self.subscription_manager.register_speaker(speaker_id.clone(), ip).await;

            // Add to state store
            self.core_state_manager.store().add_speaker(SpeakerInfo {
                id: speaker_id,
                name: device.name.clone(),
                room_name: device.name.clone(),
                ip_address: ip,
                port: device.port,
                model_name: device.model_name.clone(),
                software_version: "unknown".to_string(),
                satellites: vec![],
            });
        }

        Ok(())
    }

    /// Watch a property with automatic subscription management
    pub async fn watch_property<P: Property>(&self, speaker_id: SpeakerId) -> Result<PropertyWatcher<P>> {
        // Get speaker IP
        let speaker_ip = self.subscription_manager.get_speaker_ip(&speaker_id)
            .ok_or_else(|| StateError::SpeakerNotFound(speaker_id.clone()))?;

        // Create subscription key
        let key = SubscriptionKey {
            speaker_ip,
            service: P::SERVICE,
        };

        // Ensure service subscription exists
        self.subscription_manager.ensure_subscription(&key).await?;

        // Get property watch receiver from StateStore
        let property_receiver = self.core_state_manager.store().watch::<P>(&speaker_id);

        // Create PropertyWatcher
        Ok(PropertyWatcher::new(
            property_receiver,
            speaker_id,
            Arc::clone(&self.subscription_manager),
        ))
    }

    /// Get current property value (non-reactive)
    pub fn get_property<P: Property>(&self, speaker_id: &SpeakerId) -> Option<P> {
        self.core_state_manager.store().get::<P>(speaker_id)
    }

    /// Manually update a property value (for API fetch integration)
    /// This allows external API calls to push fresh values into the reactive state system
    pub fn update_property<P: Property>(&self, speaker_id: &SpeakerId, value: P) {
        self.core_state_manager.store().set::<P>(speaker_id, value);
    }

    /// Get subscription statistics (for debugging)
    #[cfg(debug_assertions)]
    pub async fn subscription_stats(&self) -> HashMap<SubscriptionKey, usize> {
        self.subscription_manager.subscription_stats().await
    }

    // ========================================================================
    // Global Change Iterator API
    // ========================================================================

    /// Create an async change stream for all state changes
    ///
    /// Returns a stream that emits `ChangeEvent`s for all property changes,
    /// device lifecycle events, and group changes across all speakers.
    /// Perfect for applications that need to react to any Sonos state change.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let mut changes = state_manager.changes();
    /// while let Some(change) = changes.next().await {
    ///     match change.context.rerender_scope {
    ///         RerenderScope::Full => refresh_entire_ui(),
    ///         RerenderScope::Device(speaker_id) => refresh_device_ui(&speaker_id),
    ///         _ => {}
    ///     }
    /// }
    /// ```
    pub fn changes(&self) -> ChangeStream {
        ChangeStream::new(self.core_state_manager.store().subscribe_changes())
    }

    /// Create a filtered async change stream
    ///
    /// Only emits changes that match the provided filter criteria.
    /// Useful for applications that only care about specific devices,
    /// services, or property types.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Only volume/mute changes for specific speaker
    /// let filter = ChangeFilter::for_speaker(speaker_id)
    ///     .and_properties(["volume", "mute"]);
    /// let mut changes = state_manager.changes_filtered(filter);
    /// ```
    pub fn changes_filtered(&self, filter: ChangeFilter) -> ChangeStream {
        ChangeStream::filtered(self.core_state_manager.store().subscribe_changes(), filter)
    }

    /// Create a blocking iterator for synchronous code
    ///
    /// Useful for CLI applications or other synchronous contexts that
    /// need to process Sonos state changes without async/await.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let rt = tokio::runtime::Runtime::new()?;
    /// let mut changes = state_manager.changes_blocking(rt.handle().clone());
    ///
    /// for change in changes {
    ///     println!("Change: {}", change.context.description);
    /// }
    /// ```
    pub fn changes_blocking(&self, rt: tokio::runtime::Handle) -> BlockingChangeIterator {
        BlockingChangeIterator::new(self.core_state_manager.store().subscribe_changes(), rt)
    }

    /// Create a filtered blocking iterator
    ///
    /// Combines filtering with blocking iteration for synchronous code
    /// that only needs specific types of changes.
    pub fn changes_blocking_filtered(
        &self,
        filter: ChangeFilter,
        rt: tokio::runtime::Handle,
    ) -> BlockingChangeIterator {
        BlockingChangeIterator::filtered(
            self.core_state_manager.store().subscribe_changes(),
            filter,
            rt,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::property::Volume;

    #[tokio::test]
    async fn test_simplified_reactive_architecture() {
        // This test would verify:
        // 1. Single event processor handles all events
        // 2. PropertyWatchers watch StateStore channels
        // 3. Service subscriptions are reference-counted properly
        // 4. No complex event distribution needed
    }
}