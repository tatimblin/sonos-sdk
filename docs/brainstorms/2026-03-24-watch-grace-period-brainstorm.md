# Watch Grace Period / Debounced Unsubscribe

**Date:** 2026-03-24
**Status:** Landed
**Crates affected:** sonos-event-manager, sonos-state, sonos-sdk

## What It Explored

How to stop `watch()` from churning UPnP subscriptions when a caller acquires and releases
a watch rapidly — the TUI case, where a draw method runs at 60 FPS.

## What Shipped

An RAII `WatchHandle` plus a short teardown delay, then hardened by a shared timer thread.

- `PropertyHandle::watch()` returns `WatchHandle<P>`; there is no `unwatch()` on the
  handle. `WatchHandle` is `#[must_use]` — dropping it schedules the teardown.
- `GRACE_PERIOD` is 50 ms (`sonos-event-manager/src/manager.rs`), carried per-teardown on
  `PendingTeardown::delay` so it is configurable later without touching call sites.
- `WatchHandle` is a **live view, not a snapshot**. It holds a read closure rather than a
  cached value, so `value()` re-reads the store on every call and one handle held across a
  loop reports every change. There is no `Deref` impl — the accessors are `value()`,
  `has_value()`, `mode()` and `has_realtime_events()`.
- Cleanup is `WatchCleanup`, a three-variant enum (`Guard`, `CacheOnly`,
  `CoordinatorGuard`), not a bare `WatchGuard` — the cache-only and per-coordinator paths
  need different teardown.
- `WatchGuard::drop` calls `release_watch`, which schedules a `PendingTeardown` on a single
  shared OS thread (`sonos-event-manager/src/timer.rs`) servicing a deadline-ordered heap.
  It is deliberately **off** the tokio runtime: the worker runtime is `new_current_thread`
  and the subscribe path makes blocking calls, so a `tokio::time::sleep` there would not
  fire while a SUBSCRIBE to an unreachable speaker was in flight.
- Cancellation is a **claim token**, not a post-sleep flag check. Each pending teardown
  owns an `AtomicBool`; whoever swaps it `false`→`true` owns the outcome. The cancelling
  `acquire_watch` and the expiring timer both swap while holding the pending map's mutex,
  so exactly one wins. `acquire_watch` claims *before* it registers the watch — the reverse
  order produced a live guard and a live subscription whose `(speaker, key)` pair was not
  watched, so every event for it was filtered.
- Pending teardowns are keyed `(IpAddr, Service)`, not `(speaker_id, property_key)`, and
  teardown unregisters per `(ip, service)`.
- Reference counting is unchanged: overlapping watches share one subscription, released
  when the last holder drops.

`register_watch`, `ensure_service_subscribed` and `release_service_subscription` all remain
public API; the grace period was added alongside them rather than replacing them.
`StateManager::unwatch_property_with_subscription` also remains.

## Where To Read About It Now

- [docs/WATCH_GRACE_PERIOD_DEMO.md](../WATCH_GRACE_PERIOD_DEMO.md) — the model and the demo
- [docs/watchable-properties.md](../watchable-properties.md) — handle lifetime guidance
- `docs/specs/sonos-event-manager.md` — the timer, the claim protocol and lock ordering
- `docs/plans/2026-03-25-feat-watch-grace-period-raii-guard-plan.md`
- `docs/plans/2026-09-17-teardown-timer-review-fixes-plan.md`
