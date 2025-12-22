//! HTTP server for receiving UPnP event notifications.

use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener};
use std::sync::Arc;
use tokio::sync::mpsc;
use warp::Filter;

use super::plugin::{PluginContext, PluginRegistry};
use super::router::{EventRouter, NotificationPayload};

/// HTTP callback server for receiving UPnP event notifications.
///
/// The `CallbackServer` binds to a local port and provides an HTTP endpoint
/// for receiving UPnP NOTIFY requests. It validates UPnP headers and routes
/// events through an `EventRouter` to a channel.
///
/// The server also supports a plugin system that allows extending functionality
/// with features like firewall detection, monitoring, logging, etc.
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
    /// Plugin registry for managing server extensions
    plugin_registry: Arc<tokio::sync::Mutex<PluginRegistry>>,
    /// Shutdown signal sender
    shutdown_tx: Option<mpsc::Sender<()>>,
    /// Server task handle
    server_handle: Option<tokio::task::JoinHandle<()>>,
}

impl CallbackServer {
    /// Create and start a new unified callback server with plugins.
    ///
    /// This method creates a single HTTP server that efficiently handles all UPnP
    /// event notifications from multiple speakers and services. The server:
    /// - Finds an available port in the specified range
    /// - Detects the local IP address for callback URLs
    /// - Starts an HTTP server to receive all UPnP NOTIFY requests
    /// - Routes events through a unified event router to registered handlers
    /// - Initializes all registered plugins during startup
    ///
    /// # Arguments
    ///
    /// * `port_range` - Range of ports to try binding to (start, end)
    /// * `event_sender` - Channel for sending notification payloads to the unified processor
    /// * `plugins` - Optional vector of plugins to register and initialize
    ///
    /// # Returns
    ///
    /// Returns the callback server instance or an error if no port could be bound
    /// or the local IP address could not be detected.
    pub async fn with_plugins(
        port_range: (u16, u16),
        event_sender: mpsc::UnboundedSender<NotificationPayload>,
        plugins: Option<Vec<Box<dyn crate::plugin::Plugin>>>,
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

        let base_url = format!("http://{local_ip}:{port}");

        // Create event router
        let event_router = Arc::new(EventRouter::new(event_sender));

        // Create plugin registry and register plugins
        let mut plugin_registry = PluginRegistry::new();
        if let Some(plugins) = plugins {
            for plugin in plugins {
                plugin_registry.register(plugin);
            }
        }
        let plugin_registry = Arc::new(tokio::sync::Mutex::new(plugin_registry));

        // Create shutdown channel
        let (shutdown_tx, shutdown_rx) = mpsc::channel::<()>(1);

        // Create ready signal channel
        let (ready_tx, mut ready_rx) = mpsc::channel::<()>(1);

        // Start the HTTP server
        let server_handle = Self::start_server(port, event_router.clone(), shutdown_rx, ready_tx);

        // Wait for server to be ready
        ready_rx.recv().await.ok_or_else(|| {
            "Server failed to start".to_string()
        })?;

        // Initialize plugins after server is ready
        let plugin_context = PluginContext::new(base_url.clone(), port);
        
        {
            let mut registry = plugin_registry.lock().await;
            if let Err(e) = registry.initialize_all(&plugin_context).await {
                eprintln!("⚠️  Plugin initialization completed with errors: {}", e);
            }
        }

        Ok(Self {
            port,
            base_url,
            event_router,
            plugin_registry,
            shutdown_tx: Some(shutdown_tx),
            server_handle: Some(server_handle),
        })
    }
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
        Self::with_plugins(port_range, event_sender, None).await
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

    /// Get a reference to the plugin registry.
    ///
    /// The registry can be used to register plugins before server startup.
    /// Note that plugins should be registered before calling `new()` if you
    /// want them to be initialized during server startup.
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
    /// let registry = server.plugin_registry();
    /// let count = registry.lock().await.plugin_count();
    /// println!("Registered plugins: {}", count);
    /// # }
    /// ```
    pub fn plugin_registry(&self) -> &Arc<tokio::sync::Mutex<PluginRegistry>> {
        &self.plugin_registry
    }

    /// Get the current firewall detection status.
    ///
    /// Returns the status of firewall detection if the firewall detection plugin
    /// is registered and has been initialized. Returns None if the plugin is not
    /// found or hasn't been initialized yet.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use tokio::sync::mpsc;
    /// # use callback_server::{CallbackServer, NotificationPayload, FirewallStatus};
    /// # #[tokio::main]
    /// # async fn main() {
    /// # let (tx, _rx) = mpsc::unbounded_channel::<NotificationPayload>();
    /// # let server = CallbackServer::new((3400, 3500), tx).await.unwrap();
    /// if let Some(status) = server.get_firewall_status().await {
    ///     match status {
    ///         FirewallStatus::Accessible => println!("Server is accessible"),
    ///         FirewallStatus::Blocked => println!("Server is blocked by firewall"),
    ///         FirewallStatus::Unknown => println!("Firewall status unknown"),
    ///         FirewallStatus::Error => println!("Error detecting firewall status"),
    ///     }
    /// }
    /// # }
    /// ```
    pub async fn get_firewall_status(&self) -> Option<crate::FirewallStatus> {
        // This is a simplified implementation - in a real scenario, we would
        // need to find the firewall detection plugin in the registry and query its status
        // For now, we'll return None since we don't have direct access to plugin instances
        None
    }

    /// Shutdown the callback server gracefully.
    ///
    /// This shuts down all registered plugins first, then sends a shutdown signal 
    /// to the HTTP server and waits for it to complete any in-flight requests.
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
        // Shutdown plugins first
        {
            let mut registry = self.plugin_registry.lock().await;
            if let Err(e) = registry.shutdown_all().await {
                eprintln!("⚠️  Plugin shutdown completed with errors: {}", e);
            }
        }

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
        TcpListener::bind(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
            port,
        ))
        .is_ok()
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
            // Create the NOTIFY endpoint that accepts any path (like the old code)
            let notify_route = warp::method()
                .and(warp::path::full())
                .and(warp::header::optional::<String>("sid"))
                .and(warp::header::optional::<String>("nt"))
                .and(warp::header::optional::<String>("nts"))
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
                            // Only handle NOTIFY method
                            if method != warp::http::Method::from_bytes(b"NOTIFY").unwrap() {
                                return Err(warp::reject::not_found());
                            }

                            // Log incoming request details for unified event stream monitoring
                            eprintln!("\n🌐 === UNIFIED CALLBACK SERVER: INCOMING NOTIFY ===");
                            eprintln!("📡 Method: {}", method);
                            eprintln!("📡 Path: {}", path.as_str());
                            eprintln!("📏 Body size: {} bytes", body.len());
                            eprintln!("📋 Headers:");
                            if let Some(ref sid_val) = sid {
                                eprintln!("   SID: {}", sid_val);
                            }
                            if let Some(ref nt_val) = nt {
                                eprintln!("   NT: {}", nt_val);
                            }
                            if let Some(ref nts_val) = nts {
                                eprintln!("   NTS: {}", nts_val);
                            }

                            // Convert body to string and log it
                            let event_xml = String::from_utf8_lossy(&body).to_string();
                            eprintln!("📄 Event XML (first 200 chars):");
                            let preview = if event_xml.len() > 200 {
                                format!("{}...", &event_xml[..200])
                            } else {
                                event_xml.clone()
                            };
                            eprintln!("{}", preview);
                            eprintln!("🌐 === END UNIFIED CALLBACK NOTIFICATION ===\n");

                            // Validate UPnP headers
                            if !Self::validate_upnp_headers(&sid, &nt, &nts) {
                                eprintln!("❌ Invalid UPnP headers");
                                return Err(warp::reject::custom(InvalidUpnpHeaders));
                            }

                            // Extract subscription ID from SID header (required for UPnP events)
                            let sub_id = sid.ok_or_else(|| {
                                eprintln!("❌ Missing SID header");
                                warp::reject::custom(InvalidUpnpHeaders)
                            })?;

                            // Route the event through the unified event stream
                            let routed = router.route_event(sub_id, event_xml).await;

                            if routed {
                                eprintln!("✅ Unified event stream: Event routed successfully");
                                Ok::<_ , warp::Rejection>(warp::reply::with_status(
                                    "",
                                    warp::http::StatusCode::OK,
                                ))
                            } else {
                                eprintln!("❌ Unified event stream: Event routing failed - subscription not found");
                                Err(warp::reject::not_found())
                            }
                        }
                    }
                });

            // Add firewall test endpoint
            let firewall_test_route = crate::firewall_detection::firewall_test_endpoint();

            // Combine routes
            let routes = notify_route
                .or(firewall_test_route)
                .recover(handle_rejection);

            // Create server with graceful shutdown
            let (addr, server) = warp::serve(routes)
                .bind_with_graceful_shutdown(
                    SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), port),
                    async move {
                        shutdown_rx.recv().await;
                    },
                );

            eprintln!("🌐 Unified CallbackServer listening on {addr} - ready to process events from all speakers and services");
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

        // Register subscription via router
        server.router().register(sub_id.clone()).await;

        // Unregister subscription via router
        server.router().unregister(&sub_id).await;

        // Check plugin registry is accessible
        let plugin_count = server.plugin_registry().lock().await.plugin_count();
        assert_eq!(plugin_count, 0); // No plugins registered by default

        // Cleanup
        server.shutdown().await.unwrap();
    }
}

#[cfg(test)]
mod plugin_integration_tests {
    use super::*;
    use crate::plugin::{Plugin, PluginContext, PluginError};
    use async_trait::async_trait;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    /// Test plugin that tracks its lifecycle state.
    struct TestLifecyclePlugin {
        name: &'static str,
        state: Arc<Mutex<PluginState>>,
    }

    #[derive(Debug, Clone)]
    struct PluginState {
        initialized: bool,
        shutdown: bool,
        context_received: Option<PluginContext>,
    }

    impl TestLifecyclePlugin {
        fn new(name: &'static str) -> (Self, Arc<Mutex<PluginState>>) {
            let state = Arc::new(Mutex::new(PluginState {
                initialized: false,
                shutdown: false,
                context_received: None,
            }));
            
            let plugin = Self {
                name,
                state: state.clone(),
            };
            
            (plugin, state)
        }
    }

    #[async_trait]
    impl Plugin for TestLifecyclePlugin {
        fn name(&self) -> &'static str {
            self.name
        }

        async fn initialize(&mut self, context: &PluginContext) -> Result<(), PluginError> {
            let mut state = self.state.lock().await;
            state.initialized = true;
            state.context_received = Some(context.clone());
            Ok(())
        }

        async fn shutdown(&mut self) -> Result<(), PluginError> {
            let mut state = self.state.lock().await;
            state.shutdown = true;
            Ok(())
        }
    }

    /// Test plugin that fails during initialization.
    struct FailingInitPlugin {
        name: &'static str,
    }

    impl FailingInitPlugin {
        fn new(name: &'static str) -> Self {
            Self { name }
        }
    }

    #[async_trait]
    impl Plugin for FailingInitPlugin {
        fn name(&self) -> &'static str {
            self.name
        }

        async fn initialize(&mut self, _context: &PluginContext) -> Result<(), PluginError> {
            Err(PluginError::InitializationFailed("Test failure".to_string()))
        }

        async fn shutdown(&mut self) -> Result<(), PluginError> {
            Ok(())
        }
    }

    /// Test plugin that fails during shutdown.
    struct FailingShutdownPlugin {
        name: &'static str,
        initialized: bool,
    }

    impl FailingShutdownPlugin {
        fn new(name: &'static str) -> Self {
            Self { 
                name,
                initialized: false,
            }
        }
    }

    #[async_trait]
    impl Plugin for FailingShutdownPlugin {
        fn name(&self) -> &'static str {
            self.name
        }

        async fn initialize(&mut self, _context: &PluginContext) -> Result<(), PluginError> {
            self.initialized = true;
            Ok(())
        }

        async fn shutdown(&mut self) -> Result<(), PluginError> {
            Err(PluginError::ShutdownFailed("Test failure".to_string()))
        }
    }

    #[tokio::test]
    async fn test_server_startup_with_plugins() {
        let (tx, _rx) = mpsc::unbounded_channel();
        
        // Create test plugins
        let (plugin1, state1) = TestLifecyclePlugin::new("test-plugin-1");
        let (plugin2, state2) = TestLifecyclePlugin::new("test-plugin-2");
        
        let plugins: Vec<Box<dyn Plugin>> = vec![
            Box::new(plugin1),
            Box::new(plugin2),
        ];
        
        // Create server with plugins
        let server = CallbackServer::with_plugins((50100, 50200), tx, Some(plugins))
            .await
            .expect("Failed to create server with plugins");
        
        // Verify plugins were initialized
        {
            let state = state1.lock().await;
            assert!(state.initialized, "Plugin 1 should be initialized");
            assert!(!state.shutdown, "Plugin 1 should not be shut down yet");
            assert!(state.context_received.is_some(), "Plugin 1 should have received context");
            
            // Verify context contains server information
            let context = state.context_received.as_ref().unwrap();
            assert!(context.server_url.contains(&server.port().to_string()), 
                    "Context should contain server port");
            assert_eq!(context.server_port, server.port(), 
                       "Context port should match server port");
            assert_eq!(context.test_endpoint, "/firewall-test", 
                       "Context should have test endpoint");
        }
        
        {
            let state = state2.lock().await;
            assert!(state.initialized, "Plugin 2 should be initialized");
            assert!(!state.shutdown, "Plugin 2 should not be shut down yet");
            assert!(state.context_received.is_some(), "Plugin 2 should have received context");
        }
        
        // Verify server is running
        assert!(server.port() >= 50100 && server.port() <= 50200);
        assert!(server.base_url().contains(&server.port().to_string()));
        
        // Cleanup
        server.shutdown().await.expect("Failed to shutdown server");
    }

    #[tokio::test]
    async fn test_server_shutdown_with_plugins() {
        let (tx, _rx) = mpsc::unbounded_channel();
        
        // Create test plugins
        let (plugin1, state1) = TestLifecyclePlugin::new("test-plugin-1");
        let (plugin2, state2) = TestLifecyclePlugin::new("test-plugin-2");
        
        let plugins: Vec<Box<dyn Plugin>> = vec![
            Box::new(plugin1),
            Box::new(plugin2),
        ];
        
        // Create server with plugins
        let server = CallbackServer::with_plugins((50200, 50300), tx, Some(plugins))
            .await
            .expect("Failed to create server with plugins");
        
        // Verify plugins are initialized
        {
            let state = state1.lock().await;
            assert!(state.initialized, "Plugin 1 should be initialized");
            assert!(!state.shutdown, "Plugin 1 should not be shut down yet");
        }
        
        // Shutdown server
        server.shutdown().await.expect("Failed to shutdown server");
        
        // Verify plugins were shut down
        {
            let state = state1.lock().await;
            assert!(state.initialized, "Plugin 1 should still be marked as initialized");
            assert!(state.shutdown, "Plugin 1 should be shut down");
        }
        
        {
            let state = state2.lock().await;
            assert!(state.initialized, "Plugin 2 should still be marked as initialized");
            assert!(state.shutdown, "Plugin 2 should be shut down");
        }
    }

    #[tokio::test]
    async fn test_plugin_context_creation() {
        let (tx, _rx) = mpsc::unbounded_channel();
        
        // Create test plugin
        let (plugin, state) = TestLifecyclePlugin::new("context-test-plugin");
        let plugins: Vec<Box<dyn Plugin>> = vec![Box::new(plugin)];
        
        // Create server with plugin
        let server = CallbackServer::with_plugins((50300, 50400), tx, Some(plugins))
            .await
            .expect("Failed to create server with plugins");
        
        // Verify plugin received proper context
        {
            let state = state.lock().await;
            assert!(state.context_received.is_some(), "Plugin should have received context");
            
            let context = state.context_received.as_ref().unwrap();
            
            // Verify context fields
            assert!(context.server_url.starts_with("http://"), 
                    "Server URL should start with http://");
            assert!(context.server_url.contains(&server.port().to_string()), 
                    "Server URL should contain the port");
            assert_eq!(context.server_port, server.port(), 
                       "Context port should match server port");
            assert_eq!(context.test_endpoint, "/firewall-test", 
                       "Test endpoint should be /firewall-test");
            
            // Verify HTTP client is present (we can't test much about it without making requests)
            // Just verify it's not panicking when accessed
            let _client = &context.http_client;
        }
        
        // Cleanup
        server.shutdown().await.expect("Failed to shutdown server");
    }

    #[tokio::test]
    async fn test_server_startup_with_failing_plugin() {
        let (tx, _rx) = mpsc::unbounded_channel();
        
        // Create mix of good and failing plugins
        let (good_plugin, good_state) = TestLifecyclePlugin::new("good-plugin");
        let failing_plugin = FailingInitPlugin::new("failing-plugin");
        
        let plugins: Vec<Box<dyn Plugin>> = vec![
            Box::new(good_plugin),
            Box::new(failing_plugin),
        ];
        
        // Server should still start even with failing plugin
        let server = CallbackServer::with_plugins((50400, 50500), tx, Some(plugins))
            .await
            .expect("Server should start even with failing plugin");
        
        // Good plugin should still be initialized
        {
            let state = good_state.lock().await;
            assert!(state.initialized, "Good plugin should be initialized");
        }
        
        // Server should be running normally
        assert!(server.port() >= 50400 && server.port() <= 50500);
        
        // Cleanup
        server.shutdown().await.expect("Failed to shutdown server");
    }

    #[tokio::test]
    async fn test_server_shutdown_with_failing_plugin() {
        let (tx, _rx) = mpsc::unbounded_channel();
        
        // Create mix of good and failing plugins
        let (good_plugin, good_state) = TestLifecyclePlugin::new("good-plugin");
        let failing_plugin = FailingShutdownPlugin::new("failing-shutdown-plugin");
        
        let plugins: Vec<Box<dyn Plugin>> = vec![
            Box::new(good_plugin),
            Box::new(failing_plugin),
        ];
        
        // Create server with plugins
        let server = CallbackServer::with_plugins((50500, 50600), tx, Some(plugins))
            .await
            .expect("Failed to create server with plugins");
        
        // Shutdown should succeed even with failing plugin
        server.shutdown().await.expect("Shutdown should succeed even with failing plugin");
        
        // Good plugin should still be shut down
        {
            let state = good_state.lock().await;
            assert!(state.shutdown, "Good plugin should be shut down");
        }
    }

    #[tokio::test]
    async fn test_server_without_plugins() {
        let (tx, _rx) = mpsc::unbounded_channel();
        
        // Create server without plugins (using the regular new method)
        let server = CallbackServer::new((50600, 50700), tx)
            .await
            .expect("Failed to create server without plugins");
        
        // Server should work normally
        assert!(server.port() >= 50600 && server.port() <= 50700);
        assert!(server.base_url().contains(&server.port().to_string()));
        
        // Plugin registry should be empty
        let plugin_count = server.plugin_registry().lock().await.plugin_count();
        assert_eq!(plugin_count, 0, "Server without plugins should have empty registry");
        
        // Cleanup
        server.shutdown().await.expect("Failed to shutdown server");
    }

    #[tokio::test]
    async fn test_plugin_registry_access() {
        let (tx, _rx) = mpsc::unbounded_channel();
        
        // Create test plugin
        let (plugin, _state) = TestLifecyclePlugin::new("registry-test-plugin");
        let plugins: Vec<Box<dyn Plugin>> = vec![Box::new(plugin)];
        
        // Create server with plugin
        let server = CallbackServer::with_plugins((50700, 50800), tx, Some(plugins))
            .await
            .expect("Failed to create server with plugins");
        
        // Verify plugin registry is accessible and contains the plugin
        {
            let registry = server.plugin_registry().lock().await;
            assert_eq!(registry.plugin_count(), 1, "Registry should contain one plugin");
            assert!(registry.is_initialized(), "Registry should be initialized");
            
            let names = registry.plugin_names();
            assert_eq!(names.len(), 1, "Should have one plugin name");
            assert_eq!(names[0], "registry-test-plugin", "Plugin name should match");
        }
        
        // Cleanup
        server.shutdown().await.expect("Failed to shutdown server");
    }
}
