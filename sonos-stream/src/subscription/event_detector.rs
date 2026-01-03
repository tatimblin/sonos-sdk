//! Event activity detection and automatic resync with proactive firewall detection
//!
//! This module monitors event activity for subscriptions and provides automatic resync
//! capabilities when state drift is detected. It integrates with the firewall detection
//! system to immediately switch to polling when firewall blocking is detected.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::{mpsc, RwLock};

use callback_server::firewall_detection::{FirewallDetectionPlugin, FirewallStatus};
use sonos_api::SonosClient;

use crate::error::{SubscriptionError, SubscriptionResult};
use crate::events::types::{EnrichedEvent, EventData, EventSource, ResyncReason};
use crate::registry::{RegistrationId, SpeakerServicePair};

/// Monitors event activity and detects when polling fallback is needed
pub struct EventDetector {
    /// Track last event time per registration for timeout detection
    last_event_times: Arc<RwLock<HashMap<RegistrationId, SystemTime>>>,

    /// Event timeout threshold - if no events received within this time, consider switching to polling
    event_timeout: Duration,

    /// Delay before activating polling after proactive firewall detection
    polling_activation_delay: Duration,

    /// Integration with firewall detection plugin
    firewall_detector: Option<Arc<FirewallDetectionPlugin>>,

    /// Sender for requesting polling activation
    polling_request_sender: Option<mpsc::UnboundedSender<PollingRequest>>,

    /// Sender for emitting resync events
    resync_event_sender: Option<mpsc::UnboundedSender<EnrichedEvent>>,
}

/// Request to activate or deactivate polling for a registration
#[derive(Debug, Clone)]
pub struct PollingRequest {
    pub registration_id: RegistrationId,
    pub speaker_service_pair: SpeakerServicePair,
    pub action: PollingAction,
    pub reason: ResyncReason,
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
            firewall_detector: None,
            polling_request_sender: None,
            resync_event_sender: None,
        }
    }

    /// Set the firewall detector (must be called during initialization)
    pub fn set_firewall_detector(&mut self, detector: Arc<FirewallDetectionPlugin>) {
        self.firewall_detector = Some(detector);
    }

    /// Set the polling request sender
    pub fn set_polling_request_sender(&mut self, sender: mpsc::UnboundedSender<PollingRequest>) {
        self.polling_request_sender = Some(sender);
    }

    /// Set the resync event sender
    pub fn set_resync_event_sender(&mut self, sender: mpsc::UnboundedSender<EnrichedEvent>) {
        self.resync_event_sender = Some(sender);
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
        if let Some(firewall_detector) = &self.firewall_detector {
            let status = firewall_detector.get_status().await;

            match status {
                FirewallStatus::Blocked => {
                    // Immediate polling activation - no delay needed
                    Some(PollingRequest {
                        registration_id,
                        speaker_service_pair: pair.clone(),
                        action: PollingAction::Start,
                        reason: ResyncReason::FirewallBlocked,
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
                        reason: ResyncReason::NetworkIssues,
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
                        if let Some(sender) = &polling_request_sender {
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

        let firewall_status = if let Some(detector) = &self.firewall_detector {
            detector.get_status().await
        } else {
            FirewallStatus::Unknown
        };

        EventDetectorStats {
            total_monitored,
            timeout_count,
            recent_events_count,
            firewall_status,
            event_timeout: self.event_timeout,
        }
    }
}

/// Handles state drift detection and resync event generation
pub struct ResyncDetector {
    /// Track expected state per registration for drift detection
    expected_state: Arc<RwLock<HashMap<RegistrationId, String>>>,

    /// Track when resync events were last emitted to prevent spam
    last_resync_times: Arc<RwLock<HashMap<RegistrationId, SystemTime>>>,

    /// Minimum time between resync events to prevent spam
    resync_cooldown: Duration,

    /// SonosClient for querying current device state
    sonos_client: SonosClient,
}

impl ResyncDetector {
    /// Create a new ResyncDetector
    pub fn new(resync_cooldown: Duration) -> Self {
        Self {
            expected_state: Arc::new(RwLock::new(HashMap::new())),
            last_resync_times: Arc::new(RwLock::new(HashMap::new())),
            resync_cooldown,
            sonos_client: SonosClient::new(),
        }
    }

    /// Check if a resync is needed for a registration
    pub async fn check_resync_needed(
        &self,
        registration_id: RegistrationId,
        pair: &SpeakerServicePair,
    ) -> Option<EnrichedEvent> {
        // Check cooldown period
        if !self.should_resync(registration_id).await {
            return None;
        }

        // Query current device state
        let current_state = match self.query_current_state(pair).await {
            Ok(state) => state,
            Err(_) => return None, // Skip resync if we can't query state
        };

        // Compare with expected state
        let expected_state = self.get_expected_state(registration_id).await;

        if let Some(expected) = expected_state {
            if current_state != expected {
                // State drift detected - create resync event
                return Some(self.create_resync_event(
                    registration_id,
                    pair,
                    current_state,
                    ResyncReason::PollingDiscrepancy,
                ));
            }
        } else {
            // No expected state yet - this might be initial state
            self.set_expected_state(registration_id, current_state.clone()).await;
            return Some(self.create_resync_event(
                registration_id,
                pair,
                current_state,
                ResyncReason::InitialState,
            ));
        }

        None
    }

    /// Check if enough time has passed since last resync
    async fn should_resync(&self, registration_id: RegistrationId) -> bool {
        let last_resync_times = self.last_resync_times.read().await;

        if let Some(last_resync) = last_resync_times.get(&registration_id) {
            let elapsed = SystemTime::now()
                .duration_since(*last_resync)
                .unwrap_or(Duration::ZERO);
            elapsed >= self.resync_cooldown
        } else {
            true // No previous resync
        }
    }

    /// Query current device state
    async fn query_current_state(&self, pair: &SpeakerServicePair) -> SubscriptionResult<String> {
        // This is a placeholder implementation
        // In the real implementation, we would use sonos-api operations to query state
        match pair.service {
            sonos_api::Service::AVTransport => {
                // Use GetTransportInfo operation
                Ok(format!("transport_state_placeholder_{}", pair.speaker_ip))
            }
            sonos_api::Service::RenderingControl => {
                // Use GetVolume and GetMute operations
                Ok(format!("volume_state_placeholder_{}", pair.speaker_ip))
            }
            _ => Err(SubscriptionError::ServiceError(
                "Unsupported service for state query".to_string(),
            )),
        }
    }

    /// Get expected state for a registration
    async fn get_expected_state(&self, registration_id: RegistrationId) -> Option<String> {
        let expected_state = self.expected_state.read().await;
        expected_state.get(&registration_id).cloned()
    }

    /// Set expected state for a registration
    async fn set_expected_state(&self, registration_id: RegistrationId, state: String) {
        let mut expected_state = self.expected_state.write().await;
        expected_state.insert(registration_id, state);
    }

    /// Create a resync event
    fn create_resync_event(
        &self,
        registration_id: RegistrationId,
        pair: &SpeakerServicePair,
        current_state: String,
        reason: ResyncReason,
    ) -> EnrichedEvent {
        // This is a placeholder - in real implementation we'd create proper EventData
        let event_data = match pair.service {
            sonos_api::Service::AVTransport => {
                EventData::AVTransportResync(crate::events::types::AVTransportFullState {
                    transport_state: current_state,
                    current_track_uri: None,
                    track_duration: None,
                    rel_time: None,
                    play_mode: None,
                    track_metadata: None,
                    queue_length: None,
                    track_number: None,
                })
            }
            sonos_api::Service::RenderingControl => {
                EventData::RenderingControlResync(crate::events::types::RenderingControlFullState {
                    volume: current_state,
                    mute: None,
                    bass: None,
                    treble: None,
                    loudness: None,
                    balance: None,
                })
            }
            _ => EventData::AVTransportResync(crate::events::types::AVTransportFullState {
                transport_state: "unknown".to_string(),
                current_track_uri: None,
                track_duration: None,
                rel_time: None,
                play_mode: None,
                track_metadata: None,
                queue_length: None,
                track_number: None,
            }),
        };

        EnrichedEvent {
            registration_id,
            speaker_ip: pair.speaker_ip,
            service: pair.service,
            event_source: EventSource::ResyncDetection { reason },
            timestamp: SystemTime::now(),
            event_data,
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
    async fn test_resync_detector_creation() {
        let detector = ResyncDetector::new(Duration::from_secs(30));
        assert_eq!(detector.resync_cooldown, Duration::from_secs(30));
    }

    #[tokio::test]
    async fn test_resync_cooldown() {
        let detector = ResyncDetector::new(Duration::from_secs(30));
        let registration_id = RegistrationId::new(1);

        // Should allow resync initially
        assert!(detector.should_resync(registration_id).await);

        // Simulate resync happened
        {
            let mut last_resync_times = detector.last_resync_times.write().await;
            last_resync_times.insert(registration_id, SystemTime::now());
        }

        // Should not allow resync immediately
        assert!(!detector.should_resync(registration_id).await);
    }
}