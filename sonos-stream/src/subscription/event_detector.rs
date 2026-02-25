//! Event activity detection and automatic polling with proactive firewall detection
//!
//! This module monitors event activity for subscriptions and provides automatic polling
//! fallback when events are not being received. It integrates with the firewall detection
//! system to immediately switch to polling when firewall blocking is detected.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, RwLock};

use tracing::{debug, warn};
use callback_server::{FirewallDetectionCoordinator, FirewallStatus};

use crate::broker::PollingReason;
use crate::registry::{RegistrationId, SpeakerServicePair};

/// Monitors event activity and detects when polling fallback is needed
pub struct EventDetector {
    /// Track last event time per registration for timeout detection
    last_event_times: Arc<RwLock<HashMap<RegistrationId, Instant>>>,

    /// Map registration IDs to speaker/service pairs for timeout-based polling requests
    registration_pairs: Arc<RwLock<HashMap<RegistrationId, SpeakerServicePair>>>,

    /// Track which registrations already have polling activated (avoid duplicate requests)
    polling_activated: Arc<RwLock<HashMap<RegistrationId, bool>>>,

    /// Event timeout threshold - if no events received within this time, consider switching to polling
    event_timeout: Duration,

    /// Delay before activating polling after proactive firewall detection
    polling_activation_delay: Duration,

    /// Integration with firewall detection coordinator
    firewall_coordinator: Option<Arc<FirewallDetectionCoordinator>>,

    /// Sender for requesting polling activation
    polling_request_sender: Option<mpsc::UnboundedSender<PollingRequest>>,
}

/// Request to activate or deactivate polling for a registration
#[derive(Debug, Clone)]
pub struct PollingRequest {
    pub registration_id: RegistrationId,
    pub speaker_service_pair: SpeakerServicePair,
    pub action: PollingAction,
    pub reason: PollingReason,
}

#[derive(Debug, Clone)]
pub enum PollingAction {
    Start,
    Stop,
}

impl EventDetector {
    /// Create a new EventDetector
    pub fn new(event_timeout: Duration, polling_activation_delay: Duration) -> Self {
        Self {
            last_event_times: Arc::new(RwLock::new(HashMap::new())),
            registration_pairs: Arc::new(RwLock::new(HashMap::new())),
            polling_activated: Arc::new(RwLock::new(HashMap::new())),
            event_timeout,
            polling_activation_delay,
            firewall_coordinator: None,
            polling_request_sender: None,
        }
    }

    /// Set the firewall coordinator (must be called during initialization)
    pub fn set_firewall_coordinator(&mut self, coordinator: Arc<FirewallDetectionCoordinator>) {
        self.firewall_coordinator = Some(coordinator);
    }

    /// Set the polling request sender
    pub fn set_polling_request_sender(&mut self, sender: mpsc::UnboundedSender<PollingRequest>) {
        self.polling_request_sender = Some(sender);
    }


    /// Record that an event was received for a registration
    pub async fn record_event(&self, registration_id: RegistrationId) {
        let mut last_event_times = self.last_event_times.write().await;
        last_event_times.insert(registration_id, Instant::now());
    }

    /// Check if a registration should start polling based on event timeout
    pub async fn should_start_polling(&self, registration_id: RegistrationId) -> bool {
        let last_event_times = self.last_event_times.read().await;

        if let Some(last_event_time) = last_event_times.get(&registration_id) {
            let elapsed = last_event_time.elapsed();
            elapsed > self.event_timeout
        } else {
            // No events recorded yet - give some time for initial events
            false
        }
    }

    /// Check if a registration should stop polling (events have resumed)
    pub async fn should_stop_polling(&self, registration_id: RegistrationId) -> bool {
        let last_event_times = self.last_event_times.read().await;

        if let Some(last_event_time) = last_event_times.get(&registration_id) {
            let elapsed = last_event_time.elapsed();
            elapsed <= self.polling_activation_delay
        } else {
            false
        }
    }

    /// Evaluate firewall status and make immediate polling decision
    pub async fn evaluate_firewall_status(
        &self,
        registration_id: RegistrationId,
        pair: &SpeakerServicePair,
    ) -> Option<PollingRequest> {
        if let Some(firewall_coordinator) = &self.firewall_coordinator {
            let status = firewall_coordinator.get_device_status(pair.speaker_ip).await;

            match status {
                FirewallStatus::Blocked => {
                    // Immediate polling activation - no delay needed
                    Some(PollingRequest {
                        registration_id,
                        speaker_service_pair: pair.clone(),
                        action: PollingAction::Start,
                        reason: PollingReason::FirewallBlocked,
                    })
                }
                FirewallStatus::Accessible => {
                    // Standard event monitoring
                    None
                }
                FirewallStatus::Unknown => {
                    // Use shorter timeout for unknown firewall status
                    None
                }
                FirewallStatus::Error => {
                    // Treat errors conservatively - start polling after delay
                    Some(PollingRequest {
                        registration_id,
                        speaker_service_pair: pair.clone(),
                        action: PollingAction::Start,
                        reason: PollingReason::NetworkIssues,
                    })
                }
            }
        } else {
            // No firewall detection available
            None
        }
    }

    /// Start monitoring event activity for all registered subscriptions.
    /// Returns the JoinHandle for the spawned monitoring task.
    pub async fn start_monitoring(&self) -> tokio::task::JoinHandle<()> {
        let last_event_times = Arc::clone(&self.last_event_times);
        let registration_pairs = Arc::clone(&self.registration_pairs);
        let polling_activated = Arc::clone(&self.polling_activated);
        let event_timeout = self.event_timeout;
        let polling_request_sender = self.polling_request_sender.clone();

        let check_interval = (event_timeout / 3).max(Duration::from_secs(1));

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(check_interval);

            loop {
                interval.tick().await;

                let now = Instant::now();
                let registrations: Vec<RegistrationId> = {
                    let times = last_event_times.read().await;
                    times.keys().cloned().collect()
                };

                for registration_id in registrations {
                    // Check if already activated — avoid duplicate requests
                    {
                        let activated = polling_activated.read().await;
                        if activated.get(&registration_id) == Some(&true) {
                            continue;
                        }
                    }

                    let should_poll = {
                        let times = last_event_times.read().await;
                        if let Some(last_event_time) = times.get(&registration_id) {
                            let elapsed = now.duration_since(*last_event_time);
                            elapsed > event_timeout
                        } else {
                            false
                        }
                    };

                    if should_poll {
                        if let Some(sender) = &polling_request_sender {
                            // Look up the speaker/service pair for this registration
                            let pair = {
                                let pairs = registration_pairs.read().await;
                                pairs.get(&registration_id).cloned()
                            };

                            if let Some(pair) = pair {
                                let request = PollingRequest {
                                    registration_id,
                                    speaker_service_pair: pair,
                                    action: PollingAction::Start,
                                    reason: PollingReason::EventTimeout,
                                };

                                if sender.send(request).is_ok() {
                                    // Mark as activated to avoid duplicate requests
                                    let mut activated = polling_activated.write().await;
                                    activated.insert(registration_id, true);

                                    debug!(
                                        registration_id = %registration_id,
                                        "Event timeout detected, sent polling request"
                                    );
                                }
                            } else {
                                warn!(
                                    registration_id = %registration_id,
                                    "Event timeout detected but no speaker/service pair registered"
                                );
                            }
                        }
                    }
                }
            }
        })
    }

    /// Register a new subscription for monitoring
    pub async fn register_subscription(&self, registration_id: RegistrationId) {
        let mut last_event_times = self.last_event_times.write().await;
        last_event_times.insert(registration_id, Instant::now());
    }

    /// Register the speaker/service pair for a registration (needed for timeout-based polling)
    pub async fn register_pair(&self, registration_id: RegistrationId, pair: SpeakerServicePair) {
        let mut pairs = self.registration_pairs.write().await;
        pairs.insert(registration_id, pair);
    }

    /// Unregister a subscription from monitoring
    pub async fn unregister_subscription(&self, registration_id: RegistrationId) {
        let mut last_event_times = self.last_event_times.write().await;
        last_event_times.remove(&registration_id);
        drop(last_event_times);

        let mut pairs = self.registration_pairs.write().await;
        pairs.remove(&registration_id);
        drop(pairs);

        let mut activated = self.polling_activated.write().await;
        activated.remove(&registration_id);
    }

    /// Get monitoring statistics
    pub async fn stats(&self) -> EventDetectorStats {
        let last_event_times = self.last_event_times.read().await;
        let total_monitored = last_event_times.len();

        let now = Instant::now();
        let mut timeout_count = 0;
        let mut recent_events_count = 0;

        for last_event_time in last_event_times.values() {
            let elapsed = now.duration_since(*last_event_time);
            if elapsed > self.event_timeout {
                timeout_count += 1;
            } else if elapsed <= Duration::from_secs(60) {
                recent_events_count += 1;
            }
        }

        // Firewall status is now per-device, so we return Unknown for global stats
        let firewall_status = FirewallStatus::Unknown;

        EventDetectorStats {
            total_monitored,
            timeout_count,
            recent_events_count,
            firewall_status,
            event_timeout: self.event_timeout,
        }
    }
}


/// Statistics about event detection
#[derive(Debug)]
pub struct EventDetectorStats {
    pub total_monitored: usize,
    pub timeout_count: usize,
    pub recent_events_count: usize,
    pub firewall_status: FirewallStatus,
    pub event_timeout: Duration,
}

impl std::fmt::Display for EventDetectorStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Event Detector Stats:")?;
        writeln!(f, "  Total monitored: {}", self.total_monitored)?;
        writeln!(f, "  Timeout count: {}", self.timeout_count)?;
        writeln!(f, "  Recent events: {}", self.recent_events_count)?;
        writeln!(f, "  Firewall status: {:?}", self.firewall_status)?;
        writeln!(f, "  Event timeout: {:?}", self.event_timeout)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_event_detector_creation() {
        let detector = EventDetector::new(
            Duration::from_secs(30),
            Duration::from_secs(5),
        );

        assert_eq!(detector.event_timeout, Duration::from_secs(30));
        assert_eq!(detector.polling_activation_delay, Duration::from_secs(5));
    }

    #[tokio::test]
    async fn test_event_recording() {
        let detector = EventDetector::new(
            Duration::from_secs(30),
            Duration::from_secs(5),
        );

        let registration_id = RegistrationId::new(1);

        // Initially should not suggest polling
        assert!(!detector.should_start_polling(registration_id).await);

        // Record an event
        detector.record_event(registration_id).await;

        // Should still not suggest polling immediately after event
        assert!(!detector.should_start_polling(registration_id).await);
    }

    #[tokio::test]
    async fn test_subscription_registration() {
        let detector = EventDetector::new(
            Duration::from_secs(30),
            Duration::from_secs(5),
        );

        let registration_id = RegistrationId::new(1);

        // Register subscription
        detector.register_subscription(registration_id).await;

        let stats = detector.stats().await;
        assert_eq!(stats.total_monitored, 1);

        // Unregister subscription
        detector.unregister_subscription(registration_id).await;

        let stats = detector.stats().await;
        assert_eq!(stats.total_monitored, 0);
    }

    #[tokio::test]
    async fn test_register_pair_and_unregister() {
        let detector = EventDetector::new(
            Duration::from_secs(30),
            Duration::from_secs(5),
        );

        let registration_id = RegistrationId::new(1);
        let pair = SpeakerServicePair::new(
            "192.168.1.100".parse().unwrap(),
            sonos_api::Service::AVTransport,
        );

        // Register pair
        detector.register_pair(registration_id, pair.clone()).await;

        // Verify it's stored
        let pairs = detector.registration_pairs.read().await;
        assert!(pairs.contains_key(&registration_id));
        assert_eq!(pairs[&registration_id].speaker_ip, pair.speaker_ip);
        drop(pairs);

        // Unregister cleans up pair too
        detector.register_subscription(registration_id).await;
        detector.unregister_subscription(registration_id).await;

        let pairs = detector.registration_pairs.read().await;
        assert!(!pairs.contains_key(&registration_id));
    }

    #[tokio::test]
    async fn test_event_timeout_sends_polling_request() {
        use tokio::sync::mpsc;

        // Very short timeout so we can trigger it quickly
        let mut detector = EventDetector::new(
            Duration::from_millis(50),
            Duration::from_secs(5),
        );

        let (sender, mut receiver) = mpsc::unbounded_channel();
        detector.set_polling_request_sender(sender);
        let detector = Arc::new(detector);

        let registration_id = RegistrationId::new(42);
        let pair = SpeakerServicePair::new(
            "192.168.1.100".parse().unwrap(),
            sonos_api::Service::RenderingControl,
        );

        // Register subscription and pair
        detector.register_subscription(registration_id).await;
        detector.register_pair(registration_id, pair.clone()).await;

        // Backdate the last event time to simulate a timeout
        {
            let mut times = detector.last_event_times.write().await;
            times.insert(registration_id, Instant::now() - Duration::from_secs(60));
        }

        // Start monitoring (spawns background task)
        detector.start_monitoring().await;

        // Wait for the monitoring loop to run (first tick is immediate)
        let request = tokio::time::timeout(Duration::from_secs(2), receiver.recv()).await;

        assert!(request.is_ok(), "Should receive a polling request within timeout");
        let request = request.unwrap().expect("Channel should have a message");
        assert_eq!(request.registration_id, registration_id);
        assert_eq!(request.speaker_service_pair.speaker_ip, pair.speaker_ip);
        assert!(matches!(request.action, PollingAction::Start));
        assert_eq!(request.reason, PollingReason::EventTimeout);
    }
}