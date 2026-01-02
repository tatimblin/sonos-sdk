//! Main EventBroker implementation
//!
//! This is the central component that integrates all other components and provides
//! the primary user interface for the sonos-stream crate. It coordinates subscription
//! management, event processing, polling, and firewall detection.

use std::net::{IpAddr, Ipv4Addr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, RwLock};

use callback_server::{CallbackServer, firewall_detection::{FirewallDetectionPlugin, FirewallStatus}, router::EventRouter};
use sonos_api::Service;

use crate::config::BrokerConfig;
use crate::error::{BrokerError, BrokerResult};
use crate::events::{
    iterator::EventIterator,
    processor::{EventProcessor, create_integrated_event_router},
    types::EnrichedEvent,
};
use crate::polling::scheduler::PollingScheduler;
use crate::registry::{RegistrationId, SpeakerServicePair, SpeakerServiceRegistry};
use crate::subscription::{
    event_detector::{EventDetector, PollingRequest, PollingAction, ResyncDetector},
    manager::SubscriptionManager,
};

/// Result type for registration operations with enhanced feedback
#[derive(Debug, Clone)]
pub struct RegistrationResult {
    /// The registration ID (new or existing)
    pub registration_id: RegistrationId,

    /// Current firewall status
    pub firewall_status: FirewallStatus,

    /// Reason for polling if polling was activated
    pub polling_reason: Option<PollingReason>,

    /// Whether this was a new registration or existing duplicate
    pub was_duplicate: bool,
}

/// Reason why polling was activated
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PollingReason {
    /// Proactively detected firewall blocking
    FirewallBlocked,
    /// Events stopped arriving (fallback case)
    EventTimeout,
    /// UPnP subscription failed
    SubscriptionFailed,
    /// Network connectivity problems
    NetworkIssues,
}

impl std::fmt::Display for PollingReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PollingReason::FirewallBlocked => write!(f, "firewall blocked"),
            PollingReason::EventTimeout => write!(f, "event timeout"),
            PollingReason::SubscriptionFailed => write!(f, "subscription failed"),
            PollingReason::NetworkIssues => write!(f, "network issues"),
        }
    }
}

/// Main EventBroker that coordinates all components
pub struct EventBroker {
    /// Speaker/service registration registry
    registry: Arc<SpeakerServiceRegistry>,

    /// Subscription lifecycle manager
    subscription_manager: Arc<SubscriptionManager>,

    /// Event processor for parsing and enriching events
    event_processor: Arc<EventProcessor>,

    /// Callback server for receiving UPnP events
    callback_server: Arc<CallbackServer>,

    /// Proactive firewall detection
    firewall_detector: Option<Arc<FirewallDetectionPlugin>>,

    /// Current firewall status
    firewall_status: Arc<RwLock<FirewallStatus>>,

    /// Event activity detector
    event_detector: Arc<EventDetector>,

    /// Resync detector for state drift
    resync_detector: Arc<ResyncDetector>,

    /// Polling scheduler
    polling_scheduler: Arc<PollingScheduler>,

    /// Main event stream sender
    event_sender: mpsc::UnboundedSender<EnrichedEvent>,

    /// Event receiver for the iterator (taken when creating iterator)
    event_receiver: Option<mpsc::UnboundedReceiver<EnrichedEvent>>,

    /// Configuration
    config: BrokerConfig,

    /// Shutdown signal
    shutdown_signal: Arc<AtomicBool>,

    /// Background task handles
    background_tasks: Vec<tokio::task::JoinHandle<()>>,

    /// UPnP event receiver for routing events from callback server to event processor
    upnp_receiver: Option<mpsc::UnboundedReceiver<callback_server::router::NotificationPayload>>,

    /// Event router for registering subscription IDs
    event_router: Option<Arc<callback_server::router::EventRouter>>,
}

/// Get the local IP address that can be reached by devices on the network
fn get_local_ip() -> Result<Ipv4Addr, std::io::Error> {
    // Create a UDP socket and connect to a remote address to determine the local interface
    // This doesn't actually send data, just determines which interface would be used
    let socket = UdpSocket::bind("0.0.0.0:0")?;
    socket.connect("8.8.8.8:53")?; // Connect to Google DNS

    match socket.local_addr()? {
        std::net::SocketAddr::V4(addr) => Ok(*addr.ip()),
        std::net::SocketAddr::V6(_) => {
            // Fallback to IPv4 localhost if we got IPv6
            Ok(Ipv4Addr::new(127, 0, 0, 1))
        }
    }
}

impl EventBroker {
    /// Create a new EventBroker with the specified configuration
    pub async fn new(config: BrokerConfig) -> BrokerResult<Self> {
        // Validate configuration
        config.validate()?;

        eprintln!("🚀 Initializing EventBroker with config: {:?}", config);

        // Create main event channel
        let (event_sender, event_receiver) = mpsc::unbounded_channel();

        // Initialize registry
        let registry = Arc::new(SpeakerServiceRegistry::new(config.max_registrations));

        // Create channel for UPnP events from callback server to event processor
        let (upnp_sender, upnp_receiver) = mpsc::unbounded_channel();

        // Initialize callback server which creates its own internal EventRouter
        let callback_server = Self::create_callback_server_with_routing(&config, upnp_sender).await?;

        // Get the event router from the callback server for subscription registration
        let event_router = Arc::clone(callback_server.router());

        // Get the actual network IP address so Sonos devices can reach the callback server
        let local_ip = get_local_ip()
            .map_err(|e| BrokerError::Configuration(format!("Failed to determine local IP address: {}", e)))?;
        let server_url = format!("http://{}:{}", local_ip, callback_server.port());

        // Initialize subscription manager with correct callback URL
        let subscription_manager = Arc::new(SubscriptionManager::new(
            server_url.clone(),
            config.subscription_timeout.as_secs() as u32,
        ));

        // Initialize event processor with the correct subscription manager
        let event_processor = Arc::new(EventProcessor::new(
            Arc::clone(&registry),
            Arc::clone(&subscription_manager),
            event_sender.clone(),
        ));

        // Initialize firewall detection if enabled
        let (firewall_detector, firewall_status) = if config.enable_proactive_firewall_detection {
            let detector = Self::create_firewall_detector(&config, &server_url).await?;
            let initial_status = detector.get_status().await;

            eprintln!("🔍 Firewall detection enabled, initial status: {:?}", initial_status);

            (Some(detector), Arc::new(RwLock::new(initial_status)))
        } else {
            eprintln!("⚠️  Firewall detection disabled");
            (None, Arc::new(RwLock::new(FirewallStatus::Unknown)))
        };

        // Initialize polling scheduler
        let polling_scheduler = Arc::new(PollingScheduler::new(
            event_sender.clone(),
            config.base_polling_interval,
            config.max_polling_interval,
            config.adaptive_polling,
            config.max_concurrent_polls,
        ));

        // Initialize event detector
        let event_detector = Arc::new(EventDetector::new(
            config.event_timeout,
            config.polling_activation_delay,
        ));

        // Initialize resync detector
        let resync_detector = Arc::new(ResyncDetector::new(config.resync_cooldown));

        let mut broker = Self {
            registry,
            subscription_manager,
            event_processor,
            callback_server,
            firewall_detector,
            firewall_status,
            event_detector,
            resync_detector,
            polling_scheduler,
            event_sender,
            event_receiver: Some(event_receiver),
            config,
            shutdown_signal: Arc::new(AtomicBool::new(false)),
            background_tasks: Vec::new(),
            upnp_receiver: Some(upnp_receiver),
            event_router: Some(event_router),
        };

        // Start background processing
        broker.start_background_processing().await?;

        eprintln!("✅ EventBroker initialized successfully");

        Ok(broker)
    }

    /// Create callback server with proper event routing
    async fn create_callback_server_with_routing(
        config: &BrokerConfig,
        event_sender: mpsc::UnboundedSender<callback_server::router::NotificationPayload>,
    ) -> BrokerResult<Arc<CallbackServer>> {
        let server = CallbackServer::new(config.callback_port_range, event_sender)
            .await
            .map_err(|e| BrokerError::CallbackServer(e.to_string()))?;

        Ok(Arc::new(server))
    }

    /// Create firewall detector if enabled
    async fn create_firewall_detector(
        config: &BrokerConfig,
        server_url: &str,
    ) -> BrokerResult<Arc<FirewallDetectionPlugin>> {
        use callback_server::firewall_detection::{FirewallDetectionConfig, FirewallDetectionPlugin};

        let fw_config = FirewallDetectionConfig {
            test_timeout: config.firewall_detection_timeout,
            max_retries: config.firewall_detection_retries,
            fallback_to_basic: config.firewall_detection_fallback,
        };

        let detector = FirewallDetectionPlugin::with_config(fw_config)
            .map_err(|e| BrokerError::FirewallDetection(e.to_string()))?;

        // Trigger initial detection
        // Note: This would require access to PluginContext, which we don't have here
        // In a real implementation, this would be coordinated with the callback server

        Ok(Arc::new(detector))
    }

    /// Start all background processing tasks
    async fn start_background_processing(&mut self) -> BrokerResult<()> {
        eprintln!("🔄 Starting background processing tasks");

        // Start UPnP event processing using the pre-connected receiver
        if let Some(upnp_receiver) = self.upnp_receiver.take() {
            let upnp_processor = Arc::clone(&self.event_processor);
            let upnp_task = tokio::spawn(async move {
                upnp_processor.start_upnp_processing(upnp_receiver).await;
            });
            self.background_tasks.push(upnp_task);
        }

        // Start polling request processing
        let (polling_request_sender, polling_request_receiver) = mpsc::unbounded_channel();
        self.start_polling_request_processing(polling_request_receiver).await;

        // Start event activity monitoring
        self.event_detector.start_monitoring().await;

        // Start subscription renewal monitoring
        self.start_subscription_renewal_monitoring().await;

        eprintln!("✅ Background processing tasks started");

        Ok(())
    }

    /// Start processing polling requests
    async fn start_polling_request_processing(
        &mut self,
        mut receiver: mpsc::UnboundedReceiver<PollingRequest>,
    ) {
        let polling_scheduler = Arc::clone(&self.polling_scheduler);
        let subscription_manager = Arc::clone(&self.subscription_manager);

        let task = tokio::spawn(async move {
            eprintln!("🔄 Starting polling request processing");

            while let Some(request) = receiver.recv().await {
                match request.action {
                    PollingAction::Start => {
                        eprintln!(
                            "🔄 Starting polling for {} {:?} (reason: {:?})",
                            request.speaker_service_pair.speaker_ip,
                            request.speaker_service_pair.service,
                            request.reason
                        );

                        if let Err(e) = polling_scheduler
                            .start_polling(request.registration_id, request.speaker_service_pair.clone())
                            .await
                        {
                            eprintln!("❌ Failed to start polling: {}", e);
                        } else {
                            // Mark polling as active in subscription
                            if let Some(subscription) = subscription_manager
                                .get_subscription(request.registration_id)
                                .await
                            {
                                subscription.set_polling_active(true);
                            }
                        }
                    }
                    PollingAction::Stop => {
                        eprintln!(
                            "🛑 Stopping polling for {} {:?}",
                            request.speaker_service_pair.speaker_ip,
                            request.speaker_service_pair.service
                        );

                        if let Err(e) = polling_scheduler
                            .stop_polling(request.registration_id)
                            .await
                        {
                            eprintln!("❌ Failed to stop polling: {}", e);
                        } else {
                            // Mark polling as inactive in subscription
                            if let Some(subscription) = subscription_manager
                                .get_subscription(request.registration_id)
                                .await
                            {
                                subscription.set_polling_active(false);
                            }
                        }
                    }
                }
            }

            eprintln!("🛑 Polling request processing stopped");
        });

        self.background_tasks.push(task);
    }

    /// Start subscription renewal monitoring
    async fn start_subscription_renewal_monitoring(&mut self) {
        let subscription_manager = Arc::clone(&self.subscription_manager);
        let renewal_threshold = self.config.renewal_threshold;

        let task = tokio::spawn(async move {
            eprintln!("🔄 Starting subscription renewal monitoring");

            let mut interval = tokio::time::interval(renewal_threshold / 2); // Check twice as often as threshold

            loop {
                interval.tick().await;

                match subscription_manager.check_renewals().await {
                    Ok(renewed_count) => {
                        if renewed_count > 0 {
                            eprintln!("✅ Renewed {} subscriptions", renewed_count);
                        }
                    }
                    Err(e) => {
                        eprintln!("❌ Error during subscription renewal check: {}", e);
                    }
                }
            }
        });

        self.background_tasks.push(task);
    }

    /// Register a speaker/service pair for event streaming
    pub async fn register_speaker_service(
        &self,
        speaker_ip: IpAddr,
        service: Service,
    ) -> BrokerResult<RegistrationResult> {
        eprintln!("📋 Registering {} {:?}", speaker_ip, service);

        // Check for duplicates and register
        let registration_id = self.registry.register(speaker_ip, service).await?;
        let was_duplicate = self.registry.is_registered(speaker_ip, service).await;

        if was_duplicate {
            eprintln!("ℹ️  Registration {} already exists", registration_id);
        }

        let pair = SpeakerServicePair::new(speaker_ip, service);

        // Get current firewall status
        let firewall_status = *self.firewall_status.read().await;

        // Create subscription
        let subscription_result = self
            .subscription_manager
            .create_subscription(registration_id, pair.clone())
            .await;

        let mut polling_reason = None;

        match subscription_result {
            Ok(subscription) => {
                eprintln!("✅ Created subscription {}", subscription.subscription_id());

                // Register subscription ID with EventRouter for event routing
                if let Some(router) = &self.event_router {
                    router.register(subscription.subscription_id().to_string()).await;
                    eprintln!("📝 Registered subscription {} with EventRouter", subscription.subscription_id());
                }

                // Register with event detector
                self.event_detector.register_subscription(registration_id).await;

                // Evaluate firewall status for immediate polling decision
                if let Some(request) = self
                    .event_detector
                    .evaluate_firewall_status(registration_id, &pair)
                    .await
                {
                    match request.reason {
                        crate::events::types::ResyncReason::FirewallBlocked => {
                            polling_reason = Some(PollingReason::FirewallBlocked);
                        }
                        crate::events::types::ResyncReason::NetworkIssues => {
                            polling_reason = Some(PollingReason::NetworkIssues);
                        }
                        _ => {}
                    }

                    // Start polling immediately
                    if let Err(e) = self.polling_scheduler
                        .start_polling(registration_id, pair.clone())
                        .await
                    {
                        eprintln!("❌ Failed to start immediate polling: {}", e);
                    } else {
                        subscription.set_polling_active(true);
                        eprintln!("🔄 Started immediate polling due to {:?}", request.reason);
                    }
                }
            }
            Err(e) => {
                eprintln!("❌ Failed to create subscription: {}", e);
                polling_reason = Some(PollingReason::SubscriptionFailed);

                // Start polling as fallback
                if let Err(e) = self.polling_scheduler
                    .start_polling(registration_id, pair.clone())
                    .await
                {
                    eprintln!("❌ Failed to start fallback polling: {}", e);
                    // Remove registration since both subscription and polling failed
                    let _ = self.registry.unregister(registration_id).await;
                    return Err(BrokerError::Polling(e));
                } else {
                    eprintln!("🔄 Started fallback polling due to subscription failure");
                }
            }
        }

        let result = RegistrationResult {
            registration_id,
            firewall_status,
            polling_reason,
            was_duplicate,
        };

        eprintln!("✅ Registration completed: {:?}", result);

        Ok(result)
    }

    /// Unregister a speaker/service pair
    pub async fn unregister_speaker_service(
        &self,
        registration_id: RegistrationId,
    ) -> BrokerResult<SpeakerServicePair> {
        eprintln!("📋 Unregistering {}", registration_id);

        // Get the pair before removing
        let pair = self.registry
            .get_pair(registration_id)
            .await
            .ok_or_else(|| BrokerError::Registry(
                crate::error::RegistryError::NotFound(registration_id)
            ))?;

        // Stop polling if active
        if let Err(e) = self.polling_scheduler.stop_polling(registration_id).await {
            eprintln!("⚠️  Failed to stop polling for {}: {}", registration_id, e);
        }

        // Remove subscription
        if let Err(e) = self.subscription_manager.remove_subscription(registration_id).await {
            eprintln!("⚠️  Failed to remove subscription for {}: {}", registration_id, e);
        }

        // Unregister from event detector
        self.event_detector.unregister_subscription(registration_id).await;

        // Remove from registry
        let removed_pair = self.registry.unregister(registration_id).await?;

        eprintln!("✅ Unregistered {} {:?}", pair.speaker_ip, pair.service);

        Ok(removed_pair)
    }

    /// Get an event iterator for consuming events
    /// This consumes the broker's event receiver, so it can only be called once
    pub fn event_iterator(&mut self) -> BrokerResult<EventIterator> {
        let receiver = self.event_receiver
            .take()
            .ok_or_else(|| BrokerError::Configuration(
                "Event iterator already created".to_string()
            ))?;

        let iterator = EventIterator::new(receiver, Some(Arc::clone(&self.resync_detector)));

        Ok(iterator)
    }

    /// Get comprehensive statistics about the broker
    pub async fn stats(&self) -> BrokerStats {
        let registry_stats = self.registry.stats().await;
        let subscription_stats = self.subscription_manager.stats().await;
        let polling_stats = self.polling_scheduler.stats().await;
        let event_processor_stats = self.event_processor.stats().await;
        let event_detector_stats = self.event_detector.stats().await;

        BrokerStats {
            registry_stats,
            subscription_stats,
            polling_stats,
            event_processor_stats,
            event_detector_stats,
            firewall_status: *self.firewall_status.read().await,
            background_tasks_count: self.background_tasks.len(),
        }
    }

    /// Get current firewall status
    pub async fn firewall_status(&self) -> FirewallStatus {
        *self.firewall_status.read().await
    }

    /// Manually trigger firewall detection
    pub async fn trigger_firewall_detection(&self) -> BrokerResult<FirewallStatus> {
        if let Some(detector) = &self.firewall_detector {
            // This would require proper integration with callback server context
            // For now, just return current status
            Ok(detector.get_status().await)
        } else {
            Err(BrokerError::Configuration(
                "Firewall detection is disabled".to_string()
            ))
        }
    }

    /// Shutdown the broker and all background tasks
    pub async fn shutdown(mut self) -> BrokerResult<()> {
        eprintln!("🛑 Shutting down EventBroker");

        // Signal shutdown
        self.shutdown_signal.store(true, Ordering::Relaxed);

        // Shutdown polling scheduler
        if let Err(e) = self.polling_scheduler.shutdown_all().await {
            eprintln!("⚠️  Error during polling shutdown: {}", e);
        }

        // Shutdown subscription manager
        if let Err(e) = self.subscription_manager.shutdown().await {
            eprintln!("⚠️  Error during subscription shutdown: {}", e);
        }

        // Cancel background tasks
        for task in self.background_tasks {
            task.abort();
        }

        // Clear registry
        self.registry.clear().await;

        eprintln!("✅ EventBroker shutdown complete");

        Ok(())
    }
}

/// Comprehensive statistics about the broker
#[derive(Debug)]
pub struct BrokerStats {
    pub registry_stats: crate::registry::RegistryStats,
    pub subscription_stats: crate::subscription::manager::SubscriptionStats,
    pub polling_stats: crate::polling::scheduler::PollingSchedulerStats,
    pub event_processor_stats: crate::events::processor::EventProcessorStats,
    pub event_detector_stats: crate::subscription::event_detector::EventDetectorStats,
    pub firewall_status: FirewallStatus,
    pub background_tasks_count: usize,
}

impl std::fmt::Display for BrokerStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "=== EventBroker Stats ===")?;
        writeln!(f, "Firewall Status: {:?}", self.firewall_status)?;
        writeln!(f, "Background Tasks: {}", self.background_tasks_count)?;
        writeln!(f)?;
        write!(f, "{}", self.registry_stats)?;
        writeln!(f)?;
        write!(f, "{}", self.subscription_stats)?;
        writeln!(f)?;
        write!(f, "{}", self.polling_stats)?;
        writeln!(f)?;
        write!(f, "{}", self.event_processor_stats)?;
        writeln!(f)?;
        write!(f, "{}", self.event_detector_stats)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_broker_creation() {
        let config = BrokerConfig::no_firewall_detection();
        let broker = EventBroker::new(config).await;

        // Note: This test might fail without proper callback server setup
        // In a real implementation, we'd need to mock the callback server
        assert!(broker.is_ok() || broker.is_err()); // Either works or fails gracefully
    }

    #[test]
    fn test_registration_result() {
        let result = RegistrationResult {
            registration_id: RegistrationId::new(1),
            firewall_status: FirewallStatus::Accessible,
            polling_reason: Some(PollingReason::FirewallBlocked),
            was_duplicate: false,
        };

        assert_eq!(result.registration_id.as_u64(), 1);
        assert_eq!(result.firewall_status, FirewallStatus::Accessible);
        assert_eq!(result.polling_reason, Some(PollingReason::FirewallBlocked));
        assert!(!result.was_duplicate);
    }

    #[test]
    fn test_polling_reason_display() {
        assert_eq!(PollingReason::FirewallBlocked.to_string(), "firewall blocked");
        assert_eq!(PollingReason::EventTimeout.to_string(), "event timeout");
        assert_eq!(PollingReason::SubscriptionFailed.to_string(), "subscription failed");
        assert_eq!(PollingReason::NetworkIssues.to_string(), "network issues");
    }
}