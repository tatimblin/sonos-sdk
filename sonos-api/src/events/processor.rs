//! Generic event processor for handling UPnP events across all services
//!
//! This module provides a service-agnostic event processor that can handle
//! events from any Sonos UPnP service using direct self-parsing methods.

use std::net::IpAddr;
use crate::{Result, Service};
use super::types::{EnrichedEvent, EventSource};

/// Generic event processor that can handle events from any service
pub struct EventProcessor;

impl EventProcessor {
    /// Create a new event processor
    pub fn new() -> Self {
        Self
    }

    /// Create a new event processor (alias for compatibility)
    pub fn with_default_parsers() -> Self {
        Self::new()
    }

    /// Process a UPnP event notification using direct event type parsing
    ///
    /// This method calls EventType::from_xml() directly based on the service type.
    pub fn process_upnp_event(
        &self,
        speaker_ip: IpAddr,
        service: Service,
        subscription_id: String,
        event_xml: &str,
    ) -> Result<EnrichedEvent<Box<dyn std::any::Any + Send + Sync>>> {
        let event_data = self.parse_event_for_service(&service, event_xml)?;
        let event_source = EventSource::UPnPNotification { subscription_id };

        Ok(EnrichedEvent::new(
            speaker_ip,
            service,
            event_source,
            event_data,
        ))
    }

    /// Process a polling-detected event using direct event type parsing
    pub fn process_polling_event(
        &self,
        speaker_ip: IpAddr,
        service: Service,
        poll_interval: std::time::Duration,
        event_xml: &str,
    ) -> Result<EnrichedEvent<Box<dyn std::any::Any + Send + Sync>>> {
        let event_data = self.parse_event_for_service(&service, event_xml)?;
        let event_source = EventSource::PollingDetection { poll_interval };

        Ok(EnrichedEvent::new(
            speaker_ip,
            service,
            event_source,
            event_data,
        ))
    }

    /// Process a resync event using direct event type parsing
    pub fn process_resync_event(
        &self,
        speaker_ip: IpAddr,
        service: Service,
        event_xml: &str,
    ) -> Result<EnrichedEvent<Box<dyn std::any::Any + Send + Sync>>> {
        let event_data = self.parse_event_for_service(&service, event_xml)?;
        let event_source = EventSource::ResyncOperation;

        Ok(EnrichedEvent::new(
            speaker_ip,
            service,
            event_source,
            event_data,
        ))
    }

    /// Parse event XML for the given service using direct EventType::from_xml() calls
    fn parse_event_for_service(
        &self,
        service: &Service,
        event_xml: &str,
    ) -> Result<Box<dyn std::any::Any + Send + Sync>> {
        match service {
            Service::AVTransport => {
                let event = crate::services::av_transport::AVTransportEvent::from_xml(event_xml)?;
                Ok(Box::new(event))
            }
            Service::RenderingControl => {
                let event = crate::services::rendering_control::RenderingControlEvent::from_xml(event_xml)?;
                Ok(Box::new(event))
            }
            Service::GroupRenderingControl => {
                let event = crate::services::group_rendering_control::GroupRenderingControlEvent::from_xml(event_xml)?;
                Ok(Box::new(event))
            }
            Service::ZoneGroupTopology => {
                let event = crate::services::zone_group_topology::ZoneGroupTopologyEvent::from_xml(event_xml)?;
                Ok(Box::new(event))
            }
            Service::GroupManagement => {
                let event = crate::services::group_management::GroupManagementEvent::from_xml(event_xml)?;
                Ok(Box::new(event))
            }
        }
    }

    /// Check if a service is supported by this processor
    pub fn supports_service(&self, service: &Service) -> bool {
        matches!(
            service,
            Service::AVTransport
                | Service::RenderingControl
                | Service::GroupRenderingControl
                | Service::ZoneGroupTopology
                | Service::GroupManagement
        )
    }

    /// Get all supported services
    pub fn supported_services(&self) -> Vec<Service> {
        vec![
            Service::AVTransport,
            Service::RenderingControl,
            Service::GroupRenderingControl,
            Service::ZoneGroupTopology,
            Service::GroupManagement,
        ]
    }
}

/// Event processing statistics
#[derive(Debug, Clone, Default)]
pub struct EventProcessorStats {
    /// Total events processed successfully
    pub events_processed: u64,

    /// UPnP events processed
    pub upnp_events: u64,

    /// Polling events processed
    pub polling_events: u64,

    /// Resync events processed
    pub resync_events: u64,

    /// Processing errors encountered
    pub processing_errors: u64,

    /// Events for unsupported services
    pub unsupported_services: u64,
}

impl EventProcessorStats {
    /// Get total events received (all sources)
    pub fn total_events(&self) -> u64 {
        self.upnp_events + self.polling_events + self.resync_events
    }

    /// Get processing success rate
    pub fn success_rate(&self) -> f64 {
        let total = self.total_events();
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
        writeln!(f, "    UPnP events: {}", self.upnp_events)?;
        writeln!(f, "    Polling events: {}", self.polling_events)?;
        writeln!(f, "    Resync events: {}", self.resync_events)?;
        writeln!(f, "  Errors:")?;
        writeln!(f, "    Processing errors: {}", self.processing_errors)?;
        writeln!(f, "    Unsupported services: {}", self.unsupported_services)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_processor_creation() {
        let processor = EventProcessor::new();

        // Should support all implemented services
        assert_eq!(processor.supported_services().len(), 5); // AVTransport, RenderingControl, GroupRenderingControl, ZoneGroupTopology, GroupManagement
    }

    #[test]
    fn test_event_processor_with_default_parsers() {
        let processor = EventProcessor::with_default_parsers();

        // Should be created without error
        // Should have parsers for all available services
        assert_eq!(processor.supported_services().len(), 5); // AVTransport, RenderingControl, GroupRenderingControl, ZoneGroupTopology, GroupManagement
        assert!(processor.supports_service(&Service::AVTransport));
        assert!(processor.supports_service(&Service::RenderingControl));
        assert!(processor.supports_service(&Service::GroupRenderingControl));
        assert!(processor.supports_service(&Service::ZoneGroupTopology));
        assert!(processor.supports_service(&Service::GroupManagement));
    }

    #[test]
    fn test_supported_services() {
        let processor = EventProcessor::new();

        // Should support all event-enabled services
        assert!(processor.supports_service(&Service::AVTransport));
        assert!(processor.supports_service(&Service::RenderingControl));
        assert!(processor.supports_service(&Service::GroupRenderingControl));
        assert!(processor.supports_service(&Service::ZoneGroupTopology));
        assert!(processor.supports_service(&Service::GroupManagement));
    }

    #[test]
    fn test_event_parsing_functionality() {
        let processor = EventProcessor::new();

        // Test AVTransport parsing
        let av_xml = r#"<e:propertyset xmlns:e="urn:schemas-upnp-org:event-1-0">
            <e:property>
                <LastChange>&lt;Event xmlns="urn:schemas-upnp-org:metadata-1-0/AVT/"&gt;
                    &lt;InstanceID val="0"&gt;
                        &lt;TransportState val="PLAYING"/&gt;
                    &lt;/InstanceID&gt;
                &lt;/Event&gt;</LastChange>
            </e:property>
        </e:propertyset>"#;

        let result = processor.process_upnp_event(
            "192.168.1.100".parse().unwrap(),
            Service::AVTransport,
            "uuid:123".to_string(),
            av_xml
        );

        assert!(result.is_ok());

        // Test RenderingControl parsing
        let rc_xml = r#"<e:propertyset xmlns:e="urn:schemas-upnp-org:event-1-0">
            <e:property>
                <LastChange>&lt;Event xmlns="urn:schemas-upnp-org:metadata-1-0/RCS/"&gt;
                    &lt;InstanceID val="0"&gt;
                        &lt;Volume val="50"/&gt;
                    &lt;/InstanceID&gt;
                &lt;/Event&gt;</LastChange>
            </e:property>
        </e:propertyset>"#;

        let result = processor.process_upnp_event(
            "192.168.1.100".parse().unwrap(),
            Service::RenderingControl,
            "uuid:456".to_string(),
            rc_xml
        );

        assert!(result.is_ok());

        // Test GroupRenderingControl parsing (direct properties, NOT LastChange)
        let grc_xml = r#"<e:propertyset xmlns:e="urn:schemas-upnp-org:event-1-0"><e:property><GroupVolume>14</GroupVolume></e:property><e:property><GroupMute>0</GroupMute></e:property><e:property><GroupVolumeChangeable>1</GroupVolumeChangeable></e:property></e:propertyset>"#;

        let result = processor.process_upnp_event(
            "192.168.1.100".parse().unwrap(),
            Service::GroupRenderingControl,
            "uuid:789".to_string(),
            grc_xml
        );

        assert!(result.is_ok());
        let enriched = result.unwrap();
        let grc_event = enriched.event_data
            .downcast::<crate::services::group_rendering_control::GroupRenderingControlEvent>()
            .expect("Should downcast to GroupRenderingControlEvent");
        assert_eq!(grc_event.group_volume(), Some(14));
        assert_eq!(grc_event.group_mute(), Some(false));
        assert_eq!(grc_event.group_volume_changeable(), Some(true));
    }

    #[test]
    fn test_event_processor_stats() {
        let stats = EventProcessorStats::default();
        assert_eq!(stats.total_events(), 0);
        assert_eq!(stats.success_rate(), 1.0);

        let stats = EventProcessorStats {
            events_processed: 8,
            upnp_events: 5,
            polling_events: 3,
            resync_events: 2,
            processing_errors: 2,
            unsupported_services: 0,
        };

        assert_eq!(stats.total_events(), 10);
        assert_eq!(stats.success_rate(), 0.8);
    }
}