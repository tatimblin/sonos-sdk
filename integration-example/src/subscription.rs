//! Subscription management module.
//!
//! This module provides functionality for creating and managing subscriptions
//! to AVTransport service for target devices, with proper error handling
//! and reporting.

use anyhow::{Context, Result};
use sonos_stream::{EventBroker, ServiceType, Speaker};
use std::time::Duration;
use tracing::{info, warn, error, debug};

/// Configuration for subscription management
#[derive(Debug, Clone)]
pub struct SubscriptionConfig {
    /// Maximum time to wait for subscription establishment
    pub establishment_timeout: Duration,
    /// Maximum number of retry attempts for failed subscriptions
    pub max_retry_attempts: u32,
    /// Base delay between retry attempts
    pub retry_delay: Duration,
    /// Whether to retry on network errors
    pub retry_on_network_errors: bool,
}

impl Default for SubscriptionConfig {
    fn default() -> Self {
        Self {
            establishment_timeout: Duration::from_secs(30),
            max_retry_attempts: 3,
            retry_delay: Duration::from_secs(2),
            retry_on_network_errors: true,
        }
    }
}

/// Result of a subscription attempt
#[derive(Debug, Clone)]
pub enum SubscriptionResult {
    /// Subscription was established successfully
    Success {
        speaker_id: String,
        service_type: ServiceType,
    },
    /// Subscription failed after all retry attempts
    Failed {
        speaker_id: String,
        service_type: ServiceType,
        error: String,
        attempts: u32,
    },
    /// Subscription timed out
    Timeout {
        speaker_id: String,
        service_type: ServiceType,
        timeout: Duration,
    },
}

/// Creates a subscription to AVTransport service for the target device.
///
/// This function handles the complete subscription process:
/// - Creates subscription to AVTransport service for target device
/// - Handles subscription establishment and failure cases
/// - Implements proper error reporting for subscription issues
/// - Provides retry logic for transient failures
///
/// # Arguments
///
/// * `broker` - The event broker to use for subscription
/// * `speaker` - The target speaker to subscribe to
/// * `config` - Configuration for subscription behavior
///
/// # Returns
///
/// Returns the subscription result indicating success or failure details.
///
/// # Example
///
/// ```rust,no_run
/// use integration_example::subscription::{create_av_transport_subscription, SubscriptionConfig};
/// use sonos_stream::{EventBroker, Speaker, SpeakerId};
/// use std::net::IpAddr;
///
/// # async fn example(broker: &mut EventBroker) -> anyhow::Result<()> {
/// let speaker = Speaker::new(
///     SpeakerId::new("RINCON_000E58A0123456"),
///     "192.168.1.100".parse::<IpAddr>()?,
///     "Sonos Roam 2".to_string(),
///     "Living Room".to_string(),
/// );
/// 
/// let config = SubscriptionConfig::default();
/// let result = create_av_transport_subscription(broker, &speaker, config).await?;
/// # Ok(())
/// # }
/// ```
pub async fn create_av_transport_subscription(
    broker: &mut EventBroker,
    speaker: &Speaker,
    config: SubscriptionConfig,
) -> Result<SubscriptionResult> {
    let speaker_id = speaker.id.to_string();
    let service_type = ServiceType::AVTransport;
    
    info!(
        "Creating AVTransport subscription for speaker '{}' ({})",
        speaker.name, speaker_id
    );

    debug!(
        "Subscription config: timeout={}s, max_retries={}, retry_delay={}s",
        config.establishment_timeout.as_secs(),
        config.max_retry_attempts,
        config.retry_delay.as_secs()
    );

    let mut attempts = 0;
    let mut last_error = None;

    while attempts < config.max_retry_attempts {
        attempts += 1;
        
        info!(
            "Subscription attempt {} of {} for speaker '{}'",
            attempts, config.max_retry_attempts, speaker.name
        );

        match attempt_subscription(broker, speaker, &config).await {
            Ok(result) => {
                info!(
                    "Successfully created AVTransport subscription for speaker '{}' on attempt {}",
                    speaker.name, attempts
                );
                return Ok(result);
            }
            Err(error) => {
                last_error = Some(error.to_string());
                warn!(
                    "Subscription attempt {} failed for speaker '{}': {}",
                    attempts, speaker.name, error
                );

                // Check if we should retry based on error type
                if !should_retry_error(&error, &config) {
                    error!(
                        "Non-retryable error for speaker '{}': {}",
                        speaker.name, error
                    );
                    break;
                }

                // Wait before next attempt (unless this was the last attempt)
                if attempts < config.max_retry_attempts {
                    debug!(
                        "Waiting {}s before retry attempt {} for speaker '{}'",
                        config.retry_delay.as_secs(),
                        attempts + 1,
                        speaker.name
                    );
                    tokio::time::sleep(config.retry_delay).await;
                }
            }
        }
    }

    let final_error = last_error.unwrap_or_else(|| "Unknown error".to_string());
    error!(
        "Failed to create AVTransport subscription for speaker '{}' after {} attempts: {}",
        speaker.name, attempts, final_error
    );

    Ok(SubscriptionResult::Failed {
        speaker_id,
        service_type,
        error: final_error,
        attempts,
    })
}

/// Attempts a single subscription operation with timeout.
async fn attempt_subscription(
    broker: &mut EventBroker,
    speaker: &Speaker,
    config: &SubscriptionConfig,
) -> Result<SubscriptionResult> {
    let speaker_id = speaker.id.to_string();
    let service_type = ServiceType::AVTransport;

    // Create the subscription with timeout
    let subscription_future = broker.subscribe(speaker, service_type);
    
    match tokio::time::timeout(config.establishment_timeout, subscription_future).await {
        Ok(Ok(())) => {
            debug!(
                "Subscription request sent successfully for speaker '{}'",
                speaker.name
            );
            Ok(SubscriptionResult::Success {
                speaker_id,
                service_type,
            })
        }
        Ok(Err(broker_error)) => {
            Err(anyhow::anyhow!(
                "Broker error during subscription: {}",
                broker_error
            ))
        }
        Err(_timeout_error) => {
            warn!(
                "Subscription timed out after {}s for speaker '{}'",
                config.establishment_timeout.as_secs(),
                speaker.name
            );
            Ok(SubscriptionResult::Timeout {
                speaker_id,
                service_type,
                timeout: config.establishment_timeout,
            })
        }
    }
}

/// Determines if an error should trigger a retry attempt.
fn should_retry_error(error: &anyhow::Error, config: &SubscriptionConfig) -> bool {
    let error_msg = error.to_string().to_lowercase();
    
    // Check for network-related errors
    let is_network_error = error_msg.contains("network") 
        || error_msg.contains("connection") 
        || error_msg.contains("timeout")
        || error_msg.contains("unreachable")
        || error_msg.contains("refused");
    
    // Only retry network errors if configured to do so
    if is_network_error {
        if config.retry_on_network_errors {
            debug!("Network error detected, will retry: {}", error);
            return true;
        } else {
            debug!("Network error detected, but retries disabled: {}", error);
            return false;
        }
    }

    // Retry on temporary server errors
    if error_msg.contains("server error") 
        || error_msg.contains("service unavailable")
        || error_msg.contains("busy") {
        debug!("Temporary server error detected, will retry: {}", error);
        return true;
    }

    // Don't retry on configuration or permanent errors
    if error_msg.contains("configuration") 
        || error_msg.contains("invalid") 
        || error_msg.contains("not found")
        || error_msg.contains("unauthorized")
        || error_msg.contains("forbidden") {
        debug!("Permanent error detected, will not retry: {}", error);
        return false;
    }

    // Default to retry for unknown errors
    debug!("Unknown error type, will retry: {}", error);
    true
}

/// Unsubscribes from AVTransport service for the target device.
///
/// This function handles cleanup of subscriptions when shutting down
/// or when the device becomes unavailable.
///
/// # Arguments
///
/// * `broker` - The event broker to use for unsubscription
/// * `speaker` - The target speaker to unsubscribe from
///
/// # Returns
///
/// Returns Ok(()) on successful unsubscription, Err on failure.
///
/// # Example
///
/// ```rust,no_run
/// use integration_example::subscription::unsubscribe_av_transport;
/// use sonos_stream::{EventBroker, Speaker, SpeakerId};
/// use std::net::IpAddr;
///
/// # async fn example(broker: &mut EventBroker) -> anyhow::Result<()> {
/// let speaker = Speaker::new(
///     SpeakerId::new("RINCON_000E58A0123456"),
///     "192.168.1.100".parse::<IpAddr>()?,
///     "Sonos Roam 2".to_string(),
///     "Living Room".to_string(),
/// );
/// 
/// unsubscribe_av_transport(broker, &speaker).await?;
/// # Ok(())
/// # }
/// ```
pub async fn unsubscribe_av_transport(
    broker: &mut EventBroker,
    speaker: &Speaker,
) -> Result<()> {
    let speaker_id = speaker.id.to_string();
    let service_type = ServiceType::AVTransport;

    info!(
        "Unsubscribing from AVTransport service for speaker '{}' ({})",
        speaker.name, speaker_id
    );

    broker
        .unsubscribe(speaker, service_type)
        .await
        .context("Failed to unsubscribe from AVTransport service")?;

    info!(
        "Successfully unsubscribed from AVTransport service for speaker '{}'",
        speaker.name
    );

    Ok(())
}

/// Provides detailed error reporting for subscription issues.
///
/// This function analyzes subscription errors and provides helpful
/// diagnostic information and potential solutions.
///
/// # Arguments
///
/// * `result` - The subscription result to analyze
/// * `speaker` - The speaker that was being subscribed to
///
/// # Returns
///
/// Returns a detailed error report with diagnostic information.
pub fn generate_subscription_error_report(
    result: &SubscriptionResult,
    speaker: &Speaker,
) -> String {
    match result {
        SubscriptionResult::Success { .. } => {
            "Subscription successful".to_string()
        }
        SubscriptionResult::Failed { error, attempts, .. } => {
            let mut report = format!(
                "Subscription failed for speaker '{}' ({}) after {} attempts.\n",
                speaker.name, speaker.id, attempts
            );
            
            report.push_str(&format!("Last error: {}\n", error));
            report.push_str("\nPossible causes and solutions:\n");
            
            let error_lower = error.to_lowercase();
            
            if error_lower.contains("network") || error_lower.contains("connection") {
                report.push_str("• Network connectivity issue - check network connection and firewall settings\n");
                report.push_str("• Device may be on a different network segment\n");
                report.push_str("• Try pinging the device IP address to verify connectivity\n");
            }
            
            if error_lower.contains("timeout") {
                report.push_str("• Device may be busy or unresponsive\n");
                report.push_str("• Network latency may be high - try increasing timeout\n");
                report.push_str("• Device may be in sleep mode or powered off\n");
            }
            
            if error_lower.contains("port") || error_lower.contains("bind") {
                report.push_str("• Callback server port range may be in use\n");
                report.push_str("• Try using a different port range\n");
                report.push_str("• Check for other applications using the same ports\n");
            }
            
            if error_lower.contains("service") {
                report.push_str("• AVTransport service may not be available on this device\n");
                report.push_str("• Device may not support UPnP event subscriptions\n");
                report.push_str("• Device firmware may need updating\n");
            }
            
            report.push_str(&format!("\nDevice details:\n"));
            report.push_str(&format!("• Name: {}\n", speaker.name));
            report.push_str(&format!("• Room: {}\n", speaker.room));
            report.push_str(&format!("• IP: {}\n", speaker.ip));
            report.push_str(&format!("• ID: {}\n", speaker.id));
            
            report
        }
        SubscriptionResult::Timeout { timeout, .. } => {
            format!(
                "Subscription timed out after {}s for speaker '{}' ({}).\n\
                \nPossible causes:\n\
                • Device is unresponsive or busy\n\
                • Network latency is high\n\
                • Device is in sleep mode\n\
                \nSolutions:\n\
                • Increase the subscription timeout\n\
                • Check device status and network connectivity\n\
                • Try again when device is more responsive",
                timeout.as_secs(),
                speaker.name,
                speaker.id
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sonos_stream::{Speaker, SpeakerId};
    use std::net::IpAddr;

    fn create_test_speaker() -> Speaker {
        Speaker::new(
            SpeakerId::new("RINCON_TEST123456"),
            "192.168.1.100".parse::<IpAddr>().unwrap(),
            "Test Speaker".to_string(),
            "Test Room".to_string(),
        )
    }

    #[test]
    fn test_subscription_config_default() {
        let config = SubscriptionConfig::default();
        assert_eq!(config.establishment_timeout, Duration::from_secs(30));
        assert_eq!(config.max_retry_attempts, 3);
        assert_eq!(config.retry_delay, Duration::from_secs(2));
        assert!(config.retry_on_network_errors);
    }

    #[test]
    fn test_should_retry_error_network_errors() {
        let config = SubscriptionConfig::default();
        
        let network_errors = vec![
            anyhow::anyhow!("Network connection failed"),
            anyhow::anyhow!("Connection timeout"),
            anyhow::anyhow!("Host unreachable"),
            anyhow::anyhow!("Connection refused"),
        ];
        
        for error in network_errors {
            assert!(should_retry_error(&error, &config), "Should retry network error: {}", error);
        }
    }

    #[test]
    fn test_should_retry_error_server_errors() {
        let config = SubscriptionConfig::default();
        
        let server_errors = vec![
            anyhow::anyhow!("Server error occurred"),
            anyhow::anyhow!("Service unavailable"),
            anyhow::anyhow!("Server is busy"),
        ];
        
        for error in server_errors {
            assert!(should_retry_error(&error, &config), "Should retry server error: {}", error);
        }
    }

    #[test]
    fn test_should_not_retry_error_permanent_errors() {
        let config = SubscriptionConfig::default();
        
        let permanent_errors = vec![
            anyhow::anyhow!("Configuration error"),
            anyhow::anyhow!("Invalid request"),
            anyhow::anyhow!("Device not found"),
            anyhow::anyhow!("Unauthorized access"),
            anyhow::anyhow!("Forbidden operation"),
        ];
        
        for error in permanent_errors {
            assert!(!should_retry_error(&error, &config), "Should not retry permanent error: {}", error);
        }
    }

    #[test]
    fn test_should_retry_error_network_disabled() {
        let mut config = SubscriptionConfig::default();
        config.retry_on_network_errors = false;
        
        let network_error = anyhow::anyhow!("Network connection failed");
        assert!(!should_retry_error(&network_error, &config), "Should not retry network error when disabled");
    }

    #[test]
    fn test_generate_subscription_error_report_success() {
        let speaker = create_test_speaker();
        let result = SubscriptionResult::Success {
            speaker_id: speaker.id.to_string(),
            service_type: ServiceType::AVTransport,
        };
        
        let report = generate_subscription_error_report(&result, &speaker);
        assert_eq!(report, "Subscription successful");
    }

    #[test]
    fn test_generate_subscription_error_report_failed() {
        let speaker = create_test_speaker();
        let result = SubscriptionResult::Failed {
            speaker_id: speaker.id.to_string(),
            service_type: ServiceType::AVTransport,
            error: "Network connection failed".to_string(),
            attempts: 3,
        };
        
        let report = generate_subscription_error_report(&result, &speaker);
        assert!(report.contains("Subscription failed"));
        assert!(report.contains("Test Speaker"));
        assert!(report.contains("3 attempts"));
        assert!(report.contains("Network connection failed"));
        assert!(report.contains("Network connectivity issue"));
    }

    #[test]
    fn test_generate_subscription_error_report_timeout() {
        let speaker = create_test_speaker();
        let result = SubscriptionResult::Timeout {
            speaker_id: speaker.id.to_string(),
            service_type: ServiceType::AVTransport,
            timeout: Duration::from_secs(30),
        };
        
        let report = generate_subscription_error_report(&result, &speaker);
        assert!(report.contains("timed out after 30s"));
        assert!(report.contains("Test Speaker"));
        assert!(report.contains("Device is unresponsive"));
    }

    #[test]
    fn test_subscription_result_debug() {
        let result = SubscriptionResult::Success {
            speaker_id: "test_id".to_string(),
            service_type: ServiceType::AVTransport,
        };
        
        let debug_str = format!("{:?}", result);
        assert!(debug_str.contains("Success"));
        assert!(debug_str.contains("test_id"));
        assert!(debug_str.contains("AVTransport"));
    }
}