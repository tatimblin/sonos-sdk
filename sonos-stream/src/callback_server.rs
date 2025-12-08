//! HTTP callback server for receiving UPnP event notifications.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener};
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use warp::Filter;

use crate::types::{ServiceType, SpeakerId};

/// Raw event received from the callback server.
#[derive(Debug, Clone)]
pub struct RawEvent {
    /// The subscription ID this event is for
    pub subscription_id: String,
    /// The speaker ID
    pub speaker_id: SpeakerId,
    /// The service type
    pub service_type: ServiceType,
    /// The raw XML event body
    pub event_xml: String,
}

/// Routes events from HTTP callbacks to the appropriate handlers.
#[derive(Clone)]
pub struct EventRouter {
    /// Map of subscription ID to (speaker_id, service_type)
    subscriptions: Arc<RwLock<HashMap<String, (SpeakerId, ServiceType)>>>,
    /// Channel for sending raw events to the broker
    event_sender: mpsc::UnboundedSender<RawEvent>,
}

impl EventRouter {
    /// Create a new event router.
    pub fn new(event_sender: mpsc::UnboundedSender<RawEvent>) -> Self {
        Self {
            subscriptions: Arc::new(RwLock::new(HashMap::new())),
            event_sender,
        }
    }

    /// Register a subscription for event routing.
    pub async fn register(
        &self,
        subscription_id: String,
        speaker_id: SpeakerId,
        service_type: ServiceType,
    ) {
        let mut subs = self.subscriptions.write().await;
        subs.insert(subscription_id, (speaker_id, service_type));
    }

    /// Unregister a subscription.
    pub async fn unregister(&self, subscription_id: &str) {
        let mut subs = self.subscriptions.write().await;
        subs.remove(subscription_id);
    }

    /// Route an incoming event to the broker.
    pub async fn route_event(&self, subscription_id: String, event_xml: String) -> bool {
        let subs = self.subscriptions.read().await;
        
        if let Some((speaker_id, service_type)) = subs.get(&subscription_id) {
            let event = RawEvent {
                subscription_id,
                speaker_id: speaker_id.clone(),
                service_type: *service_type,
                event_xml,
            };
            
            // Send event to broker (ignore errors if receiver is dropped)
            let _ = self.event_sender.send(event);
            true
        } else {
            false
        }
    }
}

/// HTTP callback server for receiving UPnP event notifications.
pub struct CallbackServer {
    /// The port the server is bound to
    port: u16,
    /// The base URL for callback registration
    base_url: String,
    /// Event router for handling incoming events
    event_router: Arc<EventRouter>,
    /// Shutdown signal sender
    shutdown_tx: Option<mpsc::Sender<()>>,
    /// Server task handle
    server_handle: Option<tokio::task::JoinHandle<()>>,
}

impl CallbackServer {
    /// Create and start a new callback server.
    ///
    /// # Arguments
    ///
    /// * `port_range` - Range of ports to try binding to (start, end)
    /// * `event_sender` - Channel for sending raw events to the broker
    ///
    /// # Returns
    ///
    /// Returns the callback server instance or an error if no port could be bound.
    pub async fn new(
        port_range: (u16, u16),
        event_sender: mpsc::UnboundedSender<RawEvent>,
    ) -> Result<Self, String> {
        // Find an available port in the range
        let port = Self::find_available_port(port_range.0, port_range.1)
            .ok_or_else(|| {
                format!(
                    "No available port found in range {}-{}",
                    port_range.0, port_range.1
                )
            })?;

        // Detect local IP address
        let local_ip = Self::detect_local_ip().ok_or_else(|| {
            "Failed to detect local IP address".to_string()
        })?;

        let base_url = format!("http://{}:{}", local_ip, port);

        // Create event router
        let event_router = Arc::new(EventRouter::new(event_sender));

        // Create shutdown channel
        let (shutdown_tx, shutdown_rx) = mpsc::channel::<()>(1);

        // Start the HTTP server
        let server_handle = Self::start_server(port, event_router.clone(), shutdown_rx);

        Ok(Self {
            port,
            base_url,
            event_router,
            shutdown_tx: Some(shutdown_tx),
            server_handle: Some(server_handle),
        })
    }

    /// Get the base URL for callback registration.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Get the port the server is bound to.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Register a subscription for event routing.
    pub async fn register_subscription(
        &self,
        subscription_id: String,
        speaker_id: SpeakerId,
        service_type: ServiceType,
    ) {
        self.event_router
            .register(subscription_id, speaker_id, service_type)
            .await;
    }

    /// Unregister a subscription.
    pub async fn unregister_subscription(&self, subscription_id: &str) {
        self.event_router.unregister(subscription_id).await;
    }

    /// Shutdown the callback server gracefully.
    pub async fn shutdown(mut self) -> Result<(), String> {
        // Send shutdown signal
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(()).await;
        }

        // Wait for server task to complete
        if let Some(handle) = self.server_handle.take() {
            let _ = handle.await;
        }

        Ok(())
    }

    /// Find an available port in the given range.
    fn find_available_port(start: u16, end: u16) -> Option<u16> {
        for port in start..=end {
            if Self::is_port_available(port) {
                return Some(port);
            }
        }
        None
    }

    /// Check if a port is available for binding.
    fn is_port_available(port: u16) -> bool {
        TcpListener::bind(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
            port,
        ))
        .is_ok()
    }

    /// Detect the local IP address for callback URLs.
    fn detect_local_ip() -> Option<IpAddr> {
        // Try to connect to a public IP to determine our local IP
        // We don't actually send data, just use the socket to determine routing
        let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
        socket.connect("8.8.8.8:80").ok()?;
        let local_addr = socket.local_addr().ok()?;
        Some(local_addr.ip())
    }

    /// Start the HTTP server on the given port.
    fn start_server(
        port: u16,
        event_router: Arc<EventRouter>,
        mut shutdown_rx: mpsc::Receiver<()>,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            // Create the NOTIFY endpoint
            let notify_route = warp::path!("notify" / String)
                .and(warp::post())
                .and(warp::header::optional::<String>("sid"))
                .and(warp::header::optional::<String>("nt"))
                .and(warp::header::optional::<String>("nts"))
                .and(warp::body::bytes())
                .and_then({
                    let router = event_router.clone();
                    move |subscription_id: String,
                          sid: Option<String>,
                          nt: Option<String>,
                          nts: Option<String>,
                          body: bytes::Bytes| {
                        let router = router.clone();
                        async move {
                            // Validate UPnP headers
                            if !Self::validate_upnp_headers(&sid, &nt, &nts) {
                                return Err(warp::reject::custom(InvalidUpnpHeaders));
                            }

                            // Extract subscription ID from SID header or path
                            let sub_id = sid
                                .and_then(|s| Self::extract_subscription_id(&s))
                                .unwrap_or(subscription_id);

                            // Convert body to string
                            let event_xml = String::from_utf8_lossy(&body).to_string();

                            // Route the event
                            let routed = router.route_event(sub_id, event_xml).await;

                            if routed {
                                Ok::<_, warp::Rejection>(warp::reply::with_status(
                                    "",
                                    warp::http::StatusCode::OK,
                                ))
                            } else {
                                Err(warp::reject::not_found())
                            }
                        }
                    }
                });

            // Combine routes
            let routes = notify_route.recover(handle_rejection);

            // Create server with graceful shutdown
            let (addr, server) = warp::serve(routes)
                .bind_with_graceful_shutdown(
                    SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), port),
                    async move {
                        shutdown_rx.recv().await;
                    },
                );

            eprintln!("CallbackServer listening on {}", addr);
            server.await;
        })
    }

    /// Validate UPnP event notification headers.
    fn validate_upnp_headers(
        sid: &Option<String>,
        nt: &Option<String>,
        nts: &Option<String>,
    ) -> bool {
        // SID header should be present for event notifications
        if sid.is_none() {
            return false;
        }

        // For initial subscription, NT and NTS should be present
        // For events, they may not be present
        // We'll be lenient and accept both cases
        if let (Some(nt_val), Some(nts_val)) = (nt, nts) {
            // If present, validate they have expected values
            if nt_val != "upnp:event" || nts_val != "upnp:propchange" {
                return false;
            }
        }

        true
    }

    /// Extract subscription ID from SID header.
    fn extract_subscription_id(sid: &str) -> Option<String> {
        // SID format: uuid:subscription-UUID
        sid.strip_prefix("uuid:").map(|s| s.to_string())
    }
}

/// Custom rejection for invalid UPnP headers.
#[derive(Debug)]
struct InvalidUpnpHeaders;

impl warp::reject::Reject for InvalidUpnpHeaders {}

/// Handle rejections and convert them to HTTP responses.
async fn handle_rejection(
    err: warp::Rejection,
) -> Result<impl warp::Reply, std::convert::Infallible> {
    let code;
    let message;

    if err.is_not_found() {
        code = warp::http::StatusCode::NOT_FOUND;
        message = "Subscription not found";
    } else if err.find::<InvalidUpnpHeaders>().is_some() {
        code = warp::http::StatusCode::BAD_REQUEST;
        message = "Invalid UPnP headers";
    } else {
        code = warp::http::StatusCode::INTERNAL_SERVER_ERROR;
        message = "Internal server error";
    }

    Ok(warp::reply::with_status(message, code))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_port_available() {
        // Port 0 should always be available (OS assigns a free port)
        assert!(CallbackServer::is_port_available(0));

        // Bind to a port and verify it's no longer available
        let _listener = TcpListener::bind("0.0.0.0:0").unwrap();
        let port = _listener.local_addr().unwrap().port();
        // While the listener is held, the port should not be available
        assert!(!CallbackServer::is_port_available(port));
        // Keep listener alive for the assertion
        drop(_listener);
    }

    #[test]
    fn test_find_available_port() {
        // Should find a port in a reasonable range
        let port = CallbackServer::find_available_port(50000, 50100);
        assert!(port.is_some());
        assert!(port.unwrap() >= 50000 && port.unwrap() <= 50100);
    }

    #[test]
    fn test_detect_local_ip() {
        let ip = CallbackServer::detect_local_ip();
        assert!(ip.is_some());
        
        // Should not be localhost
        if let Some(IpAddr::V4(addr)) = ip {
            assert_ne!(addr, Ipv4Addr::new(127, 0, 0, 1));
        }
    }

    #[test]
    fn test_extract_subscription_id() {
        let sid = "uuid:12345-67890-abcdef";
        let extracted = CallbackServer::extract_subscription_id(sid);
        assert_eq!(extracted, Some("12345-67890-abcdef".to_string()));

        let invalid_sid = "12345-67890-abcdef";
        let extracted = CallbackServer::extract_subscription_id(invalid_sid);
        assert_eq!(extracted, None);
    }

    #[test]
    fn test_validate_upnp_headers() {
        // Valid headers with NT and NTS
        assert!(CallbackServer::validate_upnp_headers(
            &Some("uuid:123".to_string()),
            &Some("upnp:event".to_string()),
            &Some("upnp:propchange".to_string()),
        ));

        // Valid headers without NT and NTS (event notification)
        assert!(CallbackServer::validate_upnp_headers(
            &Some("uuid:123".to_string()),
            &None,
            &None,
        ));

        // Invalid: missing SID
        assert!(!CallbackServer::validate_upnp_headers(
            &None,
            &Some("upnp:event".to_string()),
            &Some("upnp:propchange".to_string()),
        ));

        // Invalid: wrong NT value
        assert!(!CallbackServer::validate_upnp_headers(
            &Some("uuid:123".to_string()),
            &Some("wrong".to_string()),
            &Some("upnp:propchange".to_string()),
        ));

        // Invalid: wrong NTS value
        assert!(!CallbackServer::validate_upnp_headers(
            &Some("uuid:123".to_string()),
            &Some("upnp:event".to_string()),
            &Some("wrong".to_string()),
        ));
    }

    #[tokio::test]
    async fn test_event_router_register_and_route() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let router = EventRouter::new(tx);

        let sub_id = "test-sub-123".to_string();
        let speaker_id = SpeakerId::new("speaker1");
        let service_type = ServiceType::AVTransport;

        // Register subscription
        router
            .register(sub_id.clone(), speaker_id.clone(), service_type)
            .await;

        // Route an event
        let event_xml = "<event>test</event>".to_string();
        let routed = router.route_event(sub_id.clone(), event_xml.clone()).await;
        assert!(routed);

        // Verify event was sent
        let event = rx.recv().await.unwrap();
        assert_eq!(event.subscription_id, sub_id);
        assert_eq!(event.speaker_id, speaker_id);
        assert_eq!(event.service_type, service_type);
        assert_eq!(event.event_xml, event_xml);
    }

    #[tokio::test]
    async fn test_event_router_unregister() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let router = EventRouter::new(tx);

        let sub_id = "test-sub-123".to_string();
        let speaker_id = SpeakerId::new("speaker1");
        let service_type = ServiceType::AVTransport;

        // Register and then unregister
        router
            .register(sub_id.clone(), speaker_id.clone(), service_type)
            .await;
        router.unregister(&sub_id).await;

        // Try to route an event - should fail
        let event_xml = "<event>test</event>".to_string();
        let routed = router.route_event(sub_id, event_xml).await;
        assert!(!routed);

        // No event should be received
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn test_event_router_unknown_subscription() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let router = EventRouter::new(tx);

        // Try to route event for unknown subscription
        let routed = router
            .route_event("unknown-sub".to_string(), "<event>test</event>".to_string())
            .await;
        assert!(!routed);

        // No event should be received
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn test_callback_server_creation() {
        let (tx, _rx) = mpsc::unbounded_channel();
        
        let server = CallbackServer::new((50000, 50100), tx).await;
        assert!(server.is_ok());

        let server = server.unwrap();
        assert!(server.port() >= 50000 && server.port() <= 50100);
        assert!(server.base_url().contains(&server.port().to_string()));

        // Cleanup
        server.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_callback_server_register_unregister() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let server = CallbackServer::new((50000, 50100), tx).await.unwrap();

        let sub_id = "test-sub-123".to_string();
        let speaker_id = SpeakerId::new("speaker1");
        let service_type = ServiceType::AVTransport;

        // Register subscription
        server
            .register_subscription(sub_id.clone(), speaker_id.clone(), service_type)
            .await;

        // Unregister subscription
        server.unregister_subscription(&sub_id).await;

        // Cleanup
        server.shutdown().await.unwrap();
    }
}
