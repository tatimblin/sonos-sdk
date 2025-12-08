//! Automatic subscription renewal management.
//!
//! This module contains the RenewalManager which handles:
//! - Running a background task for periodic renewal checks
//! - Implementing retry logic with exponential backoff
//! - Handling subscription expiration after failed renewals
//! - Emitting renewal lifecycle events

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, RwLock};
use tokio::task::JoinHandle;
use tokio::time::{interval, timeout};

use crate::error::Result;
use crate::event::Event;
use crate::types::{BrokerConfig, SubscriptionKey};

use super::subscription_manager::ActiveSubscription;

/// Manager for automatic subscription renewal.
///
/// The RenewalManager runs a background task that periodically checks all active
/// subscriptions and renews those that are approaching expiration. It implements
/// exponential backoff retry logic for failed renewals and handles subscription
/// expiration after all retry attempts are exhausted.
///
/// # Background Task
///
/// The renewal task runs every 60 seconds and:
/// 1. Checks each subscription's `time_until_renewal()`
/// 2. Renews subscriptions within the renewal threshold
/// 3. Retries failed renewals with exponential backoff
/// 4. Expires subscriptions after max retry attempts
///
/// # Shutdown
///
/// The manager provides graceful shutdown with a 5-second timeout. If the background
/// task doesn't complete within the timeout, it is forcefully terminated.
pub struct RenewalManager {
    /// Handle to the background renewal task
    background_task: Option<JoinHandle<()>>,
    /// Channel for signaling shutdown
    shutdown_tx: Option<mpsc::Sender<()>>,
}

impl RenewalManager {
    /// Start the renewal manager with a background task.
    ///
    /// This method spawns a background task that periodically checks and renews
    /// subscriptions. The task runs until `shutdown()` is called.
    ///
    /// # Arguments
    ///
    /// * `subscriptions` - Shared subscription state
    /// * `event_sender` - Channel sender for emitting events
    /// * `config` - Broker configuration
    ///
    /// # Returns
    ///
    /// Returns a new RenewalManager with the background task running.
    pub fn start(
        subscriptions: Arc<RwLock<HashMap<SubscriptionKey, ActiveSubscription>>>,
        event_sender: mpsc::Sender<Event>,
        config: BrokerConfig,
    ) -> Self {
        let (shutdown_tx, shutdown_rx) = mpsc::channel(1);

        let background_task = tokio::spawn(Self::renewal_task(
            subscriptions,
            event_sender,
            config,
            shutdown_rx,
        ));

        Self {
            background_task: Some(background_task),
            shutdown_tx: Some(shutdown_tx),
        }
    }

    /// Shutdown the renewal manager.
    ///
    /// This method signals the background task to stop and waits up to 5 seconds
    /// for it to complete. If the task doesn't complete within the timeout, it is
    /// forcefully terminated.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if shutdown completed successfully within the timeout.
    ///
    /// # Errors
    ///
    /// Returns an error if the shutdown times out or the task panicked.
    pub async fn shutdown(mut self) -> Result<()> {
        // Signal shutdown
        if let Some(shutdown_tx) = self.shutdown_tx.take() {
            let _ = shutdown_tx.send(()).await;
        }

        // Wait for background task with timeout
        if let Some(task) = self.background_task.take() {
            match timeout(Duration::from_secs(5), task).await {
                Ok(Ok(())) => Ok(()),
                Ok(Err(e)) => Err(crate::error::BrokerError::ShutdownError(format!(
                    "Renewal task panicked: {e}"
                ))),
                Err(_) => Err(crate::error::BrokerError::ShutdownError(
                    "Renewal task shutdown timed out after 5 seconds".to_string(),
                )),
            }
        } else {
            Ok(())
        }
    }

    /// Background task for periodic renewal checks.
    ///
    /// This task runs every 60 seconds and checks all subscriptions for renewal needs.
    async fn renewal_task(
        subscriptions: Arc<RwLock<HashMap<SubscriptionKey, ActiveSubscription>>>,
        event_sender: mpsc::Sender<Event>,
        config: BrokerConfig,
        mut shutdown_rx: mpsc::Receiver<()>,
    ) {
        let mut check_interval = interval(Duration::from_secs(60));

        loop {
            tokio::select! {
                _ = check_interval.tick() => {
                    Self::check_and_renew_subscriptions(
                        &subscriptions,
                        &event_sender,
                        &config,
                    ).await;
                }
                _ = shutdown_rx.recv() => {
                    // Shutdown signal received
                    break;
                }
            }
        }
    }

    /// Check all subscriptions and renew those needing renewal.
    ///
    /// This method iterates through all active subscriptions and renews those
    /// that are within the renewal threshold.
    async fn check_and_renew_subscriptions(
        subscriptions: &Arc<RwLock<HashMap<SubscriptionKey, ActiveSubscription>>>,
        event_sender: &mpsc::Sender<Event>,
        config: &BrokerConfig,
    ) {
        // Get list of subscriptions that need renewal
        let subscriptions_to_renew: Vec<SubscriptionKey> = {
            let subs = subscriptions.read().await;
            subs.iter()
                .filter_map(|(key, active_sub)| {
                    // Check if subscription needs renewal
                    if let Some(time_until) = active_sub.subscription.time_until_renewal() {
                        if time_until <= config.renewal_threshold {
                            return Some(key.clone());
                        }
                    }
                    None
                })
                .collect()
        };

        // Renew each subscription with retry logic
        for key in subscriptions_to_renew {
            Self::renew_subscription_with_retry(
                subscriptions,
                event_sender,
                config,
                &key,
            ).await;
        }
    }

    /// Renew a subscription with exponential backoff retry logic.
    ///
    /// This method attempts to renew a subscription up to `max_retry_attempts` times,
    /// with exponential backoff between attempts. If all attempts fail, the subscription
    /// is marked as expired.
    ///
    /// # Retry Algorithm
    ///
    /// - Attempt 1: Immediate
    /// - Attempt 2: Wait `retry_backoff_base * 2^0` (e.g., 2 seconds)
    /// - Attempt 3: Wait `retry_backoff_base * 2^1` (e.g., 4 seconds)
    /// - etc.
    async fn renew_subscription_with_retry(
        subscriptions: &Arc<RwLock<HashMap<SubscriptionKey, ActiveSubscription>>>,
        event_sender: &mpsc::Sender<Event>,
        config: &BrokerConfig,
        key: &SubscriptionKey,
    ) {
        for attempt in 1..=config.max_retry_attempts {
            // Try to renew the subscription
            let renewal_result = {
                let mut subs = subscriptions.write().await;
                if let Some(active_sub) = subs.get_mut(key) {
                    active_sub.subscription.renew()
                } else {
                    // Subscription was removed, nothing to do
                    return;
                }
            };

            match renewal_result {
                Ok(()) => {
                    // Renewal successful, emit event
                    let _ = event_sender
                        .send(Event::SubscriptionRenewed {
                            speaker_id: key.speaker_id.clone(),
                            service_type: key.service_type,
                        })
                        .await;
                    return;
                }
                Err(e) => {
                    eprintln!(
                        "Warning: Failed to renew subscription {}/{:?} (attempt {}/{}): {}",
                        key.speaker_id.as_str(),
                        key.service_type,
                        attempt,
                        config.max_retry_attempts,
                        e
                    );

                    // If this was the last attempt, handle expiration
                    if attempt >= config.max_retry_attempts {
                        Self::handle_subscription_expiration(
                            subscriptions,
                            event_sender,
                            key,
                        ).await;
                        return;
                    }

                    // Calculate backoff duration: base * 2^(attempt-1)
                    let backoff_multiplier = 2u32.pow(attempt - 1);
                    let backoff_duration = config.retry_backoff_base * backoff_multiplier;

                    // Wait before next retry
                    tokio::time::sleep(backoff_duration).await;
                }
            }
        }
    }

    /// Handle subscription expiration after all renewal attempts failed.
    ///
    /// This method removes the expired subscription from the map and emits
    /// a `SubscriptionExpired` event.
    async fn handle_subscription_expiration(
        subscriptions: &Arc<RwLock<HashMap<SubscriptionKey, ActiveSubscription>>>,
        event_sender: &mpsc::Sender<Event>,
        key: &SubscriptionKey,
    ) {
        // Remove the expired subscription
        {
            let mut subs = subscriptions.write().await;
            subs.remove(key);
        }

        // Emit SubscriptionExpired event
        let _ = event_sender
            .send(Event::SubscriptionExpired {
                speaker_id: key.speaker_id.clone(),
                service_type: key.service_type,
            })
            .await;

        eprintln!(
            "Subscription expired: {}/{:?}",
            key.speaker_id.as_str(),
            key.service_type
        );
    }
}
