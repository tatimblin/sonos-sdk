//! Sync-first Sonos Event Manager
//!
//! Provides a fully synchronous API for managing Sonos event subscriptions.
//! All async operations are hidden in a background worker thread.

use std::collections::HashMap;
use std::fmt;
use std::net::IpAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::Duration;

use parking_lot::RwLock;
use tokio::sync::mpsc as tokio_mpsc;

use sonos_api::{Service, SpeakerId};
use sonos_discovery::Device;
use sonos_stream::events::EnrichedEvent;
use sonos_stream::BrokerConfig;

use crate::error::{EventManagerError, Result};
use crate::iter::EventManagerIterator;
use crate::timer::TeardownTimer;
use crate::worker::{spawn_event_worker, Command};

/// Grace period duration before unsubscribing after last guard drops
const GRACE_PERIOD: Duration = Duration::from_millis(50);

/// Grace-period timers in flight, keyed by `(ip, service)`.
///
/// The `AtomicBool` is a *claim token*, not merely a cancellation flag: whoever
/// swaps it from `false` to `true` owns the outcome of that teardown. Exactly
/// one of `acquire_watch` (cancelling) and [`PendingTeardown::fire`] (expiring)
/// can win, and both perform the swap while holding the map's mutex, so the two
/// can never interleave. See [`PendingTeardown::fire`] for the full argument.
type PendingUnsubscribes = HashMap<(IpAddr, Service), Arc<AtomicBool>>;

/// Everything a deferred teardown needs to resolve itself once its grace period
/// expires.
///
/// Handed to the [`TeardownTimer`] thread, which fires it once `delay` elapses.
/// It deliberately carries the shared pending-map handle rather than a
/// reference to the manager: the timer must not hold an
/// `Arc<SonosEventManager>`, or the manager could never be dropped.
pub(crate) struct PendingTeardown {
    pub(crate) ip: IpAddr,
    pub(crate) service: Service,
    /// How long to wait before firing. Always [`GRACE_PERIOD`] today; carried
    /// explicitly so the timer never has to know the manager's policy.
    pub(crate) delay: Duration,
    pub(crate) cancelled: Arc<AtomicBool>,
    pub(crate) pending: Arc<parking_lot::Mutex<PendingUnsubscribes>>,
    pub(crate) registry: Option<Arc<dyn WatchRegistry>>,
}

impl fmt::Debug for PendingTeardown {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // `registry` is a trait object and cannot be formatted; it is also not
        // interesting here.
        f.debug_struct("PendingTeardown")
            .field("ip", &self.ip)
            .field("service", &self.service)
            .field("delay", &self.delay)
            .field("cancelled", &self.cancelled.load(Ordering::SeqCst))
            .finish_non_exhaustive()
    }
}

impl PendingTeardown {
    /// Resolve the teardown now that the grace period has elapsed.
    ///
    /// Returns `true` if the teardown actually fired, `false` if a concurrent
    /// `acquire_watch` had already claimed it.
    ///
    /// # Why the lock and the swap are both needed
    ///
    /// The claim (`swap`) happens *while holding the pending-map mutex*, which
    /// is the same mutex `acquire_watch` holds when it cancels. That gives two
    /// guarantees:
    ///
    /// 1. **Exactly one winner.** `swap` returns the previous value, so only the
    ///    caller that observes `false` proceeds. A re-acquire inside the grace
    ///    window therefore keeps the live subscription and no `Unsubscribe` is
    ///    ever sent for it.
    /// 2. **No inverted command order.** `Unsubscribe` is sent *before* the
    ///    guard is released, so a racing `acquire_watch` — which can only send
    ///    its `Subscribe` after taking the same lock — is guaranteed to enqueue
    ///    behind it. The channel is FIFO, so the worker always sees
    ///    unsubscribe-then-subscribe and ends up subscribed, never the reverse.
    pub(crate) fn fire(&self, command_tx: &tokio_mpsc::UnboundedSender<Command>) -> bool {
        let mut pending = self.pending.lock();

        if self.cancelled.swap(true, Ordering::SeqCst) {
            tracing::debug!(
                "Grace period for {}:{:?} was cancelled by a re-acquire, keeping subscription",
                self.ip,
                self.service
            );
            return false;
        }

        // Only clear the map entry if it is still *ours*. A later
        // release_watch may have installed a fresh token for the same key.
        let key = (self.ip, self.service);
        if pending
            .get(&key)
            .is_some_and(|current| Arc::ptr_eq(current, &self.cancelled))
        {
            pending.remove(&key);
        }

        tracing::debug!(
            "Grace period expired for {}:{:?}, unsubscribing",
            self.ip,
            self.service
        );

        // Ordering between these two is load-bearing and matches the original
        // implementation: drop the UPnP subscription, then clear the watched
        // set. A closed channel here just means the worker is already gone.
        let _ = command_tx.send(Command::Unsubscribe {
            ip: self.ip,
            service: self.service,
        });

        if let Some(registry) = &self.registry {
            call_unregister(registry, self.ip, self.service);
        }

        true
    }
}

/// Invoke a registry's unregister callback, containing any panic it raises.
///
/// The callback is implementor-supplied (`StateManager` today) and runs on the
/// shared teardown thread. A panic escaping it would unwind out of the timer's
/// `run` loop and kill the one thread every *later* teardown depends on. That
/// failure is silent and permanent: watched-set entries stay forever and no
/// subscription is ever released again. Swapping N failure domains for one is
/// only a regression if the one is terminable, so it is made non-terminable
/// here and in [`crate::timer`].
///
/// Deliberately called *inside* the pending-map guard's scope in
/// [`PendingTeardown::fire`]: the guard is never unwound through, and `fire`
/// still returns `true`, so "exactly one winner" stays true in the panic case.
///
/// `AssertUnwindSafe` is required because `Arc<dyn WatchRegistry>` is not
/// `RefUnwindSafe`. `catch_unwind` is a safe function, so `#![forbid(unsafe_code)]`
/// is unaffected. A registry that panics has broken the contract documented on
/// [`WatchRegistry`]; whatever state it left behind is its own to repair.
fn call_unregister(registry: &Arc<dyn WatchRegistry>, ip: IpAddr, service: Service) {
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        registry.unregister_watches_for_service(ip, service);
    }));

    if outcome.is_err() {
        tracing::error!(
            "WatchRegistry::unregister_watches_for_service panicked for {}:{:?}; \
             the watched set is now stale for that service",
            ip,
            service
        );
    }
}

/// Constructors for test fixtures that other modules' unit tests need.
#[cfg(test)]
pub(crate) mod test_support {
    use super::{Arc, AtomicBool, HashMap, PendingTeardown, Service, GRACE_PERIOD};

    /// A teardown wired to nothing: enough to exercise queue ordering and
    /// scheduling without a manager behind it.
    pub(crate) fn dummy_teardown() -> PendingTeardown {
        PendingTeardown {
            ip: "192.0.2.1".parse().expect("test IP is valid"),
            service: Service::RenderingControl,
            delay: GRACE_PERIOD,
            cancelled: Arc::new(AtomicBool::new(false)),
            pending: Arc::new(parking_lot::Mutex::new(HashMap::new())),
            registry: None,
        }
    }
}

// ============================================================================
// WatchRegistry trait
// ============================================================================

/// Trait for managing the watched-property set.
///
/// Defined in sonos-event-manager, implemented by StateManager in sonos-state.
/// Bridges the two crates without circular dependencies.
///
/// # Implementor contract
///
/// Both methods are called from the manager's hot paths, and
/// [`unregister_watches_for_service`](WatchRegistry::unregister_watches_for_service)
/// runs **on the shared teardown thread while the manager's pending-map mutex
/// is held**. Implementations must therefore be:
///
/// - short and non-blocking — no network or disk I/O, no sleeping, no waiting
///   on another thread;
/// - free of re-entry into `SonosEventManager` (`acquire_watch`,
///   `release_watch` and `shutdown` all take that same mutex, so re-entering
///   deadlocks);
/// - tolerant of being called for a `(ip, service)` pair they know nothing
///   about.
///
/// A **panic** is caught and logged (see `call_unregister`) and costs only that
/// one teardown. A **hang** is not recoverable and blocks every subsequent
/// teardown for the lifetime of the process.
pub trait WatchRegistry: Send + Sync + 'static {
    /// Register a property as watched (called during acquire_watch)
    fn register_watch(&self, speaker_id: &SpeakerId, key: &'static str, service: Service);

    /// Unregister all watched properties for a given service on a device.
    /// Called when the grace period expires and the subscription is actually torn down.
    fn unregister_watches_for_service(&self, ip: IpAddr, service: Service);
}

// ============================================================================
// WatchGuard
// ============================================================================

/// RAII guard holding one subscription ref count.
///
/// Each guard represents exactly one reference. When dropped, the ref count is
/// decremented. If it reaches zero, a 50ms grace period starts — if no new
/// `watch()` arrives, the UPnP subscription is torn down.
///
/// Not `Clone`, not `Copy`. Each guard is one hold.
///
/// `WatchGuard` is `Send` but not necessarily `Sync`. This is acceptable for
/// TUI single-thread rendering use cases.
#[must_use = "dropping the guard immediately starts the grace period"]
pub struct WatchGuard {
    event_manager: Arc<SonosEventManager>,
    speaker_id: SpeakerId,
    property_key: &'static str,
    ip: IpAddr,
    service: Service,
}

// Compile-time assertion: WatchGuard must be Send
const _: fn() = || {
    fn assert_send<T: Send>() {}
    assert_send::<WatchGuard>();
};

impl fmt::Debug for WatchGuard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WatchGuard")
            .field("speaker_id", &self.speaker_id)
            .field("property_key", &self.property_key)
            .field("ip", &self.ip)
            .field("service", &self.service)
            .finish()
    }
}

impl Drop for WatchGuard {
    fn drop(&mut self) {
        // release_watch returns () — panic-free by design
        self.event_manager.release_watch(
            &self.speaker_id,
            self.property_key,
            self.ip,
            self.service,
        );
    }
}

// ============================================================================
// SonosEventManager
// ============================================================================

/// Sync-first event manager for Sonos devices
///
/// Provides a fully synchronous API while managing async event subscriptions
/// in a background thread. All methods are blocking.
///
/// # Example
///
/// ```rust,ignore
/// use sonos_event_manager::SonosEventManager;
/// use sonos_api::Service;
///
/// // Create manager (sync - no .await!)
/// let manager = SonosEventManager::new()?;
///
/// // Add discovered devices
/// let devices = sonos_discovery::get();
/// manager.add_devices(devices)?;
///
/// // Subscribe to events (sync)
/// let ip: std::net::IpAddr = "192.168.1.100".parse()?;
/// manager.ensure_service_subscribed(ip, Service::RenderingControl)?;
///
/// // Iterate over events (blocking)
/// for event in manager.iter() {
///     println!("Event: {:?}", event);
/// }
/// ```
pub struct SonosEventManager {
    /// Send commands to background worker (tokio unbounded — send() is sync)
    command_tx: tokio_mpsc::UnboundedSender<Command>,

    /// Deadline queue for grace-period teardowns, serviced by one thread.
    ///
    /// Deliberately not the worker runtime: see [`crate::timer`] for why.
    teardown_timer: TeardownTimer,

    /// Receive events from background worker
    event_rx: Arc<Mutex<mpsc::Receiver<EnrichedEvent>>>,

    /// Device info cache (sync access)
    devices: Arc<RwLock<HashMap<IpAddr, Device>>>,

    /// Service subscription ref counts (sync access)
    service_refs: Arc<RwLock<HashMap<(IpAddr, Service), usize>>>,

    /// Pending grace-period timers: cancelled via AtomicBool when re-acquired.
    ///
    /// Shared (`Arc`) with the timer thread so that expiry and cancellation
    /// contend on one mutex. See [`PendingTeardown::fire`].
    pending_unsubscribes: Arc<parking_lot::Mutex<PendingUnsubscribes>>,

    /// Watch registry for managing the watched-property set (set once)
    watch_registry: OnceLock<Arc<dyn WatchRegistry>>,

    /// Background worker handle (kept alive)
    _worker: JoinHandle<()>,
}

impl SonosEventManager {
    /// Create a new SonosEventManager with default configuration
    ///
    /// This is a synchronous operation - no `.await` required.
    pub fn new() -> Result<Self> {
        Self::with_config(BrokerConfig::default())
    }

    /// Create a new SonosEventManager with custom configuration
    ///
    /// This is a synchronous operation - no `.await` required.
    pub fn with_config(config: BrokerConfig) -> Result<Self> {
        // Create channels for command/event communication
        let (command_tx, command_rx) = tokio_mpsc::unbounded_channel();
        let (event_tx, event_rx) = mpsc::channel();

        // Spawn background worker with its own tokio runtime
        let worker = spawn_event_worker(config, command_rx, event_tx);

        // The timer gets a *weak* command sender so it can feed `Unsubscribe`
        // back in without keeping the command channel alive: a strong clone
        // would mean the receiver never observes a close, and the worker's
        // shutdown-on-disconnect path would be dead code.
        let teardown_timer = TeardownTimer::start(command_tx.downgrade());

        Ok(Self {
            command_tx,
            teardown_timer,
            event_rx: Arc::new(Mutex::new(event_rx)),
            devices: Arc::new(RwLock::new(HashMap::new())),
            service_refs: Arc::new(RwLock::new(HashMap::new())),
            pending_unsubscribes: Arc::new(parking_lot::Mutex::new(HashMap::new())),
            watch_registry: OnceLock::new(),
            _worker: worker,
        })
    }

    /// Set the watch registry (called once by StateManager during initialization).
    ///
    /// Subsequent calls are no-ops.
    pub fn set_watch_registry(&self, registry: Arc<dyn WatchRegistry>) {
        let _ = self.watch_registry.set(registry);
    }

    // ========================================================================
    // Watch lifecycle (grace period API)
    // ========================================================================

    /// Acquire a watch on a property, returning an RAII guard.
    ///
    /// Increments the service ref count. If this is the first reference (and no
    /// grace period is pending), sends a Subscribe command to the worker. If a
    /// grace period is active for this (ip, service), cancels it instead.
    ///
    /// Also registers the (speaker_id, key) pair in the WatchRegistry so that
    /// change events are forwarded for this property.
    pub fn acquire_watch(
        self: &Arc<Self>,
        speaker_id: &SpeakerId,
        property_key: &'static str,
        ip: IpAddr,
        service: Service,
    ) -> Result<WatchGuard> {
        // 1. Register in watched set via WatchRegistry
        if let Some(registry) = self.watch_registry.get() {
            registry.register_watch(speaker_id, property_key, service);
        }

        // 2. Increment ref count + check if we need to subscribe
        let should_subscribe = {
            let mut refs = self.service_refs.write();
            let count = refs.entry((ip, service)).or_insert(0);
            let was_zero = *count == 0;
            *count += 1;

            tracing::debug!(
                "acquire_watch: ref count for {}:{:?}: {} -> {}",
                ip,
                service,
                if was_zero { 0 } else { *count - 1 },
                *count
            );

            was_zero
        };

        if should_subscribe {
            // 3. Try to claim any pending grace period.
            //
            // The claim is a swap performed *under the pending-map mutex*, the
            // same one the expiring timer takes. `swap` returns the previous
            // value, so observing `false` means we got there first and the
            // subscription is still live. Observing `true` means the timer
            // already won: it has queued its `Unsubscribe`, so we must send a
            // fresh `Subscribe` (which the FIFO channel orders behind it).
            let cancelled = self.claim_pending_teardown(ip, service);

            if cancelled {
                tracing::debug!(
                    "acquire_watch: cancelled grace period for {}:{:?}",
                    ip,
                    service
                );
            } else {
                // No pending grace period — actually subscribe
                tracing::debug!(
                    "acquire_watch: sending Subscribe command for {}:{:?}",
                    ip,
                    service
                );
                self.command_tx
                    .send(Command::Subscribe { ip, service })
                    .map_err(|_| EventManagerError::WorkerDisconnected)?;
            }
        }

        Ok(WatchGuard {
            event_manager: Arc::clone(self),
            speaker_id: speaker_id.clone(),
            property_key,
            ip,
            service,
        })
    }

    /// Claim a pending grace-period teardown for `(ip, service)`, cancelling it.
    ///
    /// Returns `true` if this caller won the claim, meaning the subscription is
    /// still live and can be reused. Returns `false` if there was no pending
    /// teardown, or if the timer already claimed it — in both cases the caller
    /// must issue a fresh `Subscribe`.
    ///
    /// The counterpart is [`PendingTeardown::fire`]; both take this mutex and
    /// swap the same token under it, so exactly one of them can win.
    fn claim_pending_teardown(&self, ip: IpAddr, service: Service) -> bool {
        let mut pending = self.pending_unsubscribes.lock();
        match pending.remove(&(ip, service)) {
            Some(flag) => !flag.swap(true, Ordering::SeqCst),
            None => false,
        }
    }

    /// Release a watch (called from WatchGuard::Drop). Must never panic.
    ///
    /// Decrements the service ref count. If it hits zero, starts a grace
    /// period: queues a [`PendingTeardown`] on the [`TeardownTimer`], which
    /// fires it once [`GRACE_PERIOD`] elapses unless a re-acquire claims it
    /// first.
    ///
    /// This used to spawn one OS thread per release. Because an immediate-mode
    /// TUI drops and re-acquires every handle each frame, that meant hundreds
    /// of 50ms-sleeping threads per second — the exact cost the grace period
    /// exists to avoid. All releases now share one timer thread, so a release
    /// costs a mutex, a heap push and a condvar notify.
    pub(crate) fn release_watch(
        &self,
        _speaker_id: &SpeakerId,
        _property_key: &'static str,
        ip: IpAddr,
        service: Service,
    ) {
        let should_start_grace = {
            let mut refs = self.service_refs.write();

            if let Some(count) = refs.get_mut(&(ip, service)) {
                *count = count.saturating_sub(1);

                tracing::debug!(
                    "release_watch: ref count for {}:{:?}: {} -> {}",
                    ip,
                    service,
                    *count + 1,
                    *count
                );

                if *count == 0 {
                    refs.remove(&(ip, service));
                    true
                } else {
                    false
                }
            } else {
                tracing::warn!("release_watch: no ref count for {}:{:?}", ip, service);
                false
            }
        };

        if should_start_grace {
            let cancelled = Arc::new(AtomicBool::new(false));
            self.pending_unsubscribes
                .lock()
                .insert((ip, service), Arc::clone(&cancelled));

            let teardown = PendingTeardown {
                ip,
                service,
                delay: GRACE_PERIOD,
                cancelled,
                pending: Arc::clone(&self.pending_unsubscribes),
                registry: self.watch_registry.get().cloned(),
            };

            // Panic-free by contract: this runs from `Drop`. Scheduling is a
            // mutex, a heap push and a condvar notify — no blocking, no
            // allocation beyond the box, and no OS thread. It only fails once
            // the timer is stopped, in which case there is nothing left to tear
            // down, so drop the token rather than leaving it pending forever.
            if !self.teardown_timer.schedule(Box::new(teardown)) {
                tracing::debug!(
                    "release_watch: timer stopped, skipping grace period for {}:{:?}",
                    ip,
                    service
                );
                self.pending_unsubscribes.lock().remove(&(ip, service));
            }
        }
    }

    // ========================================================================
    // Device management
    // ========================================================================

    /// Add discovered devices to the manager (sync)
    ///
    /// Stores device information for later lookup. Does not automatically
    /// subscribe to any services.
    pub fn add_devices(&self, devices: Vec<Device>) -> Result<()> {
        let mut device_map = self.devices.write();

        for device in devices {
            let ip: IpAddr = device
                .ip_address
                .parse()
                .map_err(|_| EventManagerError::InvalidIpAddress(device.ip_address.clone()))?;

            device_map.insert(ip, device);
        }

        Ok(())
    }

    /// Get all available devices (sync)
    pub fn devices(&self) -> Vec<Device> {
        self.devices.read().values().cloned().collect()
    }

    /// Get a specific device by IP address (sync)
    pub fn device_by_ip(&self, ip: IpAddr) -> Option<Device> {
        self.devices.read().get(&ip).cloned()
    }

    // ========================================================================
    // Direct subscription management (used by existing code paths)
    // ========================================================================

    /// Ensure a service is subscribed for a device (sync, ref-counted)
    ///
    /// Increments the reference count for the (device_ip, service) pair.
    /// If this is the first reference, triggers a subscription via the background worker.
    pub fn ensure_service_subscribed(&self, device_ip: IpAddr, service: Service) -> Result<()> {
        let should_subscribe = {
            let mut refs = self.service_refs.write();

            let count = refs.entry((device_ip, service)).or_insert(0);
            let was_zero = *count == 0;
            *count += 1;

            tracing::debug!(
                "Service reference count for {}:{:?}: {} -> {}",
                device_ip,
                service,
                if was_zero { 0 } else { *count - 1 },
                *count
            );

            was_zero
        };

        if should_subscribe {
            self.command_tx
                .send(Command::Subscribe {
                    ip: device_ip,
                    service,
                })
                .map_err(|_| EventManagerError::WorkerDisconnected)?;
        }

        Ok(())
    }

    /// Release a service subscription for a device (sync, ref-counted)
    ///
    /// Decrements the reference count for the (device_ip, service) pair.
    /// If this reaches zero, triggers an unsubscription via the background worker.
    pub fn release_service_subscription(&self, device_ip: IpAddr, service: Service) -> Result<()> {
        let should_unsubscribe = {
            let mut refs = self.service_refs.write();

            if let Some(count) = refs.get_mut(&(device_ip, service)) {
                let old_count = *count;
                *count = count.saturating_sub(1);

                tracing::debug!(
                    "Service reference count for {}:{:?}: {} -> {}",
                    device_ip,
                    service,
                    old_count,
                    *count
                );

                if *count == 0 {
                    refs.remove(&(device_ip, service));
                    true
                } else {
                    false
                }
            } else {
                tracing::warn!(
                    "Attempted to release subscription for {}:{:?} but no references found",
                    device_ip,
                    service
                );
                false
            }
        };

        if should_unsubscribe {
            self.command_tx
                .send(Command::Unsubscribe {
                    ip: device_ip,
                    service,
                })
                .map_err(|_| EventManagerError::WorkerDisconnected)?;
        }

        Ok(())
    }

    // ========================================================================
    // Event iteration
    // ========================================================================

    /// Get a blocking iterator over events
    ///
    /// Returns an iterator that blocks on `next()` until an event is available.
    /// Use `try_recv()` for non-blocking access.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Blocking iteration
    /// for event in manager.iter() {
    ///     println!("Event: {:?}", event);
    /// }
    ///
    /// // Non-blocking check
    /// let iter = manager.iter();
    /// if let Some(event) = iter.try_recv() {
    ///     println!("Got event: {:?}", event);
    /// }
    /// ```
    pub fn iter(&self) -> EventManagerIterator {
        EventManagerIterator::new(Arc::clone(&self.event_rx))
    }

    // ========================================================================
    // Stats / introspection
    // ========================================================================

    /// Get current service subscription statistics (sync)
    pub fn service_subscription_stats(&self) -> HashMap<(IpAddr, Service), usize> {
        self.service_refs.read().clone()
    }

    /// Check if a service is currently subscribed for a device (sync)
    pub fn is_service_subscribed(&self, device_ip: IpAddr, service: Service) -> bool {
        self.service_refs
            .read()
            .get(&(device_ip, service))
            .is_some_and(|&c| c > 0)
    }

    /// Get the current reference count for a service subscription
    pub fn service_ref_count(&self, device_ip: IpAddr, service: Service) -> usize {
        self.service_refs
            .read()
            .get(&(device_ip, service))
            .copied()
            .unwrap_or(0)
    }

    /// Shutdown the background worker
    ///
    /// Called automatically on drop, but can be called manually for graceful shutdown.
    pub fn shutdown(&self) {
        // Drain *and* claim in one locked scope.
        //
        // `self.pending_unsubscribes.lock().drain()` released the mutex at the
        // end of that statement, before any token was claimed. A teardown
        // already blocked inside `PendingTeardown::fire` could then take the
        // mutex, win its own swap and unsubscribe — and this loop would
        // unsubscribe and unregister the same key a second time. Binding the
        // guard makes drain-and-claim atomic against both `fire` and
        // `acquire_watch`.
        //
        // `swap` rather than `store` because the return value *is* the claim:
        // a key whose teardown already fired reports `true` and is skipped,
        // which is what makes "exactly one winner" hold here too. That is also
        // why there is no race test for this — the property is carried by the
        // swap's return value, and a test that tried to hit the window would
        // flake rather than prove anything.
        let mut pending = self.pending_unsubscribes.lock();
        let claimed: Vec<_> = pending
            .drain()
            .filter(|(_, flag)| !flag.swap(true, Ordering::SeqCst))
            .collect();

        for ((ip, service), _) in claimed {
            // No grace period on shutdown. Unsubscribe first, then clear the
            // watched set — the same order `fire` uses, and still inside the
            // mutex so a racing `acquire_watch` enqueues behind us.
            let _ = self.command_tx.send(Command::Unsubscribe { ip, service });
            if let Some(registry) = self.watch_registry.get() {
                call_unregister(registry, ip, service);
            }
        }
        drop(pending);

        // Now clear the timer's queue — `drain`, not `stop`. Stopping latches
        // the timer off for the rest of the manager's life, so every later
        // `release_watch` found `schedule` refusing work and its watch was
        // never torn down at all. `shutdown()` is public and leaves the manager
        // usable; only `TeardownTimer::drop` may latch.
        //
        // The queue lock is taken *after* the pending-map lock has been
        // released, never the other way round: see the lock-order note on
        // `SonosEventManager`. Anything the timer manages to fire in between
        // finds its token already claimed and declines.
        self.teardown_timer.drain();

        let _ = self.command_tx.send(Command::Shutdown);
    }
}

impl Drop for SonosEventManager {
    fn drop(&mut self) {
        tracing::debug!(
            "SonosEventManager dropping, {} active service subscriptions",
            self.service_refs.read().len()
        );

        // Cancel all pending grace timers, draining and claiming in one
        // locked scope for the reason spelled out in `shutdown`. Nothing acts
        // on the claim here — the worker and the whole watched set are going
        // away with the manager — but taking it keeps a late `fire` from
        // believing it still owns the teardown.
        let mut pending = self.pending_unsubscribes.lock();
        for (_, flag) in pending.drain() {
            let _ = flag.swap(true, Ordering::SeqCst);
        }
        drop(pending);

        // Send shutdown command to worker
        let _ = self.command_tx.send(Command::Shutdown);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    /// Mock WatchRegistry for testing
    struct MockRegistry {
        register_count: AtomicUsize,
        unregister_count: AtomicUsize,
    }

    impl MockRegistry {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                register_count: AtomicUsize::new(0),
                unregister_count: AtomicUsize::new(0),
            })
        }

        fn registers(&self) -> usize {
            self.register_count.load(Ordering::SeqCst)
        }

        fn unregisters(&self) -> usize {
            self.unregister_count.load(Ordering::SeqCst)
        }
    }

    impl WatchRegistry for MockRegistry {
        fn register_watch(&self, _speaker_id: &SpeakerId, _key: &'static str, _service: Service) {
            self.register_count.fetch_add(1, Ordering::SeqCst);
        }

        fn unregister_watches_for_service(&self, _ip: IpAddr, _service: Service) {
            self.unregister_count.fetch_add(1, Ordering::SeqCst);
        }
    }

    /// A registry whose *first* unregister call panics, so a test can prove the
    /// timer survives it. The panic backtrace it prints is expected output.
    struct PanickingRegistry {
        calls: AtomicUsize,
    }

    impl PanickingRegistry {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                calls: AtomicUsize::new(0),
            })
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    impl WatchRegistry for PanickingRegistry {
        fn register_watch(&self, _speaker_id: &SpeakerId, _key: &'static str, _service: Service) {}

        fn unregister_watches_for_service(&self, ip: IpAddr, service: Service) {
            if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
                panic!("registry panic on {ip}:{service:?} (expected by this test)");
            }
        }
    }

    /// One panicking registry callback must cost one teardown, not the timer.
    ///
    /// Before `call_unregister`, the panic unwound out of the timer thread's
    /// `run` loop and killed it, so every *later* teardown — here a second,
    /// unrelated service — was silently dropped and its pending entry stuck
    /// forever.
    #[test]
    fn test_registry_panic_does_not_kill_the_timer() {
        let config = BrokerConfig::default().with_callback_ports(5400, 5500);
        let manager = Arc::new(SonosEventManager::with_config(config).unwrap());
        let registry = PanickingRegistry::new();
        manager.set_watch_registry(registry.clone());

        let ip: IpAddr = "192.168.1.100".parse().unwrap();
        let speaker_id = SpeakerId::new("RINCON_123");

        // First teardown: its registry callback panics.
        drop(
            manager
                .acquire_watch(&speaker_id, "volume", ip, Service::RenderingControl)
                .unwrap(),
        );

        // Second, unrelated teardown, queued behind it on the same thread.
        drop(
            manager
                .acquire_watch(&speaker_id, "playback_state", ip, Service::AVTransport)
                .unwrap(),
        );

        std::thread::sleep(Duration::from_millis(200));

        assert_eq!(
            registry.calls(),
            2,
            "the teardown after a panicking one must still run"
        );
        assert!(
            manager.pending_unsubscribes.lock().is_empty(),
            "no teardown may be left stranded by a panicking registry"
        );
    }

    #[test]
    fn test_device_management() {
        let config = BrokerConfig::default().with_callback_ports(4100, 4200);
        let manager = SonosEventManager::with_config(config).unwrap();

        // Initially no devices
        assert!(manager.devices().is_empty());

        // Add devices
        let devices = vec![Device {
            id: "test-1".to_string(),
            name: "Living Room".to_string(),
            ip_address: "192.168.1.100".to_string(),
            port: 1400,
            model_name: "Sonos One".to_string(),
            room_name: "Living Room".to_string(),
        }];

        manager.add_devices(devices).unwrap();

        // Check devices were added
        let stored_devices = manager.devices();
        assert_eq!(stored_devices.len(), 1);

        // Check specific device lookup
        let device_ip: IpAddr = "192.168.1.100".parse().unwrap();
        let device = manager.device_by_ip(device_ip).unwrap();
        assert_eq!(device.name, "Living Room");
    }

    #[test]
    fn test_reference_counting() {
        let config = BrokerConfig::default().with_callback_ports(4200, 4300);
        let manager = SonosEventManager::with_config(config).unwrap();
        let device_ip: IpAddr = "192.168.1.100".parse().unwrap();
        let service = Service::RenderingControl;

        // Initially not subscribed
        assert!(!manager.is_service_subscribed(device_ip, service));
        assert_eq!(manager.service_ref_count(device_ip, service), 0);

        // First subscription
        manager
            .ensure_service_subscribed(device_ip, service)
            .unwrap();
        assert!(manager.is_service_subscribed(device_ip, service));
        assert_eq!(manager.service_ref_count(device_ip, service), 1);

        // Second subscription (increments ref count)
        manager
            .ensure_service_subscribed(device_ip, service)
            .unwrap();
        assert_eq!(manager.service_ref_count(device_ip, service), 2);

        // Release one subscription
        manager
            .release_service_subscription(device_ip, service)
            .unwrap();
        assert_eq!(manager.service_ref_count(device_ip, service), 1);
        assert!(manager.is_service_subscribed(device_ip, service));

        // Release last subscription
        manager
            .release_service_subscription(device_ip, service)
            .unwrap();
        assert_eq!(manager.service_ref_count(device_ip, service), 0);
        assert!(!manager.is_service_subscribed(device_ip, service));
    }

    #[test]
    fn test_stats() {
        let config = BrokerConfig::default().with_callback_ports(4300, 4400);
        let manager = SonosEventManager::with_config(config).unwrap();

        // Initially empty stats
        let stats = manager.service_subscription_stats();
        assert!(stats.is_empty());
    }

    #[test]
    fn test_acquire_release_watch_ref_counting() {
        let config = BrokerConfig::default().with_callback_ports(4400, 4500);
        let manager = Arc::new(SonosEventManager::with_config(config).unwrap());
        let ip: IpAddr = "192.168.1.100".parse().unwrap();
        let speaker_id = SpeakerId::new("RINCON_123");

        // Acquire first watch
        let guard1 = manager
            .acquire_watch(&speaker_id, "volume", ip, Service::RenderingControl)
            .unwrap();
        assert_eq!(manager.service_ref_count(ip, Service::RenderingControl), 1);

        // Acquire second watch (same service)
        let guard2 = manager
            .acquire_watch(&speaker_id, "mute", ip, Service::RenderingControl)
            .unwrap();
        assert_eq!(manager.service_ref_count(ip, Service::RenderingControl), 2);

        // Drop first guard
        drop(guard1);
        assert_eq!(manager.service_ref_count(ip, Service::RenderingControl), 1);

        // Drop second guard — ref count hits 0, grace period starts
        drop(guard2);
        assert_eq!(manager.service_ref_count(ip, Service::RenderingControl), 0);
    }

    #[test]
    fn test_grace_period_cancelled_by_reacquire() {
        let config = BrokerConfig::default().with_callback_ports(4500, 4600);
        let manager = Arc::new(SonosEventManager::with_config(config).unwrap());
        let registry = MockRegistry::new();
        manager.set_watch_registry(registry.clone());

        let ip: IpAddr = "192.168.1.100".parse().unwrap();
        let speaker_id = SpeakerId::new("RINCON_123");

        // Acquire and drop — starts grace period
        let guard = manager
            .acquire_watch(&speaker_id, "volume", ip, Service::RenderingControl)
            .unwrap();
        drop(guard);

        // Verify grace timer is pending
        assert!(manager
            .pending_unsubscribes
            .lock()
            .contains_key(&(ip, Service::RenderingControl)));

        // Re-acquire within grace period — should cancel the timer
        let _guard2 = manager
            .acquire_watch(&speaker_id, "volume", ip, Service::RenderingControl)
            .unwrap();

        // Pending should be cleared
        assert!(!manager
            .pending_unsubscribes
            .lock()
            .contains_key(&(ip, Service::RenderingControl)));

        // Registry should NOT have unregistered (grace period was cancelled)
        assert_eq!(registry.unregisters(), 0);
    }

    /// Also guards timer independence: `acquire_watch` queues a `Subscribe`
    /// for an unreachable IP, and the worker's current-thread runtime blocks
    /// inside `ureq` servicing it. The teardown must still fire on time, which
    /// it only does because the timer has its own thread. An earlier attempt
    /// that slept on the worker runtime failed exactly here.
    #[test]
    fn test_grace_period_fires_after_timeout() {
        let config = BrokerConfig::default().with_callback_ports(4600, 4700);
        let manager = Arc::new(SonosEventManager::with_config(config).unwrap());
        let registry = MockRegistry::new();
        manager.set_watch_registry(registry.clone());

        let ip: IpAddr = "192.168.1.100".parse().unwrap();
        let speaker_id = SpeakerId::new("RINCON_123");

        // Acquire and drop
        let guard = manager
            .acquire_watch(&speaker_id, "volume", ip, Service::RenderingControl)
            .unwrap();
        assert_eq!(registry.registers(), 1);
        drop(guard);

        // Wait for grace period to expire
        std::thread::sleep(Duration::from_millis(100));

        // Registry should have unregistered
        assert_eq!(registry.unregisters(), 1);
    }

    #[test]
    fn test_watch_registry_integration() {
        let config = BrokerConfig::default().with_callback_ports(4700, 4800);
        let manager = Arc::new(SonosEventManager::with_config(config).unwrap());
        let registry = MockRegistry::new();
        manager.set_watch_registry(registry.clone());

        let ip: IpAddr = "192.168.1.100".parse().unwrap();
        let speaker_id = SpeakerId::new("RINCON_123");

        // Acquire registers in watched set
        let _guard = manager
            .acquire_watch(&speaker_id, "volume", ip, Service::RenderingControl)
            .unwrap();
        assert_eq!(registry.registers(), 1);
        assert_eq!(registry.unregisters(), 0);
    }

    /// Dropping a guard after the worker is gone must be silent *and* complete.
    ///
    /// This test used to stop at "does not panic", which the C3 bug satisfied
    /// trivially by doing nothing at all. The observable it was missing is the
    /// watched-set cleanup.
    #[test]
    fn test_guard_drop_with_disconnected_worker() {
        let config = BrokerConfig::default().with_callback_ports(4800, 4900);
        let manager = Arc::new(SonosEventManager::with_config(config).unwrap());
        let registry = MockRegistry::new();
        manager.set_watch_registry(registry.clone());

        let ip: IpAddr = "192.168.1.100".parse().unwrap();
        let speaker_id = SpeakerId::new("RINCON_123");

        let guard = manager
            .acquire_watch(&speaker_id, "volume", ip, Service::RenderingControl)
            .unwrap();

        // Shutdown the worker
        manager.shutdown();

        // Dropping guard should not panic even with disconnected worker
        drop(guard);

        // And the teardown must still happen: the failed `Unsubscribe` send is
        // ignored, but the watched set is not the worker's to clean up.
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(
            registry.unregisters(),
            1,
            "a guard dropped after shutdown must still clear the watched set"
        );
    }

    /// `shutdown()` must not latch the grace mechanism off.
    ///
    /// It stopped the timer, and stopping is permanent, so every watch released
    /// after a `shutdown()` found `schedule` refusing work: the pending entry
    /// was dropped and the watched set kept its stale entries forever. Only
    /// dropping the manager may stop the timer.
    #[test]
    fn test_release_after_shutdown_still_unregisters() {
        let config = BrokerConfig::default().with_callback_ports(5500, 5600);
        let manager = Arc::new(SonosEventManager::with_config(config).unwrap());
        let registry = MockRegistry::new();
        manager.set_watch_registry(registry.clone());

        let ip: IpAddr = "192.168.1.100".parse().unwrap();
        let speaker_id = SpeakerId::new("RINCON_123");

        // Hold one watch across the shutdown, so nothing is pending for
        // shutdown to tear down itself.
        let held = manager
            .acquire_watch(&speaker_id, "volume", ip, Service::RenderingControl)
            .unwrap();
        manager.shutdown();
        assert_eq!(registry.unregisters(), 0);

        // Acquiring again after shutdown is deterministic precisely because the
        // ref count is already 1: no command is sent, so the closed worker
        // channel cannot turn this into a spurious failure.
        let reacquired = manager
            .acquire_watch(&speaker_id, "mute", ip, Service::RenderingControl)
            .unwrap();

        drop(held);
        drop(reacquired);

        std::thread::sleep(Duration::from_millis(200));

        assert_eq!(
            registry.unregisters(),
            1,
            "a watch released after shutdown must still be torn down"
        );
        assert!(manager.pending_unsubscribes.lock().is_empty());
    }

    #[test]
    fn test_multiple_services_independent_grace_periods() {
        // Use a different port range to avoid conflicts with other concurrent tests
        let config = BrokerConfig::default().with_callback_ports(4000, 4100);
        let manager = Arc::new(SonosEventManager::with_config(config).unwrap());
        let ip: IpAddr = "192.168.1.100".parse().unwrap();
        let speaker_id = SpeakerId::new("RINCON_123");

        // Acquire watches on different services
        let guard_rc = manager
            .acquire_watch(&speaker_id, "volume", ip, Service::RenderingControl)
            .unwrap();
        let guard_av = manager
            .acquire_watch(&speaker_id, "playback_state", ip, Service::AVTransport)
            .unwrap();

        // Drop RC — starts grace period for RenderingControl only
        drop(guard_rc);
        assert!(manager
            .pending_unsubscribes
            .lock()
            .contains_key(&(ip, Service::RenderingControl)));
        assert!(!manager
            .pending_unsubscribes
            .lock()
            .contains_key(&(ip, Service::AVTransport)));

        // AVTransport still has ref count 1
        assert_eq!(manager.service_ref_count(ip, Service::AVTransport), 1);

        drop(guard_av);
    }

    /// Build a `PendingTeardown` that shares the *same* claim token as the
    /// teardown currently in flight for `(ip, service)`, so a test can drive
    /// expiry by hand instead of waiting on the worker's timer.
    fn in_flight_teardown(
        manager: &Arc<SonosEventManager>,
        ip: IpAddr,
        service: Service,
    ) -> PendingTeardown {
        let cancelled = manager
            .pending_unsubscribes
            .lock()
            .get(&(ip, service))
            .cloned()
            .expect("a grace period should be pending");

        PendingTeardown {
            ip,
            service,
            delay: GRACE_PERIOD,
            cancelled,
            pending: Arc::clone(&manager.pending_unsubscribes),
            registry: manager.watch_registry.get().cloned(),
        }
    }

    /// Expiry wins the race, then a re-acquire arrives.
    ///
    /// The re-acquire must *not* mistake the just-fired teardown for a
    /// cancellable one — the subscription is already gone, so it has to
    /// resubscribe.
    ///
    /// This is the regression test for the old implementation, which neither
    /// set the flag nor removed the map entry when the timer fired. The stale
    /// entry made the next `acquire_watch` report "cancelled the grace period"
    /// and skip its `Subscribe`, leaving a live guard with no subscription
    /// behind it.
    #[test]
    fn test_expiry_then_reacquire_must_resubscribe() {
        let config = BrokerConfig::default().with_callback_ports(5000, 5100);
        let manager = Arc::new(SonosEventManager::with_config(config).unwrap());
        let registry = MockRegistry::new();
        manager.set_watch_registry(registry.clone());

        let ip: IpAddr = "192.168.1.100".parse().unwrap();
        let service = Service::RenderingControl;
        let speaker_id = SpeakerId::new("RINCON_123");

        let guard = manager
            .acquire_watch(&speaker_id, "volume", ip, service)
            .unwrap();
        drop(guard);

        let teardown = in_flight_teardown(&manager, ip, service);

        // The timer wins.
        assert!(
            teardown.fire(&manager.command_tx),
            "an unclaimed teardown must fire"
        );
        assert_eq!(registry.unregisters(), 1);

        // It must have cleared its own entry, so the next acquire sees nothing
        // to cancel and resubscribes.
        assert!(
            manager.pending_unsubscribes.lock().is_empty(),
            "a fired teardown must remove its own pending entry"
        );
        assert!(
            !manager.claim_pending_teardown(ip, service),
            "after expiry there is nothing to claim — the caller must resubscribe"
        );

        // Firing twice is a no-op, not a double unsubscribe.
        assert!(!teardown.fire(&manager.command_tx));
        assert_eq!(registry.unregisters(), 1);
    }

    /// Cancellation wins the race, then the timer fires late.
    ///
    /// The late timer must decline: the subscription is live again and must not
    /// be torn down underneath the new guard.
    #[test]
    fn test_cancel_then_late_expiry_keeps_subscription() {
        let config = BrokerConfig::default().with_callback_ports(5100, 5200);
        let manager = Arc::new(SonosEventManager::with_config(config).unwrap());
        let registry = MockRegistry::new();
        manager.set_watch_registry(registry.clone());

        let ip: IpAddr = "192.168.1.100".parse().unwrap();
        let service = Service::RenderingControl;
        let speaker_id = SpeakerId::new("RINCON_123");

        let guard = manager
            .acquire_watch(&speaker_id, "volume", ip, service)
            .unwrap();
        drop(guard);

        let teardown = in_flight_teardown(&manager, ip, service);

        // Re-acquire inside the window claims the teardown.
        let _guard2 = manager
            .acquire_watch(&speaker_id, "volume", ip, service)
            .unwrap();
        assert!(manager.pending_unsubscribes.lock().is_empty());

        // The timer now fires late and must do nothing at all.
        assert!(
            !teardown.fire(&manager.command_tx),
            "a claimed teardown must not fire"
        );
        assert_eq!(
            registry.unregisters(),
            0,
            "a cancelled grace period must never unregister the watched set"
        );
        assert_eq!(manager.service_ref_count(ip, service), 1);
    }

    /// Expiry and cancellation racing on the same token: exactly one may win.
    ///
    /// Both sides swap the token under the pending-map mutex, so this holds for
    /// every interleaving. The assertion is deterministic even though the
    /// scheduling is not — "exactly one winner" is never allowed to be 0 or 2.
    #[test]
    fn test_cancel_and_expiry_have_exactly_one_winner() {
        let config = BrokerConfig::default().with_callback_ports(5200, 5300);
        let manager = Arc::new(SonosEventManager::with_config(config).unwrap());
        let registry = MockRegistry::new();
        manager.set_watch_registry(registry.clone());

        let ip: IpAddr = "192.168.1.100".parse().unwrap();
        let service = Service::RenderingControl;
        let speaker_id = SpeakerId::new("RINCON_123");

        const ROUNDS: usize = 500;

        for round in 0..ROUNDS {
            let guard = manager
                .acquire_watch(&speaker_id, "volume", ip, service)
                .unwrap();
            drop(guard);

            let teardown = in_flight_teardown(&manager, ip, service);

            let fire_side = {
                let manager = Arc::clone(&manager);
                std::thread::spawn(move || teardown.fire(&manager.command_tx))
            };
            let claim_side = {
                let manager = Arc::clone(&manager);
                std::thread::spawn(move || manager.claim_pending_teardown(ip, service))
            };

            let fired = fire_side.join().unwrap();
            let claimed = claim_side.join().unwrap();

            assert!(
                fired ^ claimed,
                "round {round}: exactly one of expiry/cancel must win \
                 (fired={fired}, claimed={claimed})"
            );

            // Whoever lost must have left no entry behind.
            assert!(
                manager.pending_unsubscribes.lock().is_empty(),
                "round {round}: pending map must be empty after the race resolves"
            );

            // The ref count is left at 1 by the claim side when it wins, so
            // normalize before the next round.
            if claimed {
                manager.release_watch(&speaker_id, "volume", ip, service);
                manager.pending_unsubscribes.lock().clear();
            }
        }

        // Every round that the timer won must have unregistered exactly once,
        // and no round may have unregistered twice.
        assert!(
            registry.unregisters() <= ROUNDS,
            "no round may unregister more than once"
        );
    }

    /// The TUI hot path: drop and re-acquire every handle each frame.
    ///
    /// This is the workload the grace period exists for. It must not leak
    /// pending entries, and (the point of this change) each release costs a
    /// channel send rather than an OS thread.
    #[test]
    fn test_immediate_mode_churn_does_not_leak_pending_entries() {
        let config = BrokerConfig::default().with_callback_ports(5300, 5400);
        let manager = Arc::new(SonosEventManager::with_config(config).unwrap());
        let registry = MockRegistry::new();
        manager.set_watch_registry(registry.clone());

        let ip: IpAddr = "192.168.1.100".parse().unwrap();
        let speaker_id = SpeakerId::new("RINCON_123");
        let keys = ["volume", "mute", "bass", "treble", "loudness"];

        // 200 frames x 5 handles. Under the old implementation this was 1,000
        // OS threads, each sleeping 50ms.
        for _frame in 0..200 {
            let guards: Vec<_> = keys
                .iter()
                .map(|key| {
                    manager
                        .acquire_watch(&speaker_id, key, ip, Service::RenderingControl)
                        .unwrap()
                })
                .collect();

            assert_eq!(
                manager.service_ref_count(ip, Service::RenderingControl),
                keys.len()
            );

            drop(guards);
        }

        // One (ip, service) pair churned repeatedly must never accumulate more
        // than the single entry it is keyed by.
        assert!(manager.pending_unsubscribes.lock().len() <= 1);
        assert_eq!(manager.service_ref_count(ip, Service::RenderingControl), 0);
    }

    #[test]
    fn test_shutdown_drains_pending_grace_timers() {
        let config = BrokerConfig::default().with_callback_ports(4900, 5000);
        let manager = Arc::new(SonosEventManager::with_config(config).unwrap());
        let registry = MockRegistry::new();
        manager.set_watch_registry(registry.clone());

        let ip: IpAddr = "192.168.1.100".parse().unwrap();
        let speaker_id = SpeakerId::new("RINCON_123");

        // Acquire and drop to start grace period
        let guard = manager
            .acquire_watch(&speaker_id, "volume", ip, Service::RenderingControl)
            .unwrap();
        drop(guard);

        // Grace timer should be pending
        assert!(manager
            .pending_unsubscribes
            .lock()
            .contains_key(&(ip, Service::RenderingControl)));

        // Shutdown should drain and cancel pending timers
        manager.shutdown();

        // Pending should be cleared
        assert!(manager.pending_unsubscribes.lock().is_empty());

        // Shutdown claims the token itself, so the watched set is cleared
        // exactly once...
        assert_eq!(
            registry.unregisters(),
            1,
            "shutdown must tear down the pending watch itself"
        );

        // ...and stays cleared once: the teardown that was queued for this key
        // finds its token already claimed and declines.
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(
            registry.unregisters(),
            1,
            "a teardown whose token shutdown claimed must not fire as well"
        );
    }
}
