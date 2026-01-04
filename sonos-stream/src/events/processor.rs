//! Event processing and enrichment
//!
//! This module handles processing of both UPnP events and synthetic polling events,
//! enriching them with context and routing them to the event stream.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};

use callback_server::{router::{EventRouter, NotificationPayload}, FirewallDetectionCoordinator};

use crate::error::{EventProcessingError, EventProcessingResult};
use crate::events::types::{
    AVTransportEvent, RenderingControlEvent, DevicePropertiesEvent,
    EnrichedEvent, EventData, EventSource,
    ZoneGroupTopologyEvent, ZoneGroupInfo, ZoneGroupMemberInfo, NetworkInfo, SatelliteInfo,
};
use crate::subscription::manager::SubscriptionManager;

/// Helper function to extract values from XML using basic text matching
/// This is a temporary solution until proper parsers are implemented
fn extract_xml_value(xml: &str, tag: &str) -> Option<String> {
    let start_tag = format!("<{}>", tag);
    let end_tag = format!("</{}>", tag);

    if let Some(start_pos) = xml.find(&start_tag) {
        let content_start = start_pos + start_tag.len();
        if let Some(end_pos) = xml[content_start..].find(&end_tag) {
            let value = xml[content_start..content_start + end_pos].trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

/// Trait for service-specific event parsers
pub trait EventParser: Send + Sync {
    /// Parse UPnP event XML and extract changes
    fn parse_upnp_event(&self, xml: &str) -> EventProcessingResult<EventData>;

    /// Get the service type this parser handles
    fn service_type(&self) -> sonos_api::Service;
}

/// Event parser for AVTransport service
pub struct AVTransportEventParser;

impl EventParser for AVTransportEventParser {
    fn parse_upnp_event(&self, xml: &str) -> EventProcessingResult<EventData> {
        // Parse UPnP event XML using sonos-parser
        let parsed = sonos_parser::services::av_transport::AVTransportParser::from_xml(xml)
            .map_err(|e| EventProcessingError::Parsing(format!("Failed to parse AVTransport XML: {}", e)))?;

        // Create complete event data from parsed UPnP event
        let instance = &parsed.property.last_change.instance;

        let event = AVTransportEvent {
            transport_state: Some(parsed.transport_state().to_string()),
            transport_status: instance.transport_status.as_ref().map(|v| v.val.clone()),
            speed: None, // Speed field not available in current parser
            current_track_uri: parsed.current_track_uri().map(|s| s.to_string()),
            track_duration: parsed.current_track_duration().map(|s| s.to_string()),
            rel_time: instance.relative_time_position.as_ref().map(|v| v.val.clone()),
            abs_time: instance.absolute_time_position.as_ref().map(|v| v.val.clone()),
            rel_count: instance.current_track.as_ref().and_then(|v| v.val.parse().ok()),
            abs_count: None, // No absolute count field in current parser
            play_mode: instance.current_play_mode.as_ref().map(|v| v.val.clone()),
            track_metadata: parsed.track_metadata().map(|_didl| {
                // Convert DIDL-Lite to string representation
                format!("Title: {:?}, Artist: {:?}, Album: {:?}",
                    parsed.track_title(),
                    parsed.track_artist(),
                    parsed.track_album())
            }),
            next_track_uri: instance.next_track_uri.as_ref().map(|v| v.val.clone()),
            next_track_metadata: instance.next_track_metadata.as_ref().map(|v| v.val.clone()),
            queue_length: instance.number_of_tracks.as_ref().and_then(|v| v.val.parse().ok()),
        };

        Ok(EventData::AVTransportEvent(event))
    }

    fn service_type(&self) -> sonos_api::Service {
        sonos_api::Service::AVTransport
    }
}

/// Event parser for RenderingControl service
pub struct RenderingControlEventParser;

impl EventParser for RenderingControlEventParser {
    fn parse_upnp_event(&self, xml: &str) -> EventProcessingResult<EventData> {
        // TODO: Implement proper RenderingControl parser when sonos-parser supports it
        // For now, create a basic event structure

        // Parse basic XML structure to extract any volume/mute information
        // This is a simplified implementation until a proper parser is available
        let event = RenderingControlEvent {
            master_volume: extract_xml_value(xml, "Volume"),
            lf_volume: None,
            rf_volume: None,
            master_mute: extract_xml_value(xml, "Mute"),
            lf_mute: None,
            rf_mute: None,
            bass: extract_xml_value(xml, "Bass"),
            treble: extract_xml_value(xml, "Treble"),
            loudness: extract_xml_value(xml, "Loudness"),
            balance: extract_xml_value(xml, "Balance"),
            other_channels: std::collections::HashMap::new(),
        };

        Ok(EventData::RenderingControlEvent(event))
    }

    fn service_type(&self) -> sonos_api::Service {
        sonos_api::Service::RenderingControl
    }
}

/// Event parser for ZoneGroupTopology service
pub struct ZoneGroupTopologyEventParser;

impl EventParser for ZoneGroupTopologyEventParser {
    fn parse_upnp_event(&self, xml: &str) -> EventProcessingResult<EventData> {
        // Parse the UPnP event XML using sonos-parser
        let parsed = sonos_parser::services::zone_group_topology::ZoneGroupTopologyParser::from_xml(xml)
            .map_err(|e| EventProcessingError::Parsing(format!("Failed to parse ZoneGroupTopology XML: {}", e)))?;

        // Extract zone group state from the parsed data
        let zone_group_state = parsed.zone_group_state()
            .ok_or_else(|| EventProcessingError::Parsing("No ZoneGroupState found in event".to_string()))?;

        // Convert from parser types to event types
        let zone_groups = zone_group_state.zone_groups.zone_groups
            .iter()
            .map(|group| {
                let members = group.zone_group_members
                    .iter()
                    .map(|member| {
                        // Collect all additional metadata
                        let mut metadata = std::collections::HashMap::new();
                        metadata.insert("icon".to_string(), member.icon.clone());
                        metadata.insert("configuration".to_string(), member.configuration.clone());
                        metadata.insert("sw_gen".to_string(), member.sw_gen.clone());
                        metadata.insert("min_compatible_version".to_string(), member.min_compatible_version.clone());
                        metadata.insert("legacy_compatible_version".to_string(), member.legacy_compatible_version.clone());
                        metadata.insert("boot_seq".to_string(), member.boot_seq.clone());
                        metadata.insert("ssl_port".to_string(), member.ssl_port.clone());
                        metadata.insert("hhssl_port".to_string(), member.hhssl_port.clone());
                        metadata.insert("idle_state".to_string(), member.idle_state.clone());
                        metadata.insert("more_info".to_string(), member.more_info.clone());
                        metadata.insert("orientation".to_string(), member.orientation.clone());
                        metadata.insert("room_calibration_state".to_string(), member.room_calibration_state.clone());
                        metadata.insert("secure_reg_state".to_string(), member.secure_reg_state.clone());
                        metadata.insert("voice_config_state".to_string(), member.voice_config_state.clone());
                        metadata.insert("mic_enabled".to_string(), member.mic_enabled.clone());
                        metadata.insert("airplay_enabled".to_string(), member.airplay_enabled.clone());

                        if let Some(ref ht_sat_chan) = member.ht_sat_chan_map_set {
                            metadata.insert("ht_sat_chan_map_set".to_string(), ht_sat_chan.clone());
                        }
                        if let Some(ref active_zone) = member.active_zone_id {
                            metadata.insert("active_zone_id".to_string(), active_zone.clone());
                        }
                        if let Some(ref vli_source) = member.virtual_line_in_source {
                            metadata.insert("virtual_line_in_source".to_string(), vli_source.clone());
                        }

                        // Convert satellites
                        let satellites = member.satellites
                            .iter()
                            .map(|sat| SatelliteInfo {
                                uuid: sat.uuid.clone(),
                                location: sat.location.clone(),
                                zone_name: sat.zone_name.clone(),
                                ht_sat_chan_map_set: sat.ht_sat_chan_map_set.clone(),
                                invisible: sat.invisible.clone(),
                            })
                            .collect();

                        ZoneGroupMemberInfo {
                            uuid: member.uuid.clone(),
                            location: member.location.clone(),
                            zone_name: member.zone_name.clone(),
                            software_version: member.software_version.clone(),
                            network_info: NetworkInfo {
                                wireless_mode: member.wireless_mode.clone(),
                                wifi_enabled: member.wifi_enabled.clone(),
                                eth_link: member.eth_link.clone(),
                                channel_freq: member.channel_freq.clone(),
                                behind_wifi_extender: member.behind_wifi_extender.clone(),
                            },
                            satellites,
                            metadata,
                        }
                    })
                    .collect();

                ZoneGroupInfo {
                    coordinator: group.coordinator.clone(),
                    id: group.id.clone(),
                    members,
                }
            })
            .collect();

        let topology_event = ZoneGroupTopologyEvent {
            zone_groups,
            vanished_devices: Vec::new(), // TODO: Parse vanished devices if needed
        };

        Ok(EventData::ZoneGroupTopologyEvent(topology_event))
    }

    fn service_type(&self) -> sonos_api::Service {
        sonos_api::Service::ZoneGroupTopology
    }
}

/// Event parser for DeviceProperties service
pub struct DevicePropertiesEventParser;

impl EventParser for DevicePropertiesEventParser {
    fn parse_upnp_event(&self, xml: &str) -> EventProcessingResult<EventData> {
        // TODO: Implement proper DeviceProperties parser when available
        // For now, create basic event structure with extracted values

        let event = DevicePropertiesEvent {
            zone_name: extract_xml_value(xml, "ZoneName"),
            zone_icon: extract_xml_value(xml, "Icon"),
            configuration: extract_xml_value(xml, "Configuration"),
            capabilities: extract_xml_value(xml, "Capabilities"),
            software_version: extract_xml_value(xml, "SoftwareVersion"),
            model_name: extract_xml_value(xml, "ModelName"),
            display_version: extract_xml_value(xml, "DisplayVersion"),
            hardware_version: extract_xml_value(xml, "HardwareVersion"),
            additional_properties: std::collections::HashMap::new(),
        };

        Ok(EventData::DevicePropertiesEvent(event))
    }

    fn service_type(&self) -> sonos_api::Service {
        // DeviceProperties service doesn't exist in sonos-api, using ZoneGroupTopology as fallback
        sonos_api::Service::ZoneGroupTopology
    }
}

/// Main event processor that coordinates event parsing, enrichment, and routing
pub struct EventProcessor {
    /// Service-specific event parsers
    service_parsers: HashMap<sonos_api::Service, Box<dyn EventParser>>,


    /// Subscription manager for looking up subscriptions by SID
    subscription_manager: Arc<SubscriptionManager>,

    /// Sender for enriched events
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
        let mut service_parsers: HashMap<sonos_api::Service, Box<dyn EventParser>> =
            HashMap::new();

        // Register service-specific parsers
        service_parsers.insert(
            sonos_api::Service::AVTransport,
            Box::new(AVTransportEventParser),
        );
        service_parsers.insert(
            sonos_api::Service::RenderingControl,
            Box::new(RenderingControlEventParser),
        );
        service_parsers.insert(
            sonos_api::Service::ZoneGroupTopology,
            Box::new(ZoneGroupTopologyEventParser),
        );

        Self {
            service_parsers,
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

        // Parse the event XML using service-specific parser
        let event_data = match self.service_parsers.get(&pair.service) {
            Some(parser) => parser.parse_upnp_event(&payload.event_xml)?,
            None => {
                let mut stats = self.stats.write().await;
                stats.unsupported_services += 1;
                return Err(EventProcessingError::Parsing(format!(
                    "No parser available for service: {:?}",
                    pair.service
                )));
            }
        };

        // Create enriched event
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

    /// Start processing UPnP events from the callback server
    pub async fn start_upnp_processing(
        &self,
        mut upnp_receiver: mpsc::UnboundedReceiver<NotificationPayload>,
    ) {
        eprintln!("📡 Starting UPnP event processing");

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
        self.service_parsers.keys().cloned().collect()
    }

    /// Check if a service type is supported
    pub fn is_service_supported(&self, service: &sonos_api::Service) -> bool {
        self.service_parsers.contains_key(service)
    }
}

/// Statistics about event processing
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

        assert_eq!(processor.supported_services().len(), 3);
        assert!(processor.is_service_supported(&sonos_api::Service::AVTransport));
        assert!(processor.is_service_supported(&sonos_api::Service::RenderingControl));
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

    #[test]
    fn test_service_parser_types() {
        let av_parser = AVTransportEventParser;
        let rc_parser = RenderingControlEventParser;

        assert_eq!(av_parser.service_type(), sonos_api::Service::AVTransport);
        assert_eq!(
            rc_parser.service_type(),
            sonos_api::Service::RenderingControl
        );
    }

    #[test]
    fn test_event_processor_stats_calculations() {
        let mut stats = EventProcessorStats::new();
        stats.upnp_events_received = 10;
        stats.polling_events_received = 5;
        stats.resync_events_received = 2;
        stats.events_processed = 15;

        assert_eq!(stats.total_events_received(), 17);
        assert!((stats.success_rate() - (15.0 / 17.0)).abs() < 0.01);
    }

    // Note: More comprehensive tests would require mocking the subscription manager
    // and registry components, which is beyond the scope of this basic test suite
}