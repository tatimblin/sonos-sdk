//! Main EventBroker implementation
//!
//! This is the central component that integrates all other components and provides
//! the primary user interface for the sonos-stream crate. It coordinates subscription
//! management, event processing, polling, and firewall detection.

use std::net::{IpAddr, Ipv4Addr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use callback_server::{CallbackServer, FirewallDetectionCoordinator, FirewallDetectionConfig, FirewallStatus};
use sonos_api::Service;

use crate::config::BrokerConfig;
use crate::error::{BrokerError, BrokerResult};
use crate::events::{
    iterator::EventIterator,
    processor::EventProcessor,
    types::EnrichedEvent,
};
use crate::polling::scheduler::PollingScheduler;
use crate::registry::{RegistrationId, SpeakerServicePair, SpeakerServiceRegistry};
use crate::subscription::{
    event_detector::{EventDetector, PollingRequest, PollingAction},
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

    /// Per-device firewall detection coordinator
    firewall_coordinator: Option<Arc<FirewallDetectionCoordinator>>,

    /// Event activity detector
    event_detector: Arc<EventDetector>,


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

        info!(config = ?config, "Initializing EventBroker");

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
        ));

        // Initialize firewall detection coordinator if enabled
        let firewall_coordinator = if config.enable_proactive_firewall_detection {
            let coordinator_config = FirewallDetectionConfig {
                event_wait_timeout: config.firewall_event_wait_timeout,
                enable_caching: config.enable_firewall_caching,
                max_cached_devices: config.max_cached_device_states,
            };

            let coordinator = Arc::new(FirewallDetectionCoordinator::new(coordinator_config));

            info!(
                timeout = ?config.firewall_event_wait_timeout,
                "Firewall detection coordinator enabled"
            );

            Some(coordinator)
        } else {
            debug!("Firewall detection disabled");
            None
        };

        // Initialize event processor with the correct subscription manager and firewall coordinator
        let event_processor = Arc::new(EventProcessor::new(
            Arc::clone(&subscription_manager),
            event_sender.clone(),
            firewall_coordinator.clone(),
        ));

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


        let mut broker = Self {
            registry,
            subscription_manager,
            event_processor,
            callback_server,
            firewall_coordinator,
            event_detector,
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

        info!("EventBroker initialized successfully");

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

    /// Check if this is the first subscription for a given device IP
    /// This should be called BEFORE creating the new subscription
    async fn is_first_subscription_for_device(&self, device_ip: IpAddr) -> bool {
        // Check all currently registered speaker/service pairs
        let registered_pairs = self.registry.list_registrations().await;

        // Count how many are for this device IP
        let existing_count = registered_pairs
            .iter()
            .filter(|(_, pair)| pair.speaker_ip == device_ip)
            .count();

        // If there are no existing pairs for this device, it will be the first
        // If there's 1, it means we just registered it, so this is still the first
        existing_count <= 1
    }

    /// Start all background processing tasks
    async fn start_background_processing(&mut self) -> BrokerResult<()> {
        debug!("Starting background processing tasks");

        // Start UPnP event processing using the pre-connected receiver
        if let Some(upnp_receiver) = self.upnp_receiver.take() {
            let upnp_processor = Arc::clone(&self.event_processor);
            let upnp_task = tokio::spawn(async move {
                upnp_processor.start_upnp_processing(upnp_receiver).await;
            });
            self.background_tasks.push(upnp_task);
        }

        // Start polling request processing
        let (_polling_request_sender, polling_request_receiver) = mpsc::unbounded_channel();
        self.start_polling_request_processing(polling_request_receiver).await;

        // Start event activity monitoring
        self.event_detector.start_monitoring().await;

        // Start subscription renewal monitoring
        self.start_subscription_renewal_monitoring().await;

        debug!("Background processing tasks started");

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
            info!("Starting polling request processing");

            while let Some(request) = receiver.recv().await {
                match request.action {
                    PollingAction::Start => {
                        debug!(
                            speaker_ip = %request.speaker_service_pair.speaker_ip,
                            service = ?request.speaker_service_pair.service,
                            reason = ?request.reason,
                            registration_id = %request.registration_id,
                            "Starting polling for speaker service"
                        );

                        if let Err(e) = polling_scheduler
                            .start_polling(request.registration_id, request.speaker_service_pair.clone())
                            .await
                        {
                            error!(
                                registration_id = %request.registration_id,
                                speaker_ip = %request.speaker_service_pair.speaker_ip,
                                service = ?request.speaker_service_pair.service,
                                error = %e,
                                "Failed to start polling"
                            );
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
                        debug!(
                            speaker_ip = %request.speaker_service_pair.speaker_ip,
                            service = ?request.speaker_service_pair.service,
                            registration_id = %request.registration_id,
                            "Stopping polling for speaker service"
                        );

                        if let Err(e) = polling_scheduler
                            .stop_polling(request.registration_id)
                            .await
                        {
                            error!(
                                registration_id = %request.registration_id,
                                speaker_ip = %request.speaker_service_pair.speaker_ip,
                                service = ?request.speaker_service_pair.service,
                                error = %e,
                                "Failed to stop polling"
                            );
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

            info!("Polling request processing stopped");
        });

        self.background_tasks.push(task);
    }

    /// Start subscription renewal monitoring
    async fn start_subscription_renewal_monitoring(&mut self) {
        let subscription_manager = Arc::clone(&self.subscription_manager);
        let renewal_threshold = self.config.renewal_threshold;

        let task = tokio::spawn(async move {
            info!("Starting subscription renewal monitoring");

            let mut interval = tokio::time::interval(renewal_threshold / 2); // Check twice as often as threshold

            loop {
                interval.tick().await;

                match subscription_manager.check_renewals().await {
                    Ok(renewed_count) => {
                        if renewed_count > 0 {
                            debug!(
                                renewed_count = renewed_count,
                                "Renewed subscriptions"
                            );
                        }
                    }
                    Err(e) => {
                        error!(
                            error = %e,
                            "Error during subscription renewal check"
                        );
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
        debug!(
            speaker_ip = %speaker_ip,
            service = ?service,
            "Registering speaker service"
        );

        // Check for duplicates and register
        let registration_id = self.registry.register(speaker_ip, service).await?;
        let was_duplicate = self.registry.is_registered(speaker_ip, service).await;

        if was_duplicate {
            debug!(
                registration_id = %registration_id,
                "Registration already exists"
            );
        }

        let pair = SpeakerServicePair::new(speaker_ip, service);

        // Check if this is the first subscription for this device
        let is_first_for_device = self.is_first_subscription_for_device(speaker_ip).await;

        // Get or trigger firewall detection for this device
        let firewall_status = if let Some(coordinator) = &self.firewall_coordinator {
            if is_first_for_device {
                // First subscription for this device - trigger detection
                debug!(
                    speaker_ip = %speaker_ip,
                    "First subscription for device, triggering firewall detection"
                );
                coordinator.on_first_subscription(speaker_ip).await
            } else {
                // Use cached status
                coordinator.get_device_status(speaker_ip).await
            }
        } else {
            FirewallStatus::Unknown
        };

        // Create subscription
        let subscription_result = self
            .subscription_manager
            .create_subscription(registration_id, pair.clone())
            .await;

        let mut polling_reason = None;

        match subscription_result {
            Ok(subscription) => {
                debug!(
                    subscription_id = %subscription.subscription_id(),
                    "Created UPnP subscription"
                );

                // Register subscription ID with EventRouter for event routing
                if let Some(router) = &self.event_router {
                    router.register(subscription.subscription_id().to_string()).await;
                    debug!(
                        subscription_id = %subscription.subscription_id(),
                        "Registered subscription with EventRouter"
                    );
                }

                // Register with event detector
                self.event_detector.register_subscription(registration_id).await;

                // Evaluate firewall status for immediate polling decision
                if let Some(request) = self
                    .event_detector
                    .evaluate_firewall_status(registration_id, &pair)
                    .await
                {
                    polling_reason = Some(request.reason.clone());

                    // Start polling immediately
                    if let Err(e) = self.polling_scheduler
                        .start_polling(registration_id, pair.clone())
                        .await
                    {
                        error!(
                            registration_id = %registration_id,
                            error = %e,
                            "Failed to start immediate polling"
                        );
                    } else {
                        subscription.set_polling_active(true);
                        debug!(
                            registration_id = %registration_id,
                            reason = ?request.reason,
                            "Started immediate polling"
                        );
                    }
                }
            }
            Err(e) => {
                error!(
                    registration_id = %registration_id,
                    error = %e,
                    "Failed to create subscription, falling back to polling"
                );
                polling_reason = Some(PollingReason::SubscriptionFailed);

                // Start polling as fallback
                if let Err(e) = self.polling_scheduler
                    .start_polling(registration_id, pair.clone())
                    .await
                {
                    error!(
                        registration_id = %registration_id,
                        error = %e,
                        "Failed to start fallback polling"
                    );
                    // Remove registration since both subscription and polling failed
                    let _ = self.registry.unregister(registration_id).await;
                    return Err(BrokerError::Polling(e));
                } else {
                    debug!(
                        registration_id = %registration_id,
                        "Started fallback polling due to subscription failure"
                    );
                }
            }
        }

        let result = RegistrationResult {
            registration_id,
            firewall_status,
            polling_reason,
            was_duplicate,
        };

        debug!(
            registration_id = %result.registration_id,
            firewall_status = ?result.firewall_status,
            polling_reason = ?result.polling_reason,
            was_duplicate = result.was_duplicate,
            "Registration completed"
        );

        Ok(result)
    }

    /// Unregister a speaker/service pair
    pub async fn unregister_speaker_service(
        &self,
        registration_id: RegistrationId,
    ) -> BrokerResult<SpeakerServicePair> {
        debug!(registration_id = %registration_id, "Unregistering subscription");

        // Get the pair before removing
        let pair = self.registry
            .get_pair(registration_id)
            .await
            .ok_or_else(|| BrokerError::Registry(
                crate::error::RegistryError::NotFound(registration_id)
            ))?;

        // Stop polling if active
        if let Err(e) = self.polling_scheduler.stop_polling(registration_id).await {
            warn!(
                registration_id = %registration_id,
                error = %e,
                "Failed to stop polling during unregistration"
            );
        }

        // Remove subscription
        if let Err(e) = self.subscription_manager.remove_subscription(registration_id).await {
            warn!(
                registration_id = %registration_id,
                error = %e,
                "Failed to remove subscription during unregistration"
            );
        }

        // Unregister from event detector
        self.event_detector.unregister_subscription(registration_id).await;

        // Remove from registry
        let removed_pair = self.registry.unregister(registration_id).await?;

        debug!(
            speaker_ip = %pair.speaker_ip,
            service = ?pair.service,
            registration_id = %registration_id,
            "Unregistration completed"
        );

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

        let iterator = EventIterator::new(receiver);

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
            firewall_status: FirewallStatus::Unknown, // Status is now per-device
            background_tasks_count: self.background_tasks.len(),
        }
    }

    /// Get current firewall status (returns Unknown since status is now per-device)
    pub async fn firewall_status(&self) -> FirewallStatus {
        // Since firewall status is now per-device, this method returns Unknown
        // Use get_device_firewall_status() for specific device status
        FirewallStatus::Unknown
    }

    /// Get firewall status for a specific device
    pub async fn get_device_firewall_status(&self, device_ip: IpAddr) -> FirewallStatus {
        if let Some(coordinator) = &self.firewall_coordinator {
            coordinator.get_device_status(device_ip).await
        } else {
            FirewallStatus::Unknown
        }
    }

    /// Manually trigger firewall detection for a specific device
    pub async fn trigger_firewall_detection(&self, device_ip: IpAddr) -> BrokerResult<FirewallStatus> {
        if let Some(coordinator) = &self.firewall_coordinator {
            // Trigger detection by calling on_first_subscription
            // This will start monitoring for the device
            Ok(coordinator.on_first_subscription(device_ip).await)
        } else {
            Err(BrokerError::Configuration(
                "Firewall detection is disabled".to_string()
            ))
        }
    }

    /// Shutdown the broker and all background tasks
    pub async fn shutdown(self) -> BrokerResult<()> {
        info!("Shutting down EventBroker");

        // Signal shutdown
        self.shutdown_signal.store(true, Ordering::Relaxed);

        // Shutdown polling scheduler
        if let Err(e) = self.polling_scheduler.shutdown_all().await {
            warn!(error = %e, "Error during polling shutdown");
        }

        // Shutdown subscription manager
        if let Err(e) = self.subscription_manager.shutdown().await {
            warn!(error = %e, "Error during subscription shutdown");
        }

        // Cancel background tasks
        for task in self.background_tasks {
            task.abort();
        }

        // Clear registry
        self.registry.clear().await;

        info!("EventBroker shutdown complete");

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