//! EventData trait implementations for parser types.
//!
//! This module provides EventData trait implementations for parser types
//! from the sonos-parser crate, enabling them to be used directly with
//! the typed event system.

use crate::event::EventData;
use crate::types::ServiceType;
use sonos_parser::services::av_transport::AVTransportParser;

/// Implement EventData for AVTransportParser to use it directly as event data.
impl EventData for AVTransportParser {
    fn event_type(&self) -> &str {
        "av_transport_event"
    }
    
    fn service_type(&self) -> ServiceType {
        ServiceType::AVTransport
    }
    
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    
    fn clone_box(&self) -> Box<dyn EventData> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::TypedEvent;

    #[test]
    fn test_av_transport_parser_event_data_trait() {
        let xml = r#"<e:propertyset xmlns:e="urn:schemas-upnp-org:event-1-0"><e:property><LastChange>&lt;Event xmlns=&quot;urn:schemas-upnp-org:metadata-1-0/AVT/&quot;&gt;&lt;InstanceID val=&quot;0&quot;&gt;&lt;TransportState val=&quot;PLAYING&quot;/&gt;&lt;CurrentTrackURI val=&quot;x-sonos-spotify:track123&quot;/&gt;&lt;/InstanceID&gt;&lt;/Event&gt;</LastChange></e:property></e:propertyset>"#;
        
        let parser = AVTransportParser::from_xml(xml).unwrap();
        
        // Test EventData trait methods
        assert_eq!(parser.event_type(), "av_transport_event");
        assert_eq!(parser.service_type(), ServiceType::AVTransport);
        
        // Test as_any method
        let any_ref = parser.as_any();
        let downcast_result = any_ref.downcast_ref::<AVTransportParser>();
        assert!(downcast_result.is_some());
        
        // Test clone_box method
        let cloned_box = parser.clone_box();
        assert_eq!(cloned_box.event_type(), "av_transport_event");
        assert_eq!(cloned_box.service_type(), ServiceType::AVTransport);
        
        // Test with TypedEvent
        let typed_event = TypedEvent::new(Box::new(parser));
        assert_eq!(typed_event.event_type(), "av_transport_event");
        assert_eq!(typed_event.service_type(), ServiceType::AVTransport);
        
        // Test downcasting from TypedEvent
        let downcast_parser = typed_event.downcast_ref::<AVTransportParser>();
        assert!(downcast_parser.is_some());
        
        let parser_ref = downcast_parser.unwrap();
        assert_eq!(parser_ref.transport_state(), "PLAYING");
        assert_eq!(parser_ref.current_track_uri(), Some("x-sonos-spotify:track123"));
    }
}