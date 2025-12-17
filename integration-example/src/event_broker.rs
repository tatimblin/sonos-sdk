//! Event broker setup and configuration module.
//!
//! This module provides functionality for creating and configuring the EventBroker
//! with AVTransportStrategy for subscribing to Sonos device events.

use anyhow::{Context, Result};
use sonos_stream::{EventBroker, EventBrokerBuilder, Strategy};
use std::time::Duration;
use tracing::{info, warn, error};

/// Configuration for the event broker setup
#[derive(Debug, Clone)]
pub struct BrokerConfig {
    /// Port range for the callback server (start, end)
    pub callback_port_range: (u16, u16),
    /// Subscription timeout duration
    pub subscription_timeout: Duration,
    /// Renewal threshold - how far before expiration to renew
    pub renewal_threshold: Duration,
    /// Maximum retry attempts for failed operations
    pub max_retry_attempts: u32,
    /// Base duration for exponential backoff
    pub retry_backoff_base: Duration,
    /// Event buffer size for the event channel
    pub event_buffer_size: usize,
}

impl Default for BrokerConfig {
    fn default() -> Self {
        Self {
            callback_port_range: (3400, 3500),
            subscription_timeout: Duration::from_secs(1800), // 30 minutes
            renewal_threshold: Duration::from_secs(300),     // 5 minutes
            max_retry_attempts: 3,
            retry_backoff_base: Duration::from_secs(2),
            event_buffer_size: 100,
        }
    }
}

/// Creates and configures an EventBroker with AVTransportStrategy.
///
/// This function handles the complete broker initialization process:
/// - Creates EventBrokerBuilder with AVTransportStrategy
/// - Configures callback server port range and timeouts
/// - Handles broker creation errors gracefully
///
/// # Arguments
///
/// * `config` - Configuration for the broker setup
///
/// # Returns
///
/// Returns the configured EventBroker on success.
///
/// # Errors
///
/// Returns an error if:
/// - Broker configuration is invalid
/// - Callback server fails to start (all ports in range are in use)
/// - Strategy registration fails
///
/// # Example
///
/// ```rust,no_run
/// use integration_example::event_broker::{create_event_broker, BrokerConfig};
///
/// # async fn example() -> anyhow::Result<()> {
/// let config = BrokerConfig::default();
/// let broker = create_event_broker(config).await?;
/// # Ok(())
/// # }
/// ```
pub async fn create_event_broker(config: BrokerConfig) -> Result<EventBroker> {
    info!("Initializing event broker with AVTransport strategy");
    
    // Log configuration details
    info!(
        "Broker configuration: port_range={:?}, timeout={}s, renewal_threshold={}s, max_retries={}, buffer_size={}",
        config.callback_port_range,
        config.subscription_timeout.as_secs(),
        config.renewal_threshold.as_secs(),
        config.max_retry_attempts,
        config.event_buffer_size
    );

    // Create AVTransport strategy
    let av_transport_strategy = Strategy::AVTransport;
    info!("Created AVTransport strategy for media transport events");

    // Build the event broker with configuration
    let broker = EventBrokerBuilder::new()
        .with_strategy(Box::new(av_transport_strategy))
        .with_port_range(config.callback_port_range.0, config.callback_port_range.1)
        .with_subscription_timeout(config.subscription_timeout)
        .with_renewal_threshold(config.renewal_threshold)
        .with_retry_config(config.max_retry_attempts, config.retry_backoff_base)
        .with_event_buffer_size(config.event_buffer_size)
        .build()
        .await
        .context("Failed to create event broker")?;

    info!(
        "Event broker initialized successfully with callback server on port range {:?}",
        config.callback_port_range
    );

    Ok(broker)
}

/// Creates an event broker with custom configuration.
///
/// This is a convenience function for creating a broker with specific settings
/// that differ from the defaults.
///
/// # Arguments
///
/// * `port_start` - Start of callback server port range
/// * `port_end` - End of callback server port range
/// * `timeout_secs` - Subscription timeout in seconds
///
/// # Returns
///
/// Returns the configured EventBroker on success.
///
/// # Example
///
/// ```rust,no_run
/// use integration_example::event_broker::create_event_broker_with_config;
///
/// # async fn example() -> anyhow::Result<()> {
/// // Create broker with custom port range and 1-hour timeout
/// let broker = create_event_broker_with_config(4000, 4100, 3600).await?;
/// # Ok(())
/// # }
/// ```
pub async fn create_event_broker_with_config(
    port_start: u16,
    port_end: u16,
    timeout_secs: u64,
) -> Result<EventBroker> {
    let config = BrokerConfig {
        callback_port_range: (port_start, port_end),
        subscription_timeout: Duration::from_secs(timeout_secs),
        ..Default::default()
    };

    create_event_broker(config).await
}

/// Validates broker configuration before creation.
///
/// This function performs validation checks on the broker configuration
/// to catch common configuration errors early.
///
/// # Arguments
///
/// * `config` - Configuration to validate
///
/// # Returns
///
/// Returns Ok(()) if configuration is valid, Err otherwise.
///
/// # Errors
///
/// Returns an error if:
/// - Port range is invalid (start > end, contains port 0)
/// - Timeout values are invalid (zero or renewal >= timeout)
/// - Retry configuration is invalid (zero attempts or backoff)
/// - Buffer size is zero
pub fn validate_broker_config(config: &BrokerConfig) -> Result<()> {
    let (start_port, end_port) = config.callback_port_range;
    
    // Validate port range
    if start_port == 0 || end_port == 0 {
        return Err(anyhow::anyhow!(
            "Port range must not include port 0: {:?}",
            config.callback_port_range
        ));
    }
    
    if start_port > end_port {
        return Err(anyhow::anyhow!(
            "Invalid port range: start ({}) > end ({})",
            start_port,
            end_port
        ));
    }

    // Validate timeout
    if config.subscription_timeout.as_secs() == 0 {
        return Err(anyhow::anyhow!("Subscription timeout must be positive"));
    }

    // Validate renewal threshold
    if config.renewal_threshold >= config.subscription_timeout {
        return Err(anyhow::anyhow!(
            "Renewal threshold ({:?}) must be less than subscription timeout ({:?})",
            config.renewal_threshold,
            config.subscription_timeout
        ));
    }

    // Validate retry parameters
    if config.max_retry_attempts == 0 {
        return Err(anyhow::anyhow!("Max retry attempts must be at least 1"));
    }
    
    if config.retry_backoff_base.as_millis() == 0 {
        return Err(anyhow::anyhow!("Retry backoff base must be positive"));
    }

    // Validate buffer size
    if config.event_buffer_size == 0 {
        return Err(anyhow::anyhow!("Event buffer size must be at least 1"));
    }

    Ok(())
}

/// Handles broker creation errors and provides user-friendly error messages.
///
/// This function analyzes broker creation errors and provides more helpful
/// error messages and potential solutions.
///
/// # Arguments
///
/// * `error` - The error from broker creation
/// * `config` - The configuration that was used
///
/// # Returns
///
/// Returns a more descriptive error with context and potential solutions.
pub fn handle_broker_creation_error(
    error: sonos_stream::BrokerError,
    config: &BrokerConfig,
) -> anyhow::Error {
    match error {
        sonos_stream::BrokerError::ConfigurationError(msg) => {
            anyhow::anyhow!("Broker configuration error: {}", msg)
        }
        sonos_stream::BrokerError::CallbackServerError(msg) => {
            if msg.contains("Address already in use") || msg.contains("bind") {
                warn!(
                    "All ports in range {:?} are in use. Consider using a different port range.",
                    config.callback_port_range
                );
                anyhow::anyhow!(
                    "Failed to start callback server: {}. All ports in range {:?} may be in use. Try a different port range or stop other services using these ports.",
                    msg,
                    config.callback_port_range
                )
            } else {
                anyhow::anyhow!("Callback server error: {}", msg)
            }
        }
        _ => {
            error!("Unexpected broker error: {:?}", error);
            anyhow::anyhow!("Unexpected broker error: {}", error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_broker_config_default() {
        let config = BrokerConfig::default();
        assert_eq!(config.callback_port_range, (3400, 3500));
        assert_eq!(config.subscription_timeout, Duration::from_secs(1800));
        assert_eq!(config.renewal_threshold, Duration::from_secs(300));
        assert_eq!(config.max_retry_attempts, 3);
        assert_eq!(config.retry_backoff_base, Duration::from_secs(2));
        assert_eq!(config.event_buffer_size, 100);
    }

    #[test]
    fn test_validate_broker_config_valid() {
        let config = BrokerConfig::default();
        assert!(validate_broker_config(&config).is_ok());
    }

    #[test]
    fn test_validate_broker_config_invalid_port_range() {
        let mut config = BrokerConfig::default();
        config.callback_port_range = (0, 100);
        assert!(validate_broker_config(&config).is_err());

        config.callback_port_range = (100, 0);
        assert!(validate_broker_config(&config).is_err());

        config.callback_port_range = (200, 100);
        assert!(validate_broker_config(&config).is_err());
    }

    #[test]
    fn test_validate_broker_config_invalid_timeout() {
        let mut config = BrokerConfig::default();
        config.subscription_timeout = Duration::from_secs(0);
        assert!(validate_broker_config(&config).is_err());
    }

    #[test]
    fn test_validate_broker_config_invalid_renewal_threshold() {
        let mut config = BrokerConfig::default();
        config.renewal_threshold = Duration::from_secs(2000);
        config.subscription_timeout = Duration::from_secs(1800);
        assert!(validate_broker_config(&config).is_err());
    }

    #[test]
    fn test_validate_broker_config_invalid_retry() {
        let mut config = BrokerConfig::default();
        config.max_retry_attempts = 0;
        assert!(validate_broker_config(&config).is_err());

        config.max_retry_attempts = 3;
        config.retry_backoff_base = Duration::from_millis(0);
        assert!(validate_broker_config(&config).is_err());
    }

    #[test]
    fn test_validate_broker_config_invalid_buffer_size() {
        let mut config = BrokerConfig::default();
        config.event_buffer_size = 0;
        assert!(validate_broker_config(&config).is_err());
    }

    #[tokio::test]
    async fn test_create_event_broker_success() {
        let config = BrokerConfig {
            callback_port_range: (50000, 50100), // Use high port range to avoid conflicts
            ..Default::default()
        };

        let result = create_event_broker(config).await;
        assert!(result.is_ok(), "Broker creation should succeed: {:?}", result.err());

        // Clean up
        if let Ok(broker) = result {
            let _ = broker.shutdown().await;
        }
    }

    #[tokio::test]
    async fn test_create_event_broker_with_config() {
        let result = create_event_broker_with_config(50100, 50200, 3600).await;
        assert!(result.is_ok(), "Broker creation with config should succeed: {:?}", result.err());

        // Clean up
        if let Ok(broker) = result {
            let _ = broker.shutdown().await;
        }
    }
}