//! Main EventBroker implementation
//!
//! This is the central component that integrates all other components and provides
//! the primary user interface for the sonos-stream crate. It coordinates subscription
//! management, event processing, polling, and firewall detection.

use std::net::IpAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use callback_server::{
    CallbackServer, FirewallDetectionConfig, FirewallDetectionCoordinator, FirewallStatus,
};
use sonos_api::Service;

use crate::config::BrokerConfig;
use crate::error::{BrokerError, BrokerResult};
use crate::events::{iterator::EventIterator, processor::EventProcessor, types::EnrichedEvent};
use crate::polling::scheduler::PollingScheduler;
use crate::registry::{RegistrationId, SpeakerServicePair, SpeakerServiceRegistry};
use crate::subscription::{
    event_detector::{EventDetector, PollingAction, PollingRequest},
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
    /// Forced polling mode (config-driven, e.g. firewall simulation)
    ForcedPolling,
}

impl std::fmt::Display for PollingReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PollingReason::FirewallBlocked => write!(f, "firewall blocked"),
            PollingReason::EventTimeout => write!(f, "event timeout"),
            PollingReason::SubscriptionFailed => write!(f, "subscription failed"),
            PollingReason::NetworkIssues => write!(f, "network issues"),
            PollingReason::ForcedPolling => write!(f, "forced polling"),
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

    /// Callback server for receiving UPnP events (kept alive via Arc)
    _callback_server: Arc<CallbackServer>,

    /// Per-device firewall detection coordinator
    firewall_coordinator: Option<Arc<FirewallDetectionCoordinator>>,

    /// Event activity detector
    event_detector: Arc<EventDetector>,

    /// Polling scheduler
    polling_scheduler: Arc<PollingScheduler>,

    /// Main event stream sender (kept alive for channel)
    _event_sender: mpsc::UnboundedSender<EnrichedEvent>,

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

    /// Polling request channel receiver (taken during background processing startup)
    polling_request_receiver: Option<mpsc::UnboundedReceiver<PollingRequest>>,
}

/// The callback URL handed to every UPnP SUBSCRIBE.
///
/// Reads the callback server's own `base_url()` verbatim. It used to run a second,
/// independent route-to-8.8.8.8 probe and rebuild `http://{ip}:{port}` by hand —
/// duplicating a derivation `CallbackServer` had already done. Two copies is what
/// let them drift, and a wrong one means speakers are told an address they cannot
/// reach, so every event is silently lost and then misreported as a firewall
/// block. One authoritative source.
///
/// A named function so that single-source property is directly testable.
fn subscription_callback_url(callback_server: &CallbackServer) -> String {
    callback_server.base_url().to_string()
}

/// Warn when a speaker is on no subnet we hold an address on.
///
/// The callback URL is a single `base_url`, chosen before any speaker is known, so
/// a speaker off that subnet cannot reach it and will fall back to polling. This
/// uses each interface's real netmask: on this project's own network — one flat
/// `192.168.4.0/22` — a `192.168.5.x` speaker *is* reachable from `192.168.4.32`,
/// and a /24 assumption would raise a false warning for half the household.
fn warn_if_speaker_unreachable(callback_server: &CallbackServer, speaker_ip: IpAddr) {
    let IpAddr::V4(v4) = speaker_ip else {
        return;
    };
    if CallbackServer::local_ip_for_speaker(v4).is_none() {
        warn!(
            speaker_ip = %speaker_ip,
            callback_url = %callback_server.base_url(),
            "Speaker is not on any local subnet; UPnP events will not reach the \
             callback server and this device will fall back to polling"
        );
    }
}

/// Release a UPnP subscription ID from the [`EventRouter`]'s active set.
///
/// Without this the router's SID set grows for the entire process lifetime and
/// keeps accepting events for subscriptions that no longer exist. It is called
/// unconditionally on the captured SID: even if the UPnP UNSUBSCRIBE failed, the
/// local subscription is gone, so routing its events serves no purpose.
///
/// A free function taking `Option`s so the leak can be tested without standing up
/// a broker (which needs a bound callback server and a real device).
async fn release_router_sid(
    router: Option<&callback_server::router::EventRouter>,
    subscription_id: Option<&str>,
) {
    if let (Some(router), Some(sid)) = (router, subscription_id) {
        router.unregister(sid).await;
        debug!(
            subscription_id = %sid,
            "Unregistered subscription from EventRouter"
        );
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
        let callback_server =
            Self::create_callback_server_with_routing(&config, upnp_sender).await?;

        // Get the event router from the callback server for subscription registration
        let event_router = Arc::clone(callback_server.router());

        // Initialize subscription manager with the callback server's own URL.
        let subscription_manager = Arc::new(SubscriptionManager::new(subscription_callback_url(
            &callback_server,
        )));

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

        // Create polling request channel (sender kept alive for EventDetector)
        let (polling_request_sender, polling_request_receiver) = mpsc::unbounded_channel();

        // Initialize event detector and connect to firewall coordinator + polling channel.
        //
        // This must be built BEFORE the EventProcessor: the processor holds an
        // Arc<EventDetector> so it can report UPnP event liveness. Both only depend
        // on `config` and `firewall_coordinator`, which already exist, so the
        // detector is safe to construct here.
        let mut event_detector = EventDetector::new(config.event_timeout);
        if let Some(ref coordinator) = firewall_coordinator {
            event_detector.set_firewall_coordinator(Arc::clone(coordinator));
        }
        event_detector.set_polling_request_sender(polling_request_sender);
        let event_detector = Arc::new(event_detector);

        // Initialize event processor with the correct subscription manager, firewall
        // coordinator, and event detector
        let event_processor = Arc::new(EventProcessor::new(
            Arc::clone(&subscription_manager),
            event_sender.clone(),
            firewall_coordinator.clone(),
            Arc::clone(&event_detector),
        ));

        // Initialize polling scheduler
        let polling_scheduler = Arc::new(PollingScheduler::new(
            event_sender.clone(),
            config.base_polling_interval,
            config.max_polling_interval,
            config.adaptive_polling,
            config.max_concurrent_polls,
        ));

        let mut broker = Self {
            registry,
            subscription_manager,
            event_processor,
            _callback_server: callback_server,
            firewall_coordinator,
            event_detector,
            polling_scheduler,
            _event_sender: event_sender,
            event_receiver: Some(event_receiver),
            config,
            shutdown_signal: Arc::new(AtomicBool::new(false)),
            background_tasks: Vec::new(),
            upnp_receiver: Some(upnp_receiver),
            event_router: Some(event_router),
            polling_request_receiver: Some(polling_request_receiver),
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

        // Start polling request processing using pre-created channel
        if let Some(polling_request_receiver) = self.polling_request_receiver.take() {
            self.start_polling_request_processing(polling_request_receiver)
                .await;
        }

        // Start event activity monitoring
        let monitoring_handle = self.event_detector.start_monitoring().await;
        self.background_tasks.push(monitoring_handle);

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
        let event_detector = Arc::clone(&self.event_detector);
        let registry = Arc::clone(&self.registry);

        // Requests are handled strictly in order on a single task. Serialising is
        // deliberate: concurrent handling could apply a Start and a Stop for the same
        // registration out of order.
        //
        // A Stop can still block this loop. Shutdown now interrupts the polling loop's
        // sleeps promptly — its signal carries a Notify and every sleep selects on it —
        // but it must still await the *in-flight poll*: 4-5 sequential SOAP calls, each
        // with a 5s connect / 10s read timeout. Against an unreachable speaker that is
        // seconds. Requests queued behind a Stop can therefore still be stale by the
        // time they are handled, which is why `handle_polling_request` re-checks
        // liveness.
        let task = tokio::spawn(async move {
            info!("Starting polling request processing");

            while let Some(request) = receiver.recv().await {
                Self::handle_polling_request(
                    request,
                    &polling_scheduler,
                    &subscription_manager,
                    &event_detector,
                    &registry,
                )
                .await;
            }

            info!("Polling request processing stopped");
        });

        self.background_tasks.push(task);
    }

    /// Apply a single [`PollingRequest`].
    ///
    /// A free-standing associated function taking its collaborators explicitly so the
    /// start/stop transitions can be unit-tested without a live broker (which needs a
    /// bound callback server and a reachable device).
    async fn handle_polling_request(
        request: PollingRequest,
        polling_scheduler: &PollingScheduler,
        subscription_manager: &SubscriptionManager,
        event_detector: &EventDetector,
        registry: &SpeakerServiceRegistry,
    ) {
        match request.action {
            PollingAction::Start => {
                // Re-check liveness before spawning anything. This request may have
                // been queued behind a slow Stop (see the comment in
                // `start_polling_request_processing`) and the registration may have
                // been unregistered in the meantime. `start_polling` validates only
                // "already polling" and the concurrency cap — it will happily spawn a
                // task for a registration that no longer exists anywhere, and nothing
                // could then stop it: no UPnP event can arrive without a subscription,
                // the detector has no entry, and `unregister_speaker_service` returns
                // NotFound before touching the scheduler. Only `shutdown_all()` would
                // ever reap it.
                if registry.get_pair(request.registration_id).await.is_none() {
                    debug!(
                        registration_id = %request.registration_id,
                        speaker_ip = %request.speaker_service_pair.speaker_ip,
                        service = ?request.speaker_service_pair.service,
                        "Dropping stale polling start: registration no longer exists"
                    );
                    return;
                }

                debug!(
                    speaker_ip = %request.speaker_service_pair.speaker_ip,
                    service = ?request.speaker_service_pair.service,
                    reason = ?request.reason,
                    registration_id = %request.registration_id,
                    "Starting polling for speaker service"
                );

                if let Err(e) = polling_scheduler
                    .start_polling(
                        request.registration_id,
                        request.speaker_service_pair.clone(),
                    )
                    .await
                {
                    error!(
                        registration_id = %request.registration_id,
                        speaker_ip = %request.speaker_service_pair.speaker_ip,
                        service = ?request.speaker_service_pair.service,
                        error = %e,
                        "Failed to start polling"
                    );
                    // The detector marked this registration as polling when it
                    // sent the request. Polling never started, so clear the
                    // marker or timeout detection stays suppressed forever.
                    event_detector
                        .clear_polling_active(request.registration_id)
                        .await;
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
                    reason = ?request.reason,
                    "UPnP events resumed; stopping polling for speaker service"
                );

                // `stop_polling` removes the task from the scheduler map before
                // awaiting its shutdown, so a failure here still means the task
                // is gone from the scheduler's view. Clear the subscription flag
                // either way, otherwise stats would report polling forever.
                if let Err(e) = polling_scheduler
                    .stop_polling(request.registration_id)
                    .await
                {
                    error!(
                        registration_id = %request.registration_id,
                        speaker_ip = %request.speaker_service_pair.speaker_ip,
                        service = ?request.speaker_service_pair.service,
                        error = %e,
                        "Error while stopping polling task"
                    );
                }

                if let Some(subscription) = subscription_manager
                    .get_subscription(request.registration_id)
                    .await
                {
                    subscription.set_polling_active(false);
                }

                info!(
                    registration_id = %request.registration_id,
                    speaker_ip = %request.speaker_service_pair.speaker_ip,
                    service = ?request.speaker_service_pair.service,
                    "Polling stopped; back to UPnP events"
                );
            }
        }
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
                            debug!(renewed_count = renewed_count, "Renewed subscriptions");
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

        // Check for duplicates and register. The duplicate verdict is decided inside
        // `register_reporting_duplicate`, under the locks that perform the insert. It
        // used to be a separate `is_registered` call made *after* `register`, which
        // always answered `true` — see the doc comment on that method.
        let (registration_id, was_duplicate) = self
            .registry
            .register_reporting_duplicate(speaker_ip, service)
            .await?;

        let pair = SpeakerServicePair::new(speaker_ip, service);

        // Short-circuit an already-registered pair. Falling through would perform a
        // second UPnP SUBSCRIBE, yielding a *new* SID, and
        // `SubscriptionManager::create_subscription` would overwrite the wrapper holding
        // the old one. The superseded SID stays in the EventRouter's active set for the
        // process lifetime (nothing else can name it any more) and a later
        // `unregister_speaker_service` releases only the newest. Returning the existing
        // registration untouched is also what callers expect from a registry that
        // deduplicates: the first registration's subscription, polling state and
        // detector entry are still live and correct.
        if was_duplicate {
            debug!(
                registration_id = %registration_id,
                speaker_ip = %speaker_ip,
                service = ?service,
                "Registration already exists; reusing it without re-subscribing"
            );

            return Ok(RegistrationResult {
                registration_id,
                firewall_status: self.get_device_firewall_status(speaker_ip).await,
                // `polling_reason` reports what *this* call activated, and this call
                // activated nothing. Whether the reused registration is currently
                // polling is a separate question, answered by `stats()`.
                polling_reason: None,
                was_duplicate: true,
            });
        }

        let mut polling_reason = None;
        let firewall_status;

        if self.config.force_polling_mode {
            // Force polling mode: skip UPnP subscription entirely, go straight to polling
            debug!(
                registration_id = %registration_id,
                speaker_ip = %speaker_ip,
                service = ?service,
                "Force polling mode: skipping UPnP subscription"
            );

            firewall_status = FirewallStatus::Blocked;
            polling_reason = Some(PollingReason::ForcedPolling);

            // Skip EventDetector registration — no UPnP events will arrive,
            // so monitoring would just detect a false timeout.

            // Start polling immediately
            if let Err(e) = self
                .polling_scheduler
                .start_polling(registration_id, pair.clone())
                .await
            {
                error!(
                    registration_id = %registration_id,
                    error = %e,
                    "Failed to start forced polling"
                );
                let _ = self.registry.unregister(registration_id).await;
                return Err(BrokerError::Polling(e));
            }
        } else {
            // Normal mode: attempt UPnP subscription with firewall detection

            // Check if this is the first subscription for this device
            let is_first_for_device = self.is_first_subscription_for_device(speaker_ip).await;

            if is_first_for_device {
                warn_if_speaker_unreachable(&self._callback_server, speaker_ip);
            }

            // Get or trigger firewall detection for this device
            firewall_status = if let Some(coordinator) = &self.firewall_coordinator {
                if is_first_for_device {
                    debug!(
                        speaker_ip = %speaker_ip,
                        "First subscription for device, triggering firewall detection"
                    );
                    coordinator.on_first_subscription(speaker_ip).await
                } else {
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

            match subscription_result {
                Ok(subscription) => {
                    debug!(
                        subscription_id = %subscription.subscription_id(),
                        "Created UPnP subscription"
                    );

                    // Register subscription ID with EventRouter for event routing
                    if let Some(router) = &self.event_router {
                        router
                            .register(subscription.subscription_id().to_string())
                            .await;
                        debug!(
                            subscription_id = %subscription.subscription_id(),
                            "Registered subscription with EventRouter"
                        );
                    }

                    // Register with event detector for timeout monitoring
                    self.event_detector
                        .register_subscription(registration_id, pair.clone())
                        .await;

                    // Evaluate firewall status for immediate polling decision
                    let eager = Self::activate_eager_polling(
                        registration_id,
                        &pair,
                        &self.event_detector,
                        &self.polling_scheduler,
                    )
                    .await;
                    if let Some((reason, started)) = eager {
                        polling_reason = Some(reason);
                        if started {
                            subscription.set_polling_active(true);
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
                    if let Err(e) = self
                        .polling_scheduler
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
        let pair = self.registry.get_pair(registration_id).await.ok_or({
            BrokerError::Registry(crate::error::RegistryError::NotFound(registration_id))
        })?;

        // Capture the UPnP subscription ID before the subscription is dropped — it is
        // needed to release the SID from the EventRouter during teardown. Resolving it
        // here, and passing it in, is what guarantees the read happens before
        // `remove_subscription` destroys the wrapper.
        let subscription_id = self
            .subscription_manager
            .get_subscription(registration_id)
            .await
            .map(|s| s.subscription_id().to_string());

        let removed_pair = Self::teardown_registration(
            registration_id,
            subscription_id.as_deref(),
            &self.polling_scheduler,
            &self.subscription_manager,
            self.event_router.as_deref(),
            &self.event_detector,
            &self.registry,
        )
        .await?;

        debug!(
            speaker_ip = %pair.speaker_ip,
            service = ?pair.service,
            registration_id = %registration_id,
            "Unregistration completed"
        );

        Ok(removed_pair)
    }

    /// Decide whether firewall status warrants starting polling immediately, and if so
    /// start it and record the fact with the detector.
    ///
    /// Returns `Some((reason, started))` when polling was called for, where `started`
    /// says whether the polling task actually began. Marking the detector is what makes
    /// the later `PollingAction::Stop` possible: without it the detector has no idea
    /// this registration is polling and would never emit a stop when events resume.
    ///
    /// Split out of `register_speaker_service`, and taking its collaborators
    /// explicitly, because the call site sits in the `Ok(subscription)` branch and is
    /// otherwise reachable only with a real `ManagedSubscription` from a live UPnP
    /// SUBSCRIBE.
    async fn activate_eager_polling(
        registration_id: RegistrationId,
        pair: &SpeakerServicePair,
        event_detector: &EventDetector,
        polling_scheduler: &PollingScheduler,
    ) -> Option<(PollingReason, bool)> {
        let request = event_detector
            .evaluate_firewall_status(registration_id, pair)
            .await?;

        if let Err(e) = polling_scheduler
            .start_polling(registration_id, pair.clone())
            .await
        {
            error!(
                registration_id = %registration_id,
                error = %e,
                "Failed to start immediate polling"
            );
            return Some((request.reason, false));
        }

        // Tell the detector this registration is polling, so the first UPnP event
        // that does arrive emits a Stop.
        event_detector
            .mark_polling_active(registration_id, request.reason.clone())
            .await;
        debug!(
            registration_id = %registration_id,
            reason = ?request.reason,
            "Started immediate polling"
        );

        Some((request.reason, true))
    }

    /// Tear down all state for a registration, given its already-resolved SID.
    ///
    /// Split out of [`Self::unregister_speaker_service`], and taking its collaborators
    /// explicitly, so the teardown sequence — including the EventRouter SID release
    /// that used to be missing entirely — is reachable from unit tests without
    /// constructing a broker (which binds a real callback-server socket).
    ///
    /// The SID is a parameter rather than looked up here both because obtaining a real
    /// one requires a live `ManagedSubscription` and because it must be read before
    /// `remove_subscription` drops the wrapper.
    #[allow(clippy::too_many_arguments)]
    async fn teardown_registration(
        registration_id: RegistrationId,
        subscription_id: Option<&str>,
        polling_scheduler: &PollingScheduler,
        subscription_manager: &SubscriptionManager,
        event_router: Option<&callback_server::router::EventRouter>,
        event_detector: &EventDetector,
        registry: &SpeakerServiceRegistry,
    ) -> BrokerResult<SpeakerServicePair> {
        // Stop polling if active
        if let Err(e) = polling_scheduler.stop_polling(registration_id).await {
            warn!(
                registration_id = %registration_id,
                error = %e,
                "Failed to stop polling during unregistration"
            );
        }

        // Remove subscription
        if let Err(e) = subscription_manager
            .remove_subscription(registration_id)
            .await
        {
            warn!(
                registration_id = %registration_id,
                error = %e,
                "Failed to remove subscription during unregistration"
            );
        }

        // Release the SID from the EventRouter.
        release_router_sid(event_router, subscription_id).await;

        // Unregister from event detector
        event_detector
            .unregister_subscription(registration_id)
            .await;

        // Remove from registry
        Ok(registry.unregister(registration_id).await?)
    }

    /// Get an event iterator for consuming events
    /// This consumes the broker's event receiver, so it can only be called once
    pub fn event_iterator(&mut self) -> BrokerResult<EventIterator> {
        let receiver = self.event_receiver.take().ok_or_else(|| {
            BrokerError::Configuration("Event iterator already created".to_string())
        })?;

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
    pub async fn trigger_firewall_detection(
        &self,
        device_ip: IpAddr,
    ) -> BrokerResult<FirewallStatus> {
        if let Some(coordinator) = &self.firewall_coordinator {
            // Trigger detection by calling on_first_subscription
            // This will start monitoring for the device
            Ok(coordinator.on_first_subscription(device_ip).await)
        } else {
            Err(BrokerError::Configuration(
                "Firewall detection is disabled".to_string(),
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
    use std::time::Duration;

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

    /// The broker used to derive its own callback URL with a second
    /// route-to-8.8.8.8 probe plus a hand-built `http://{ip}:{port}`, duplicating
    /// what `CallbackServer` had already computed. Asserting byte equality with
    /// `base_url()` pins the URL to one authoritative source, so the two cannot
    /// drift again.
    ///
    /// Binds a real callback server on an OS-assigned port; no speaker is contacted.
    #[tokio::test]
    async fn test_base_url_is_reused_not_rebuilt() {
        let port = {
            let listener = std::net::TcpListener::bind("0.0.0.0:0").unwrap();
            listener.local_addr().unwrap().port()
        };

        let (upnp_tx, _upnp_rx) = mpsc::unbounded_channel();
        let server = CallbackServer::new((port, port), upnp_tx)
            .await
            .expect("callback server should bind an OS-assigned port");

        assert_eq!(subscription_callback_url(&server), server.base_url());
        // Consumed verbatim, not re-derived: no separate IP probe can disagree.
        assert!(subscription_callback_url(&server).ends_with(&format!(":{port}")));

        server.shutdown().await.unwrap();
    }

    /// The unregistration path used to tear down its own state but never call
    /// `EventRouter::unregister`, so SIDs accumulated for the process lifetime and the
    /// router kept routing events for subscriptions that no longer existed.
    ///
    /// This drives the broker's real teardown sequence. The SID is supplied as an
    /// argument because obtaining a genuine one needs a live `ManagedSubscription`
    /// (only `ManagedSubscription::create()` builds one, via a real UPnP SUBSCRIBE);
    /// the release call site itself is inside `teardown_registration` and is therefore
    /// covered.
    #[tokio::test]
    async fn test_unregister_releases_router_sid() {
        let (event_tx, _event_rx) = mpsc::unbounded_channel();
        let scheduler = PollingScheduler::new(
            event_tx,
            Duration::from_secs(5),
            Duration::from_secs(30),
            false,
            50,
        );
        let subscription_manager =
            Arc::new(SubscriptionManager::new("http://callback.url".to_string()));
        let detector = EventDetector::new(Duration::from_secs(30));
        let registry = SpeakerServiceRegistry::new(100);

        // A router whose channel this test owns, standing in for the broker's.
        let (router_tx, mut router_rx) = mpsc::unbounded_channel();
        let router = callback_server::router::EventRouter::new(router_tx);

        let sid = "uuid:leak-check";
        router.register(sid.to_string()).await;

        let speaker_ip: IpAddr = "203.0.113.10".parse().unwrap();
        let registration_id = registry
            .register(speaker_ip, Service::AVTransport)
            .await
            .unwrap();

        // Real teardown sequence, with the SID resolved the way
        // unregister_speaker_service resolves it.
        EventBroker::teardown_registration(
            registration_id,
            Some(sid),
            &scheduler,
            &subscription_manager,
            Some(&router),
            &detector,
            &registry,
        )
        .await
        .expect("teardown should succeed");

        // A registered SID is forwarded to the channel immediately; an unregistered
        // one is only buffered. So the discriminator is *whether the event arrives
        // now*: if teardown released the SID, nothing is forwarded.
        router
            .route_event(sid.to_string(), "<event>stale</event>".to_string())
            .await;
        assert!(
            router_rx.try_recv().is_err(),
            "teardown must remove the SID from the router's active set, so events for \
             it are no longer forwarded"
        );

        // And it really was buffered rather than dropped: re-registering replays it.
        // This also proves the assertion above cannot pass by the event vanishing.
        router.register(sid.to_string()).await;
        let replayed = router_rx
            .try_recv()
            .expect("buffered event should replay on re-register");
        assert_eq!(replayed.subscription_id, sid);
        assert!(replayed.event_xml.contains("stale"));
    }

    /// A `Start` can be queued behind a slow `Stop` and drain after its registration
    /// has been unregistered. `start_polling` validates only "already polling" and the
    /// concurrency cap, so without a staleness check it spawns a polling task that
    /// nothing can ever stop: no subscription exists so no UPnP event can arrive, the
    /// detector has no entry, and `unregister_speaker_service` returns NotFound before
    /// reaching the scheduler. It would poll until process exit.
    #[tokio::test]
    async fn test_stale_start_does_not_spawn_polling_task() {
        let (event_tx, _event_rx) = mpsc::unbounded_channel();
        let scheduler = PollingScheduler::new(
            event_tx,
            Duration::from_secs(5),
            Duration::from_secs(30),
            false,
            50,
        );
        let subscription_manager =
            Arc::new(SubscriptionManager::new("http://callback.url".to_string()));
        let detector = EventDetector::new(Duration::from_secs(30));
        let registry = SpeakerServiceRegistry::new(100);

        let speaker_ip: IpAddr = "203.0.113.20".parse().unwrap();
        let pair = SpeakerServicePair::new(speaker_ip, Service::AVTransport);

        // A registration that was unregistered before the queued Start drained: the
        // registry has no entry for this ID.
        let stale_id = RegistrationId::new(999);

        EventBroker::handle_polling_request(
            PollingRequest {
                registration_id: stale_id,
                speaker_service_pair: pair.clone(),
                action: PollingAction::Start,
                reason: PollingReason::EventTimeout,
            },
            &scheduler,
            &subscription_manager,
            &detector,
            &registry,
        )
        .await;

        assert!(
            !scheduler.is_polling(stale_id).await,
            "a Start for an unregistered registration must not spawn a polling task"
        );

        // A live registration is still honoured, so the guard is not simply blocking
        // every Start.
        let live_id = registry
            .register(speaker_ip, Service::AVTransport)
            .await
            .unwrap();
        EventBroker::handle_polling_request(
            PollingRequest {
                registration_id: live_id,
                speaker_service_pair: pair,
                action: PollingAction::Start,
                reason: PollingReason::EventTimeout,
            },
            &scheduler,
            &subscription_manager,
            &detector,
            &registry,
        )
        .await;

        assert!(
            scheduler.is_polling(live_id).await,
            "a Start for a live registration must still start polling"
        );

        scheduler.shutdown_all().await.unwrap();
    }

    /// Eagerly-started (firewall-driven) polling must be recorded with the detector.
    /// Without it the detector has no entry saying this registration is polling, so the
    /// first resumed UPnP event would never emit a `PollingAction::Stop` and the
    /// fallback would run forever — fix (b) would cover only the timeout path.
    #[tokio::test]
    async fn test_eager_polling_is_recorded_with_detector() {
        // A coordinator with a tiny wait timeout reaches `Blocked` without any network:
        // detection starts, no event arrives, and it times out as blocked.
        let coordinator = Arc::new(FirewallDetectionCoordinator::new(FirewallDetectionConfig {
            event_wait_timeout: Duration::from_millis(50),
            enable_caching: true,
            max_cached_devices: 16,
        }));

        let speaker_ip: IpAddr = "203.0.113.40".parse().unwrap();
        coordinator.on_first_subscription(speaker_ip).await;

        // Wait for detection to conclude "blocked".
        let mut status = coordinator.get_device_status(speaker_ip).await;
        for _ in 0..40 {
            if status == FirewallStatus::Blocked {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
            status = coordinator.get_device_status(speaker_ip).await;
        }
        assert_eq!(
            status,
            FirewallStatus::Blocked,
            "precondition: detection should time out as blocked"
        );

        let mut detector = EventDetector::new(Duration::from_secs(30));
        detector.set_firewall_coordinator(Arc::clone(&coordinator));
        let (req_tx, mut req_rx) = mpsc::unbounded_channel();
        detector.set_polling_request_sender(req_tx);

        let (event_tx, _event_rx) = mpsc::unbounded_channel();
        let scheduler = PollingScheduler::new(
            event_tx,
            Duration::from_secs(60),
            Duration::from_secs(120),
            false,
            50,
        );

        let registration_id = RegistrationId::new(1);
        let pair = SpeakerServicePair::new(speaker_ip, Service::AVTransport);
        detector
            .register_subscription(registration_id, pair.clone())
            .await;

        let outcome =
            EventBroker::activate_eager_polling(registration_id, &pair, &detector, &scheduler)
                .await;
        assert_eq!(
            outcome,
            Some((PollingReason::FirewallBlocked, true)),
            "blocked firewall should start polling immediately"
        );

        // The detector must now consider this registration to be polling, which is
        // observable as a Stop on the next UPnP event.
        detector.record_event(registration_id).await;
        let request = req_rx
            .try_recv()
            .expect("eager polling must be recorded, otherwise no Stop is ever emitted");
        assert!(matches!(request.action, PollingAction::Stop));
        assert_eq!(request.reason, PollingReason::FirewallBlocked);

        scheduler.shutdown_all().await.unwrap();
    }

    /// When a `Start` fails, the detector's polling marker must be cleared. The
    /// detector set it when it sent the request; leaving it set would suppress timeout
    /// detection for that registration forever, so it could never get another polling
    /// fallback. Failure is induced with the concurrency cap — no network needed.
    #[tokio::test]
    async fn test_failed_start_clears_polling_marker() {
        let (event_tx, _event_rx) = mpsc::unbounded_channel();
        // Concurrency cap of 0: `start_polling` checks `tasks.len() >= cap` before
        // spawning, so every request fails and no polling task is ever created. That
        // exercises the failure branch with no network and no waiting.
        let scheduler = PollingScheduler::new(
            event_tx,
            Duration::from_secs(5),
            Duration::from_secs(30),
            false,
            0,
        );
        let subscription_manager =
            Arc::new(SubscriptionManager::new("http://callback.url".to_string()));
        // Short timeout plus a request sender, so the monitoring sweep can be used as
        // the observable for whether the polling marker was cleared.
        let mut detector = EventDetector::new(Duration::from_millis(50));
        let (req_tx, mut req_rx) = mpsc::unbounded_channel();
        detector.set_polling_request_sender(req_tx);
        let registry = SpeakerServiceRegistry::new(100);

        // The registration whose Start will fail.
        let victim_ip: IpAddr = "203.0.113.31".parse().unwrap();
        let victim_pair = SpeakerServicePair::new(victim_ip, Service::RenderingControl);
        let victim_id = registry
            .register(victim_ip, Service::RenderingControl)
            .await
            .unwrap();
        detector
            .register_subscription(victim_id, victim_pair.clone())
            .await;
        detector
            .mark_polling_active(victim_id, PollingReason::EventTimeout)
            .await;

        EventBroker::handle_polling_request(
            PollingRequest {
                registration_id: victim_id,
                speaker_service_pair: victim_pair,
                action: PollingAction::Start,
                reason: PollingReason::EventTimeout,
            },
            &scheduler,
            &subscription_manager,
            &detector,
            &registry,
        )
        .await;

        assert!(
            !scheduler.is_polling(victim_id).await,
            "precondition: the Start should have failed on the concurrency cap"
        );

        // Marker cleared => the timeout sweep can request polling again. The sweep
        // skips registrations whose `polling_reason` is set, so a fresh Start request
        // arriving is the observable proof the marker was cleared.
        detector.backdate_last_event_for_test(victim_id).await;
        let detector = Arc::new(detector);
        let sweep = detector.start_monitoring().await;

        let request = tokio::time::timeout(Duration::from_secs(2), req_rx.recv())
            .await
            .expect(
                "a failed Start must clear the polling marker, otherwise the sweep \
                     skips this registration and timeout detection stays suppressed forever",
            )
            .expect("channel should deliver a request");
        assert_eq!(request.registration_id, victim_id);
        assert!(matches!(request.action, PollingAction::Start));

        sweep.abort();
        scheduler.shutdown_all().await.unwrap();
    }

    /// A port range the OS just told us is free.
    ///
    /// Hardcoded ranges caused a real flake in this workspace: concurrent test runs
    /// raced for the same ports and one failed to bind. Separate `CARGO_TARGET_DIR`s do
    /// not help — the contended resource is the host's port space.
    ///
    /// Unlike `callback_server::server::tests::free_port_range`, this returns a
    /// two-port range: `BrokerConfig::validate` rejects `start >= end`, so a single-port
    /// `(p, p)` range cannot be used here. `find_available_port` scans `start..=end` in
    /// order and takes the first free port, so the OS-assigned one is used unless it was
    /// taken in the interim, in which case `p + 1` is the fallback.
    fn free_port_range() -> (u16, u16) {
        let listener = std::net::TcpListener::bind("0.0.0.0:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        (port, port + 1)
    }

    /// A broker whose callback port is OS-assigned and whose polling intervals are long
    /// enough that no fallback task actually polls during a test.
    async fn test_broker() -> EventBroker {
        let mut config = BrokerConfig::no_firewall_detection();
        config.callback_port_range = free_port_range();
        config.base_polling_interval = Duration::from_secs(300);
        config.max_polling_interval = Duration::from_secs(600);
        EventBroker::new(config)
            .await
            .expect("broker should start on an OS-assigned callback port")
    }

    /// `was_duplicate` must distinguish a first registration from a repeat.
    ///
    /// It was computed as `registry.register(..)` followed by
    /// `registry.is_registered(..)` — asked *after* the insert, so it always answered
    /// `true`. Every registration was reported to callers as a duplicate, brand-new ones
    /// included. The pre-existing `test_registration_result` did not catch this: it
    /// constructs a `RegistrationResult` literal with `was_duplicate: false` and asserts
    /// the field reads back, never invoking the computation.
    ///
    /// Offline: the speaker is an RFC 5737 TEST-NET-3 address, so no real device is
    /// contacted.
    #[tokio::test]
    async fn test_was_duplicate_distinguishes_first_registration_from_repeat() {
        let broker = test_broker().await;
        let speaker_ip: IpAddr = "203.0.113.70".parse().unwrap();

        let first = broker
            .register_speaker_service(speaker_ip, Service::AVTransport)
            .await
            .expect("first registration should succeed");
        assert!(
            !first.was_duplicate,
            "a first-ever registration must not be reported as a duplicate"
        );

        let second = broker
            .register_speaker_service(speaker_ip, Service::AVTransport)
            .await
            .expect("duplicate registration should succeed");
        assert!(
            second.was_duplicate,
            "a repeat registration of the same speaker/service must be reported as a \
             duplicate"
        );
        assert_eq!(
            second.registration_id, first.registration_id,
            "the duplicate must reuse the existing registration ID"
        );

        // A different service on the same speaker is a distinct pair, so it is new.
        let other_service = broker
            .register_speaker_service(speaker_ip, Service::RenderingControl)
            .await
            .expect("registering a second service should succeed");
        assert!(
            !other_service.was_duplicate,
            "a different service on the same speaker is a new registration, not a \
             duplicate"
        );

        broker.shutdown().await.expect("shutdown should succeed");
    }

    /// A duplicate registration must short-circuit instead of re-running the subscribe
    /// path — which is what orphaned a SID in the EventRouter.
    ///
    /// `registry.register()` returns the *existing* ID for an already-registered pair,
    /// and the function did not stop there. It went on to `create_subscription`, issuing
    /// a second UPnP SUBSCRIBE that yields a **new** SID, while
    /// `subscriptions.insert(registration_id, wrapper)` overwrote the wrapper holding the
    /// old one. The superseded SID was then unnameable by any code path yet stayed in
    /// the router's active set for the process lifetime, and a later
    /// `unregister_speaker_service` released only the newest.
    ///
    /// The discriminator is `polling_reason`. Offline the SUBSCRIBE cannot succeed, so
    /// re-entering the subscribe path lands in the fallback branch and reports
    /// `Some(SubscriptionFailed)`; the short-circuit reports `None` because it performed
    /// no subscribe work at all. Registry and subscription counts pin that nothing was
    /// added either way.
    ///
    /// Honest scope: because SUBSCRIBE cannot succeed without a real speaker, this
    /// asserts *that the second subscribe never happens*, not the growth of the router's
    /// SID set — with no live SID there is no non-vacuous set assertion to make offline.
    #[tokio::test]
    async fn test_duplicate_registration_does_not_resubscribe() {
        let broker = test_broker().await;
        let speaker_ip: IpAddr = "203.0.113.71".parse().unwrap();

        let first = broker
            .register_speaker_service(speaker_ip, Service::AVTransport)
            .await
            .expect("first registration should succeed");
        // Precondition: the first call really did attempt a subscribe and fall back.
        assert_eq!(
            first.polling_reason,
            Some(PollingReason::SubscriptionFailed),
            "precondition: offline, the first registration attempts SUBSCRIBE and falls \
             back to polling"
        );

        let registrations_after_first = broker.registry.count().await;
        let subscriptions_after_first =
            broker.subscription_manager.list_subscriptions().await.len();

        let second = broker
            .register_speaker_service(speaker_ip, Service::AVTransport)
            .await
            .expect("duplicate registration should succeed");

        assert_eq!(
            second.polling_reason, None,
            "a duplicate must short-circuit before the subscribe path; a second \
             SUBSCRIBE is what produced a new SID and orphaned the previous one"
        );
        assert_eq!(
            broker.registry.count().await,
            registrations_after_first,
            "a duplicate must not add a registry entry"
        );
        assert_eq!(
            broker.subscription_manager.list_subscriptions().await.len(),
            subscriptions_after_first,
            "a duplicate must not create a second subscription, which is what \
             overwrote the wrapper and orphaned the previous SID"
        );

        broker.shutdown().await.expect("shutdown should succeed");
    }

    #[test]
    fn test_polling_reason_display() {
        assert_eq!(
            PollingReason::FirewallBlocked.to_string(),
            "firewall blocked"
        );
        assert_eq!(PollingReason::EventTimeout.to_string(), "event timeout");
        assert_eq!(
            PollingReason::SubscriptionFailed.to_string(),
            "subscription failed"
        );
        assert_eq!(PollingReason::NetworkIssues.to_string(), "network issues");
        assert_eq!(PollingReason::ForcedPolling.to_string(), "forced polling");
    }
}
