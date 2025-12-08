//! Subscription trait and related types for managing UPnP event subscriptions.

use crate::error::SubscriptionError;
use crate::types::{ServiceType, SpeakerId};
use std::time::Duration;

/// Trait representing an active UPnP event subscription.
///
/// Implementations of this trait handle the lifecycle of a single subscription,
/// including renewal and unsubscription operations. The trait is designed to be
/// service-agnostic, with service-specific details handled by strategy implementations.
///
/// # Thread Safety
///
/// Implementations must be `Send + Sync` to allow the broker to manage subscriptions
/// across async tasks.
pub trait Subscription: Send + Sync {
    /// Get the UPnP subscription ID.
    ///
    /// This ID is assigned by the UPnP device when the subscription is created
    /// and is used to identify the subscription in subsequent operations.
    fn subscription_id(&self) -> &str;

    /// Renew the subscription before it expires.
    ///
    /// This method should send a SUBSCRIBE request with the existing subscription ID
    /// to extend the subscription timeout. The implementation should update internal
    /// state to reflect the new expiration time.
    ///
    /// # Errors
    ///
    /// Returns `SubscriptionError::RenewalFailed` if the renewal request fails.
    /// Returns `SubscriptionError::NetworkError` if a network error occurs.
    /// Returns `SubscriptionError::Expired` if the subscription has already expired.
    fn renew(&mut self) -> Result<(), SubscriptionError>;

    /// Unsubscribe and clean up the subscription.
    ///
    /// This method should send an UNSUBSCRIBE request to the UPnP device and
    /// mark the subscription as inactive. After calling this method, the subscription
    /// should not be used for any further operations.
    ///
    /// # Errors
    ///
    /// Returns `SubscriptionError::UnsubscribeFailed` if the unsubscribe request fails.
    /// Returns `SubscriptionError::NetworkError` if a network error occurs.
    fn unsubscribe(&mut self) -> Result<(), SubscriptionError>;

    /// Check if the subscription is still active.
    ///
    /// A subscription is considered active if it has not expired and has not been
    /// explicitly unsubscribed.
    fn is_active(&self) -> bool;

    /// Get the time until renewal is needed.
    ///
    /// Returns `Some(duration)` if the subscription is active and renewal is needed
    /// within the configured threshold. Returns `None` if the subscription is not active
    /// or renewal is not yet needed.
    ///
    /// The broker uses this method to determine when to trigger automatic renewal.
    fn time_until_renewal(&self) -> Option<Duration>;

    /// Get the speaker ID this subscription is for.
    fn speaker_id(&self) -> &SpeakerId;

    /// Get the service type this subscription is for.
    fn service_type(&self) -> ServiceType;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    /// Mock subscription for testing
    struct MockSubscription {
        id: String,
        speaker_id: SpeakerId,
        service_type: ServiceType,
        active: bool,
        expires_at: SystemTime,
        renewal_threshold: Duration,
    }

    impl MockSubscription {
        fn new(
            id: String,
            speaker_id: SpeakerId,
            service_type: ServiceType,
            timeout: Duration,
        ) -> Self {
            Self {
                id,
                speaker_id,
                service_type,
                active: true,
                expires_at: SystemTime::now() + timeout,
                renewal_threshold: Duration::from_secs(300),
            }
        }
    }

    impl Subscription for MockSubscription {
        fn subscription_id(&self) -> &str {
            &self.id
        }

        fn renew(&mut self) -> Result<(), SubscriptionError> {
            if !self.active {
                return Err(SubscriptionError::Expired);
            }
            // Simulate renewal by extending expiration
            self.expires_at = SystemTime::now() + Duration::from_secs(1800);
            Ok(())
        }

        fn unsubscribe(&mut self) -> Result<(), SubscriptionError> {
            if !self.active {
                return Err(SubscriptionError::UnsubscribeFailed(
                    "Already unsubscribed".to_string(),
                ));
            }
            self.active = false;
            Ok(())
        }

        fn is_active(&self) -> bool {
            self.active && SystemTime::now() < self.expires_at
        }

        fn time_until_renewal(&self) -> Option<Duration> {
            if !self.active {
                return None;
            }

            let now = SystemTime::now();
            if now >= self.expires_at {
                return Some(Duration::ZERO);
            }

            let time_until_expiry = self.expires_at.duration_since(now).ok()?;
            if time_until_expiry <= self.renewal_threshold {
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

    #[test]
    fn test_subscription_trait_basic_operations() {
        let sub = MockSubscription::new(
            "uuid:sub-123".to_string(),
            SpeakerId::new("speaker1"),
            ServiceType::AVTransport,
            Duration::from_secs(1800),
        );

        assert_eq!(sub.subscription_id(), "uuid:sub-123");
        assert_eq!(sub.speaker_id().as_str(), "speaker1");
        assert_eq!(sub.service_type(), ServiceType::AVTransport);
        assert!(sub.is_active());
    }

    #[test]
    fn test_subscription_renewal() {
        let mut sub = MockSubscription::new(
            "uuid:sub-123".to_string(),
            SpeakerId::new("speaker1"),
            ServiceType::AVTransport,
            Duration::from_secs(1800),
        );

        assert!(sub.renew().is_ok());
        assert!(sub.is_active());
    }

    #[test]
    fn test_subscription_unsubscribe() {
        let mut sub = MockSubscription::new(
            "uuid:sub-123".to_string(),
            SpeakerId::new("speaker1"),
            ServiceType::AVTransport,
            Duration::from_secs(1800),
        );

        assert!(sub.unsubscribe().is_ok());
        assert!(!sub.is_active());

        // Second unsubscribe should fail
        assert!(sub.unsubscribe().is_err());
    }

    #[test]
    fn test_subscription_renewal_after_unsubscribe() {
        let mut sub = MockSubscription::new(
            "uuid:sub-123".to_string(),
            SpeakerId::new("speaker1"),
            ServiceType::AVTransport,
            Duration::from_secs(1800),
        );

        sub.unsubscribe().unwrap();
        let result = sub.renew();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), SubscriptionError::Expired));
    }

    #[test]
    fn test_time_until_renewal_not_needed() {
        let sub = MockSubscription::new(
            "uuid:sub-123".to_string(),
            SpeakerId::new("speaker1"),
            ServiceType::AVTransport,
            Duration::from_secs(1800), // 30 minutes
        );

        // Renewal threshold is 5 minutes, so with 30 minutes left, no renewal needed
        assert!(sub.time_until_renewal().is_none());
    }

    #[test]
    fn test_time_until_renewal_needed() {
        let sub = MockSubscription::new(
            "uuid:sub-123".to_string(),
            SpeakerId::new("speaker1"),
            ServiceType::AVTransport,
            Duration::from_secs(200), // 200 seconds (less than 5 minute threshold)
        );

        // Should need renewal since we're within the threshold
        let time_until = sub.time_until_renewal();
        assert!(time_until.is_some());
        assert!(time_until.unwrap() <= Duration::from_secs(200));
    }

    #[test]
    fn test_time_until_renewal_inactive() {
        let mut sub = MockSubscription::new(
            "uuid:sub-123".to_string(),
            SpeakerId::new("speaker1"),
            ServiceType::AVTransport,
            Duration::from_secs(200),
        );

        sub.unsubscribe().unwrap();
        assert!(sub.time_until_renewal().is_none());
    }
}
