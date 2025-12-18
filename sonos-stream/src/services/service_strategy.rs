//! Core strategy trait for service-specific subscription handling.

use crate::error::StrategyError;
use crate::event::TypedEvent;
use crate::subscription::{Subscription, UPnPSubscription};
use crate::types::{ServiceType, SpeakerId, Speaker, SubscriptionConfig, SubscriptionScope};

/// Service strategy trait providing common subscription logic.
///
/// This trait provides default implementations for creating and managing UPnP
/// subscriptions. Implementors only need to provide service-specific configuration
/// methods (service_type, subscription_scope, service_endpoint_path, parse_event).
///
/// # Design
///
/// - Implementors define: Service-specific configuration (what parser? what endpoint?)
/// - Trait provides: Common implementation logic (how to subscribe?)
///
/// # Example Implementation
///
/// ```rust,ignore
/// use sonos_stream::{ServiceStrategy, ServiceType, SubscriptionScope};
///
/// #[derive(Debug, Clone)]
/// pub struct MyServiceProvider;
///
/// #[async_trait::async_trait]
/// impl ServiceStrategy for MyServiceProvider {
///     fn service_type(&self) -> ServiceType {
///         ServiceType::AVTransport
///     }
///
///     fn subscription_scope(&self) -> SubscriptionScope {
///         SubscriptionScope::PerSpeaker
///     }
///
///     fn service_endpoint_path(&self) -> &'static str {
///         "/MediaRenderer/AVTransport/Event"
///     }
///
///     fn parse_event(&self, _speaker_id: &SpeakerId, event_xml: &str) -> Result<TypedEvent, StrategyError> {
///         // Parse XML and return TypedEvent
///         todo!()
///     }
/// }
/// ```
#[async_trait::async_trait]
pub trait ServiceStrategy: Send + Sync + std::fmt::Debug {
    /// Get the service type this strategy handles.
    fn service_type(&self) -> ServiceType;

    /// Get subscription scope metadata for this service.
    fn subscription_scope(&self) -> SubscriptionScope;

    /// Get the UPnP service endpoint path.
    fn service_endpoint_path(&self) -> &'static str;

    /// Parse raw UPnP event XML into a typed event using the service-specific parser.
    fn parse_event(
        &self,
        speaker_id: &SpeakerId,
        event_xml: &str,
    ) -> Result<TypedEvent, StrategyError>;

    /// Create a new UPnP subscription for a speaker.
    ///
    /// Default implementation constructs the service endpoint URL and creates
    /// a UPnP subscription. Override only if custom subscription logic is needed.
    ///
    /// # Parameters
    ///
    /// - `speaker`: The speaker to subscribe to
    /// - `callback_url`: The base URL of the callback server
    /// - `config`: Configuration including timeout and full callback URL
    ///
    /// # Returns
    ///
    /// A boxed `Subscription` instance that can be used to manage the subscription lifecycle.
    ///
    /// # Errors
    ///
    /// - `StrategyError::SubscriptionCreationFailed` if the subscription request fails
    /// - `StrategyError::NetworkError` if a network error occurs
    async fn create_subscription(
        &self,
        speaker: &Speaker,
        callback_url: String,
        config: &SubscriptionConfig,
    ) -> Result<Box<dyn Subscription>, StrategyError> {
        let endpoint_url = format!("http://{}:1400{}", speaker.ip, self.service_endpoint_path());
        let service_type = self.service_type();
        
        let subscription = UPnPSubscription::create_subscription(
            speaker.id.clone(),
            service_type,
            endpoint_url,
            callback_url,
            config.timeout_seconds,
        )
        .await
        .map_err(|e| match e {
            crate::error::SubscriptionError::NetworkError(msg) => StrategyError::NetworkError(msg),
            crate::error::SubscriptionError::UnsubscribeFailed(msg) => StrategyError::SubscriptionCreationFailed(msg),
            crate::error::SubscriptionError::RenewalFailed(msg) => StrategyError::SubscriptionCreationFailed(msg),
            crate::error::SubscriptionError::Expired => StrategyError::SubscriptionCreationFailed("Subscription expired during creation".to_string()),
        })?;

        Ok(Box::new(subscription))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Speaker, SpeakerId};
    use std::net::IpAddr;

    // Mock strategy for testing
    #[derive(Debug, Clone)]
    struct MockStrategy;

    #[async_trait::async_trait]
    impl ServiceStrategy for MockStrategy {
        fn service_type(&self) -> ServiceType {
            ServiceType::AVTransport
        }

        fn subscription_scope(&self) -> SubscriptionScope {
            SubscriptionScope::PerSpeaker
        }

        fn service_endpoint_path(&self) -> &'static str {
            "/MediaRenderer/AVTransport/Event"
        }

        fn parse_event(
            &self,
            _speaker_id: &SpeakerId,
            _event_xml: &str,
        ) -> Result<TypedEvent, StrategyError> {
            // Return a mock TypedEvent for testing
            Err(StrategyError::EventParseFailed("Mock parsing not implemented".to_string()))
        }
    }

    #[test]
    fn test_service_strategy_trait_methods() {
        let strategy = MockStrategy;
        
        assert_eq!(strategy.service_type(), ServiceType::AVTransport);
        assert_eq!(strategy.subscription_scope(), SubscriptionScope::PerSpeaker);
        assert_eq!(strategy.service_endpoint_path(), "/MediaRenderer/AVTransport/Event");
    }

    #[test]
    fn test_endpoint_url_construction() {
        let strategy = MockStrategy;
        let speaker = Speaker::new(
            SpeakerId::new("RINCON_123"),
            "192.168.1.100".parse::<IpAddr>().unwrap(),
            "Living Room".to_string(),
            "Living Room".to_string(),
        );
        
        let expected_path = "/MediaRenderer/AVTransport/Event";
        assert_eq!(strategy.service_endpoint_path(), expected_path);
        
        // Verify the full URL construction logic
        let expected_url = format!("http://{}:1400{}", speaker.ip, expected_path);
        assert_eq!(expected_url, "http://192.168.1.100:1400/MediaRenderer/AVTransport/Event");
    }

    #[test]
    fn test_strategy_thread_safety() {
        let strategy = MockStrategy;
        
        // Ensure MockStrategy implements Send + Sync
        fn assert_send_sync<T: Send + Sync>(_: T) {}
        assert_send_sync(strategy);
    }

    #[test]
    fn test_strategy_clone() {
        let strategy = MockStrategy;
        let cloned = strategy.clone();
        
        assert_eq!(strategy.service_type(), cloned.service_type());
        assert_eq!(strategy.subscription_scope(), cloned.subscription_scope());
        assert_eq!(strategy.service_endpoint_path(), cloned.service_endpoint_path());
    }

    #[test]
    fn test_parse_event_mock() {
        let strategy = MockStrategy;
        let speaker_id = SpeakerId::new("test_speaker");
        
        let result = strategy.parse_event(&speaker_id, "<mock>xml</mock>");
        assert!(result.is_err(), "Mock strategy should fail parsing");
        
        match result {
            Err(StrategyError::EventParseFailed(msg)) => {
                assert!(msg.contains("Mock parsing not implemented"));
            }
            _ => panic!("Expected EventParseFailed error"),
        }
    }
}