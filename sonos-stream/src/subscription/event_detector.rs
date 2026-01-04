//! Event activity detection and automatic polling with proactive firewall detection
//!
//! This module monitors event activity for subscriptions and provides automatic polling
//! fallback when events are not being received. It integrates with the firewall detection
//! system to immediately switch to polling when firewall blocking is detected.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::{mpsc, RwLock};

use callback_server::{FirewallDetectionCoordinator, FirewallStatus};

use crate::broker::PollingReason;
use crate::registry::{RegistrationId, SpeakerServicePair};

/// Monitors event activity and detects when polling fallback is needed
pub struct EventDetector {
    /// Track last event time per registration for timeout detection
    last_event_times: Arc<RwLock<HashMap<RegistrationId, SystemTime>>>,

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
        last_event_times.insert(registration_id, SystemTime::now());
    }

    /// Check if a registration should start polling based on event timeout
    pub async fn should_start_polling(&self, registration_id: RegistrationId) -> bool {
        let last_event_times = self.last_event_times.read().await;

        if let Some(last_event_time) = last_event_times.get(&registration_id) {
            let elapsed = SystemTime::now()
                .duration_since(*last_event_time)
                .unwrap_or(Duration::ZERO);
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
            let elapsed = SystemTime::now()
                .duration_since(*last_event_time)
                .unwrap_or(Duration::ZERO);
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

    /// Start monitoring event activity for all registered subscriptions
    pub async fn start_monitoring(&self) {
        let last_event_times = Arc::clone(&self.last_event_times);
        let event_timeout = self.event_timeout;
        let polling_request_sender = self.polling_request_sender.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(10));

            loop {
                interval.tick().await;

                let now = SystemTime::now();
                let registrations: Vec<RegistrationId> = {
                    let times = last_event_times.read().await;
                    times.keys().cloned().collect()
                };

                for registration_id in registrations {
                    let should_poll = {
                        let times = last_event_times.read().await;
                        if let Some(last_event_time) = times.get(&registration_id) {
                            let elapsed = now.duration_since(*last_event_time).unwrap_or(Duration::ZERO);
                            elapsed > event_timeout
                        } else {
                            false
                        }
                    };

                    if should_poll {
                        if let Some(_sender) = &polling_request_sender {
                            // We need the speaker/service pair to make the request
                            // In a real implementation, this would be looked up from the registry
                            // For now, we'll emit a placeholder request
                            eprintln!("⏰ Event timeout detected for registration {}", registration_id);
                            // TODO: Look up speaker/service pair and send actual request
                        }
                    }
                }
            }
        });
    }

    /// Register a new subscription for monitoring
    pub async fn register_subscription(&self, registration_id: RegistrationId) {
        let mut last_event_times = self.last_event_times.write().await;
        last_event_times.insert(registration_id, SystemTime::now());
    }

    /// Unregister a subscription from monitoring
    pub async fn unregister_subscription(&self, registration_id: RegistrationId) {
        let mut last_event_times = self.last_event_times.write().await;
        last_event_times.remove(&registration_id);
    }

    /// Get monitoring statistics
    pub async fn stats(&self) -> EventDetectorStats {
        let last_event_times = self.last_event_times.read().await;
        let total_monitored = last_event_times.len();

        let now = SystemTime::now();
        let mut timeout_count = 0;
        let mut recent_events_count = 0;

        for last_event_time in last_event_times.values() {
            let elapsed = now.duration_since(*last_event_time).unwrap_or(Duration::ZERO);
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

}