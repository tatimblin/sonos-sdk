//! HTTP server for receiving UPnP event notifications.

use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace};
use warp::Filter;

use super::router::{EventRouter, NotificationPayload};

/// Maximum accepted size of a UPnP NOTIFY body, in bytes (64 KiB).
///
/// The endpoint is unauthenticated by design (UPnP eventing has no auth), so any
/// host on the LAN can POST to it. Without a limit, warp would buffer the whole
/// body and then a second full copy is made when it is decoded to a `String` —
/// two concurrent 1 GB posts would be ~4 GB resident.
///
/// Sizing: real Sonos propertysets are ~1-5 KB. The largest realistic case is a
/// `ZoneGroupTopology` event, whose double-escaped `ZoneGroupState` payload runs
/// a few hundred bytes per player; at the 32-player household maximum that is
/// roughly 20-30 KB. 64 KiB leaves ~2x headroom over that worst case and ~10x
/// over typical events, while capping the cost of a hostile request at 128 KiB
/// (bytes + string copy) instead of unbounded.
///
/// Requests without a `Content-Length` header (e.g. chunked bodies, which Sonos
/// never sends) are rejected with 411 Length Required.
const MAX_NOTIFY_BODY_BYTES: u64 = 64 * 1024;

/// How many bytes of event XML to include in trace-level logs.
const TRACE_PREVIEW_BYTES: usize = 200;

/// Truncate `s` to at most `max` bytes, snapping back to a UTF-8 char boundary.
///
/// Slicing a `&str` at a byte index that falls inside a multi-byte codepoint
/// panics. Event XML routinely carries non-ASCII track metadata, so a naive
/// `&s[..max]` in the trace-logging path is a reachable panic.
fn preview(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// HTTP callback server for receiving UPnP event notifications.
///
/// The `CallbackServer` binds to a local port and provides an HTTP endpoint
/// for receiving UPnP NOTIFY requests. It validates UPnP headers and routes
/// events through an `EventRouter` to a channel.
///
/// # Example
///
/// ```no_run
/// use tokio::sync::mpsc;
/// use callback_server::{CallbackServer, NotificationPayload};
///
/// #[tokio::main]
/// async fn main() {
///     let (tx, mut rx) = mpsc::unbounded_channel::<NotificationPayload>();
///     
///     let server = CallbackServer::new((3400, 3500), tx)
///         .await
///         .expect("Failed to create callback server");
///     
///     println!("Server listening at: {}", server.base_url());
///     
///     // Process notifications
///     while let Some(notification) = rx.recv().await {
///         println!("Received event for subscription: {}", notification.subscription_id);
///     }
/// }
/// ```
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
    /// Create and start a new unified callback server.
    ///
    /// This method creates a single HTTP server that efficiently handles all UPnP
    /// event notifications from multiple speakers and services. The server:
    /// - Finds an available port in the specified range
    /// - Detects the local IP address for callback URLs
    /// - Starts an HTTP server to receive all UPnP NOTIFY requests
    /// - Routes events through a unified event router to registered handlers
    ///
    /// # Unified Event Stream Processing
    ///
    /// The callback server is designed to support the unified event stream processor
    /// pattern where a single HTTP endpoint receives events from multiple UPnP
    /// services and speakers, then routes them to appropriate handlers based on
    /// subscription IDs.
    ///
    /// # Arguments
    ///
    /// * `port_range` - Range of ports to try binding to (start, end)
    /// * `event_sender` - Channel for sending notification payloads to the unified processor
    ///
    /// # Returns
    ///
    /// Returns the callback server instance or an error if no port could be bound
    /// or the local IP address could not be detected.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use tokio::sync::mpsc;
    /// # use callback_server::{CallbackServer, NotificationPayload};
    /// # #[tokio::main]
    /// # async fn main() {
    /// let (tx, _rx) = mpsc::unbounded_channel::<NotificationPayload>();
    /// let server = CallbackServer::new((3400, 3500), tx).await.unwrap();
    /// println!("Unified callback server listening at: {}", server.base_url());
    /// # }
    /// ```
    pub async fn new(
        port_range: (u16, u16),
        event_sender: mpsc::UnboundedSender<NotificationPayload>,
    ) -> Result<Self, String> {
        // Find an available port in the range
        let port = Self::find_available_port(port_range.0, port_range.1).ok_or_else(|| {
            format!(
                "No available port found in range {}-{}",
                port_range.0, port_range.1
            )
        })?;

        // Detect local IP address
        let local_ip = Self::detect_local_ip()
            .ok_or_else(|| "Failed to detect local IP address".to_string())?;

        let base_url = format!("http://{local_ip}:{port}");

        // Create event router
        let event_router = Arc::new(EventRouter::new(event_sender));

        // Create shutdown channel
        let (shutdown_tx, shutdown_rx) = mpsc::channel::<()>(1);

        // Create ready signal channel
        let (ready_tx, mut ready_rx) = mpsc::channel::<()>(1);

        // Start the HTTP server
        let server_handle = Self::start_server(port, event_router.clone(), shutdown_rx, ready_tx);

        // Wait for server to be ready
        ready_rx
            .recv()
            .await
            .ok_or_else(|| "Server failed to start".to_string())?;

        Ok(Self {
            port,
            base_url,
            event_router,
            shutdown_tx: Some(shutdown_tx),
            server_handle: Some(server_handle),
        })
    }

    /// Get the unified callback URL for subscription registration.
    ///
    /// This URL should be used when subscribing to UPnP events from any speaker
    /// or service. The unified callback server will route all incoming events
    /// based on their subscription IDs to the appropriate handlers.
    ///
    /// The format is `http://<local_ip>:<port>` and this same URL is used for
    /// all subscriptions, enabling the unified event stream processing pattern.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use tokio::sync::mpsc;
    /// # use callback_server::{CallbackServer, NotificationPayload};
    /// # #[tokio::main]
    /// # async fn main() {
    /// # let (tx, _rx) = mpsc::unbounded_channel::<NotificationPayload>();
    /// # let server = CallbackServer::new((3400, 3500), tx).await.unwrap();
    /// let callback_url = server.base_url();
    /// println!("Use this URL for all subscriptions: {}", callback_url);
    /// # }
    /// ```
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Get the port the server is bound to.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Get a reference to the event router.
    ///
    /// The router can be used to register and unregister subscription IDs
    /// for event routing.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use tokio::sync::mpsc;
    /// # use callback_server::{CallbackServer, NotificationPayload};
    /// # #[tokio::main]
    /// # async fn main() {
    /// # let (tx, _rx) = mpsc::unbounded_channel::<NotificationPayload>();
    /// # let server = CallbackServer::new((3400, 3500), tx).await.unwrap();
    /// server.router().register("uuid:subscription-123".to_string()).await;
    /// # }
    /// ```
    pub fn router(&self) -> &Arc<EventRouter> {
        &self.event_router
    }

    /// Shutdown the callback server gracefully.
    ///
    /// Sends a shutdown signal to the HTTP server and waits for it to complete
    /// any in-flight requests.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use tokio::sync::mpsc;
    /// # use callback_server::{CallbackServer, NotificationPayload};
    /// # #[tokio::main]
    /// # async fn main() {
    /// # let (tx, _rx) = mpsc::unbounded_channel::<NotificationPayload>();
    /// # let server = CallbackServer::new((3400, 3500), tx).await.unwrap();
    /// server.shutdown().await.unwrap();
    /// # }
    /// ```
    pub async fn shutdown(mut self) -> Result<(), String> {
        // Send shutdown signal to HTTP server
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
        (start..=end).find(|&port| Self::is_port_available(port))
    }

    /// Check if a port is available for binding.
    fn is_port_available(port: u16) -> bool {
        TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), port)).is_ok()
    }

    /// Detect the local IP address for callback URLs.
    ///
    /// This uses a UDP socket connection to determine the local IP address
    /// that would be used for outbound connections. No data is actually sent.
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
        ready_tx: mpsc::Sender<()>,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            // Only NOTIFY is handled. This gate runs *before* the body filters so
            // that ordinary bodiless requests (e.g. a browser GET) still get 404
            // rather than 411 Length Required.
            let notify_method = warp::method().and_then(|method: warp::http::Method| async move {
                if method == warp::http::Method::from_bytes(b"NOTIFY").unwrap() {
                    Ok(method)
                } else {
                    Err(warp::reject::not_found())
                }
            });

            // Create the NOTIFY endpoint that accepts any path (like the old code)
            let notify_route = notify_method
                .and(warp::path::full())
                .and(warp::header::optional::<String>("sid"))
                .and(warp::header::optional::<String>("nt"))
                .and(warp::header::optional::<String>("nts"))
                // Cap the body before warp buffers it — see MAX_NOTIFY_BODY_BYTES.
                .and(warp::body::content_length_limit(MAX_NOTIFY_BODY_BYTES))
                .and(warp::body::bytes())
                .and_then({
                    let router = event_router.clone();
                    move |method: warp::http::Method,
                          path: warp::path::FullPath,
                          sid: Option<String>,
                          nt: Option<String>,
                          nts: Option<String>,
                          body: bytes::Bytes| {
                        let router = router.clone();
                        async move {
                            // Log incoming request details for unified event stream monitoring
                            debug!(
                                method = %method,
                                path = %path.as_str(),
                                body_size = body.len(),
                                sid = ?sid,
                                nt = ?nt,
                                nts = ?nts,
                                "Received UPnP NOTIFY event"
                            );

                            // Validate UPnP headers *before* decoding the body, so
                            // junk requests never pay for the full allocation.
                            if !Self::validate_upnp_headers(&sid, &nt, &nts) {
                                error!(
                                    sid = ?sid,
                                    nt = ?nt,
                                    nts = ?nts,
                                    "Invalid UPnP headers in NOTIFY request"
                                );
                                return Err(warp::reject::custom(InvalidUpnpHeaders));
                            }

                            // Extract subscription ID from SID header (required for UPnP events)
                            let sub_id = sid.ok_or_else(|| {
                                error!("Missing required SID header in UPnP NOTIFY request");
                                warp::reject::custom(InvalidUpnpHeaders)
                            })?;

                            // Convert body to string and log content at trace level only
                            let event_xml = String::from_utf8_lossy(&body).to_string();
                            if event_xml.len() > TRACE_PREVIEW_BYTES {
                                trace!(
                                    event_xml_preview = %preview(&event_xml, TRACE_PREVIEW_BYTES),
                                    total_length = event_xml.len(),
                                    "UPnP event XML content (truncated)"
                                );
                            } else {
                                trace!(
                                    event_xml = %event_xml,
                                    "UPnP event XML content (full)"
                                );
                            }

                            // Route the event through the unified event stream.
                            // Events are either delivered immediately (registered SID)
                            // or buffered for replay when register() is called.
                            router.route_event(sub_id.clone(), event_xml).await;

                            debug!(
                                subscription_id = %sub_id,
                                "UPnP event accepted"
                            );
                            // Always 200 OK — event is either routed or buffered.
                            // Returning 404 could cause the speaker to cancel the subscription.
                            Ok::<_, warp::Rejection>(warp::reply::with_status(
                                "",
                                warp::http::StatusCode::OK,
                            ))
                        }
                    }
                });

            // Configure routes with just the NOTIFY endpoint
            let routes = notify_route.recover(handle_rejection);

            // Create server with graceful shutdown
            let (addr, server) = warp::serve(routes).bind_with_graceful_shutdown(
                SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), port),
                async move {
                    shutdown_rx.recv().await;
                },
            );

            info!(
                address = %addr,
                "CallbackServer listening - ready to process UPnP events"
            );
            // Signal that server is ready
            let _ = ready_tx.send(()).await;
            server.await;
        })
    }

    /// Validate UPnP event notification headers.
    ///
    /// Checks that the required SID header is present and validates optional
    /// NT and NTS headers if they are provided.
    fn validate_upnp_headers(
        sid: &Option<String>,
        nt: &Option<String>,
        nts: &Option<String>,
    ) -> bool {
        // SID header is required for event notifications
        if sid.is_none() {
            return false;
        }

        // For UPnP events, NT and NTS headers are typically present
        // If present, validate they have expected values
        if let (Some(nt_val), Some(nts_val)) = (nt, nts) {
            if nt_val != "upnp:event" || nts_val != "upnp:propchange" {
                return false;
            }
        }

        true
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
    } else if err.find::<warp::reject::PayloadTooLarge>().is_some() {
        error!(
            limit_bytes = MAX_NOTIFY_BODY_BYTES,
            "Rejected NOTIFY body exceeding size limit"
        );
        code = warp::http::StatusCode::PAYLOAD_TOO_LARGE;
        message = "NOTIFY body too large";
    } else if err.find::<warp::reject::LengthRequired>().is_some() {
        // Sonos always sends Content-Length; a missing one means we cannot
        // bound the body before reading it, so refuse.
        code = warp::http::StatusCode::LENGTH_REQUIRED;
        message = "Content-Length header is required";
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

    /// Slicing event XML at a fixed byte offset panics when that offset lands
    /// inside a multi-byte codepoint — very reachable for non-Latin track titles.
    #[test]
    fn test_event_xml_preview_handles_multibyte_boundary() {
        // 198 ASCII bytes + a 3-byte codepoint occupying bytes 198..201, so byte
        // index 200 is *not* a char boundary.
        let xml = format!("{}日本語", "a".repeat(198));
        assert!(!xml.is_char_boundary(TRACE_PREVIEW_BYTES));

        let out = preview(&xml, TRACE_PREVIEW_BYTES);

        // Snapped back to the boundary at 198, still valid UTF-8, still a prefix.
        assert_eq!(out.len(), 198);
        assert!(xml.starts_with(out));
        assert!(std::str::from_utf8(out.as_bytes()).is_ok());
    }

    #[test]
    fn test_event_xml_preview_short_and_exact_inputs() {
        // Shorter than the limit is returned whole.
        assert_eq!(preview("<event/>", TRACE_PREVIEW_BYTES), "<event/>");
        // Exactly at the limit is returned whole.
        let exact = "a".repeat(TRACE_PREVIEW_BYTES);
        assert_eq!(
            preview(&exact, TRACE_PREVIEW_BYTES).len(),
            TRACE_PREVIEW_BYTES
        );
        // A single wide codepoint over the limit truncates to empty, not a panic.
        assert_eq!(preview("日", 1), "");
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
        let server = CallbackServer::new((51000, 51100), tx).await.unwrap();

        let sub_id = "test-sub-123".to_string();

        // Register subscription via router
        server.router().register(sub_id.clone()).await;

        // Unregister subscription via router
        server.router().unregister(&sub_id).await;

        // Plugin system has been removed

        // Cleanup
        server.shutdown().await.unwrap();
    }
}
