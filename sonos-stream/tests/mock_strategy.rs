//! Mock strategy and subscription implementations for testing.
//!
//! This module provides mock implementations of the `BaseStrategy` and
//! `Subscription` traits that can be used in tests without making real UPnP calls.
//! The mock implementations support configurable failure modes to test error paths.

use sonos_stream::{
    BaseStrategy, EventData, ServiceType, SpeakerId, Speaker, SubscriptionConfig, SubscriptionScope,
    StrategyError, Subscription, SubscriptionError, TypedEvent,
};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
use async_trait::async_trait;

/// Shared mock event data for testing
#[derive(Debug, Clone)]
pub struct MockEventData {
    pub event_type: String,
    pub service_type: ServiceType,
    pub data: HashMap<String, String>,
}

impl EventData for MockEventData {
    fn event_type(&self) -> &str {
        &self.event_type
    }

    fn service_type(&self) -> ServiceType {
        self.service_type
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn clone_box(&self) -> Box<dyn EventData> {
        Box::new(self.clone())
    }
}

/// Mock strategy for testing that doesn't make real UPnP calls.
///
/// This strategy can be configured to simulate various failure scenarios
/// for testing error handling paths.
#[derive(Clone)]
pub struct MockStrategy {
    service_type: ServiceType,
    subscription_scope: SubscriptionScope,
    should_fail_creation: Arc<AtomicBool>,
    should_fail_parsing: Arc<AtomicBool>,
    creation_count: Arc<AtomicU32>,
    parse_count: Arc<AtomicU32>,
}

impl MockStrategy {
    /// Create a new mock strategy for the given service type.
    pub fn new(service_type: ServiceType) -> Self {
        Self {
            service_type,
            subscription_scope: SubscriptionScope::PerSpeaker,
            should_fail_creation: Arc::new(AtomicBool::new(false)),
            should_fail_parsing: Arc::new(AtomicBool::new(false)),
            creation_count: Arc::new(AtomicU32::new(0)),
            parse_count: Arc::new(AtomicU32::new(0)),
        }
    }

    /// Create a new mock strategy with network-wide scope.
    pub fn new_network_wide(service_type: ServiceType) -> Self {
        Self {
            service_type,
            subscription_scope: SubscriptionScope::NetworkWide,
            should_fail_creation: Arc::new(AtomicBool::new(false)),
            should_fail_parsing: Arc::new(AtomicBool::new(false)),
            creation_count: Arc::new(AtomicU32::new(0)),
            parse_count: Arc::new(AtomicU32::new(0)),
        }
    }

    /// Configure the strategy to fail subscription creation.
    pub fn set_fail_creation(&self, should_fail: bool) {
        self.should_fail_creation.store(should_fail, Ordering::Relaxed);
    }

    /// Configure the strategy to fail event parsing.
    pub fn set_fail_parsing(&self, should_fail: bool) {
        self.should_fail_parsing.store(should_fail, Ordering::Relaxed);
    }

    /// Get the number of times create_subscription was called.
    pub fn creation_count(&self) -> u32 {
        self.creation_count.load(Ordering::Relaxed)
    }

    /// Get the number of times parse_event was called.
    pub fn parse_count(&self) -> u32 {
        self.parse_count.load(Ordering::Relaxed)
    }

    /// Reset all counters.
    pub fn reset_counters(&self) {
        self.creation_count.store(0, Ordering::Relaxed);
        self.parse_count.store(0, Ordering::Relaxed);
    }
}

#[async_trait]
impl BaseStrategy for MockStrategy {
    fn service_type(&self) -> ServiceType {
        self.service_type
    }

    fn subscription_scope(&self) -> SubscriptionScope {
        self.subscription_scope
    }

    fn service_endpoint_path(&self) -> &'static str {
        "/MockService/Event"
    }

    fn parse_event(
        &self,
        speaker_id: &SpeakerId,
        event_xml: &str,
    ) -> Result<TypedEvent, StrategyError> {
        self.parse_count.fetch_add(1, Ordering::Relaxed);

        if self.should_fail_parsing.load(Ordering::Relaxed) {
            return Err(StrategyError::EventParseFailed(
                "Mock failure: event parsing disabled".to_string(),
            ));
        }

        // Simple mock parsing: extract event type from XML or use default
        let event_type = if event_xml.contains("test_event") {
            "test_event"
        } else if event_xml.contains("volume_changed") {
            "volume_changed"
        } else if event_xml.contains("transport_state") {
            "transport_state_changed"
        } else {
            "mock_event"
        };

        let mut data = HashMap::new();
        data.insert("speaker_id".to_string(), speaker_id.as_str().to_string());
        data.insert("xml_length".to_string(), event_xml.len().to_string());
        data.insert("service_type".to_string(), format!("{:?}", self.service_type));

        // Extract simple key-value pairs from XML if present
        if let Some(start) = event_xml.find("<data>") {
            if let Some(end) = event_xml.find("</data>") {
                let data_section = &event_xml[start + 6..end];
                for line in data_section.lines() {
                    if let Some((key, value)) = line.split_once('=') {
                        data.insert(key.trim().to_string(), value.trim().to_string());
                    }
                }
            }
        }

        // Add volume and channel for volume_changed events
        if event_type == "volume_changed" {
            data.insert("volume".to_string(), "50".to_string());
            data.insert("channel".to_string(), "Master".to_string());
        }

        let mock_data = MockEventData {
            event_type: event_type.to_string(),
            service_type: self.service_type,
            data,
        };

        Ok(TypedEvent::new(Box::new(mock_data)))
    }

    // Override create_subscription to add mock-specific behavior
    async fn create_subscription(
        &self,
        speaker: &Speaker,
        callback_url: String,
        config: &SubscriptionConfig,
    ) -> Result<Box<dyn Subscription>, StrategyError> {
        self.creation_count.fetch_add(1, Ordering::Relaxed);

        if self.should_fail_creation.load(Ordering::Relaxed) {
            return Err(StrategyError::SubscriptionCreationFailed(
                "Mock failure: subscription creation disabled".to_string(),
            ));
        }

        // Generate a mock subscription ID
        let subscription_id = format!("uuid:mock-sub-{}-{}", speaker.id.as_str(), self.service_type as u32);

        Ok(Box::new(MockSubscription::new(
            subscription_id,
            speaker.id.clone(),
            self.service_type,
            Duration::from_secs(config.timeout_seconds as u64),
            callback_url,
        )))
    }
}

/// Mock subscription that tracks state without making real UPnP calls.
pub struct MockSubscription {
    subscription_id: String,
    speaker_id: SpeakerId,
    service_type: ServiceType,
    callback_url: String,
    state: Arc<Mutex<SubscriptionState>>,
}

#[derive(Debug)]
struct SubscriptionState {
    active: bool,
    expires_at: SystemTime,
    renewal_threshold: Duration,
    should_fail_renewal: bool,
    should_fail_unsubscribe: bool,
    renewal_count: u32,
    unsubscribe_count: u32,
}

impl MockSubscription {
    /// Create a new mock subscription.
    pub fn new(
        subscription_id: String,
        speaker_id: SpeakerId,
        service_type: ServiceType,
        timeout: Duration,
        callback_url: String,
    ) -> Self {
        Self {
            subscription_id,
            speaker_id,
            service_type,
            callback_url,
            state: Arc::new(Mutex::new(SubscriptionState {
                active: true,
                expires_at: SystemTime::now() + timeout,
                renewal_threshold: Duration::from_secs(300), // 5 minutes
                should_fail_renewal: false,
                should_fail_unsubscribe: false,
                renewal_count: 0,
                unsubscribe_count: 0,
            })),
        }
    }

    /// Configure the subscription to fail renewal attempts.
    pub fn set_fail_renewal(&self, should_fail: bool) {
        if let Ok(mut state) = self.state.lock() {
            state.should_fail_renewal = should_fail;
        }
    }

    /// Configure the subscription to fail unsubscribe attempts.
    pub fn set_fail_unsubscribe(&self, should_fail: bool) {
        if let Ok(mut state) = self.state.lock() {
            state.should_fail_unsubscribe = should_fail;
        }
    }

    /// Get the number of times renew was called.
    pub fn renewal_count(&self) -> u32 {
        self.state.lock().map(|s| s.renewal_count).unwrap_or(0)
    }

    /// Get the number of times unsubscribe was called.
    pub fn unsubscribe_count(&self) -> u32 {
        self.state.lock().map(|s| s.unsubscribe_count).unwrap_or(0)
    }

    /// Get the callback URL for this subscription.
    pub fn callback_url(&self) -> &str {
        &self.callback_url
    }

    /// Set the expiration time for testing.
    #[allow(dead_code)]
    pub fn set_expires_at(&self, expires_at: SystemTime) {
        if let Ok(mut state) = self.state.lock() {
            state.expires_at = expires_at;
        }
    }

    /// Set the renewal threshold for testing.
    #[allow(dead_code)]
    pub fn set_renewal_threshold(&self, threshold: Duration) {
        if let Ok(mut state) = self.state.lock() {
            state.renewal_threshold = threshold;
        }
    }
}

#[async_trait]
impl Subscription for MockSubscription {
    fn subscription_id(&self) -> &str {
        &self.subscription_id
    }

    async fn renew(&mut self) -> Result<(), SubscriptionError> {
        let mut state = self.state.lock().map_err(|_| {
            SubscriptionError::RenewalFailed("Failed to acquire lock".to_string())
        })?;

        state.renewal_count += 1;

        if !state.active {
            return Err(SubscriptionError::Expired);
        }

        if state.should_fail_renewal {
            return Err(SubscriptionError::RenewalFailed(
                "Mock failure: renewal disabled".to_string(),
            ));
        }

        // Simulate successful renewal by extending expiration
        state.expires_at = SystemTime::now() + Duration::from_secs(1800);
        Ok(())
    }

    async fn unsubscribe(&mut self) -> Result<(), SubscriptionError> {
        let mut state = self.state.lock().map_err(|_| {
            SubscriptionError::UnsubscribeFailed("Failed to acquire lock".to_string())
        })?;

        state.unsubscribe_count += 1;

        if !state.active {
            return Err(SubscriptionError::UnsubscribeFailed(
                "Already unsubscribed".to_string(),
            ));
        }

        if state.should_fail_unsubscribe {
            return Err(SubscriptionError::UnsubscribeFailed(
                "Mock failure: unsubscribe disabled".to_string(),
            ));
        }

        state.active = false;
        Ok(())
    }

    fn is_active(&self) -> bool {
        if let Ok(state) = self.state.lock() {
            state.active && SystemTime::now() < state.expires_at
        } else {
            false
        }
    }

    fn time_until_renewal(&self) -> Option<Duration> {
        let state = self.state.lock().ok()?;

        if !state.active {
            return None;
        }

        let now = SystemTime::now();
        if now >= state.expires_at {
            return Some(Duration::ZERO);
        }

        let time_until_expiry = state.expires_at.duration_since(now).ok()?;
        if time_until_expiry <= state.renewal_threshold {
            Some(time_until_expiry)
        } else {
            None
        }
    }

    fn speaker_id(&self) -> &SpeakerId {
        &self.speaker_id
    }

    fn service_type(&self) -> ServiceType {
        self.service_type
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::IpAddr;

    fn create_test_speaker() -> Speaker {
        Speaker::new(
            SpeakerId::new("RINCON_TEST123"),
            "192.168.1.100".parse::<IpAddr>().unwrap(),
            "Test Speaker".to_string(),
            "Living Room".to_string(),
        )
    }

    #[test]
    fn test_mock_strategy_creation() {
        let strategy = MockStrategy::new(ServiceType::AVTransport);
        assert_eq!(strategy.service_type(), ServiceType::AVTransport);
        assert_eq!(strategy.subscription_scope(), SubscriptionScope::PerSpeaker);
    }

    #[test]
    fn test_mock_strategy_network_wide() {
        let strategy = MockStrategy::new_network_wide(ServiceType::ZoneGroupTopology);
        assert_eq!(strategy.service_type(), ServiceType::ZoneGroupTopology);
        assert_eq!(strategy.subscription_scope(), SubscriptionScope::NetworkWide);
    }

    #[tokio::test]
    async fn test_mock_strategy_create_subscription_success() {
        let strategy = MockStrategy::new(ServiceType::AVTransport);
        let speaker = create_test_speaker();
        let config = SubscriptionConfig::new(1800, "http://192.168.1.50:3400/notify".to_string());

        let result = strategy.create_subscription(
            &speaker,
            "http://192.168.1.50:3400".to_string(),
            &config,
        ).await;

        assert!(result.is_ok());
        assert_eq!(strategy.creation_count(), 1);

        let subscription = result.unwrap();
        assert!(subscription.subscription_id().contains("mock-sub"));
        assert_eq!(subscription.speaker_id().as_str(), "RINCON_TEST123");
        assert_eq!(subscription.service_type(), ServiceType::AVTransport);
    }

    #[tokio::test]
    async fn test_mock_strategy_create_subscription_failure() {
        let strategy = MockStrategy::new(ServiceType::RenderingControl);
        strategy.set_fail_creation(true);

        let speaker = create_test_speaker();
        let config = SubscriptionConfig::new(1800, "http://192.168.1.50:3400/notify".to_string());

        let result = strategy.create_subscription(
            &speaker,
            "http://192.168.1.50:3400".to_string(),
            &config,
        ).await;

        assert!(result.is_err());
        assert_eq!(strategy.creation_count(), 1);

        if let Err(e) = result {
            match e {
                StrategyError::SubscriptionCreationFailed(msg) => {
                    assert!(msg.contains("Mock failure"));
                }
                _ => panic!("Expected SubscriptionCreationFailed error"),
            }
        }
    }

    #[test]
    fn test_mock_strategy_parse_event_success() {
        let strategy = MockStrategy::new(ServiceType::AVTransport);
        let speaker_id = SpeakerId::new("RINCON_TEST123");
        let event_xml = "<event><test_event>data</test_event></event>";

        let result = strategy.parse_event(&speaker_id, event_xml);

        assert!(result.is_ok());
        assert_eq!(strategy.parse_count(), 1);

        let typed_event = result.unwrap();
        assert_eq!(typed_event.event_type(), "test_event");
        
        // Downcast to MockEventData to access the data
        if let Some(mock_data) = typed_event.downcast_ref::<MockEventData>() {
            assert_eq!(
                mock_data.data.get("speaker_id").map(|s| s.as_str()),
                Some("RINCON_TEST123")
            );
        } else {
            panic!("Failed to downcast to MockEventData");
        }
    }

    #[test]
    fn test_mock_strategy_parse_event_with_data() {
        let strategy = MockStrategy::new(ServiceType::RenderingControl);
        let speaker_id = SpeakerId::new("RINCON_TEST456");
        let event_xml = r#"<event>
            <volume_changed>
                <data>
                    volume=50
                    channel=Master
                </data>
            </volume_changed>
        </event>"#;

        let result = strategy.parse_event(&speaker_id, event_xml);

        assert!(result.is_ok());
        let typed_event = result.unwrap();
        assert_eq!(typed_event.event_type(), "volume_changed");
        
        // Downcast to MockEventData to access the data
        if let Some(mock_data) = typed_event.downcast_ref::<MockEventData>() {
            assert_eq!(mock_data.data.get("volume").map(|s| s.as_str()), Some("50"));
            assert_eq!(mock_data.data.get("channel").map(|s| s.as_str()), Some("Master"));
        } else {
            panic!("Failed to downcast to MockEventData");
        }
    }

    #[test]
    fn test_mock_strategy_parse_event_failure() {
        let strategy = MockStrategy::new(ServiceType::AVTransport);
        strategy.set_fail_parsing(true);

        let speaker_id = SpeakerId::new("RINCON_TEST123");
        let event_xml = "<event>data</event>";

        let result = strategy.parse_event(&speaker_id, event_xml);

        assert!(result.is_err());
        assert_eq!(strategy.parse_count(), 1);

        if let Err(e) = result {
            match e {
                StrategyError::EventParseFailed(msg) => {
                    assert!(msg.contains("Mock failure"));
                }
                _ => panic!("Expected EventParseFailed error"),
            }
        }
    }

    #[test]
    fn test_mock_subscription_basic_operations() {
        let subscription = MockSubscription::new(
            "uuid:test-123".to_string(),
            SpeakerId::new("RINCON_TEST123"),
            ServiceType::AVTransport,
            Duration::from_secs(1800),
            "http://192.168.1.50:3400/notify".to_string(),
        );

        assert_eq!(subscription.subscription_id(), "uuid:test-123");
        assert_eq!(subscription.speaker_id().as_str(), "RINCON_TEST123");
        assert_eq!(subscription.service_type(), ServiceType::AVTransport);
        assert_eq!(subscription.callback_url(), "http://192.168.1.50:3400/notify");
        assert!(subscription.is_active());
    }

    #[tokio::test]
    async fn test_mock_subscription_renewal() {
        let mut subscription = MockSubscription::new(
            "uuid:test-123".to_string(),
            SpeakerId::new("RINCON_TEST123"),
            ServiceType::AVTransport,
            Duration::from_secs(1800),
            "http://192.168.1.50:3400/notify".to_string(),
        );

        assert_eq!(subscription.renewal_count(), 0);
        assert!(subscription.renew().await.is_ok());
        assert_eq!(subscription.renewal_count(), 1);
        assert!(subscription.is_active());
    }

    #[tokio::test]
    async fn test_mock_subscription_renewal_failure() {
        let mut subscription = MockSubscription::new(
            "uuid:test-123".to_string(),
            SpeakerId::new("RINCON_TEST123"),
            ServiceType::AVTransport,
            Duration::from_secs(1800),
            "http://192.168.1.50:3400/notify".to_string(),
        );

        subscription.set_fail_renewal(true);
        let result = subscription.renew().await;

        assert!(result.is_err());
        assert_eq!(subscription.renewal_count(), 1);

        match result.unwrap_err() {
            SubscriptionError::RenewalFailed(msg) => {
                assert!(msg.contains("Mock failure"));
            }
            _ => panic!("Expected RenewalFailed error"),
        }
    }

    #[tokio::test]
    async fn test_mock_subscription_unsubscribe() {
        let mut subscription = MockSubscription::new(
            "uuid:test-123".to_string(),
            SpeakerId::new("RINCON_TEST123"),
            ServiceType::AVTransport,
            Duration::from_secs(1800),
            "http://192.168.1.50:3400/notify".to_string(),
        );

        assert_eq!(subscription.unsubscribe_count(), 0);
        assert!(subscription.unsubscribe().await.is_ok());
        assert_eq!(subscription.unsubscribe_count(), 1);
        assert!(!subscription.is_active());

        // Second unsubscribe should fail
        let result = subscription.unsubscribe().await;
        assert!(result.is_err());
        assert_eq!(subscription.unsubscribe_count(), 2);
    }

    #[tokio::test]
    async fn test_mock_subscription_unsubscribe_failure() {
        let mut subscription = MockSubscription::new(
            "uuid:test-123".to_string(),
            SpeakerId::new("RINCON_TEST123"),
            ServiceType::AVTransport,
            Duration::from_secs(1800),
            "http://192.168.1.50:3400/notify".to_string(),
        );

        subscription.set_fail_unsubscribe(true);
        let result = subscription.unsubscribe().await;

        assert!(result.is_err());
        assert_eq!(subscription.unsubscribe_count(), 1);

        match result.unwrap_err() {
            SubscriptionError::UnsubscribeFailed(msg) => {
                assert!(msg.contains("Mock failure"));
            }
            _ => panic!("Expected UnsubscribeFailed error"),
        }
    }

    #[test]
    fn test_mock_subscription_time_until_renewal() {
        let subscription = MockSubscription::new(
            "uuid:test-123".to_string(),
            SpeakerId::new("RINCON_TEST123"),
            ServiceType::AVTransport,
            Duration::from_secs(1800), // 30 minutes
            "http://192.168.1.50:3400/notify".to_string(),
        );

        // With 30 minutes left and 5 minute threshold, no renewal needed
        assert!(subscription.time_until_renewal().is_none());
    }

    #[test]
    fn test_mock_subscription_time_until_renewal_needed() {
        let subscription = MockSubscription::new(
            "uuid:test-123".to_string(),
            SpeakerId::new("RINCON_TEST123"),
            ServiceType::AVTransport,
            Duration::from_secs(200), // Less than 5 minute threshold
            "http://192.168.1.50:3400/notify".to_string(),
        );

        let time_until = subscription.time_until_renewal();
        assert!(time_until.is_some());
        assert!(time_until.unwrap() <= Duration::from_secs(200));
    }

    #[tokio::test]
    async fn test_mock_subscription_renewal_after_unsubscribe() {
        let mut subscription = MockSubscription::new(
            "uuid:test-123".to_string(),
            SpeakerId::new("RINCON_TEST123"),
            ServiceType::AVTransport,
            Duration::from_secs(1800),
            "http://192.168.1.50:3400/notify".to_string(),
        );

        subscription.unsubscribe().await.unwrap();
        let result = subscription.renew().await;

        assert!(result.is_err());
        match result.unwrap_err() {
            SubscriptionError::Expired => {}
            _ => panic!("Expected Expired error"),
        }
    }

    #[tokio::test]
    async fn test_mock_strategy_counters() {
        let strategy = MockStrategy::new(ServiceType::AVTransport);
        let speaker = create_test_speaker();
        let config = SubscriptionConfig::new(1800, "http://192.168.1.50:3400/notify".to_string());

        assert_eq!(strategy.creation_count(), 0);
        assert_eq!(strategy.parse_count(), 0);

        let _ = strategy.create_subscription(&speaker, "http://192.168.1.50:3400".to_string(), &config).await;
        assert_eq!(strategy.creation_count(), 1);

        let _ = strategy.parse_event(&SpeakerId::new("test"), "<event/>");
        assert_eq!(strategy.parse_count(), 1);

        strategy.reset_counters();
        assert_eq!(strategy.creation_count(), 0);
        assert_eq!(strategy.parse_count(), 0);
    }
}
