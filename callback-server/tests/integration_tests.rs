//! Integration tests for the callback server.
//!
//! These tests start a real HTTP server, send actual HTTP requests,
//! and verify end-to-end functionality.

use callback_server::{CallbackServer, NotificationPayload};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::timeout;

/// Test that the callback server can start, receive events, and process them correctly.
#[tokio::test]
async fn test_callback_server_end_to_end() {
    // Create a channel to receive notifications
    let (tx, mut rx) = mpsc::unbounded_channel::<NotificationPayload>();

    // Start the callback server
    let server = CallbackServer::new((50000, 50100), tx)
        .await
        .expect("Failed to create callback server");

    let base_url = server.base_url().to_string();

    println!("Server started at: {}", base_url);

    // Register a subscription
    let subscription_id = "test-subscription-123".to_string();
    server.router().register(subscription_id.clone()).await;

    // Create HTTP client
    let client = reqwest::Client::new();

    // Test 1: Send a valid UPnP event notification
    let event_xml = r#"<?xml version="1.0"?>
<e:propertyset xmlns:e="urn:schemas-upnp-org:event-1-0">
    <e:property>
        <TransportState>PLAYING</TransportState>
    </e:property>
    <e:property>
        <CurrentTrackURI>x-sonos-spotify:spotify%3atrack%3a1234567890</CurrentTrackURI>
    </e:property>
</e:propertyset>"#;

    let notify_url = format!("{}/notify/{}", base_url, subscription_id);
    
    let response = client
        .post(&notify_url)
        .header("SID", format!("uuid:{}", subscription_id))
        .header("NT", "upnp:event")
        .header("NTS", "upnp:propchange")
        .header("Content-Type", "text/xml")
        .body(event_xml.to_string())
        .send()
        .await
        .expect("Failed to send HTTP request");

    assert_eq!(response.status(), 200);

    // Verify we received the notification
    let notification = timeout(Duration::from_secs(1), rx.recv())
        .await
        .expect("Timeout waiting for notification")
        .expect("No notification received");

    assert_eq!(notification.subscription_id, subscription_id);
    assert!(notification.event_xml.contains("TransportState"));
    assert!(notification.event_xml.contains("PLAYING"));

    // Test 2: Send event with SID header only (no NT/NTS)
    let event_xml2 = r#"<?xml version="1.0"?>
<e:propertyset xmlns:e="urn:schemas-upnp-org:event-1-0">
    <e:property>
        <Volume>50</Volume>
    </e:property>
</e:propertyset>"#;

    let response2 = client
        .post(&notify_url)
        .header("SID", format!("uuid:{}", subscription_id))
        .header("Content-Type", "text/xml")
        .body(event_xml2.to_string())
        .send()
        .await
        .expect("Failed to send second HTTP request");

    assert_eq!(response2.status(), 200);

    // Verify we received the second notification
    let notification2 = timeout(Duration::from_secs(1), rx.recv())
        .await
        .expect("Timeout waiting for second notification")
        .expect("No second notification received");

    assert_eq!(notification2.subscription_id, subscription_id);
    assert!(notification2.event_xml.contains("Volume"));
    assert!(notification2.event_xml.contains("50"));

    // Test 3: Send event for unregistered subscription (should get 404)
    let unregistered_url = format!("{}/notify/unregistered-sub", base_url);
    
    let response3 = client
        .post(&unregistered_url)
        .header("SID", "uuid:unregistered-sub")
        .header("Content-Type", "text/xml")
        .body("<event>test</event>")
        .send()
        .await
        .expect("Failed to send third HTTP request");

    assert_eq!(response3.status(), 404);

    // Verify no notification was received for unregistered subscription
    let no_notification = timeout(Duration::from_millis(100), rx.recv()).await;
    assert!(no_notification.is_err(), "Should not receive notification for unregistered subscription");

    // Test 4: Send invalid request (missing SID header)
    let response4 = client
        .post(&notify_url)
        .header("Content-Type", "text/xml")
        .body("<event>test</event>")
        .send()
        .await
        .expect("Failed to send fourth HTTP request");

    assert_eq!(response4.status(), 400);

    // Cleanup
    server.shutdown().await.expect("Failed to shutdown server");
}

/// Test multiple subscriptions and concurrent events.
#[tokio::test]
async fn test_multiple_subscriptions_concurrent_events() {
    let (tx, mut rx) = mpsc::unbounded_channel::<NotificationPayload>();
    let server = CallbackServer::new((50200, 50300), tx)
        .await
        .expect("Failed to create callback server");

    let base_url = server.base_url().to_string();

    // Register multiple subscriptions
    let sub1 = "subscription-1".to_string();
    let sub2 = "subscription-2".to_string();
    let sub3 = "subscription-3".to_string();

    server.router().register(sub1.clone()).await;
    server.router().register(sub2.clone()).await;
    server.router().register(sub3.clone()).await;

    let client = reqwest::Client::new();

    // Send events concurrently to all subscriptions
    let handles = vec![
        tokio::spawn({
            let client = client.clone();
            let base_url = base_url.clone();
            let sub1 = sub1.clone();
            async move {
                client
                    .post(format!("{}/notify/{}", base_url, sub1))
                    .header("SID", format!("uuid:{}", sub1))
                    .body("<event>data1</event>")
                    .send()
                    .await
            }
        }),
        tokio::spawn({
            let client = client.clone();
            let base_url = base_url.clone();
            let sub2 = sub2.clone();
            async move {
                client
                    .post(format!("{}/notify/{}", base_url, sub2))
                    .header("SID", format!("uuid:{}", sub2))
                    .body("<event>data2</event>")
                    .send()
                    .await
            }
        }),
        tokio::spawn({
            let client = client.clone();
            let base_url = base_url.clone();
            let sub3 = sub3.clone();
            async move {
                client
                    .post(format!("{}/notify/{}", base_url, sub3))
                    .header("SID", format!("uuid:{}", sub3))
                    .body("<event>data3</event>")
                    .send()
                    .await
            }
        }),
    ];

    // Wait for all requests to complete
    for handle in handles {
        let response = handle.await.expect("Task failed").expect("HTTP request failed");
        assert_eq!(response.status(), 200);
    }

    // Collect all notifications
    let mut notifications = Vec::new();
    for _ in 0..3 {
        let notification = timeout(Duration::from_secs(1), rx.recv())
            .await
            .expect("Timeout waiting for notification")
            .expect("No notification received");
        notifications.push(notification);
    }

    // Verify we received notifications for all subscriptions
    let mut received_subs: Vec<String> = notifications
        .iter()
        .map(|n| n.subscription_id.clone())
        .collect();
    received_subs.sort();

    let mut expected_subs = vec![sub1, sub2, sub3];
    expected_subs.sort();

    assert_eq!(received_subs, expected_subs);

    // Verify each notification has the correct content
    for notification in notifications {
        match notification.subscription_id.as_str() {
            "subscription-1" => assert!(notification.event_xml.contains("data1")),
            "subscription-2" => assert!(notification.event_xml.contains("data2")),
            "subscription-3" => assert!(notification.event_xml.contains("data3")),
            _ => panic!("Unexpected subscription ID: {}", notification.subscription_id),
        }
    }

    server.shutdown().await.expect("Failed to shutdown server");
}

/// Test subscription registration and unregistration during server operation.
#[tokio::test]
async fn test_dynamic_subscription_management() {
    let (tx, mut rx) = mpsc::unbounded_channel::<NotificationPayload>();
    let server = CallbackServer::new((50400, 50500), tx)
        .await
        .expect("Failed to create callback server");

    let base_url = server.base_url().to_string();
    let client = reqwest::Client::new();

    let subscription_id = "dynamic-subscription".to_string();
    let notify_url = format!("{}/notify/{}", base_url, subscription_id);

    // Initially, subscription is not registered - should get 404
    let response1 = client
        .post(&notify_url)
        .header("SID", format!("uuid:{}", subscription_id))
        .body("<event>before_register</event>")
        .send()
        .await
        .expect("Failed to send HTTP request");

    assert_eq!(response1.status(), 404);

    // Register the subscription
    server.router().register(subscription_id.clone()).await;

    // Now the same request should succeed
    let response2 = client
        .post(&notify_url)
        .header("SID", format!("uuid:{}", subscription_id))
        .body("<event>after_register</event>")
        .send()
        .await
        .expect("Failed to send HTTP request");

    assert_eq!(response2.status(), 200);

    // Verify notification was received
    let notification = timeout(Duration::from_secs(1), rx.recv())
        .await
        .expect("Timeout waiting for notification")
        .expect("No notification received");

    assert_eq!(notification.subscription_id, subscription_id);
    assert!(notification.event_xml.contains("after_register"));

    // Unregister the subscription
    server.router().unregister(&subscription_id).await;

    // Request should now fail again
    let response3 = client
        .post(&notify_url)
        .header("SID", format!("uuid:{}", subscription_id))
        .body("<event>after_unregister</event>")
        .send()
        .await
        .expect("Failed to send HTTP request");

    assert_eq!(response3.status(), 404);

    // Verify no notification was received
    let no_notification = timeout(Duration::from_millis(100), rx.recv()).await;
    assert!(no_notification.is_err(), "Should not receive notification after unregistration");

    server.shutdown().await.expect("Failed to shutdown server");
}

/// Test server IP detection and URL formation.
#[tokio::test]
async fn test_server_ip_and_url_detection() {
    let (tx, _rx) = mpsc::unbounded_channel::<NotificationPayload>();
    let server = CallbackServer::new((50600, 50700), tx)
        .await
        .expect("Failed to create callback server");

    let base_url = server.base_url();
    let port = server.port();

    // Verify URL format
    assert!(base_url.starts_with("http://"));
    assert!(base_url.contains(&port.to_string()));
    assert!(port >= 50600 && port <= 50700);

    // Verify the URL is reachable by making a request to a non-existent endpoint
    let client = reqwest::Client::new();
    let test_url = format!("{}/nonexistent", base_url);
    
    let response = client
        .get(&test_url)
        .send()
        .await
        .expect("Failed to connect to server");

    // Should get 404 for non-existent endpoint, but connection should succeed
    assert_eq!(response.status(), 404);

    server.shutdown().await.expect("Failed to shutdown server");
}

/// Test error handling for malformed requests.
#[tokio::test]
async fn test_error_handling() {
    let (tx, mut rx) = mpsc::unbounded_channel::<NotificationPayload>();
    let server = CallbackServer::new((50800, 50900), tx)
        .await
        .expect("Failed to create callback server");

    let base_url = server.base_url().to_string();
    let client = reqwest::Client::new();

    let subscription_id = "error-test-sub".to_string();
    server.router().register(subscription_id.clone()).await;

    let notify_url = format!("{}/notify/{}", base_url, subscription_id);

    // Test various malformed requests
    
    // 1. Wrong HTTP method (GET instead of POST)
    let response1 = client
        .get(&notify_url)
        .header("SID", format!("uuid:{}", subscription_id))
        .send()
        .await
        .expect("Failed to send GET request");

    // Should return an error status (not 200)
    assert_ne!(response1.status(), 200);

    // 2. Invalid NT header value
    let response2 = client
        .post(&notify_url)
        .header("SID", format!("uuid:{}", subscription_id))
        .header("NT", "invalid-value")
        .header("NTS", "upnp:propchange")
        .body("<event>test</event>")
        .send()
        .await
        .expect("Failed to send request with invalid NT");

    assert_eq!(response2.status(), 400); // Bad Request

    // 3. Invalid NTS header value
    let response3 = client
        .post(&notify_url)
        .header("SID", format!("uuid:{}", subscription_id))
        .header("NT", "upnp:event")
        .header("NTS", "invalid-value")
        .body("<event>test</event>")
        .send()
        .await
        .expect("Failed to send request with invalid NTS");

    assert_eq!(response3.status(), 400); // Bad Request

    // Verify no notifications were received for any of the malformed requests
    let no_notification = timeout(Duration::from_millis(100), rx.recv()).await;
    assert!(no_notification.is_err(), "Should not receive notifications for malformed requests");

    server.shutdown().await.expect("Failed to shutdown server");
}