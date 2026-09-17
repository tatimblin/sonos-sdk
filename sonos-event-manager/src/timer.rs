//! Grace-period teardown timer.
//!
//! One thread per [`SonosEventManager`](crate::SonosEventManager), servicing a
//! deadline-ordered queue of pending teardowns.
//!
//! # Why a thread, and why only one
//!
//! `release_watch` runs from `WatchGuard::drop`, and the grace period exists so
//! an immediate-mode TUI can drop and re-acquire every handle each frame. That
//! makes it a hot path: ~9 handles at 60fps is ~540 releases/sec. The previous
//! implementation spawned one OS thread *per release*, each sleeping 50ms, so
//! the optimization paid for itself several hundred times over in thread
//! creation. Here a release costs a mutex, a heap push and a condvar notify.
//!
//! The timer deliberately does **not** live on the event worker's tokio
//! runtime. That runtime is `new_current_thread`, and the UPnP subscribe path
//! reaches blocking `ureq` calls inside `async fn`s without `spawn_blocking`.
//! A single SUBSCRIBE to an unreachable speaker therefore wedges the whole
//! runtime, and a `tokio::time::sleep` scheduled on it will not fire until the
//! network I/O returns — which can be seconds, or the socket timeout. Teardown
//! timing has to be independent of broker health, so it gets its own thread.

use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::sync::Arc;
use std::time::Instant;

use parking_lot::{Condvar, Mutex};
use tokio::sync::mpsc as tokio_mpsc;

use crate::manager::PendingTeardown;
use crate::worker::Command;

/// A teardown waiting for its deadline.
struct Scheduled {
    due: Instant,
    teardown: Box<PendingTeardown>,
}

// Ordered by deadline only, and *reversed*, so `BinaryHeap` (a max-heap) yields
// the earliest deadline first.
impl Ord for Scheduled {
    fn cmp(&self, other: &Self) -> Ordering {
        other.due.cmp(&self.due)
    }
}

impl PartialOrd for Scheduled {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for Scheduled {
    fn eq(&self, other: &Self) -> bool {
        self.due == other.due
    }
}

impl Eq for Scheduled {}

#[derive(Default)]
struct Queue {
    heap: BinaryHeap<Scheduled>,
    stopped: bool,
}

struct Shared {
    queue: Mutex<Queue>,
    /// Signalled when a teardown is enqueued or the timer is stopped.
    wakeup: Condvar,

    /// Times [`timer_thread`] has restarted `run` after catching a panic.
    ///
    /// Test-only, and the only way the restart loop is observable at all: it
    /// exists so that a *later* teardown still fires, which is exactly what a
    /// surviving `call_unregister` also produces. Without this counter a test
    /// that watches outcomes cannot tell the two layers apart, and deleting
    /// either one leaves the suite green.
    #[cfg(test)]
    restarts: std::sync::atomic::AtomicUsize,

    /// Test-only panic injection: when set, `run` panics once, at the point
    /// where it is holding the queue lock.
    ///
    /// The restart loop cannot be reached through the registry — the whole
    /// point of `call_unregister` is that a registry panic never gets this far
    /// — so the second layer has to be driven directly to be tested at all.
    /// Panicking under the queue guard is deliberate: it also exercises the
    /// claim on [`timer_thread`] that `parking_lot` guards unlock on unwind.
    #[cfg(test)]
    panic_once: std::sync::atomic::AtomicBool,
}

/// Handle to the timer thread. Stopping is idempotent and happens on drop.
pub(crate) struct TeardownTimer {
    shared: Arc<Shared>,
}

impl TeardownTimer {
    /// Start the timer thread.
    ///
    /// `command_tx` is weak on purpose: a strong sender parked in this thread
    /// would hold the worker's command channel open forever, so the worker
    /// would never observe the disconnect that tells it to shut down.
    pub(crate) fn start(command_tx: tokio_mpsc::WeakUnboundedSender<Command>) -> Self {
        let shared = Arc::new(Shared {
            queue: Mutex::new(Queue::default()),
            wakeup: Condvar::new(),
            #[cfg(test)]
            restarts: std::sync::atomic::AtomicUsize::new(0),
            #[cfg(test)]
            panic_once: std::sync::atomic::AtomicBool::new(false),
        });

        let worker_shared = Arc::clone(&shared);
        if let Err(e) = std::thread::Builder::new()
            .name("sonos-teardown-timer".to_string())
            .spawn(move || timer_thread(&worker_shared, &command_tx))
        {
            // No servicing thread means nothing enqueued here would ever fire.
            // Latch the timer off so `schedule` refuses immediately and hands
            // the teardown back to the caller to resolve inline: silently
            // accumulating never-fired teardowns at 540 releases/sec is the one
            // outcome nobody can diagnose from the outside.
            tracing::error!(
                "Failed to spawn teardown timer thread ({}); grace periods will be \
                 resolved inline instead",
                e
            );
            shared.queue.lock().stopped = true;
        }

        Self { shared }
    }

    /// Enqueue a teardown to fire after its own `delay`.
    ///
    /// Never panics — this is reached from `Drop`. On refusal the teardown is
    /// handed back in the `Err`, because a refused teardown still has to be
    /// resolved by somebody; the caller owns it from that point.
    ///
    /// There are exactly two refusals:
    ///
    /// 1. the timer is stopped — its thread failed to spawn, or the manager is
    ///    being dropped;
    /// 2. `now + delay` is not a representable [`Instant`]. `Instant + Duration`
    ///    *panics* on overflow, and `delay` rides on the teardown rather than
    ///    being a constant here, so the day the grace period becomes
    ///    configurable a bad value must not become a panic inside `Drop`.
    pub(crate) fn schedule(
        &self,
        teardown: Box<PendingTeardown>,
    ) -> Result<(), Box<PendingTeardown>> {
        let Some(due) = Instant::now().checked_add(teardown.delay) else {
            tracing::error!(
                "Grace period of {:?} for {}:{:?} is not a representable deadline; \
                 handing the teardown back to be resolved without one",
                teardown.delay,
                teardown.ip,
                teardown.service
            );
            return Err(teardown);
        };

        let mut queue = self.shared.queue.lock();
        if queue.stopped {
            return Err(teardown);
        }

        // Only the earliest deadline needs to wake the thread; anything later
        // is already covered by the timeout it is currently waiting on.
        let is_earliest = queue.heap.peek().is_none_or(|next| due < next.due);
        queue.heap.push(Scheduled { due, teardown });
        drop(queue);

        if is_earliest {
            self.shared.wakeup.notify_one();
        }

        Ok(())
    }

    /// Discard everything currently queued, leaving the timer running.
    ///
    /// `SonosEventManager::shutdown` uses this rather than [`stop`](Self::stop):
    /// it has already claimed every pending token by hand, so the queued
    /// teardowns have nothing left to resolve, but the manager stays usable and
    /// a watch released afterwards must still get its grace period. Latching
    /// the timer off is `Drop`'s job alone.
    ///
    /// No notify: the thread wakes at the deadline it was already waiting on,
    /// finds an empty heap and goes back to waiting.
    pub(crate) fn drain(&self) {
        self.shared.queue.lock().heap.clear();
    }

    /// Stop the timer permanently, discarding anything still pending.
    ///
    /// Idempotent, and **one-way**: a stopped timer refuses all later work, so
    /// this belongs to [`Drop`] and to tests. Anything that leaves the manager
    /// alive wants [`drain`](Self::drain) instead.
    pub(crate) fn stop(&self) {
        let mut queue = self.shared.queue.lock();
        queue.stopped = true;
        queue.heap.clear();
        drop(queue);

        self.shared.wakeup.notify_all();
    }

    /// Number of teardowns currently queued. Test/introspection only.
    #[cfg(test)]
    pub(crate) fn queued(&self) -> usize {
        self.shared.queue.lock().heap.len()
    }

    /// Times the thread body has restarted `run` after a panic.
    ///
    /// Test-only. A teardown whose registry callback panicked must leave this
    /// at 0: `call_unregister` is the layer that is supposed to contain it, and
    /// reaching the restart loop means it did not.
    #[cfg(test)]
    pub(crate) fn restarts(&self) -> usize {
        self.shared
            .restarts
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Make the timer thread panic once, from inside `run`. Test-only.
    ///
    /// Wakes the thread so the injected panic is reached without waiting for a
    /// deadline.
    #[cfg(test)]
    pub(crate) fn inject_panic(&self) {
        self.shared
            .panic_once
            .store(true, std::sync::atomic::Ordering::SeqCst);
        self.shared.wakeup.notify_all();
    }
}

impl Drop for TeardownTimer {
    fn drop(&mut self) {
        // Signal only; deliberately no join.
        //
        // The guarantee is ownership, not timing. The thread holds an
        // `Arc<Shared>` and a `WeakUnboundedSender<Command>` and nothing else:
        // it borrows no manager state, so there is nothing it can outlive. The
        // `Arc` keeps `Shared` alive for exactly as long as the thread needs
        // it, and the weak sender simply fails to upgrade once the manager's
        // command channel is gone. Stopping is therefore sufficient — the
        // thread wakes, sees `stopped`, and exits on its own.
        //
        // Joining would block a `Drop` on a thread that may be inside
        // `PendingTeardown::fire`, and therefore inside an implementor-supplied
        // registry callback: a hang with no upper bound and no diagnosis.
        //
        // The cost is worth stating plainly rather than hiding: dropping the
        // manager *does* block for one in-flight registry callback, because
        // `SonosEventManager::drop` takes the pending-map mutex that `fire`
        // holds across it. That is the visible price of the ordering guarantee
        // documented on `fire`, and it is bounded by the `WatchRegistry`
        // contract, not by this `Drop`.
        self.stop();
    }
}

/// Thread body: `run` under a restart loop.
///
/// A panic escaping `run` — from a registry callback that slipped past
/// `call_unregister`, or from the timer's own code — would otherwise terminate
/// the single thread that services *every* teardown, silently and permanently.
/// One panicking teardown must cost one teardown, not the service.
///
/// Restarting is safe because the thread owns no state: everything lives behind
/// `Arc<Shared>`, `parking_lot` mutexes do not poison, their guards unlock on
/// unwind, and the teardown that panicked was already popped off the heap. So
/// there is nothing to replay and no restart storm to fear.
fn timer_thread(shared: &Arc<Shared>, command_tx: &tokio_mpsc::WeakUnboundedSender<Command>) {
    loop {
        let outcome =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(shared, command_tx)));

        if outcome.is_ok() {
            // `run` returns only when the timer has been stopped.
            return;
        }

        tracing::error!("Teardown timer thread panicked; resuming the timer loop");

        #[cfg(test)]
        shared
            .restarts
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);

        if shared.queue.lock().stopped {
            return;
        }
    }
}

fn run(shared: &Arc<Shared>, command_tx: &tokio_mpsc::WeakUnboundedSender<Command>) {
    loop {
        let teardown = {
            let mut queue = shared.queue.lock();

            loop {
                if queue.stopped {
                    tracing::debug!("Teardown timer stopped");
                    return;
                }

                // Test-only, compiled out of every real build. Panics while
                // `queue` is held on purpose: see `Shared::panic_once`.
                #[cfg(test)]
                if shared
                    .panic_once
                    .swap(false, std::sync::atomic::Ordering::SeqCst)
                {
                    panic!("injected teardown timer panic (expected by this test)");
                }

                // Copy the deadline out so the borrow on `queue` ends before
                // the wait below re-borrows it mutably.
                match queue.heap.peek().map(|next| next.due) {
                    None => shared.wakeup.wait(&mut queue),
                    Some(due) if due <= Instant::now() => break,
                    Some(due) => {
                        shared.wakeup.wait_until(&mut queue, due);
                    }
                }
            }

            queue.heap.pop()
        };

        // Fire outside the queue lock: `fire` takes the manager's pending-map
        // mutex and calls into the watch registry.
        if let Some(Scheduled { teardown, .. }) = teardown {
            match command_tx.upgrade() {
                Some(command_tx) => {
                    teardown.fire(&command_tx);
                }
                None => tracing::debug!(
                    "Grace period for {}:{:?} outlived the manager, skipping teardown",
                    teardown.ip,
                    teardown.service
                ),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_earliest_deadline_pops_first() {
        let now = Instant::now();
        let mut heap = BinaryHeap::new();

        // Push out of order; the heap must still yield soonest-first.
        for offset_ms in [90_u64, 10, 50] {
            heap.push(Scheduled {
                due: now + std::time::Duration::from_millis(offset_ms),
                teardown: Box::new(crate::manager::test_support::dummy_teardown()),
            });
        }

        let order: Vec<_> = std::iter::from_fn(|| heap.pop())
            .map(|s| s.due.duration_since(now).as_millis())
            .collect();

        assert_eq!(order, vec![10, 50, 90]);
    }

    #[test]
    fn test_schedule_after_stop_is_refused() {
        let (tx, _rx) = tokio_mpsc::unbounded_channel::<Command>();
        let timer = TeardownTimer::start(tx.downgrade());

        timer.stop();

        assert!(
            timer
                .schedule(Box::new(crate::manager::test_support::dummy_teardown()))
                .is_err(),
            "a stopped timer must hand the teardown back rather than swallow it"
        );
        assert_eq!(timer.queued(), 0);
    }

    /// Wait for `cond`, polling, up to `limit`. Returns whether it came true.
    ///
    /// Used instead of a flat sleep so the restart test states the outcome it
    /// is waiting for rather than a duration that happens to be long enough.
    #[cfg(test)]
    fn wait_for(limit: std::time::Duration, mut cond: impl FnMut() -> bool) -> bool {
        let deadline = Instant::now() + limit;
        while Instant::now() < deadline {
            if cond() {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        cond()
    }

    /// A panic that escapes `run` must restart the loop, not end the service.
    ///
    /// This is the second `catch_unwind` layer, and the only test that reaches
    /// it: `call_unregister` contains every registry panic, so the restart loop
    /// is unreachable through the public path by design. The panic is therefore
    /// injected directly, from under the queue lock, which also pins the claim
    /// that `parking_lot` guards unlock on unwind — if they did not, the
    /// schedule below would deadlock rather than fire.
    ///
    /// The panic backtrace this prints is expected output.
    #[test]
    fn test_timer_thread_restarts_after_a_panic() {
        let (tx, _rx) = tokio_mpsc::unbounded_channel::<Command>();
        let timer = TeardownTimer::start(tx.downgrade());

        assert_eq!(timer.restarts(), 0, "a healthy timer never restarts");

        timer.inject_panic();

        assert!(
            wait_for(std::time::Duration::from_secs(2), || timer.restarts() == 1),
            "a panic escaping `run` must restart the loop, not kill the thread \
             (restarts = {})",
            timer.restarts()
        );

        // Restarting is only worth anything if the thread still services the
        // queue afterwards.
        let mut teardown = crate::manager::test_support::dummy_teardown();
        teardown.delay = std::time::Duration::from_millis(1);
        timer
            .schedule(Box::new(teardown))
            .expect("a restarted timer must still accept work");

        assert!(
            wait_for(std::time::Duration::from_secs(2), || timer.queued() == 0),
            "a restarted timer must still fire what it is handed"
        );
        assert_eq!(
            timer.restarts(),
            1,
            "the restart must happen once, not in a storm"
        );
    }

    #[test]
    fn test_unrepresentable_deadline_is_refused() {
        let (tx, _rx) = tokio_mpsc::unbounded_channel::<Command>();
        let timer = TeardownTimer::start(tx.downgrade());

        let mut teardown = crate::manager::test_support::dummy_teardown();
        teardown.delay = std::time::Duration::MAX;

        assert!(
            timer.schedule(Box::new(teardown)).is_err(),
            "a deadline that cannot be represented must be refused, not panicked on"
        );
        assert_eq!(timer.queued(), 0);
    }
}
