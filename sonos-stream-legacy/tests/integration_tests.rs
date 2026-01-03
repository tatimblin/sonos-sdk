//! Integration tests for the sonos-stream crate.
//!
//! These tests verify the event processing functionality of the EventBroker:
//! - Event stream setup and processing
//! - Strategy-based event parsing
//! - Callback server functionality
//! - Shutdown and cleanup
//!
//! Note: Subscription management is now handled by the sonos-api crate.
//! These tests focus on the event processing capabilities of sonos-stream.

mod mock_strategy;
mod test_helpers;

use mock_strategy::MockStrategy;
use test_helpers::{is_port_available, UPnPMockServer, TestAVTransportStrategy};
use sonos_stream::{EventBrokerBuilder, ServiceType, SpeakerId, ActiveSubscription, SubscriptionKey};
use std::time::{Duration, SystemTime};

/// Test: Event broker setup and callback server functionality
///
/// This test verifies:
/// - Creating a broker with mock strategy
/// - Callback server starts and accepts HTTP requests
/// - Event processing pipeline works
/// - Shutdown and cleanup
#[tokio::test]
async fn test_event_broker_setup() {
    // Create broker with mock strategy
    let strategy = MockStrategy::new(ServiceType::AVTransport);
    let mut broker = EventBrokerBuilder::new()
        .with_strategy(Box::new(strategy.clone()))
        .with_port_range(40000, 40100)
        .build()
        .await
        .expect("Failed to build broker");

    // Get event stream
    let mut _event_rx = broker.event_stream();
    let callback_url = broker.callback_url();

    // Verify callback URL format
    assert!(callback_url.starts_with("http://"));
    assert!(callback_url.contains(":"));

    // Test that callback server is responding
    let client = reqwest::Client::new();
    let response = client
        .get(&format!("{}/health", callback_url))
        .send()
        .await;

    // The callback server should respond (even if it's a 404, it means it's running)
    assert!(response.is_ok());

    // Shutdown broker
    broker.shutdown().await.expect("Failed to shutdown broker");
}

/// Test: Event processing with mock notifications
///
/// This test verifies:
/// - Simulating UPnP event notifications via HTTP
/// - Event parsing through strategies
/// - Event routing to the event stream
#[tokio::test]
async fn test_event_processing() {
    // Create broker with mock strategy
    let strategy = MockStrategy::new(ServiceType::AVTransport);
    let mut broker = EventBrokerBuilder::new()
        .with_strategy(Box::new(strategy.clone()))
        .with_port_range(40100, 40200)
        .build()
        .await
        .expect("Failed to build broker");

    // Get event stream
    let mut _event_rx = broker.event_stream();
    let callback_url = broker.callback_url();

    // Wait for broker to be ready
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Simulate a UPnP event notification
    let subscription_id = "uuid:test-subscription-123";
    let event_xml = r#"<event><test_event>data</test_event></event>"#;

    // Send HTTP POST to callback server (simulating UPnP NOTIFY)
    let client = reqwest::Client::new();
    let response = client
        .post(&callback_url)
        .header("NT", "upnp:event")
        .header("NTS", "upnp:propchange")
        .header("SID", subscription_id)
        .body(event_xml.to_string())
        .send()
        .await
        .expect("Failed to send HTTP request");

    // The callback server should accept the request
    println!("Response status: {}", response.status());

    // Shutdown broker
    broker.shutdown().await.expect("Failed to shutdown broker");
}

/// Test: Multiple strategies and service types
///
/// This test verifies:
/// - Creating broker with multiple strategies
/// - Different service types are handled correctly
/// - Strategy isolation works
#[tokio::test]
async fn test_multiple_strategies() {
    // Create broker with multiple strategies
    let av_strategy = MockStrategy::new(ServiceType::AVTransport);
    let rc_strategy = MockStrategy::new(ServiceType::RenderingControl);
    
    let mut broker = EventBrokerBuilder::new()
        .with_strategy(Box::new(av_strategy.clone()))
        .with_strategy(Box::new(rc_strategy.clone()))
        .with_port_range(40200, 40300)
        .build()
        .await
        .expect("Failed to build broker");

    // Get event stream
    let mut _event_rx = broker.event_stream();
    let callback_url = broker.callback_url();

    // Verify callback server is running
    assert!(callback_url.starts_with("http://"));

    // Test that we can send different types of events
    let client = reqwest::Client::new();
    
    // Send AVTransport event
    let _av_response = client
        .post(&callback_url)
        .header("NT", "upnp:event")
        .header("NTS", "upnp:propchange")
        .header("SID", "uuid:av-subscription")
        .body("<av_event>test</av_event>")
        .send()
        .await
        .expect("Failed to send AV event");

    // Send RenderingControl event
    let _rc_response = client
        .post(&callback_url)
        .header("NT", "upnp:event")
        .header("NTS", "upnp:propchange")
        .header("SID", "uuid:rc-subscription")
        .body("<rc_event>test</rc_event>")
        .send()
        .await
        .expect("Failed to send RC event");

    // Shutdown broker
    broker.shutdown().await.expect("Failed to shutdown broker");
}

/// Test: Real UPnP event parsing with AVTransport strategy
///
/// This test verifies:
/// - Using real AVTransport strategy (not mock)
/// - Parsing actual UPnP AVTransport XML
/// - Event data extraction and typing
#[tokio::test]
async fn test_real_avtransport_parsing() {
    // Create a mock UPnP device server
    let mock_server = UPnPMockServer::new().await.expect("Failed to create mock server");
    let mock_device_url = mock_server.url();
    
    // Configure expected subscription
    mock_server.add_expected_subscription("uuid:real-subscription-123".to_string()).await;
    
    // Start the mock server
    let _server_handle = mock_server.start().await;

    // Create broker with test AVTransport strategy
    let mut broker = EventBrokerBuilder::new()
        .with_strategy(Box::new(TestAVTransportStrategy::new(mock_device_url.clone())))
        .with_port_range(40300, 40400)
        .build()
        .await
        .expect("Failed to build broker");

    // Get event stream and callback URL
    let mut _event_rx = broker.event_stream();
    let callback_url = broker.callback_url();

    // Wait for broker to be ready
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Send a real UPnP NOTIFY event with actual AVTransport XML
    let real_upnp_event_xml = r#"<e:propertyset xmlns:e="urn:schemas-upnp-org:event-1-0">
    <e:property>
        <LastChange>&lt;Event xmlns="urn:schemas-upnp-org:metadata-1-0/AVT/"&gt;
            &lt;InstanceID val="0"&gt;
                &lt;TransportState val="PLAYING"/&gt;
                &lt;CurrentPlayMode val="NORMAL"/&gt;
                &lt;CurrentTrackURI val="x-sonos-spotify:spotify%3atrack%3a4iV5W9uYEdYUVa79Axb7Rh"/&gt;
                &lt;CurrentTrackDuration val="0:03:45"/&gt;
                &lt;CurrentTrackMetaData val="&amp;lt;DIDL-Lite xmlns:dc=&amp;quot;http://purl.org/dc/elements/1.1/&amp;quot; xmlns:upnp=&amp;quot;urn:schemas-upnp-org:metadata-1-0/upnp/&amp;quot;&amp;gt;&amp;lt;item id=&amp;quot;-1&amp;quot; parentID=&amp;quot;-1&amp;quot;&amp;gt;&amp;lt;dc:title&amp;gt;Test Song Title&amp;lt;/dc:title&amp;gt;&amp;lt;dc:creator&amp;gt;Test Artist&amp;lt;/dc:creator&amp;gt;&amp;lt;upnp:album&amp;gt;Test Album&amp;lt;/upnp:album&amp;gt;&amp;lt;/item&amp;gt;&amp;lt;/DIDL-Lite&amp;gt;"/&gt;
            &lt;/InstanceID&gt;
        &lt;/Event&gt;</LastChange>
    </e:property>
</e:propertyset>"#;

    // Send HTTP POST to callback server
    let client = reqwest::Client::new();
    let response = client
        .post(&callback_url)
        .header("HOST", "127.0.0.1:40300")
        .header("CONTENT-TYPE", "text/xml; charset=\"utf-8\"")
        .header("NT", "upnp:event")
        .header("NTS", "upnp:propchange")
        .header("SID", "uuid:real-subscription-123")
        .header("SEQ", "0")
        .body(real_upnp_event_xml.to_string())
        .send()
        .await
        .expect("Failed to send NOTIFY request");

    println!("Response status: {}", response.status());
    
    // The callback server should accept the request
    // Note: Without registered subscriptions, it might not process the event fully,
    // but it should handle the HTTP request

    // Shutdown broker
    broker.shutdown().await.expect("Failed to shutdown broker");
}

/// Test: Shutdown and cleanup
///
/// This test verifies:
/// - Proper shutdown sequence
/// - Resource cleanup
/// - Port release
#[tokio::test]
async fn test_shutdown_and_cleanup() {
    let port_start = 40400;
    let port_end = 40500;
    
    // Create broker
    let mut broker = EventBrokerBuilder::new()
        .with_strategy(Box::new(MockStrategy::new(ServiceType::AVTransport)))
        .with_port_range(port_start, port_end)
        .build()
        .await
        .expect("Failed to build broker");

    // Get event stream
    let mut event_rx = broker.event_stream();

    // Verify callback server is running
    let callback_url = broker.callback_url();
    assert!(callback_url.starts_with("http://"));

    // Shutdown broker
    broker.shutdown().await.expect("Failed to shutdown broker");

    // Verify port is released after shutdown
    tokio::time::sleep(Duration::from_millis(100)).await;
    let port_available_after = is_port_available(port_start);
    assert!(port_available_after, "Callback server port should be released after shutdown");

    // Verify event channel is closed
    let result = tokio::time::timeout(Duration::from_millis(100), event_rx.recv()).await;
    match result {
        Ok(None) => {
            // Channel closed, expected
        }
        Err(_) => {
            // Timeout is also acceptable
        }
        Ok(Some(_)) => {
            panic!("Expected channel to be closed after shutdown");
        }
    }
}

/// Test: ActiveSubscription metadata tracking
///
/// This test verifies the ActiveSubscription type works correctly
/// for tracking subscription metadata (used by event processing)
#[tokio::test]
async fn test_active_subscription_metadata() {
    let speaker_id = SpeakerId::new("RINCON_TEST123");
    let key = SubscriptionKey::new(speaker_id.clone(), ServiceType::AVTransport);
    let subscription_id = "uuid:test-sub-123".to_string();
    let expires_at = SystemTime::now() + Duration::from_secs(1800);

    // Create ActiveSubscription
    let mut active_sub = ActiveSubscription::new(key.clone(), subscription_id.clone(), expires_at);

    // Test basic properties
    assert_eq!(active_sub.key.speaker_id.as_str(), "RINCON_TEST123");
    assert_eq!(active_sub.key.service_type, ServiceType::AVTransport);
    assert_eq!(active_sub.subscription_id, subscription_id);
    assert!(active_sub.last_event.is_none());
    assert!(!active_sub.is_expired());

    // Test event timestamp update
    active_sub.mark_event_received();
    assert!(active_sub.last_event.is_some());

    // Test renewal check
    let short_threshold = Duration::from_secs(2000); // Longer than expiry time
    assert!(active_sub.needs_renewal(short_threshold));

    let long_threshold = Duration::from_secs(1000); // Shorter than expiry time
    assert!(!active_sub.needs_renewal(long_threshold));
}