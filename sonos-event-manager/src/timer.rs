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
        });

        let worker_shared = Arc::clone(&shared);
        std::thread::Builder::new()
            .name("sonos-teardown-timer".to_string())
            .spawn(move || run(&worker_shared, &command_tx))
            // A failure to spawn leaves `shared` with no servicing thread:
            // teardowns still enqueue and are simply never fired, which is the
            // same degraded-but-silent outcome as a dead worker.
            .map_err(|e| tracing::warn!("Failed to spawn teardown timer thread: {}", e))
            .ok();

        Self { shared }
    }

    /// Enqueue a teardown to fire after its own `delay`.
    ///
    /// Never panics — this is reached from `Drop`. Returns `false` if the timer
    /// is already stopped, in which case the caller owns the cleanup.
    pub(crate) fn schedule(&self, teardown: Box<PendingTeardown>) -> bool {
        let due = Instant::now() + teardown.delay;

        let mut queue = self.shared.queue.lock();
        if queue.stopped {
            return false;
        }

        // Only the earliest deadline needs to wake the thread; anything later
        // is already covered by the timeout it is currently waiting on.
        let is_earliest = queue.heap.peek().is_none_or(|next| due < next.due);
        queue.heap.push(Scheduled { due, teardown });
        drop(queue);

        if is_earliest {
            self.shared.wakeup.notify_one();
        }

        true
    }

    /// Stop the timer, discarding anything still pending. Idempotent.
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
}

impl Drop for TeardownTimer {
    fn drop(&mut self) {
        // Signal only; deliberately no join. The thread may be inside
        // `PendingTeardown::fire`, which reaches into the watch registry, and
        // blocking a `Drop` on that invites lock-order surprises during
        // teardown. It owns nothing but `Arc`s and exits on its own.
        self.stop();
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
            !timer.schedule(Box::new(crate::manager::test_support::dummy_teardown())),
            "a stopped timer must refuse work rather than silently swallow it"
        );
        assert_eq!(timer.queued(), 0);
    }
}
