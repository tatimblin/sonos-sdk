//! Polling task scheduler and management
//!
//! This module provides intelligent polling task management with support for
//! adaptive intervals, graceful shutdown, and coordination with the event system.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};
use tokio::sync::{mpsc, Notify, RwLock};
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};

use crate::error::{PollingError, PollingResult};
use crate::events::types::{EnrichedEvent, EventSource};
use crate::polling::strategies::DeviceStatePoller;
use crate::registry::{RegistrationId, SpeakerServicePair};

/// Shutdown signal for a polling task: a flag plus a wakeup.
///
/// The flag alone is not enough. It was previously only read at the top of the
/// polling loop, so a requested stop had to wait out whatever the current iteration
/// was doing: up to `current_interval` of sleep, then a full poll (several sequential
/// SOAP calls, each with a 10s ureq read timeout), then — on the error path — a
/// backoff sleep capped at `max_polling_interval` that was not guarded at all.
/// Against an unreachable speaker that is over a minute.
///
/// [`ShutdownSignal::request`] wakes any sleep in progress, so
/// [`ShutdownSignal::sleep_or_shutdown`] returns as soon as a stop is requested
/// rather than at the end of the interval.
#[derive(Debug)]
struct ShutdownSignal {
    requested: AtomicBool,
    wakeup: Notify,
}

impl ShutdownSignal {
    fn new() -> Self {
        Self {
            requested: AtomicBool::new(false),
            wakeup: Notify::new(),
        }
    }

    /// Request shutdown and wake a sleeping polling loop.
    fn request(&self) {
        self.requested.store(true, Ordering::Relaxed);
        // `notify_waiters` would be lost if no task is parked *right now*; the polling
        // loop may instead be mid-poll. `notify_one` stores a permit, so the next
        // `notified().await` returns immediately and the flag is observed.
        self.wakeup.notify_one();
    }

    fn is_requested(&self) -> bool {
        self.requested.load(Ordering::Relaxed)
    }

    /// Sleep for `duration`, returning early if shutdown is requested.
    ///
    /// Returns `true` when shutdown was requested (so the caller should stop) and
    /// `false` when the full duration elapsed.
    async fn sleep_or_shutdown(&self, duration: Duration) -> bool {
        if self.is_requested() {
            return true;
        }

        tokio::select! {
            biased;
            _ = self.wakeup.notified() => true,
            _ = tokio::time::sleep(duration) => self.is_requested(),
        }
    }
}

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
    shutdown_signal: Arc<ShutdownSignal>,

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
        let shutdown_signal = Arc::new(ShutdownSignal::new());
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
    #[allow(clippy::too_many_arguments)]
    async fn polling_loop(
        registration_id: RegistrationId,
        pair: SpeakerServicePair,
        mut current_interval: Duration,
        max_interval: Duration,
        adaptive_polling: bool,
        device_poller: Arc<DeviceStatePoller>,
        event_sender: mpsc::UnboundedSender<EnrichedEvent>,
        shutdown_signal: Arc<ShutdownSignal>,
        error_count: Arc<RwLock<u32>>,
        poll_count: Arc<RwLock<u64>>,
    ) {
        info!(
            speaker_ip = %pair.speaker_ip,
            service = ?pair.service,
            ?current_interval,
            "Starting polling task"
        );

        // Track last state locally within the loop
        let mut last_state: Option<String> = None;

        loop {
            // Check for shutdown signal
            if shutdown_signal.is_requested() {
                info!(
                    speaker_ip = %pair.speaker_ip,
                    service = ?pair.service,
                    "Polling task shutting down"
                );
                break;
            }

            // Sleep for the current interval, but wake immediately on shutdown rather
            // than making a stop wait out the whole interval.
            if shutdown_signal.sleep_or_shutdown(current_interval).await {
                info!(
                    speaker_ip = %pair.speaker_ip,
                    service = ?pair.service,
                    "Polling task shutting down during interval sleep"
                );
                break;
            }

            // Increment poll count
            {
                let mut count = poll_count.write().await;
                *count += 1;
            }

            // Capture the observation instant *before* the request goes out.
            //
            // A poll response describes the device as of the request, exactly like
            // `fetch()` — everything after this point (the SOAP round trip, the JSON
            // change comparison, `state_to_event_data`) happens after the device was
            // observed and must not be counted as part of the observation. Stamping on
            // return would make the event look one round trip newer than the state it
            // describes, and against a slow speaker that round trip is seconds — long
            // enough to wrongly supersede a genuinely fresher UPnP event or local write.
            //
            // Deliberately *not* hoisted above the interval sleep: the sleep is not part
            // of the request, and stamping before it would make a legitimately newer poll
            // look older than it is and get dropped as stale.
            let observed_at = Instant::now();

            // Poll the device state
            match device_poller.poll_device_state(&pair).await {
                Ok(current_state) => {
                    // Reset error count on success
                    {
                        let mut errors = error_count.write().await;
                        *errors = 0;
                    }

                    // Check for state changes (compare without cloning)
                    let state_changed = last_state.as_deref() != Some(current_state.as_str());

                    if state_changed {
                        last_state = Some(current_state.clone());
                    }

                    if state_changed {
                        debug!(
                            speaker_ip = %pair.speaker_ip,
                            service = ?pair.service,
                            "State change detected"
                        );

                        // Convert JSON snapshot to EventData and emit full-state event
                        match device_poller.state_to_event_data(&pair.service, &current_state) {
                            Ok(event_data) => {
                                let enriched_event = EnrichedEvent::observed_at(
                                    registration_id,
                                    pair.speaker_ip,
                                    pair.service,
                                    EventSource::PollingDetection {
                                        poll_interval: current_interval,
                                    },
                                    event_data,
                                    observed_at,
                                );

                                if event_sender.send(enriched_event).is_err() {
                                    error!(
                                        speaker_ip = %pair.speaker_ip,
                                        service = ?pair.service,
                                        "Failed to send polling event — channel closed"
                                    );
                                    return;
                                }
                            }
                            Err(e) => {
                                warn!(
                                    speaker_ip = %pair.speaker_ip,
                                    service = ?pair.service,
                                    error = %e,
                                    "Failed to convert state to event data"
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

                    warn!(
                        speaker_ip = %pair.speaker_ip,
                        service = ?pair.service,
                        attempt = error_count_value,
                        error = %e,
                        "Polling error"
                    );

                    // Use exponential backoff for errors
                    if error_count_value >= 5 {
                        error!(
                            speaker_ip = %pair.speaker_ip,
                            service = ?pair.service,
                            "Too many consecutive errors, stopping polling"
                        );
                        break;
                    }

                    // Exponential backoff up to max interval. This sleep used to be
                    // unguarded entirely, making it the single worst contributor to
                    // shutdown latency: up to `max_interval` (30s by default) on top of
                    // everything else.
                    let backoff_interval = current_interval * (2_u32.pow(error_count_value.min(6)));
                    let capped_interval = backoff_interval.min(max_interval);
                    if shutdown_signal.sleep_or_shutdown(capped_interval).await {
                        info!(
                            speaker_ip = %pair.speaker_ip,
                            service = ?pair.service,
                            "Polling task shutting down during error backoff"
                        );
                        break;
                    }
                }
            }
        }

        info!(
            speaker_ip = %pair.speaker_ip,
            service = ?pair.service,
            "Polling task ended"
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
        // Signal shutdown *and* wake any sleep in progress, so this await is bounded by
        // the in-flight poll rather than by a full interval plus error backoff.
        self.shutdown_signal.request();

        // Wait for task to complete
        match self.task_handle.await {
            Ok(()) => Ok(()),
            Err(e) => Err(PollingError::TaskSpawn(format!(
                "Failed to await task completion: {e}"
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

        info!(
            speaker_ip = %pair.speaker_ip,
            service = ?pair.service,
            "Started polling"
        );

        Ok(())
    }

    /// Stop polling for a registration ID
    ///
    /// The `active_tasks` write guard is released *before* awaiting the task's
    /// shutdown. `remove` has already taken the task out of the map, so the guard
    /// protects nothing during the await and holding it only blocks every other
    /// accessor — `start_polling`, `is_polling` and `stats()`, and through the last of
    /// those `EventBroker::stats()`. Since the awaited shutdown is bounded by an
    /// in-flight poll (several sequential SOAP calls against a possibly unreachable
    /// speaker), that could stall unrelated callers for tens of seconds.
    ///
    /// Dropping the guard early is safe: the map no longer references this task, so no
    /// concurrent caller can observe or re-enter it, and a `start_polling` for the same
    /// registration that races in now correctly sees "not polling" and spawns a fresh
    /// task rather than blocking on the outgoing one.
    pub async fn stop_polling(&self, registration_id: RegistrationId) -> PollingResult<()> {
        let removed = {
            let mut tasks = self.active_tasks.write().await;
            tasks.remove(&registration_id)
        };

        if let Some(task) = removed {
            let pair = task.speaker_service_pair().clone();
            // Shutdown happens when task is dropped, but we can explicitly shut it down
            task.shutdown().await?;

            info!(
                speaker_ip = %pair.speaker_ip,
                service = ?pair.service,
                "Stopped polling"
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
    ///
    /// Drains the map under the lock and releases it before awaiting any shutdown, for
    /// the same reason as [`Self::stop_polling`]: the drained tasks are no longer
    /// reachable through the map, so the guard would protect nothing while blocking
    /// every other accessor for the duration of every in-flight poll.
    pub async fn shutdown_all(&self) -> PollingResult<()> {
        let drained: Vec<(RegistrationId, PollingTask)> = {
            let mut tasks = self.active_tasks.write().await;
            tasks.drain().collect()
        };

        for (registration_id, task) in drained {
            match task.shutdown().await {
                Ok(()) => {
                    debug!(%registration_id, "Shutdown polling task");
                }
                Err(e) => {
                    error!(%registration_id, error = %e, "Failed to shutdown polling task");
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
    use crate::events::types::{EventData, RenderingControlState};
    use crate::polling::strategies::ServicePoller;
    use async_trait::async_trait;
    use sonos_api::{Service, SonosClient};
    use std::sync::atomic::AtomicU32;
    use std::sync::Mutex;
    use tokio::sync::mpsc;

    /// Instants recorded around one simulated poll request.
    #[derive(Debug, Clone, Copy)]
    struct PollTiming {
        /// When the poller was entered — i.e. when the request effectively went out.
        entered: Instant,
        /// Halfway through the round trip: stands in for a UPnP NOTIFY or local write
        /// observed *while* the poll was still in flight.
        midpoint: Instant,
        /// When the response came back.
        returned: Instant,
    }

    /// A `ServicePoller` with a controllable request/response gap and no network I/O.
    ///
    /// The scheduler's observation stamping can only be measured against a poll whose
    /// request and response are distinguishable in time, which a real poller against a
    /// reachable speaker is not (and an unreachable one never returns a state at all).
    /// Each poll reports a different volume so the scheduler's change detection fires
    /// every time.
    struct FakePoller {
        round_trip: Duration,
        timings: Arc<Mutex<Vec<PollTiming>>>,
        polls: AtomicU32,
    }

    impl FakePoller {
        fn new(round_trip: Duration) -> (Self, Arc<Mutex<Vec<PollTiming>>>) {
            let timings = Arc::new(Mutex::new(Vec::new()));
            (
                Self {
                    round_trip,
                    timings: Arc::clone(&timings),
                    polls: AtomicU32::new(0),
                },
                timings,
            )
        }
    }

    #[async_trait]
    impl ServicePoller for FakePoller {
        async fn poll_state(
            &self,
            _client: &SonosClient,
            _pair: &SpeakerServicePair,
        ) -> PollingResult<String> {
            let entered = Instant::now();
            tokio::time::sleep(self.round_trip / 2).await;
            let midpoint = Instant::now();
            tokio::time::sleep(self.round_trip / 2).await;
            self.timings.lock().unwrap().push(PollTiming {
                entered,
                midpoint,
                returned: Instant::now(),
            });

            let volume = self.polls.fetch_add(1, Ordering::Relaxed);
            let state = RenderingControlState {
                master_volume: Some(volume.to_string()),
                master_mute: None,
                lf_volume: None,
                rf_volume: None,
                lf_mute: None,
                rf_mute: None,
                bass: None,
                treble: None,
                loudness: None,
                balance: None,
                other_channels: HashMap::new(),
            };
            serde_json::to_string(&state).map_err(|e| PollingError::StateParsing(e.to_string()))
        }

        fn state_to_event_data(&self, json_state: &str) -> PollingResult<EventData> {
            let state: RenderingControlState = serde_json::from_str(json_state)
                .map_err(|e| PollingError::StateParsing(e.to_string()))?;
            Ok(EventData::RenderingControl(state))
        }

        fn service_type(&self) -> Service {
            Service::RenderingControl
        }
    }

    /// Start a real polling task backed by [`FakePoller`].
    fn start_faked_task(
        interval: Duration,
        round_trip: Duration,
    ) -> (
        PollingTask,
        mpsc::UnboundedReceiver<EnrichedEvent>,
        Arc<Mutex<Vec<PollTiming>>>,
    ) {
        let (poller, timings) = FakePoller::new(round_trip);
        let device_poller = Arc::new(
            DeviceStatePoller::new()
                .with_service_poller(Service::RenderingControl, Box::new(poller)),
        );
        let (event_sender, event_receiver) = mpsc::unbounded_channel();

        let task = PollingTask::start(
            RegistrationId::new(1),
            // RFC 5737 TEST-NET-3; never contacted, the poller is faked.
            SpeakerServicePair::new("203.0.113.70".parse().unwrap(), Service::RenderingControl),
            interval,
            Duration::from_secs(600),
            false,
            device_poller,
            event_sender,
        );

        (task, event_receiver, timings)
    }

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
        scheduler
            .start_polling(registration_id, pair.clone())
            .await
            .unwrap();
        assert!(scheduler.is_polling(registration_id).await);

        // Stop polling
        scheduler.stop_polling(registration_id).await.unwrap();
        assert!(!scheduler.is_polling(registration_id).await);
    }

    /// `stop_polling` must not hold the `active_tasks` write guard while awaiting the
    /// task's shutdown.
    ///
    /// It used to `remove()` and then `shutdown().await` with the guard still alive.
    /// Shutdown is bounded by the in-flight poll — several sequential SOAP calls, each
    /// with a 5s connect / 10s read timeout against a possibly unreachable speaker — so
    /// for that whole window every other accessor of the map blocked: `start_polling`,
    /// `is_polling`, and `stats()`. `EventBroker::stats()` calls the last of those, so a
    /// caller merely asking for statistics could hang for over a minute.
    ///
    /// The speaker is an RFC 5737 TEST-NET-3 address, so the poll's connect attempt
    /// stalls without reaching any real device.
    #[tokio::test]
    async fn test_stop_polling_does_not_hold_lock_across_shutdown() {
        let (event_sender, _event_receiver) = mpsc::unbounded_channel();
        let scheduler = Arc::new(PollingScheduler::new(
            event_sender,
            // Enter the first poll almost immediately, so a slow shutdown is in flight
            // by the time we probe the lock.
            Duration::from_millis(20),
            Duration::from_secs(30),
            false,
            5,
        ));

        let registration_id = RegistrationId::new(1);
        let pair = SpeakerServicePair::new(
            "203.0.113.60".parse().unwrap(),
            sonos_api::Service::AVTransport,
        );

        scheduler
            .start_polling(registration_id, pair)
            .await
            .unwrap();

        // Let the task get past its interval sleep and into the (stalling) poll.
        tokio::time::sleep(Duration::from_millis(300)).await;

        let stopper = {
            let scheduler = Arc::clone(&scheduler);
            tokio::spawn(async move { scheduler.stop_polling(registration_id).await })
        };

        // Give the stopper time to take the lock and begin awaiting shutdown.
        tokio::time::sleep(Duration::from_millis(200)).await;

        // The load-bearing assertion: readers must not be blocked by the in-flight
        // shutdown. With the guard held across the await these both block until the
        // stalling poll finishes.
        tokio::time::timeout(Duration::from_secs(2), scheduler.stats())
            .await
            .expect("stats() must not block on an in-flight stop_polling");
        tokio::time::timeout(
            Duration::from_secs(2),
            scheduler.is_polling(registration_id),
        )
        .await
        .expect("is_polling() must not block on an in-flight stop_polling");

        // Proves the overlap was real: had the shutdown already completed, the
        // assertions above would be vacuous.
        assert!(
            !stopper.is_finished(),
            "precondition: shutdown should still be in flight, otherwise this test \
             proves nothing about lock scope"
        );

        stopper.await.unwrap().unwrap();
    }

    /// A requested shutdown must interrupt a pending interval sleep rather than waiting
    /// it out.
    ///
    /// The shutdown flag used to be read only at the *top* of the polling loop, so a
    /// stop issued while the task slept had to wait for the full `current_interval`
    /// first (and, on the error path, an unguarded backoff sleep capped at
    /// `max_polling_interval` on top). Here the interval is 60s: if the sleep is not
    /// interruptible, `stop_polling` cannot return within the timeout.
    ///
    /// Fully offline — the task is stopped during its very first sleep, so it never
    /// reaches a poll and contacts nothing.
    #[tokio::test]
    async fn test_shutdown_interrupts_pending_sleep() {
        let (event_sender, _event_receiver) = mpsc::unbounded_channel();
        let scheduler = PollingScheduler::new(
            event_sender,
            // Far longer than the assertion timeout below, so passing requires the
            // sleep to actually be cut short.
            Duration::from_secs(60),
            Duration::from_secs(120),
            false,
            5,
        );

        let registration_id = RegistrationId::new(1);
        let pair = SpeakerServicePair::new(
            "203.0.113.61".parse().unwrap(),
            sonos_api::Service::RenderingControl,
        );

        scheduler
            .start_polling(registration_id, pair)
            .await
            .unwrap();

        // Let the task reach its interval sleep.
        tokio::time::sleep(Duration::from_millis(100)).await;

        tokio::time::timeout(
            Duration::from_secs(5),
            scheduler.stop_polling(registration_id),
        )
        .await
        .expect("shutdown must interrupt the interval sleep, not wait out all 60s")
        .expect("stop_polling should succeed");

        assert!(!scheduler.is_polling(registration_id).await);
    }

    /// Shutdown must also interrupt the **error-backoff** sleep, which was previously
    /// unguarded entirely and is the worst single contributor to shutdown latency.
    ///
    /// Distinct from `test_shutdown_interrupts_pending_sleep`: the stop is issued once
    /// the first poll has already *failed*, so the loop is parked in the backoff sleep
    /// rather than in its interval sleep. Reverting only the backoff guard leaves that
    /// other test passing, so this is the one that pins it.
    ///
    /// The timings are set so the backoff is unmistakably long. Backoff is
    /// `current_interval * 2^error_count` capped at `max_polling_interval`, so it scales
    /// off the *base* interval — a short base interval yields a backoff of milliseconds
    /// and would make this test vacuous. With a 5s base: the interval sleep runs
    /// t=0..5s, the poll fails at t≈10s (5s ureq connect timeout against an unreachable
    /// address), and the backoff then covers t≈10..20s. The stop is issued at t=11s, so
    /// an unguarded backoff makes it wait ~9s while a guarded one returns at once.
    ///
    /// **Timing-sensitive**, deliberately loosely bounded: the 6s assertion timeout sits
    /// between those two outcomes with margin on both sides. Offline throughout — RFC
    /// 5737 TEST-NET-3, so the poll fails by connect timeout without reaching a device.
    #[tokio::test]
    async fn test_shutdown_interrupts_error_backoff() {
        let (event_sender, _event_receiver) = mpsc::unbounded_channel();
        let scheduler = PollingScheduler::new(
            event_sender,
            // Backoff derives from this, so it must not be tiny.
            Duration::from_secs(5),
            // High enough that the cap never shortens the backoff.
            Duration::from_secs(300),
            false,
            5,
        );

        let registration_id = RegistrationId::new(1);
        let pair = SpeakerServicePair::new(
            "203.0.113.62".parse().unwrap(),
            sonos_api::Service::AVTransport,
        );

        scheduler
            .start_polling(registration_id, pair)
            .await
            .unwrap();

        // Past the first poll's failure (t≈10s), so the loop is in error backoff.
        tokio::time::sleep(Duration::from_secs(11)).await;

        tokio::time::timeout(
            Duration::from_secs(6),
            scheduler.stop_polling(registration_id),
        )
        .await
        .expect("shutdown must interrupt the error-backoff sleep rather than waiting it out")
        .expect("stop_polling should succeed");

        assert!(!scheduler.is_polling(registration_id).await);
    }

    /// The shutdown request must be observable even when it is issued while the polling
    /// loop is *not* parked in a sleep.
    ///
    /// This pins the choice of `notify_one` over `notify_waiters`: the latter is dropped
    /// when no task is currently waiting, so a stop requested mid-poll would leave the
    /// *next* sleep to run to completion. `notify_one` stores a permit, so the following
    /// `notified()` returns at once.
    #[tokio::test]
    async fn test_shutdown_signal_wakes_a_later_sleep() {
        let signal = ShutdownSignal::new();

        // Requested before anyone waits — the flag short-circuits immediately.
        signal.request();
        assert!(
            signal.sleep_or_shutdown(Duration::from_secs(600)).await,
            "an already-requested shutdown must not sleep at all"
        );

        // A signal with no request outstanding does sleep, and reports "not shutting
        // down" when the duration elapses.
        let fresh = ShutdownSignal::new();
        assert!(
            !fresh.sleep_or_shutdown(Duration::from_millis(10)).await,
            "without a shutdown request the sleep should run to completion"
        );
    }

    /// The write-ordering rule consumers apply (`sonos-state`'s `WriteStamp`): a value
    /// observed no later than the stored one is rejected as stale. Restated here because
    /// `sonos-state` depends on this crate, not the other way round.
    fn supersedes(candidate: Instant, stored: Instant) -> bool {
        candidate > stored
    }

    /// A polling event's `observed_at` must come from before its request, not after its
    /// response.
    ///
    /// The scheduler used to call `EnrichedEvent::new`, which stamps "now" at
    /// construction — after the SOAP round trip and after `state_to_event_data`. The
    /// event then looked one round trip newer than the state it actually described.
    ///
    /// Offline: the poller is faked, so nothing is sent to the TEST-NET-3 address.
    #[tokio::test]
    async fn test_polling_event_observed_before_its_request() {
        // A round trip far wider than scheduling jitter, so "before request" and "after
        // response" cannot be confused for each other.
        let (task, mut events, timings) =
            start_faked_task(Duration::from_millis(10), Duration::from_millis(400));

        let event = events.recv().await.expect("polling event");
        task.shutdown().await.unwrap();

        let timing = timings.lock().unwrap()[0];
        assert!(
            event.observed_at <= timing.entered,
            "observed_at must precede the poll request, but it was {:?} after it",
            event.observed_at - timing.entered
        );
        assert!(
            event.observed_at < timing.returned,
            "observed_at must not be stamped on the response"
        );
    }

    /// A polling event must not supersede a value observed *during* its round trip.
    ///
    /// This is the user-visible symptom: a volume change seen via UPnP NOTIFY (or a local
    /// `set_volume`) while a slow poll is in flight would be overwritten by the poll's
    /// older reading — "volume snaps back". Distinct from the test above, which checks the
    /// stamp's provenance; this one checks the ordering decision that stamp drives.
    #[tokio::test]
    async fn test_polling_event_does_not_supersede_a_fresher_observation() {
        let (task, mut events, timings) =
            start_faked_task(Duration::from_millis(10), Duration::from_millis(400));

        let event = events.recv().await.expect("polling event");
        task.shutdown().await.unwrap();

        // Stands in for a NOTIFY arrival or local write observed mid-round-trip.
        let fresher_observation = timings.lock().unwrap()[0].midpoint;

        assert!(
            !supersedes(event.observed_at, fresher_observation),
            "a poll observed before a competing write must not overwrite it"
        );
    }

    /// The inverse-failure guard: a genuinely newer polling event must still win.
    ///
    /// Backdating too far — hoisting the capture above the interval sleep, or out of the
    /// loop entirely so every event shares the task's start instant — would satisfy both
    /// tests above while silently discarding real device changes. Here a competing value
    /// is observed *after* the first polling event lands; the second poll genuinely
    /// observes the device later than that, so it must supersede it.
    #[tokio::test]
    async fn test_newer_polling_event_still_supersedes() {
        let (task, mut events, _timings) =
            start_faked_task(Duration::from_millis(10), Duration::from_millis(100));

        let first = events.recv().await.expect("first polling event");

        // Observed between the two polls: after the first event, before the second poll's
        // request goes out.
        let competing_observation = Instant::now();

        let second = events.recv().await.expect("second polling event");
        task.shutdown().await.unwrap();

        assert!(
            supersedes(second.observed_at, competing_observation),
            "a poll whose request post-dates a competing write must still overwrite it"
        );
        assert!(
            supersedes(second.observed_at, first.observed_at),
            "each poll must carry its own observation instant, not a shared one"
        );
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
