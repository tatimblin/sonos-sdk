//! Builder for creating and configuring the EventBroker.
//!
//! The `EventBrokerBuilder` provides a fluent API for configuring and creating
//! an `EventBroker` instance. It validates configuration and ensures all required
//! components are properly initialized.
//!
//! # Example
//!
//! ```rust,ignore
//! use sonos_stream::{EventBrokerBuilder, ServiceType};
//! use std::time::Duration;
//!
//! let broker = EventBrokerBuilder::new()
//!     .with_strategy(Box::new(AVTransportStrategy))
//!     .with_strategy(Box::new(RenderingControlStrategy))
//!     .with_port_range(3400, 3500)
//!     .with_subscription_timeout(Duration::from_secs(1800))
//!     .with_retry_config(3, Duration::from_secs(2))
//!     .build()
//!     .await?;
//! ```

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, RwLock};

// Use new broker module structure
use crate::broker::{EventBroker, EventProcessor, RenewalManager, SubscriptionManager};
use crate::callback::CallbackServer;
use crate::error::{BrokerError, Result};
use crate::strategy::SubscriptionStrategy;
use crate::types::{BrokerConfig, ServiceType};

/// Builder for creating and configuring an EventBroker.
///
/// The builder provides a fluent API for:
/// - Registering service strategies
/// - Configuring callback server port range
/// - Setting subscription timeouts
/// - Configuring retry behavior
///
/// # Validation
///
/// The builder validates configuration when `build()` is called:
/// - At least one strategy must be registered
/// - Port range must be valid (start <= end, both > 0)
/// - Timeout must be positive
/// - Retry parameters must be valid
///
/// # Example
///
/// ```rust,ignore
/// use sonos_stream::EventBrokerBuilder;
///
/// let broker = EventBrokerBuilder::new()
///     .with_strategy(Box::new(MyStrategy))
///     .build()
///     .await?;
/// ```
pub struct EventBrokerBuilder {
    /// Registered strategies by service type
    strategies: HashMap<ServiceType, Box<dyn SubscriptionStrategy>>,
    /// Broker configuration
    config: BrokerConfig,
}

impl EventBrokerBuilder {
    /// Create a new builder with default configuration.
    ///
    /// Default configuration:
    /// - Port range: 3400-3500
    /// - Subscription timeout: 30 minutes (1800 seconds)
    /// - Renewal threshold: 5 minutes (300 seconds)
    /// - Max retry attempts: 3
    /// - Retry backoff base: 2 seconds
    /// - Event buffer size: 100
    ///
    /// # Example
    ///
    /// ```rust
    /// use sonos_stream::EventBrokerBuilder;
    ///
    /// let builder = EventBrokerBuilder::new();
    /// ```
    pub fn new() -> Self {
        Self {
            strategies: HashMap::new(),
            config: BrokerConfig::default(),
        }
    }

    /// Register a strategy for a service type.
    ///
    /// The strategy will be used to create subscriptions and parse events for
    /// the service type it handles. Multiple strategies can be registered for
    /// different service types.
    ///
    /// If a strategy is already registered for the service type, it will be replaced.
    ///
    /// # Parameters
    ///
    /// * `strategy` - The strategy implementation to register
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use sonos_stream::EventBrokerBuilder;
    ///
    /// let builder = EventBrokerBuilder::new()
    ///     .with_strategy(Box::new(AVTransportStrategy))
    ///     .with_strategy(Box::new(RenderingControlStrategy));
    /// ```
    pub fn with_strategy(mut self, strategy: Box<dyn SubscriptionStrategy>) -> Self {
        let service_type = strategy.service_type();
        self.strategies.insert(service_type, strategy);
        self
    }

    /// Set the callback server port range.
    ///
    /// The callback server will attempt to bind to a port within this range.
    /// If all ports are in use, the build will fail.
    ///
    /// # Parameters
    ///
    /// * `start` - Start of the port range (inclusive)
    /// * `end` - End of the port range (inclusive)
    ///
    /// # Example
    ///
    /// ```rust
    /// use sonos_stream::EventBrokerBuilder;
    ///
    /// let builder = EventBrokerBuilder::new()
    ///     .with_port_range(3400, 3500);
    /// ```
    pub fn with_port_range(mut self, start: u16, end: u16) -> Self {
        self.config.callback_port_range = (start, end);
        self
    }

    /// Set the subscription timeout.
    ///
    /// This is the duration for which subscriptions remain valid before needing
    /// renewal. The broker will automatically renew subscriptions before they expire.
    ///
    /// # Parameters
    ///
    /// * `timeout` - The subscription timeout duration
    ///
    /// # Example
    ///
    /// ```rust
    /// use sonos_stream::EventBrokerBuilder;
    /// use std::time::Duration;
    ///
    /// let builder = EventBrokerBuilder::new()
    ///     .with_subscription_timeout(Duration::from_secs(1800)); // 30 minutes
    /// ```
    pub fn with_subscription_timeout(mut self, timeout: Duration) -> Self {
        self.config.subscription_timeout = timeout;
        self
    }

    /// Set the renewal threshold.
    ///
    /// This is how far before expiration the broker will attempt to renew subscriptions.
    /// For example, with a 30-minute timeout and 5-minute threshold, renewal will be
    /// attempted at the 25-minute mark.
    ///
    /// # Parameters
    ///
    /// * `threshold` - The renewal threshold duration
    ///
    /// # Example
    ///
    /// ```rust
    /// use sonos_stream::EventBrokerBuilder;
    /// use std::time::Duration;
    ///
    /// let builder = EventBrokerBuilder::new()
    ///     .with_renewal_threshold(Duration::from_secs(300)); // 5 minutes
    /// ```
    pub fn with_renewal_threshold(mut self, threshold: Duration) -> Self {
        self.config.renewal_threshold = threshold;
        self
    }

    /// Set retry configuration for failed operations.
    ///
    /// When subscription operations fail, the broker will retry up to `max_attempts` times
    /// with exponential backoff starting from `base_backoff`.
    ///
    /// # Parameters
    ///
    /// * `max_attempts` - Maximum number of retry attempts
    /// * `base_backoff` - Base duration for exponential backoff
    ///
    /// # Example
    ///
    /// ```rust
    /// use sonos_stream::EventBrokerBuilder;
    /// use std::time::Duration;
    ///
    /// let builder = EventBrokerBuilder::new()
    ///     .with_retry_config(3, Duration::from_secs(2));
    /// ```
    pub fn with_retry_config(mut self, max_attempts: u32, base_backoff: Duration) -> Self {
        self.config.max_retry_attempts = max_attempts;
        self.config.retry_backoff_base = base_backoff;
        self
    }

    /// Set the event buffer size.
    ///
    /// This is the size of the channel buffer for events emitted by the broker.
    /// If the buffer fills up, the broker will block when emitting events until
    /// the application consumes them.
    ///
    /// # Parameters
    ///
    /// * `size` - The buffer size
    ///
    /// # Example
    ///
    /// ```rust
    /// use sonos_stream::EventBrokerBuilder;
    ///
    /// let builder = EventBrokerBuilder::new()
    ///     .with_event_buffer_size(200);
    /// ```
    pub fn with_event_buffer_size(mut self, size: usize) -> Self {
        self.config.event_buffer_size = size;
        self
    }

    /// Build the EventBroker.
    ///
    /// This method validates the configuration and creates all required components:
    /// - Validates that at least one strategy is registered
    /// - Validates port range, timeout, and retry parameters
    /// - Creates and starts the callback server
    /// - Creates event channels
    /// - Starts the background renewal task
    ///
    /// # Returns
    ///
    /// Returns the configured `EventBroker` on success.
    ///
    /// # Errors
    ///
    /// * `BrokerError::ConfigurationError` - If configuration is invalid
    /// * `BrokerError::CallbackServerError` - If callback server fails to start
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use sonos_stream::EventBrokerBuilder;
    ///
    /// let broker = EventBrokerBuilder::new()
    ///     .with_strategy(Box::new(MyStrategy))
    ///     .build()
    ///     .await?;
    /// ```
    pub async fn build(self) -> Result<EventBroker> {
        // Validate that at least one strategy is registered
        if self.strategies.is_empty() {
            return Err(BrokerError::ConfigurationError(
                "At least one strategy must be registered".to_string(),
            ));
        }

        // Validate port range
        let (start_port, end_port) = self.config.callback_port_range;
        if start_port == 0 || end_port == 0 {
            return Err(BrokerError::ConfigurationError(
                "Port range must not include port 0".to_string(),
            ));
        }
        if start_port > end_port {
            return Err(BrokerError::ConfigurationError(format!(
                "Invalid port range: start ({start_port}) > end ({end_port})"
            )));
        }

        // Validate timeout
        if self.config.subscription_timeout.as_secs() == 0 {
            return Err(BrokerError::ConfigurationError(
                "Subscription timeout must be positive".to_string(),
            ));
        }

        // Validate renewal threshold
        if self.config.renewal_threshold >= self.config.subscription_timeout {
            return Err(BrokerError::ConfigurationError(
                "Renewal threshold must be less than subscription timeout".to_string(),
            ));
        }

        // Validate retry parameters
        if self.config.max_retry_attempts == 0 {
            return Err(BrokerError::ConfigurationError(
                "Max retry attempts must be at least 1".to_string(),
            ));
        }
        if self.config.retry_backoff_base.as_millis() == 0 {
            return Err(BrokerError::ConfigurationError(
                "Retry backoff base must be positive".to_string(),
            ));
        }

        // Validate event buffer size
        if self.config.event_buffer_size == 0 {
            return Err(BrokerError::ConfigurationError(
                "Event buffer size must be at least 1".to_string(),
            ));
        }

        // Create raw event channel for callback server -> event processor communication
        let (raw_event_tx, raw_event_rx) = mpsc::unbounded_channel();

        // Create and start callback server
        let callback_server = CallbackServer::new(self.config.callback_port_range, raw_event_tx)
            .await
            .map_err(|e| {
                BrokerError::CallbackServerError(format!("Failed to start callback server: {e}"))
            })?;

        // Wrap callback server in Arc for sharing between components
        let callback_server_arc = Arc::new(callback_server);

        // Create event channel for broker -> application communication
        let (event_tx, event_rx) = mpsc::channel(self.config.event_buffer_size);

        // Create shared subscription state
        let subscriptions = Arc::new(RwLock::new(HashMap::new()));

        // Create strategies Arc for sharing between components
        let strategies_arc = Arc::new(self.strategies);

        // Create SubscriptionManager
        let subscription_manager = SubscriptionManager::new(
            subscriptions.clone(),
            strategies_arc.clone(),
            callback_server_arc.clone(),
            event_tx.clone(),
            self.config.clone(),
        );

        // Start RenewalManager with background task
        let renewal_manager = RenewalManager::start(
            subscriptions.clone(),
            event_tx.clone(),
            self.config.clone(),
        );

        // Start EventProcessor with background task
        let event_processor = EventProcessor::start(
            raw_event_rx,
            strategies_arc.clone(),
            subscriptions.clone(),
            event_tx.clone(),
        );

        // Create the broker with the shared Arc-wrapped components
        Ok(EventBroker::new(
            strategies_arc,
            callback_server_arc,
            self.config,
            event_tx,
            event_rx,
            subscription_manager,
            renewal_manager,
            event_processor,
        ))
    }
}

impl Default for EventBrokerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::StrategyError;
    use crate::subscription::Subscription;
    use crate::types::{SpeakerId, Speaker, SubscriptionConfig, SubscriptionScope};

    // Mock strategy for testing
    struct MockStrategy {
        service_type: ServiceType,
    }

    impl MockStrategy {
        fn new(service_type: ServiceType) -> Self {
            Self { service_type }
        }
    }

    impl SubscriptionStrategy for MockStrategy {
        fn service_type(&self) -> ServiceType {
            self.service_type
        }

        fn subscription_scope(&self) -> SubscriptionScope {
            SubscriptionScope::PerSpeaker
        }

        fn create_subscription(
            &self,
            speaker: &Speaker,
            _callback_url: String,
            _config: &SubscriptionConfig,
        ) -> std::result::Result<Box<dyn Subscription>, StrategyError> {
            // Mock subscription
            struct MockSub {
                id: String,
                speaker_id: SpeakerId,
                service_type: ServiceType,
            }

            impl Subscription for MockSub {
                fn subscription_id(&self) -> &str {
                    &self.id
                }
                fn renew(&mut self) -> std::result::Result<(), crate::error::SubscriptionError> {
                    Ok(())
                }
                fn unsubscribe(
                    &mut self,
                ) -> std::result::Result<(), crate::error::SubscriptionError> {
                    Ok(())
                }
                fn is_active(&self) -> bool {
                    true
                }
                fn time_until_renewal(&self) -> Option<Duration> {
                    None
                }
                fn speaker_id(&self) -> &SpeakerId {
                    &self.speaker_id
                }
                fn service_type(&self) -> ServiceType {
                    self.service_type
                }
            }

            Ok(Box::new(MockSub {
                id: format!("mock-sub-{}", speaker.id.as_str()),
                speaker_id: speaker.id.clone(),
                service_type: self.service_type,
            }))
        }

        fn parse_event(
            &self,
            _speaker_id: &SpeakerId,
            _event_xml: &str,
        ) -> std::result::Result<Vec<crate::event::ParsedEvent>, StrategyError> {
            Ok(vec![])
        }
    }

    #[test]
    fn test_builder_new() {
        let builder = EventBrokerBuilder::new();
        assert_eq!(builder.strategies.len(), 0);
        assert_eq!(builder.config.callback_port_range, (3400, 3500));
        assert_eq!(
            builder.config.subscription_timeout,
            Duration::from_secs(1800)
        );
        assert_eq!(builder.config.renewal_threshold, Duration::from_secs(300));
        assert_eq!(builder.config.max_retry_attempts, 3);
        assert_eq!(builder.config.retry_backoff_base, Duration::from_secs(2));
        assert_eq!(builder.config.event_buffer_size, 100);
    }

    #[test]
    fn test_builder_default() {
        let builder = EventBrokerBuilder::default();
        assert_eq!(builder.strategies.len(), 0);
        assert_eq!(builder.config.callback_port_range, (3400, 3500));
    }

    #[test]
    fn test_builder_with_strategy() {
        let builder = EventBrokerBuilder::new()
            .with_strategy(Box::new(MockStrategy::new(ServiceType::AVTransport)));

        assert_eq!(builder.strategies.len(), 1);
        assert!(builder.strategies.contains_key(&ServiceType::AVTransport));
    }

    #[test]
    fn test_builder_with_multiple_strategies() {
        let builder = EventBrokerBuilder::new()
            .with_strategy(Box::new(MockStrategy::new(ServiceType::AVTransport)))
            .with_strategy(Box::new(MockStrategy::new(
                ServiceType::RenderingControl,
            )))
            .with_strategy(Box::new(MockStrategy::new(
                ServiceType::ZoneGroupTopology,
            )));

        assert_eq!(builder.strategies.len(), 3);
        assert!(builder.strategies.contains_key(&ServiceType::AVTransport));
        assert!(builder
            .strategies
            .contains_key(&ServiceType::RenderingControl));
        assert!(builder
            .strategies
            .contains_key(&ServiceType::ZoneGroupTopology));
    }

    #[test]
    fn test_builder_with_strategy_replacement() {
        let builder = EventBrokerBuilder::new()
            .with_strategy(Box::new(MockStrategy::new(ServiceType::AVTransport)))
            .with_strategy(Box::new(MockStrategy::new(ServiceType::AVTransport)));

        // Should only have one strategy (second replaces first)
        assert_eq!(builder.strategies.len(), 1);
        assert!(builder.strategies.contains_key(&ServiceType::AVTransport));
    }

    #[test]
    fn test_builder_with_port_range() {
        let builder = EventBrokerBuilder::new().with_port_range(4000, 4100);

        assert_eq!(builder.config.callback_port_range, (4000, 4100));
    }

    #[test]
    fn test_builder_with_subscription_timeout() {
        let builder =
            EventBrokerBuilder::new().with_subscription_timeout(Duration::from_secs(3600));

        assert_eq!(
            builder.config.subscription_timeout,
            Duration::from_secs(3600)
        );
    }

    #[test]
    fn test_builder_with_renewal_threshold() {
        let builder =
            EventBrokerBuilder::new().with_renewal_threshold(Duration::from_secs(600));

        assert_eq!(builder.config.renewal_threshold, Duration::from_secs(600));
    }

    #[test]
    fn test_builder_with_retry_config() {
        let builder = EventBrokerBuilder::new().with_retry_config(5, Duration::from_secs(3));

        assert_eq!(builder.config.max_retry_attempts, 5);
        assert_eq!(builder.config.retry_backoff_base, Duration::from_secs(3));
    }

    #[test]
    fn test_builder_with_event_buffer_size() {
        let builder = EventBrokerBuilder::new().with_event_buffer_size(200);

        assert_eq!(builder.config.event_buffer_size, 200);
    }

    #[test]
    fn test_builder_fluent_api() {
        let builder = EventBrokerBuilder::new()
            .with_strategy(Box::new(MockStrategy::new(ServiceType::AVTransport)))
            .with_port_range(4000, 4100)
            .with_subscription_timeout(Duration::from_secs(3600))
            .with_renewal_threshold(Duration::from_secs(600))
            .with_retry_config(5, Duration::from_secs(3))
            .with_event_buffer_size(200);

        assert_eq!(builder.strategies.len(), 1);
        assert_eq!(builder.config.callback_port_range, (4000, 4100));
        assert_eq!(
            builder.config.subscription_timeout,
            Duration::from_secs(3600)
        );
        assert_eq!(builder.config.renewal_threshold, Duration::from_secs(600));
        assert_eq!(builder.config.max_retry_attempts, 5);
        assert_eq!(builder.config.retry_backoff_base, Duration::from_secs(3));
        assert_eq!(builder.config.event_buffer_size, 200);
    }

    #[tokio::test]
    async fn test_build_no_strategies() {
        let builder = EventBrokerBuilder::new();

        let result = builder.build().await;
        assert!(result.is_err());

        match result {
            Err(BrokerError::ConfigurationError(msg)) => {
                assert!(msg.contains("At least one strategy"));
            }
            _ => panic!("Expected ConfigurationError"),
        }
    }

    #[tokio::test]
    async fn test_build_invalid_port_range_zero() {
        let builder = EventBrokerBuilder::new()
            .with_strategy(Box::new(MockStrategy::new(ServiceType::AVTransport)))
            .with_port_range(0, 100);

        let result = builder.build().await;
        assert!(result.is_err());

        match result {
            Err(BrokerError::ConfigurationError(msg)) => {
                assert!(msg.contains("port 0"));
            }
            _ => panic!("Expected ConfigurationError"),
        }
    }

    #[tokio::test]
    async fn test_build_invalid_port_range_reversed() {
        let builder = EventBrokerBuilder::new()
            .with_strategy(Box::new(MockStrategy::new(ServiceType::AVTransport)))
            .with_port_range(4100, 4000);

        let result = builder.build().await;
        assert!(result.is_err());

        match result {
            Err(BrokerError::ConfigurationError(msg)) => {
                assert!(msg.contains("Invalid port range"));
                assert!(msg.contains("4100"));
                assert!(msg.contains("4000"));
            }
            _ => panic!("Expected ConfigurationError"),
        }
    }

    #[tokio::test]
    async fn test_build_zero_timeout() {
        let builder = EventBrokerBuilder::new()
            .with_strategy(Box::new(MockStrategy::new(ServiceType::AVTransport)))
            .with_subscription_timeout(Duration::from_secs(0));

        let result = builder.build().await;
        assert!(result.is_err());

        match result {
            Err(BrokerError::ConfigurationError(msg)) => {
                assert!(msg.contains("timeout must be positive"));
            }
            _ => panic!("Expected ConfigurationError"),
        }
    }

    #[tokio::test]
    async fn test_build_renewal_threshold_too_large() {
        let builder = EventBrokerBuilder::new()
            .with_strategy(Box::new(MockStrategy::new(ServiceType::AVTransport)))
            .with_subscription_timeout(Duration::from_secs(1800))
            .with_renewal_threshold(Duration::from_secs(2000));

        let result = builder.build().await;
        assert!(result.is_err());

        match result {
            Err(BrokerError::ConfigurationError(msg)) => {
                assert!(msg.contains("Renewal threshold must be less than"));
            }
            _ => panic!("Expected ConfigurationError"),
        }
    }

    #[tokio::test]
    async fn test_build_zero_retry_attempts() {
        let builder = EventBrokerBuilder::new()
            .with_strategy(Box::new(MockStrategy::new(ServiceType::AVTransport)))
            .with_retry_config(0, Duration::from_secs(2));

        let result = builder.build().await;
        assert!(result.is_err());

        match result {
            Err(BrokerError::ConfigurationError(msg)) => {
                assert!(msg.contains("retry attempts must be at least 1"));
            }
            _ => panic!("Expected ConfigurationError"),
        }
    }

    #[tokio::test]
    async fn test_build_zero_backoff() {
        let builder = EventBrokerBuilder::new()
            .with_strategy(Box::new(MockStrategy::new(ServiceType::AVTransport)))
            .with_retry_config(3, Duration::from_millis(0));

        let result = builder.build().await;
        assert!(result.is_err());

        match result {
            Err(BrokerError::ConfigurationError(msg)) => {
                assert!(msg.contains("backoff base must be positive"));
            }
            _ => panic!("Expected ConfigurationError"),
        }
    }

    #[tokio::test]
    async fn test_build_zero_buffer_size() {
        let builder = EventBrokerBuilder::new()
            .with_strategy(Box::new(MockStrategy::new(ServiceType::AVTransport)))
            .with_event_buffer_size(0);

        let result = builder.build().await;
        assert!(result.is_err());

        match result {
            Err(BrokerError::ConfigurationError(msg)) => {
                assert!(msg.contains("buffer size must be at least 1"));
            }
            _ => panic!("Expected ConfigurationError"),
        }
    }

    #[tokio::test]
    async fn test_build_success() {
        let builder = EventBrokerBuilder::new()
            .with_strategy(Box::new(MockStrategy::new(ServiceType::AVTransport)))
            .with_port_range(50000, 50100); // Use high port range to avoid conflicts

        let result = builder.build().await;
        assert!(
            result.is_ok(),
            "Build should succeed with valid configuration"
        );

        // Clean up
        if let Ok(broker) = result {
            let _ = broker.shutdown().await;
        }
    }

    #[tokio::test]
    async fn test_build_with_multiple_strategies() {
        let builder = EventBrokerBuilder::new()
            .with_strategy(Box::new(MockStrategy::new(ServiceType::AVTransport)))
            .with_strategy(Box::new(MockStrategy::new(
                ServiceType::RenderingControl,
            )))
            .with_strategy(Box::new(MockStrategy::new(
                ServiceType::ZoneGroupTopology,
            )))
            .with_port_range(50100, 50200);

        let result = builder.build().await;
        assert!(
            result.is_ok(),
            "Build should succeed with multiple strategies"
        );

        // Clean up
        if let Ok(broker) = result {
            let _ = broker.shutdown().await;
        }
    }

    #[tokio::test]
    async fn test_build_with_custom_config() {
        let builder = EventBrokerBuilder::new()
            .with_strategy(Box::new(MockStrategy::new(ServiceType::AVTransport)))
            .with_port_range(50200, 50300)
            .with_subscription_timeout(Duration::from_secs(3600))
            .with_renewal_threshold(Duration::from_secs(600))
            .with_retry_config(5, Duration::from_secs(3))
            .with_event_buffer_size(200);

        let result = builder.build().await;
        assert!(
            result.is_ok(),
            "Build should succeed with custom configuration"
        );

        // Clean up
        if let Ok(broker) = result {
            let _ = broker.shutdown().await;
        }
    }
}
