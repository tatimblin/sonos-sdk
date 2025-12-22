use callback_server::{CallbackServer, NotificationPayload};
use tokio::sync::mpsc;

#[tokio::test]
async fn test_firewall_test_endpoint() {
    // Create a callback server
    let (tx, _rx) = mpsc::unbounded_channel::<NotificationPayload>();
    let server = CallbackServer::new((50900, 51000), tx)
        .await
        .expect("Failed to create server");

    let base_url = server.base_url();
    let test_url = format!("{}/firewall-test", base_url);

    // Make a GET request to the firewall test endpoint
    let client = reqwest::Client::new();
    let response = client.get(&test_url).send().await.expect("Failed to send request");

    // Should get a 200 OK response
    assert_eq!(response.status(), 200);
    
    let body = response.text().await.expect("Failed to read response body");
    assert_eq!(body, "OK");

    // Cleanup
    server.shutdown().await.expect("Failed to shutdown server");
}