//! Polling task scheduler and management
//!
//! This module provides intelligent polling task management with support for
//! adaptive intervals, graceful shutdown, and coordination with the event system.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::{mpsc, RwLock};
use tokio::task::JoinHandle;

use crate::error::{PollingError, PollingResult};
use crate::events::types::{EnrichedEvent, EventSource};
use crate::polling::strategies::DeviceStatePoller;
use crate::registry::{RegistrationId, SpeakerServicePair};

/// A single polling task with state management
#[derive(Debug)]
pub struct PollingTask {
    /// Registration ID this task is polling for
    registration_id: RegistrationId,

    /// Speaker/service pair being polled
    speaker_service_pair: SpeakerServicePair,

    /// Current polling interval
    current_interval: Duration,

    /// Task handle for the background polling loop
    task_handle: JoinHandle<()>,

    /// Shutdown signal for graceful termination
    shutdown_signal: Arc<AtomicBool>,

    /// When this task was started
    started_at: SystemTime,

    /// Number of consecutive errors
    error_count: Arc<RwLock<u32>>,

    /// Total number of polls performed
    poll_count: Arc<RwLock<u64>>,
}

impl PollingTask {
    /// Create and start a new polling task
    pub fn start(
        registration_id: RegistrationId,
        speaker_service_pair: SpeakerServicePair,
        initial_interval: Duration,
        max_interval: Duration,
        adaptive_polling: bool,
        device_poller: Arc<DeviceStatePoller>,
        event_sender: mpsc::UnboundedSender<EnrichedEvent>,
    ) -> Self {
        let shutdown_signal = Arc::new(AtomicBool::new(false));
        let error_count = Arc::new(RwLock::new(0));
        let poll_count = Arc::new(RwLock::new(0));

        // Clone for the task
        let task_registration_id = registration_id;
        let task_pair = speaker_service_pair.clone();
        let task_shutdown_signal = Arc::clone(&shutdown_signal);
        let task_error_count = Arc::clone(&error_count);
        let task_poll_count = Arc::clone(&poll_count);

        let task_handle = tokio::spawn(async move {
            Self::polling_loop(
                task_registration_id,
                task_pair,
                initial_interval,
                max_interval,
                adaptive_polling,
                device_poller,
                event_sender,
                task_shutdown_signal,
                task_error_count,
                task_poll_count,
            )
            .await;
        });

        Self {
            registration_id,
            speaker_service_pair,
            current_interval: initial_interval,
            task_handle,
            shutdown_signal,
            started_at: SystemTime::now(),
            error_count,
            poll_count,
        }
    }

    /// Main polling loop
    async fn polling_loop(
        registration_id: RegistrationId,
        pair: SpeakerServicePair,
        mut current_interval: Duration,
        max_interval: Duration,
        adaptive_polling: bool,
        device_poller: Arc<DeviceStatePoller>,
        event_sender: mpsc::UnboundedSender<EnrichedEvent>,
        shutdown_signal: Arc<AtomicBool>,
        error_count: Arc<RwLock<u32>>,
        poll_count: Arc<RwLock<u64>>,
    ) {
        eprintln!(
            "🔄 Starting polling task for {} {:?} (interval: {:?})",
            pair.speaker_ip, pair.service, current_interval
        );

        // Track last state locally within the loop
        let mut last_state: Option<String> = None;

        loop {
            // Check for shutdown signal
            if shutdown_signal.load(Ordering::Relaxed) {
                eprintln!(
                    "🛑 Polling task shutting down for {} {:?}",
                    pair.speaker_ip, pair.service
                );
                break;
            }

            // Sleep for the current interval
            tokio::time::sleep(current_interval).await;

            // Increment poll count
            {
                let mut count = poll_count.write().await;
                *count += 1;
            }

            // Poll the device state
            match device_poller.poll_device_state(&pair).await {
                Ok(current_state) => {
                    // Reset error count on success
                    {
                        let mut errors = error_count.write().await;
                        *errors = 0;
                    }

                    // Check for state changes
                    let state_changed = {
                        let previous_state = last_state.clone();

                        if let Some(ref previous) = previous_state {
                            if previous != &current_state {
                                last_state = Some(current_state.clone());
                                true
                            } else {
                                false
                            }
                        } else {
                            // First poll - store initial state
                            last_state = Some(current_state.clone());
                            true // Treat as change for initial state
                        }
                    };

                    if state_changed {
                        eprintln!(
                            "📊 State change detected for {} {:?}",
                            pair.speaker_ip, pair.service
                        );

                        // Convert JSON snapshot to EventData and emit full-state event
                        match device_poller.state_to_event_data(&pair.service, &current_state) {
                            Ok(event_data) => {
                                let enriched_event = EnrichedEvent::new(
                                    registration_id,
                                    pair.speaker_ip,
                                    pair.service,
                                    EventSource::PollingDetection {
                                        poll_interval: current_interval,
                                    },
                                    event_data,
                                );

                                if event_sender.send(enriched_event).is_err() {
                                    eprintln!(
                                        "❌ Failed to send polling event for {} {:?}",
                                        pair.speaker_ip, pair.service
                                    );
                                    return;
                                }
                            }
                            Err(e) => {
                                eprintln!(
                                    "⚠️ Failed to convert state to event data for {} {:?}: {}",
                                    pair.speaker_ip, pair.service, e
                                );
                            }
                        }

                        // Adjust interval if adaptive polling is enabled
                        if adaptive_polling {
                            current_interval = Self::calculate_adaptive_interval(
                                current_interval,
                                max_interval,
                                SystemTime::now(),
                            );
                        }
                    }
                }
                Err(e) => {
                    // Increment error count
                    let error_count_value = {
                        let mut errors = error_count.write().await;
                        *errors += 1;
                        *errors
                    };

                    eprintln!(
                        "❌ Polling error for {} {:?} (attempt {}): {}",
                        pair.speaker_ip, pair.service, error_count_value, e
                    );

                    // Use exponential backoff for errors
                    if error_count_value >= 5 {
                        eprintln!(
                            "💥 Too many consecutive errors for {} {:?}, stopping polling",
                            pair.speaker_ip, pair.service
                        );
                        break;
                    }

                    // Exponential backoff up to max interval
                    let backoff_interval = current_interval * (2_u32.pow(error_count_value.min(6)));
                    let capped_interval = backoff_interval.min(max_interval);
                    tokio::time::sleep(capped_interval).await;
                }
            }
        }

        eprintln!(
            "🏁 Polling task ended for {} {:?}",
            pair.speaker_ip, pair.service
        );
    }

    /// Calculate adaptive polling interval based on recent activity
    fn calculate_adaptive_interval(
        current_interval: Duration,
        max_interval: Duration,
        last_change_time: SystemTime,
    ) -> Duration {
        let time_since_change = SystemTime::now()
            .duration_since(last_change_time)
            .unwrap_or(Duration::ZERO);

        if time_since_change < Duration::from_secs(30) {
            // Recent activity - poll faster
            (current_interval / 2).max(Duration::from_secs(2))
        } else if time_since_change > Duration::from_secs(300) {
            // No recent activity - poll slower
            (current_interval * 2).min(max_interval)
        } else {
            current_interval
        }
    }

    /// Get the registration ID for this task
    pub fn registration_id(&self) -> RegistrationId {
        self.registration_id
    }

    /// Get the speaker/service pair for this task
    pub fn speaker_service_pair(&self) -> &SpeakerServicePair {
        &self.speaker_service_pair
    }

    /// Get the current polling interval
    pub fn current_interval(&self) -> Duration {
        self.current_interval
    }

    /// Check if the task is still running
    pub fn is_running(&self) -> bool {
        !self.task_handle.is_finished()
    }

    /// Get task statistics
    pub async fn stats(&self) -> PollingTaskStats {
        let error_count = *self.error_count.read().await;
        let poll_count = *self.poll_count.read().await;

        PollingTaskStats {
            registration_id: self.registration_id,
            speaker_service_pair: self.speaker_service_pair.clone(),
            current_interval: self.current_interval,
            started_at: self.started_at,
            error_count,
            poll_count,
            is_running: self.is_running(),
        }
    }

    /// Request graceful shutdown of this polling task
    pub async fn shutdown(self) -> PollingResult<()> {
        // Signal shutdown
        self.shutdown_signal.store(true, Ordering::Relaxed);

        // Wait for task to complete
        match self.task_handle.await {
            Ok(()) => Ok(()),
            Err(e) => Err(PollingError::TaskSpawn(format!(
                "Failed to await task completion: {}",
                e
            ))),
        }
    }
}

/// Statistics for a polling task
#[derive(Debug, Clone)]
pub struct PollingTaskStats {
    pub registration_id: RegistrationId,
    pub speaker_service_pair: SpeakerServicePair,
    pub current_interval: Duration,
    pub started_at: SystemTime,
    pub error_count: u32,
    pub poll_count: u64,
    pub is_running: bool,
}

/// Manages multiple polling tasks
pub struct PollingScheduler {
    /// Active polling tasks indexed by registration ID
    active_tasks: Arc<RwLock<HashMap<RegistrationId, PollingTask>>>,

    /// Device state poller for making actual polling requests
    device_poller: Arc<DeviceStatePoller>,

    /// Event sender for emitting synthetic events
    event_sender: mpsc::UnboundedSender<EnrichedEvent>,

    /// Base polling interval
    base_interval: Duration,

    /// Maximum polling interval for adaptive polling
    max_interval: Duration,

    /// Whether to use adaptive polling intervals
    adaptive_polling: bool,

    /// Maximum number of concurrent polling tasks
    max_concurrent_tasks: usize,
}

impl PollingScheduler {
    /// Create a new polling scheduler
    pub fn new(
        event_sender: mpsc::UnboundedSender<EnrichedEvent>,
        base_interval: Duration,
        max_interval: Duration,
        adaptive_polling: bool,
        max_concurrent_tasks: usize,
    ) -> Self {
        Self {
            active_tasks: Arc::new(RwLock::new(HashMap::new())),
            device_poller: Arc::new(DeviceStatePoller::new()),
            event_sender,
            base_interval,
            max_interval,
            adaptive_polling,
            max_concurrent_tasks,
        }
    }

    /// Start polling for a speaker/service pair
    pub async fn start_polling(
        &self,
        registration_id: RegistrationId,
        pair: SpeakerServicePair,
    ) -> PollingResult<()> {
        let mut tasks = self.active_tasks.write().await;

        // Check if already polling
        if tasks.contains_key(&registration_id) {
            return Ok(()); // Already polling
        }

        // Check concurrent task limit
        if tasks.len() >= self.max_concurrent_tasks {
            return Err(PollingError::TooManyErrors {
                error_count: tasks.len() as u32,
            });
        }

        // Start new polling task
        let task = PollingTask::start(
            registration_id,
            pair.clone(),
            self.base_interval,
            self.max_interval,
            self.adaptive_polling,
            Arc::clone(&self.device_poller),
            self.event_sender.clone(),
        );

        tasks.insert(registration_id, task);

        eprintln!(
            "✅ Started polling for {} {:?}",
            pair.speaker_ip, pair.service
        );

        Ok(())
    }

    /// Stop polling for a registration ID
    pub async fn stop_polling(&self, registration_id: RegistrationId) -> PollingResult<()> {
        let mut tasks = self.active_tasks.write().await;

        if let Some(task) = tasks.remove(&registration_id) {
            let pair = task.speaker_service_pair().clone();
            // Shutdown happens when task is dropped, but we can explicitly shut it down
            task.shutdown().await?;

            eprintln!(
                "🛑 Stopped polling for {} {:?}",
                pair.speaker_ip, pair.service
            );
        }

        Ok(())
    }

    /// Check if a registration is currently being polled
    pub async fn is_polling(&self, registration_id: RegistrationId) -> bool {
        let tasks = self.active_tasks.read().await;
        tasks.contains_key(&registration_id)
    }

    /// Get statistics for all active polling tasks
    pub async fn stats(&self) -> PollingSchedulerStats {
        let tasks = self.active_tasks.read().await;
        let total_tasks = tasks.len();

        let mut task_stats = Vec::new();
        for task in tasks.values() {
            task_stats.push(task.stats().await);
        }

        PollingSchedulerStats {
            total_active_tasks: total_tasks,
            max_concurrent_tasks: self.max_concurrent_tasks,
            base_interval: self.base_interval,
            max_interval: self.max_interval,
            adaptive_polling: self.adaptive_polling,
            task_stats,
        }
    }

    /// Shutdown all polling tasks
    pub async fn shutdown_all(&self) -> PollingResult<()> {
        let mut tasks = self.active_tasks.write().await;

        for (registration_id, task) in tasks.drain() {
            match task.shutdown().await {
                Ok(()) => {
                    eprintln!("✅ Shutdown polling task {}", registration_id);
                }
                Err(e) => {
                    eprintln!("❌ Failed to shutdown polling task {}: {}", registration_id, e);
                }
            }
        }

        Ok(())
    }
}

/// Statistics for the polling scheduler
#[derive(Debug)]
pub struct PollingSchedulerStats {
    pub total_active_tasks: usize,
    pub max_concurrent_tasks: usize,
    pub base_interval: Duration,
    pub max_interval: Duration,
    pub adaptive_polling: bool,
    pub task_stats: Vec<PollingTaskStats>,
}

impl std::fmt::Display for PollingSchedulerStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Polling Scheduler Stats:")?;
        writeln!(
            f,
            "  Active tasks: {}/{}",
            self.total_active_tasks, self.max_concurrent_tasks
        )?;
        writeln!(f, "  Base interval: {:?}", self.base_interval)?;
        writeln!(f, "  Max interval: {:?}", self.max_interval)?;
        writeln!(f, "  Adaptive polling: {}", self.adaptive_polling)?;

        if !self.task_stats.is_empty() {
            writeln!(f, "  Task details:")?;
            for stat in &self.task_stats {
                writeln!(
                    f,
                    "    {}: {} {:?} (interval: {:?}, polls: {}, errors: {})",
                    stat.registration_id,
                    stat.speaker_service_pair.speaker_ip,
                    stat.speaker_service_pair.service,
                    stat.current_interval,
                    stat.poll_count,
                    stat.error_count
                )?;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;

    #[tokio::test]
    async fn test_polling_scheduler_creation() {
        let (event_sender, _event_receiver) = mpsc::unbounded_channel();
        let scheduler = PollingScheduler::new(
            event_sender,
            Duration::from_secs(5),
            Duration::from_secs(30),
            true,
            10,
        );

        let stats = scheduler.stats().await;
        assert_eq!(stats.total_active_tasks, 0);
        assert_eq!(stats.max_concurrent_tasks, 10);
        assert!(stats.adaptive_polling);
    }

    #[tokio::test]
    async fn test_polling_task_lifecycle() {
        let (event_sender, _event_receiver) = mpsc::unbounded_channel();
        let scheduler = PollingScheduler::new(
            event_sender,
            Duration::from_millis(100), // Fast polling for testing
            Duration::from_secs(1),
            false,
            5,
        );

        let registration_id = RegistrationId::new(1);
        let pair = SpeakerServicePair::new(
            "192.168.1.100".parse().unwrap(),
            sonos_api::Service::AVTransport,
        );

        // Start polling
        scheduler.start_polling(registration_id, pair.clone()).await.unwrap();
        assert!(scheduler.is_polling(registration_id).await);

        // Stop polling
        scheduler.stop_polling(registration_id).await.unwrap();
        assert!(!scheduler.is_polling(registration_id).await);
    }

    #[test]
    fn test_adaptive_interval_calculation() {
        let current = Duration::from_secs(5);
        let max = Duration::from_secs(30);
        let recent_change = SystemTime::now() - Duration::from_secs(10);

        let new_interval = PollingTask::calculate_adaptive_interval(current, max, recent_change);
        // Should decrease interval for recent activity
        assert!(new_interval <= current);

        let old_change = SystemTime::now() - Duration::from_secs(400);
        let new_interval = PollingTask::calculate_adaptive_interval(current, max, old_change);
        // Should increase interval for old activity
        assert!(new_interval >= current);
    }
}