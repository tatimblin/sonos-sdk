//! Event activity detection and automatic polling with proactive firewall detection
//!
//! This module monitors event activity for subscriptions and provides automatic polling
//! fallback when events are not being received. It integrates with the firewall detection
//! system to immediately switch to polling when firewall blocking is detected.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, RwLock};

use callback_server::{FirewallDetectionCoordinator, FirewallStatus};
use tracing::debug;

use crate::broker::PollingReason;
use crate::registry::{RegistrationId, SpeakerServicePair};

/// A single monitored registration combining event time, pair, and polling state
struct MonitoredRegistration {
    last_event_time: Instant,
    pair: SpeakerServicePair,
    polling_activated: bool,
}

/// Monitors event activity and detects when polling fallback is needed
pub struct EventDetector {
    /// All monitored registrations in a single map
    registrations: Arc<RwLock<HashMap<RegistrationId, MonitoredRegistration>>>,

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
            registrations: Arc::new(RwLock::new(HashMap::new())),
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
        let mut registrations = self.registrations.write().await;
        if let Some(reg) = registrations.get_mut(&registration_id) {
            reg.last_event_time = Instant::now();
        }
    }

    /// Check if a registration should start polling based on event timeout
    pub async fn should_start_polling(&self, registration_id: RegistrationId) -> bool {
        let registrations = self.registrations.read().await;
        registrations
            .get(&registration_id)
            .map(|reg| reg.last_event_time.elapsed() > self.event_timeout)
            .unwrap_or(false)
    }

    /// Check if a registration should stop polling (events have resumed)
    pub async fn should_stop_polling(&self, registration_id: RegistrationId) -> bool {
        let registrations = self.registrations.read().await;
        registrations
            .get(&registration_id)
            .map(|reg| reg.last_event_time.elapsed() <= self.polling_activation_delay)
            .unwrap_or(false)
    }

    /// Evaluate firewall status and make immediate polling decision
    pub async fn evaluate_firewall_status(
        &self,
        registration_id: RegistrationId,
        pair: &SpeakerServicePair,
    ) -> Option<PollingRequest> {
        if let Some(firewall_coordinator) = &self.firewall_coordinator {
            let status = firewall_coordinator
                .get_device_status(pair.speaker_ip)
                .await;

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
        let registrations = Arc::clone(&self.registrations);
        let event_timeout = self.event_timeout;
        let polling_request_sender = self.polling_request_sender.clone();

        let check_interval = (event_timeout / 3).max(Duration::from_secs(1));

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(check_interval);

            loop {
                interval.tick().await;

                let now = Instant::now();

                // Snapshot registration IDs and check timeouts in a single lock
                let timed_out: Vec<(RegistrationId, SpeakerServicePair)> = {
                    let regs = registrations.read().await;
                    regs.iter()
                        .filter(|(_, reg)| {
                            !reg.polling_activated
                                && now.duration_since(reg.last_event_time) > event_timeout
                        })
                        .map(|(id, reg)| (*id, reg.pair.clone()))
                        .collect()
                };

                for (registration_id, pair) in timed_out {
                    if let Some(sender) = &polling_request_sender {
                        let request = PollingRequest {
                            registration_id,
                            speaker_service_pair: pair,
                            action: PollingAction::Start,
                            reason: PollingReason::EventTimeout,
                        };

                        if sender.send(request).is_ok() {
                            // Mark as activated to avoid duplicate requests
                            let mut regs = registrations.write().await;
                            if let Some(reg) = regs.get_mut(&registration_id) {
                                reg.polling_activated = true;
                            }

                            debug!(
                                registration_id = %registration_id,
                                "Event timeout detected, sent polling request"
                            );
                        }
                    }
                }
            }
        })
    }

    /// Register a new subscription for monitoring with its speaker/service pair
    pub async fn register_subscription(
        &self,
        registration_id: RegistrationId,
        pair: SpeakerServicePair,
    ) {
        let mut registrations = self.registrations.write().await;
        registrations.insert(
            registration_id,
            MonitoredRegistration {
                last_event_time: Instant::now(),
                pair,
                polling_activated: false,
            },
        );
    }

    /// Unregister a subscription from monitoring
    pub async fn unregister_subscription(&self, registration_id: RegistrationId) {
        let mut registrations = self.registrations.write().await;
        registrations.remove(&registration_id);
    }

    /// Get monitoring statistics
    pub async fn stats(&self) -> EventDetectorStats {
        let registrations = self.registrations.read().await;
        let total_monitored = registrations.len();

        let now = Instant::now();
        let mut timeout_count = 0;
        let mut recent_events_count = 0;

        for reg in registrations.values() {
            let elapsed = now.duration_since(reg.last_event_time);
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
        let detector = EventDetector::new(Duration::from_secs(30), Duration::from_secs(5));

        assert_eq!(detector.event_timeout, Duration::from_secs(30));
        assert_eq!(detector.polling_activation_delay, Duration::from_secs(5));
    }

    #[tokio::test]
    async fn test_event_recording() {
        let detector = EventDetector::new(Duration::from_secs(30), Duration::from_secs(5));

        let registration_id = RegistrationId::new(1);
        let pair = SpeakerServicePair::new(
            "192.168.1.100".parse().unwrap(),
            sonos_api::Service::AVTransport,
        );

        // Initially should not suggest polling (not registered)
        assert!(!detector.should_start_polling(registration_id).await);

        // Register and record an event
        detector.register_subscription(registration_id, pair).await;
        detector.record_event(registration_id).await;

        // Should still not suggest polling immediately after event
        assert!(!detector.should_start_polling(registration_id).await);
    }

    #[tokio::test]
    async fn test_subscription_registration() {
        let detector = EventDetector::new(Duration::from_secs(30), Duration::from_secs(5));

        let registration_id = RegistrationId::new(1);
        let pair = SpeakerServicePair::new(
            "192.168.1.100".parse().unwrap(),
            sonos_api::Service::AVTransport,
        );

        // Register subscription
        detector.register_subscription(registration_id, pair).await;

        let stats = detector.stats().await;
        assert_eq!(stats.total_monitored, 1);

        // Unregister subscription
        detector.unregister_subscription(registration_id).await;

        let stats = detector.stats().await;
        assert_eq!(stats.total_monitored, 0);
    }

    #[tokio::test]
    async fn test_register_and_unregister() {
        let detector = EventDetector::new(Duration::from_secs(30), Duration::from_secs(5));

        let registration_id = RegistrationId::new(1);
        let pair = SpeakerServicePair::new(
            "192.168.1.100".parse().unwrap(),
            sonos_api::Service::AVTransport,
        );

        // Register
        detector
            .register_subscription(registration_id, pair.clone())
            .await;

        // Verify it's stored
        let regs = detector.registrations.read().await;
        assert!(regs.contains_key(&registration_id));
        assert_eq!(regs[&registration_id].pair.speaker_ip, pair.speaker_ip);
        drop(regs);

        // Unregister is a single remove
        detector.unregister_subscription(registration_id).await;

        let regs = detector.registrations.read().await;
        assert!(!regs.contains_key(&registration_id));
    }

    #[tokio::test]
    async fn test_event_timeout_sends_polling_request() {
        use tokio::sync::mpsc;

        // Very short timeout so we can trigger it quickly
        let mut detector = EventDetector::new(Duration::from_millis(50), Duration::from_secs(5));

        let (sender, mut receiver) = mpsc::unbounded_channel();
        detector.set_polling_request_sender(sender);
        let detector = Arc::new(detector);

        let registration_id = RegistrationId::new(42);
        let pair = SpeakerServicePair::new(
            "192.168.1.100".parse().unwrap(),
            sonos_api::Service::RenderingControl,
        );

        // Register subscription with pair
        detector
            .register_subscription(registration_id, pair.clone())
            .await;

        // Backdate the last event time to simulate a timeout
        {
            let mut regs = detector.registrations.write().await;
            if let Some(reg) = regs.get_mut(&registration_id) {
                reg.last_event_time = Instant::now() - Duration::from_secs(60);
            }
        }

        // Start monitoring (spawns background task)
        detector.start_monitoring().await;

        // Wait for the monitoring loop to run (first tick is immediate)
        let request = tokio::time::timeout(Duration::from_secs(2), receiver.recv()).await;

        assert!(
            request.is_ok(),
            "Should receive a polling request within timeout"
        );
        let request = request.unwrap().expect("Channel should have a message");
        assert_eq!(request.registration_id, registration_id);
        assert_eq!(request.speaker_service_pair.speaker_ip, pair.speaker_ip);
        assert!(matches!(request.action, PollingAction::Start));
        assert_eq!(request.reason, PollingReason::EventTimeout);
    }
}
