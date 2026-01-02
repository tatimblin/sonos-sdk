//! Integration tests for AVTransportProvider.
//!
//! This test file verifies that the new AVTransportProvider can be used
//! as a drop-in replacement for the Strategy enum in real scenarios.

use sonos_stream::{AVTransportProvider, EventBrokerBuilder, ServiceStrategy};
use std::time::Duration;

#[tokio::test]
async fn test_av_transport_provider_integration() {
    // Test that AVTransportProvider can be used with EventBrokerBuilder
    let provider = AVTransportProvider::new();
    
    // Verify provider configuration
    assert_eq!(provider.service_type(), sonos_stream::ServiceType::AVTransport);
    assert_eq!(provider.subscription_scope(), sonos_stream::SubscriptionScope::PerSpeaker);
    assert_eq!(provider.service_endpoint_path(), "/MediaRenderer/AVTransport/Event");
    
    // Test that it can be used with the builder
    let builder = EventBrokerBuilder::new()
        .with_strategy(Box::new(provider))
        .with_port_range(50300, 50400) // Use high port range to avoid conflicts
        .with_subscription_timeout(Duration::from_secs(1800));
    
    // Build should succeed
    let result = builder.build().await;
    assert!(result.is_ok(), "EventBroker should build successfully with AVTransportProvider");
    
    // Clean up
    if let Ok(broker) = result {
        let _ = broker.shutdown().await;
    }
}

#[tokio::test]
async fn test_av_transport_provider_parsing() {
    let provider = AVTransportProvider::new();
    let speaker_id = sonos_stream::SpeakerId::new("test_speaker");
    
    // Test parsing valid AVTransport XML
    let valid_xml = r#"<e:propertyset xmlns:e="urn:schemas-upnp-org:event-1-0"><e:property><LastChange>&lt;Event xmlns="urn:schemas-upnp-org:metadata-1-0/AVT/"&gt;&lt;InstanceID val="0"&gt;&lt;TransportState val="PLAYING"/&gt;&lt;CurrentTrackURI val="x-sonos-spotify:track123"/&gt;&lt;/InstanceID&gt;&lt;/Event&gt;</LastChange></e:property></e:propertyset>"#;
    
    let result = provider.parse_event(&speaker_id, valid_xml);
    assert!(result.is_ok(), "Should parse valid AVTransport XML");
    
    let typed_event = result.unwrap();
    assert_eq!(typed_event.event_type(), "av_transport_event");
    assert_eq!(typed_event.service_type(), sonos_stream::ServiceType::AVTransport);
    
    // Test downcasting to AVTransportParser
    let parser = typed_event.downcast_ref::<sonos_parser::services::av_transport::AVTransportParser>();
    assert!(parser.is_some(), "Should be able to downcast to AVTransportParser");
    
    let parser = parser.unwrap();
    assert_eq!(parser.transport_state(), "PLAYING");
    assert_eq!(parser.current_track_uri(), Some("x-sonos-spotify:track123"));
}

#[test]
fn test_av_transport_provider_thread_safety() {
    let provider = AVTransportProvider::new();
    
    // Test that provider can be sent across threads
    let handle = std::thread::spawn(move || {
        assert_eq!(provider.service_type(), sonos_stream::ServiceType::AVTransport);
    });
    
    handle.join().unwrap();
}

#[test]
fn test_av_transport_provider_clone() {
    let provider = AVTransportProvider::new();
    let cloned = provider.clone();
    
    // Verify that cloned provider has same configuration
    assert_eq!(provider.service_type(), cloned.service_type());
    assert_eq!(provider.subscription_scope(), cloned.subscription_scope());
    assert_eq!(provider.service_endpoint_path(), cloned.service_endpoint_path());
}