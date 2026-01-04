//! Configuration types for the sonos-stream crate
//!
//! This module defines configuration structures that control the behavior
//! of the EventBroker, including firewall detection, polling intervals,
//! and event processing settings.

use std::time::Duration;

/// Configuration for the EventBroker
///
/// This struct controls all aspects of the event broker's behavior, from
/// callback server settings to polling intervals and firewall detection.
#[derive(Debug, Clone)]
pub struct BrokerConfig {
    /// Port range for the callback server
    /// Default: (3400, 3500)
    pub callback_port_range: (u16, u16),

    /// Timeout for detecting event failures (fallback after proactive detection)
    /// Default: 30 seconds
    pub event_timeout: Duration,

    /// Delay after proactive firewall detection before activating polling
    /// Default: 5 seconds
    pub polling_activation_delay: Duration,

    /// Base interval for polling operations
    /// Default: 5 seconds
    pub base_polling_interval: Duration,

    /// Maximum interval for adaptive polling
    /// Default: 30 seconds
    pub max_polling_interval: Duration,

    /// Timeout for UPnP subscriptions
    /// Default: 1800 seconds (30 minutes)
    pub subscription_timeout: Duration,

    /// Buffer size for event channels
    /// Default: 1000
    pub event_buffer_size: usize,

    /// Maximum number of concurrent polling tasks
    /// Default: 50
    pub max_concurrent_polls: usize,

    /// Enable proactive firewall detection
    /// Default: true
    pub enable_proactive_firewall_detection: bool,

    /// Timeout for firewall detection operations
    /// Default: 10 seconds
    pub firewall_detection_timeout: Duration,

    /// Number of retries for firewall detection
    /// Default: 2
    pub firewall_detection_retries: u32,

    /// Enable fallback to basic firewall detection if UPnP detection fails
    /// Default: true
    pub firewall_detection_fallback: bool,

    /// Timeout for waiting for first event to determine firewall status
    /// Default: 15 seconds
    pub firewall_event_wait_timeout: Duration,

    /// Enable per-device firewall detection caching
    /// Default: true
    pub enable_firewall_caching: bool,

    /// Maximum number of cached device firewall states
    /// Default: 100
    pub max_cached_device_states: usize,

    /// Cooldown period between resync events to prevent spam
    /// Default: 30 seconds
    pub resync_cooldown: Duration,

    /// Maximum number of registrations allowed
    /// Default: 1000
    pub max_registrations: usize,

    /// Enable adaptive polling intervals based on change frequency
    /// Default: true
    pub adaptive_polling: bool,

    /// Threshold for subscription renewal (time before expiration)
    /// Default: 5 minutes
    pub renewal_threshold: Duration,
}

impl Default for BrokerConfig {
    fn default() -> Self {
        Self {
            callback_port_range: (3400, 3500),
            event_timeout: Duration::from_secs(30),
            polling_activation_delay: Duration::from_secs(5),
            base_polling_interval: Duration::from_secs(5),
            max_polling_interval: Duration::from_secs(30),
            subscription_timeout: Duration::from_secs(1800), // 30 minutes
            event_buffer_size: 1000,
            max_concurrent_polls: 50,
            enable_proactive_firewall_detection: true,
            firewall_detection_timeout: Duration::from_secs(10),
            firewall_detection_retries: 2,
            firewall_detection_fallback: true,
            firewall_event_wait_timeout: Duration::from_secs(15),
            enable_firewall_caching: true,
            max_cached_device_states: 100,
            resync_cooldown: Duration::from_secs(30),
            max_registrations: 1000,
            adaptive_polling: true,
            renewal_threshold: Duration::from_secs(300), // 5 minutes
        }
    }
}

impl BrokerConfig {
    /// Create a new BrokerConfig with default values
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a BrokerConfig optimized for fast polling fallback
    pub fn fast_polling() -> Self {
        Self {
            base_polling_interval: Duration::from_secs(2),
            max_polling_interval: Duration::from_secs(10),
            polling_activation_delay: Duration::from_secs(1),
            event_timeout: Duration::from_secs(15),
            firewall_detection_timeout: Duration::from_secs(5),
            firewall_event_wait_timeout: Duration::from_secs(5), // Faster detection
            ..Default::default()
        }
    }

    /// Create a BrokerConfig optimized for resource efficiency
    pub fn resource_efficient() -> Self {
        Self {
            base_polling_interval: Duration::from_secs(10),
            max_polling_interval: Duration::from_secs(60),
            event_buffer_size: 100,
            max_concurrent_polls: 10,
            max_registrations: 100,
            max_cached_device_states: 50, // Fewer cached devices for resource efficiency
            ..Default::default()
        }
    }

    /// Create a BrokerConfig with firewall detection disabled
    pub fn no_firewall_detection() -> Self {
        Self {
            enable_proactive_firewall_detection: false,
            firewall_detection_fallback: false,
            ..Default::default()
        }
    }

    /// Validate the configuration and return any issues
    pub fn validate(&self) -> Result<(), crate::BrokerError> {
        if self.callback_port_range.0 >= self.callback_port_range.1 {
            return Err(crate::BrokerError::Configuration(
                "Invalid callback port range: start must be less than end".to_string(),
            ));
        }

        if self.base_polling_interval >= self.max_polling_interval {
            return Err(crate::BrokerError::Configuration(
                "Invalid polling interval: base must be less than max".to_string(),
            ));
        }

        if self.event_buffer_size == 0 {
            return Err(crate::BrokerError::Configuration(
                "Event buffer size must be greater than 0".to_string(),
            ));
        }

        if self.max_concurrent_polls == 0 {
            return Err(crate::BrokerError::Configuration(
                "Max concurrent polls must be greater than 0".to_string(),
            ));
        }

        if self.max_registrations == 0 {
            return Err(crate::BrokerError::Configuration(
                "Max registrations must be greater than 0".to_string(),
            ));
        }

        if self.max_cached_device_states == 0 {
            return Err(crate::BrokerError::Configuration(
                "Max cached device states must be greater than 0".to_string(),
            ));
        }

        if self.firewall_event_wait_timeout == Duration::ZERO {
            return Err(crate::BrokerError::Configuration(
                "Firewall event wait timeout must be greater than 0".to_string(),
            ));
        }

        Ok(())
    }

    /// Builder pattern methods for fluent configuration

    pub fn with_callback_ports(mut self, start: u16, end: u16) -> Self {
        self.callback_port_range = (start, end);
        self
    }

    pub fn with_polling_interval(mut self, base: Duration, max: Duration) -> Self {
        self.base_polling_interval = base;
        self.max_polling_interval = max;
        self
    }

    pub fn with_event_timeout(mut self, timeout: Duration) -> Self {
        self.event_timeout = timeout;
        self
    }

    pub fn with_buffer_size(mut self, size: usize) -> Self {
        self.event_buffer_size = size;
        self
    }

    pub fn with_firewall_detection(mut self, enabled: bool) -> Self {
        self.enable_proactive_firewall_detection = enabled;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = BrokerConfig::default();
        assert_eq!(config.callback_port_range, (3400, 3500));
        assert_eq!(config.event_timeout, Duration::from_secs(30));
        assert!(config.enable_proactive_firewall_detection);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_config_validation() {
        let invalid_config = BrokerConfig {
            callback_port_range: (3500, 3400), // Invalid: start > end
            ..Default::default()
        };
        assert!(invalid_config.validate().is_err());

        let invalid_polling = BrokerConfig {
            base_polling_interval: Duration::from_secs(30),
            max_polling_interval: Duration::from_secs(10), // Invalid: base > max
            ..Default::default()
        };
        assert!(invalid_polling.validate().is_err());
    }

    #[test]
    fn test_config_presets() {
        let fast = BrokerConfig::fast_polling();
        assert_eq!(fast.base_polling_interval, Duration::from_secs(2));
        assert!(fast.validate().is_ok());

        let efficient = BrokerConfig::resource_efficient();
        assert_eq!(efficient.max_concurrent_polls, 10);
        assert!(efficient.validate().is_ok());

        let no_fw = BrokerConfig::no_firewall_detection();
        assert!(!no_fw.enable_proactive_firewall_detection);
        assert!(no_fw.validate().is_ok());
    }

    #[test]
    fn test_builder_pattern() {
        let config = BrokerConfig::new()
            .with_callback_ports(4000, 4100)
            .with_polling_interval(Duration::from_secs(3), Duration::from_secs(15))
            .with_event_timeout(Duration::from_secs(45))
            .with_buffer_size(2000)
            .with_firewall_detection(false);

        assert_eq!(config.callback_port_range, (4000, 4100));
        assert_eq!(config.base_polling_interval, Duration::from_secs(3));
        assert_eq!(config.event_buffer_size, 2000);
        assert!(!config.enable_proactive_firewall_detection);
        assert!(config.validate().is_ok());
    }
}