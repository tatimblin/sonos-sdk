//! Simplified event processor that delegates to sonos-api event framework
//!
//! This processor replaces the old service-specific processing logic with
//! a simple delegation to the sonos-api EventProcessor.

use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};

use callback_server::{router::{EventRouter, NotificationPayload}, FirewallDetectionCoordinator};
use sonos_api::events::{EventProcessor as ApiEventProcessor};

use crate::error::{EventProcessingError, EventProcessingResult};
use crate::subscription::manager::SubscriptionManager;
use crate::events::types::{EnrichedEvent, EventData, EventSource};

/// Simplified event processor that delegates to sonos-api event framework
pub struct EventProcessor {
    /// The sonos-api event processor that handles service-specific parsing
    api_processor: ApiEventProcessor,

    /// Subscription manager for looking up subscriptions by SID
    subscription_manager: Arc<SubscriptionManager>,

    /// Sender for enriched events (maintains compatibility with existing code)
    event_sender: mpsc::UnboundedSender<EnrichedEvent>,

    /// Statistics tracking
    stats: Arc<RwLock<EventProcessorStats>>,

    /// Firewall detection coordinator for event arrival notifications
    firewall_coordinator: Option<Arc<FirewallDetectionCoordinator>>,
}

impl EventProcessor {
    /// Create a new event processor
    pub fn new(
        subscription_manager: Arc<SubscriptionManager>,
        event_sender: mpsc::UnboundedSender<EnrichedEvent>,
        firewall_coordinator: Option<Arc<FirewallDetectionCoordinator>>,
    ) -> Self {
        Self {
            api_processor: ApiEventProcessor::with_default_parsers(),
            subscription_manager,
            event_sender,
            stats: Arc::new(RwLock::new(EventProcessorStats::new())),
            firewall_coordinator,
        }
    }

    /// Process a UPnP notification payload from the callback server
    pub async fn process_upnp_notification(
        &self,
        payload: NotificationPayload,
    ) -> EventProcessingResult<()> {
        // Update stats
        {
            let mut stats = self.stats.write().await;
            stats.upnp_events_received += 1;
        }

        // Look up subscription by SID
        let subscription_wrapper = self
            .subscription_manager
            .get_subscription_by_sid(&payload.subscription_id)
            .await
            .ok_or_else(|| {
                EventProcessingError::Enrichment(format!(
                    "No subscription found for SID: {}",
                    payload.subscription_id
                ))
            })?;

        // Get speaker/service pair from subscription
        let pair = subscription_wrapper.speaker_service_pair();
        let registration_id = subscription_wrapper.registration_id();

        // Record that we received an event for this subscription
        subscription_wrapper.record_event_received().await;
        self.subscription_manager
            .record_event_received(&payload.subscription_id)
            .await;

        // Notify firewall coordinator that an event was received
        if let Some(coordinator) = &self.firewall_coordinator {
            coordinator.on_event_received(pair.speaker_ip).await;
        }

        // Parse the event using sonos-api event processor
        let api_enriched_event = self.api_processor
            .process_upnp_event(
                pair.speaker_ip, // speaker_ip is already an IpAddr
                pair.service,
                payload.subscription_id.clone(),
                &payload.event_xml,
            )
            .map_err(|e| EventProcessingError::Parsing(format!("API processing failed: {}", e)))?;

        // Convert from sonos-api enriched event to sonos-stream compatible format
        let event_data = self.convert_api_event_data(&pair.service, api_enriched_event.event_data)?;

        // Create enriched event compatible with existing sonos-stream code
        let enriched_event = EnrichedEvent::new(
            registration_id,
            pair.speaker_ip,
            pair.service,
            EventSource::UPnPNotification {
                subscription_id: payload.subscription_id,
            },
            event_data,
        );

        // Send enriched event
        self.event_sender
            .send(enriched_event)
            .map_err(|_| EventProcessingError::ChannelClosed)?;

        // Update success stats
        {
            let mut stats = self.stats.write().await;
            stats.events_processed += 1;
        }

        Ok(())
    }

    /// Process a synthetic event from polling (already enriched)
    pub async fn process_polling_event(&self, event: EnrichedEvent) -> EventProcessingResult<()> {
        // Update stats
        {
            let mut stats = self.stats.write().await;
            stats.polling_events_received += 1;
        }

        // Send the event (it's already enriched)
        self.event_sender
            .send(event)
            .map_err(|_| EventProcessingError::ChannelClosed)?;

        // Update success stats
        {
            let mut stats = self.stats.write().await;
            stats.events_processed += 1;
        }

        Ok(())
    }

    /// Process a resync event (already enriched)
    pub async fn process_resync_event(&self, event: EnrichedEvent) -> EventProcessingResult<()> {
        // Update stats
        {
            let mut stats = self.stats.write().await;
            stats.resync_events_received += 1;
        }

        // Send the event (it's already enriched)
        self.event_sender
            .send(event)
            .map_err(|_| EventProcessingError::ChannelClosed)?;

        // Update success stats
        {
            let mut stats = self.stats.write().await;
            stats.events_processed += 1;
        }

        Ok(())
    }

    /// Convert from sonos-api event data to sonos-stream compatible EventData
    fn convert_api_event_data(
        &self,
        service: &sonos_api::Service,
        api_event_data: Box<dyn std::any::Any + Send + Sync>,
    ) -> EventProcessingResult<EventData> {
        match service {
            sonos_api::Service::AVTransport => {
                let av_event = api_event_data
                    .downcast::<sonos_api::services::av_transport::AVTransportEvent>()
                    .map_err(|_| EventProcessingError::Parsing("Failed to downcast AVTransport event".to_string()))?;

                // Convert from sonos-api AVTransportEvent to sonos-stream AVTransportEvent
                let stream_event = crate::events::types::AVTransportEvent {
                    transport_state: av_event.transport_state.clone(),
                    transport_status: av_event.transport_status.clone(),
                    speed: av_event.speed.clone(),
                    current_track_uri: av_event.current_track_uri.clone(),
                    track_duration: av_event.track_duration.clone(),
                    rel_time: av_event.rel_time.clone(),
                    abs_time: av_event.abs_time.clone(),
                    rel_count: av_event.rel_count,
                    abs_count: av_event.abs_count,
                    play_mode: av_event.play_mode.clone(),
                    track_metadata: av_event.track_metadata.clone(),
                    next_track_uri: av_event.next_track_uri.clone(),
                    next_track_metadata: av_event.next_track_metadata.clone(),
                    queue_length: av_event.queue_length,
                };

                Ok(EventData::AVTransportEvent(stream_event))
            }

            sonos_api::Service::RenderingControl | sonos_api::Service::GroupRenderingControl => {
                // Both RenderingControl and GroupRenderingControl use the same event structure
                let rc_event = api_event_data
                    .downcast::<sonos_api::services::rendering_control::RenderingControlEvent>()
                    .map_err(|_| EventProcessingError::Parsing("Failed to downcast RenderingControl event".to_string()))?;

                // Convert from sonos-api RenderingControlEvent to sonos-stream RenderingControlEvent
                let stream_event = crate::events::types::RenderingControlEvent {
                    master_volume: rc_event.master_volume.clone(),
                    lf_volume: rc_event.lf_volume.clone(),
                    rf_volume: rc_event.rf_volume.clone(),
                    master_mute: rc_event.master_mute.clone(),
                    lf_mute: rc_event.lf_mute.clone(),
                    rf_mute: rc_event.rf_mute.clone(),
                    bass: rc_event.bass.clone(),
                    treble: rc_event.treble.clone(),
                    loudness: rc_event.loudness.clone(),
                    balance: rc_event.balance.clone(),
                    other_channels: rc_event.other_channels.clone(),
                };

                Ok(EventData::RenderingControlEvent(stream_event))
            }

            sonos_api::Service::ZoneGroupTopology => {
                let zgt_event = api_event_data
                    .downcast::<sonos_api::services::zone_group_topology::ZoneGroupTopologyEvent>()
                    .map_err(|_| EventProcessingError::Parsing("Failed to downcast ZoneGroupTopology event".to_string()))?;

                // Convert from sonos-api to sonos-stream types
                let stream_zone_groups = zgt_event.zone_groups.into_iter().map(|group| {
                    let stream_members = group.members.into_iter().map(|member| {
                        let stream_satellites = member.satellites.into_iter().map(|sat| {
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
                            satellites: stream_satellites,
                            metadata: member.metadata,
                        }
                    }).collect();

                    crate::events::types::ZoneGroupInfo {
                        coordinator: group.coordinator,
                        id: group.id,
                        members: stream_members,
                    }
                }).collect();

                let stream_event = crate::events::types::ZoneGroupTopologyEvent {
                    zone_groups: stream_zone_groups,
                    vanished_devices: zgt_event.vanished_devices,
                };

                Ok(EventData::ZoneGroupTopologyEvent(stream_event))
            }
        }
    }

    /// Start processing UPnP events from the callback server
    pub async fn start_upnp_processing(
        &self,
        mut upnp_receiver: mpsc::UnboundedReceiver<NotificationPayload>,
    ) {
        eprintln!("📡 Starting UPnP event processing (using sonos-api framework)");

        while let Some(payload) = upnp_receiver.recv().await {
            match self.process_upnp_notification(payload).await {
                Ok(()) => {
                    // Event processed successfully
                }
                Err(e) => {
                    eprintln!("❌ Failed to process UPnP event: {}", e);
                    let mut stats = self.stats.write().await;
                    stats.processing_errors += 1;
                }
            }
        }

        eprintln!("🛑 UPnP event processing stopped");
    }

    /// Start processing polling events
    pub async fn start_polling_processing(
        &self,
        mut polling_receiver: mpsc::UnboundedReceiver<EnrichedEvent>,
    ) {
        eprintln!("🔄 Starting polling event processing");

        while let Some(event) = polling_receiver.recv().await {
            match self.process_polling_event(event).await {
                Ok(()) => {
                    // Event processed successfully
                }
                Err(e) => {
                    eprintln!("❌ Failed to process polling event: {}", e);
                    let mut stats = self.stats.write().await;
                    stats.processing_errors += 1;
                }
            }
        }

        eprintln!("🛑 Polling event processing stopped");
    }

    /// Start processing resync events
    pub async fn start_resync_processing(
        &self,
        mut resync_receiver: mpsc::UnboundedReceiver<EnrichedEvent>,
    ) {
        eprintln!("🔄 Starting resync event processing");

        while let Some(event) = resync_receiver.recv().await {
            match self.process_resync_event(event).await {
                Ok(()) => {
                    // Event processed successfully
                }
                Err(e) => {
                    eprintln!("❌ Failed to process resync event: {}", e);
                    let mut stats = self.stats.write().await;
                    stats.processing_errors += 1;
                }
            }
        }

        eprintln!("🛑 Resync event processing stopped");
    }

    /// Get event processor statistics
    pub async fn stats(&self) -> EventProcessorStats {
        let stats = self.stats.read().await;
        stats.clone()
    }

    /// Get list of supported service types
    pub fn supported_services(&self) -> Vec<sonos_api::Service> {
        self.api_processor.supported_services()
    }

    /// Check if a service type is supported
    pub fn is_service_supported(&self, service: &sonos_api::Service) -> bool {
        self.api_processor.supports_service(service)
    }
}

/// Statistics about event processing (maintained for compatibility)
#[derive(Debug, Clone)]
pub struct EventProcessorStats {
    /// Total events processed successfully
    pub events_processed: u64,

    /// UPnP events received from callback server
    pub upnp_events_received: u64,

    /// Polling events received
    pub polling_events_received: u64,

    /// Resync events received
    pub resync_events_received: u64,

    /// Processing errors encountered
    pub processing_errors: u64,

    /// Events for unsupported services
    pub unsupported_services: u64,
}

impl EventProcessorStats {
    fn new() -> Self {
        Self {
            events_processed: 0,
            upnp_events_received: 0,
            polling_events_received: 0,
            resync_events_received: 0,
            processing_errors: 0,
            unsupported_services: 0,
        }
    }

    /// Get total events received (all sources)
    pub fn total_events_received(&self) -> u64 {
        self.upnp_events_received + self.polling_events_received + self.resync_events_received
    }

    /// Get processing success rate
    pub fn success_rate(&self) -> f64 {
        let total = self.total_events_received();
        if total == 0 {
            1.0
        } else {
            self.events_processed as f64 / total as f64
        }
    }
}

impl std::fmt::Display for EventProcessorStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Event Processor Stats:")?;
        writeln!(f, "  Total processed: {}", self.events_processed)?;
        writeln!(f, "  Success rate: {:.1}%", self.success_rate() * 100.0)?;
        writeln!(f, "  Event sources:")?;
        writeln!(f, "    UPnP events: {}", self.upnp_events_received)?;
        writeln!(f, "    Polling events: {}", self.polling_events_received)?;
        writeln!(f, "    Resync events: {}", self.resync_events_received)?;
        writeln!(f, "  Errors:")?;
        writeln!(f, "    Processing errors: {}", self.processing_errors)?;
        writeln!(f, "    Unsupported services: {}", self.unsupported_services)?;
        Ok(())
    }
}

/// Helper function to create an EventRouter integrated with EventProcessor
pub async fn create_integrated_event_router(
    _event_processor: Arc<EventProcessor>,
) -> (Arc<EventRouter>, mpsc::UnboundedReceiver<NotificationPayload>) {
    let (upnp_sender, upnp_receiver) = mpsc::unbounded_channel();
    let router = Arc::new(EventRouter::new(upnp_sender));

    (router, upnp_receiver)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::IpAddr;

    #[test]
    fn test_event_processor_creation() {
        let (event_sender, _event_receiver) = mpsc::unbounded_channel();
        let subscription_manager = Arc::new(SubscriptionManager::new(
            "http://callback.url".to_string(),
        ));

        let processor = EventProcessor::new(subscription_manager, event_sender, None);

        // Should have the supported services from sonos-api
        assert_eq!(processor.supported_services().len(), 4); // AVTransport, RenderingControl, GroupRenderingControl, ZoneGroupTopology
        assert!(processor.is_service_supported(&sonos_api::Service::AVTransport));
        assert!(processor.is_service_supported(&sonos_api::Service::RenderingControl));
        assert!(processor.is_service_supported(&sonos_api::Service::GroupRenderingControl));
        assert!(processor.is_service_supported(&sonos_api::Service::ZoneGroupTopology));
    }

    #[tokio::test]
    async fn test_event_processor_stats() {
        let (event_sender, _event_receiver) = mpsc::unbounded_channel();
        let subscription_manager = Arc::new(SubscriptionManager::new(
            "http://callback.url".to_string(),
        ));

        let processor = EventProcessor::new(subscription_manager, event_sender, None);

        let stats = processor.stats().await;
        assert_eq!(stats.events_processed, 0);
        assert_eq!(stats.total_events_received(), 0);
        assert_eq!(stats.success_rate(), 1.0);
    }
}