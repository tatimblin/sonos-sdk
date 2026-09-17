# Plan: resolve the review findings on PR #108

**Date**: 2026-09-17
**Status**: Completed — all 11 findings (C1–C11) are implemented at HEAD
**PR**: https://github.com/tatimblin/sonos-sdk/pull/108 (`fix/watch-teardown-timer` → `main`, base commit `ef0ba44`)
**Origin**: code review of PR #108; findings C1–C11

## Two facts that shape everything below

1. **Pre-PR `release_watch` used `std::thread::spawn`, which panics on spawn failure** — from `Drop`. So `main`'s C4 behaviour is a panic in `Drop`, not a leak. The PR leaks instead. Neither is right.
2. **`acquire_watch` calls `registry.register_watch` *before* it takes the pending mutex.** This is load-bearing and undocumented. It is why the C2 remedy as filed is unsafe.

## Invariants no fix may break

1. `release_watch` runs from `WatchGuard::drop` and must never panic.
2. The `Unsubscribe` send stays **inside** the pending-map mutex — that is what forces a racing `acquire_watch`'s `Subscribe` to enqueue behind it.
3. The teardown timer must not run on the worker's tokio runtime (blocking `ureq` inside `async fn` on `new_current_thread`; one SUBSCRIBE to an unreachable speaker wedged it 5,002.7 ms while the dedicated thread fired at 50.4 ms).
4. Public API unchanged: `acquire_watch` / `release_watch` / `WatchGuard`, sync-first blocking design.
5. `GRACE_PERIOD` stays 50 ms here; `PendingTeardown::delay` stays per-teardown so configurability is later plumbing only.

---

## C1 — decision: `catch_unwind` at two depths, no supervisor thread

Swapping N failure domains for one is only a regression if the one domain is *terminable*. It is terminable today because a panic escapes `run` and nothing restarts it. Make the loop non-terminating and the argument dissolves: one panicking teardown costs one teardown, not the service.

A supervisor thread buys nothing over an in-thread restart loop and costs a thread, a join handle, a liveness protocol, and a shutdown race with `TeardownTimer::drop`. **Rejected.**

**Depth A** — `call_unregister(registry, ip, service)` free function in `manager.rs` wrapping the callback in `catch_unwind(AssertUnwindSafe(..))`, logging `tracing::error!` with `(ip, service)` on panic. Called from `PendingTeardown::fire` and from `shutdown`. `AssertUnwindSafe` is needed because `Arc<dyn WatchRegistry>` is not `RefUnwindSafe`; `catch_unwind` is safe, so `#![forbid(unsafe_code)]` is unaffected. Catch *inside* the `pending` guard scope on purpose — the guard is never unwound through, and `fire` still returns `true`, keeping "exactly one winner" true in the panic case.

**Depth B** — wrap the thread body in `timer.rs::start` in a restart loop: `catch_unwind` around `run(..)`; `Ok` means the timer was stopped, return; `Err` logs and re-enters unless `queue.lock().stopped`. Safe because all state is in `Arc<Shared>`, `parking_lot` does not poison, guards unlock on unwind, and the panicking teardown was already popped — no restart storm.

**Contract to write on the `WatchRegistry` trait:** implementations run on the shared teardown thread while the pending-map mutex is held. Short, non-blocking, no I/O, must not re-enter `SonosEventManager`. A panic is caught and logged; a *hang* is not recoverable and blocks all teardowns.

## C2 — decision: reject the filed remedy, fix the ordering bug it would expose, decline the offload

Holding `pending` across `unregister_watches_for_service` is currently the **only** thing ordering the watched-set mutation against `acquire_watch`. Adding `drop(pending)` before the callback opens this interleaving on one `(ip, service)` — exactly the TUI frame loop:

```
acquire_watch: registry.register_watch(speaker, "volume", RC)   // subscription = true
fire:          lock(pending); swap(token) -> wins; remove entry;
               send Unsubscribe; drop(pending)                  // <- the proposed change
acquire_watch: lock(pending); nothing to claim -> send Subscribe; returns a live guard
fire:          registry.unregister_watches_for_service(ip, RC)  // subscription = false
```

Result: a live `WatchGuard` and a live UPnP subscription whose `(speaker, "volume")` pair is **not** in the watched set, so `StateManager::emit_change` filters every event until the guard drops. Same class of bug the PR was written to fix, re-entered through the registry. Today the window is a few hundred nanoseconds; `drop(pending)` widens it to the whole callback.

- **2a.** Do not add `drop(pending)`. Comment at the top of `fire` naming this interleaving.
- **2b.** Close the existing narrow window: move `registry.register_watch` in `acquire_watch` from step 1 to *after* the claim and *before* the `Subscribe` send. No new lock nesting — `register_watch` still runs with no manager lock held.
- **2c.** Decline the offload **permanently**. An offload puts `unregister` on a queue `register_watch` is not on — precisely the 2a reordering. If a registry needs slow work, the deferral belongs inside the registry, which alone can order its own mutations. The measured 327 ms came from a 400 ms stress callback that violates the §C1 contract; the real callback is microseconds.
- **2d.** Defer the *granularity* fix (shard `pending_unsubscribes`, or per-key token locks) with a trigger: any registry callback measured above 100 µs, or observed contention. Invariant 2 only needs same-key serialization, which a per-key lock satisfies.
- **Deferred:** the sibling-key variant (thread B acquiring `"mute"` while A acquires `"volume"` takes the `should_subscribe == false` path and never touches the pending mutex). Closing it needs the `service_refs` increment under the pending mutex — changes the manager's whole locking shape. Unreachable on the single-threaded TUI path. Record in spec §14.1.

---

## Ordered steps

Order is dependency-driven: C1 first because C4's fallback calls `fire` from `Drop` and needs it panic-free; C6 before C3 because C3 rewrites the same `shutdown` body; C9 after C4 because it reuses C4's refusal contract.

| # | Finding | Change | Test observable, and why it can fail on `main` |
|---|---|---|---|
| 1 | C1 | `call_unregister` + restart loop | `test_registry_panic_does_not_kill_the_timer`: a registry panicking on the *first* call; assert the **second, unrelated** teardown still runs (`calls == 2`, pending empty). On `ef0ba44` → `1` with an entry stuck forever. |
| 2 | C6 | `shutdown`/`Drop`: bind the guard, `drain()` and `swap(true)` in **one** locked scope; `store` → `swap`; use `call_unregister` | Extend `test_shutdown_drains_pending_grace_timers`: `unregisters == 1`, still `1` after 100 ms. Becomes load-bearing only after Step 3. The C6 interleaving itself is proven by the `swap` return value — a race test would flake; say so in a comment instead. |
| 3 | C3 | Add `TeardownTimer::drain()` (clears the heap, does not latch). `shutdown` calls `drain()` not `stop()`, **after** the Step 2 locked section (pending → queue, matching `release_watch`). `stop()` belongs to `Drop`. | `test_release_after_shutdown_still_unregisters`: after `shutdown()`, acquire+drop, assert `unregisters == 1`. On `ef0ba44` → `0`, entry stuck. Also fix `test_guard_drop_with_disconnected_worker`, which currently absorbs the bug and asserts nothing. |
| 4 | C4 | `start` sets `stopped = true` on spawn failure, logs `error!`. `schedule` returns `Result<(), Box<PendingTeardown>>` so `release_watch` can fall back to firing **inline**. | `test_inline_teardown_when_timer_unavailable`: stop the timer, acquire+drop, assert `unregisters == 1` **without sleeping**. The no-sleep assertion is what makes it an inline-path test. |
| 5 | C9 | `Instant::now().checked_add(delay)`, else log and `Err`. Doc: "Never panics" → states the two `Err` paths. | `test_unrepresentable_deadline_is_refused` with `Duration::MAX`. Deterministic on every platform. |
| 6 | C2 | Apply 2b; reciprocal comments on `fire` and `acquire_watch` | `test_expiry_racing_reacquire_keeps_the_watch_registered`. **The deterministic variant is the real test**: a registry whose unregister sleeps 50 ms, `fire` on a spawned thread, re-acquire on main — under the lock the re-acquire provably blocks and the register lands last; with `drop(pending)` it provably does not. |
| 7 | C7 | Document the total order `service_refs → pending_unsubscribes → timer.queue` and `pending_unsubscribes → registry`. Make (b) **structural**: named guard + explicit `drop(pending)` in `release_watch` instead of relying on temporary-drop timing. Cross-crate note on `StateWatchRegistry`. | No behaviour change. `clippy::significant_drop_in_scrutinee` is the only thing guarding this today. |
| 8 | C8 | Keep not joining; upgrade the comment to state the real guarantee (thread owns only `Arc`s and a `WeakUnboundedSender`) | `test_manager_drop_during_teardown_fire` with a barrier. Assert completion once, and a generous 1 s bound so it catches a *deadlock*, not a slow machine. Note honestly that `drop` blocks for the callback — the visible cost of the C2 decision. |
| 9 | C5 | Rewrite as `test_immediate_mode_churn_costs_no_threads` | Phase 1 keeps the leak asserts. Phase 2: 1,000 single-key acquire/drop cycles (200 frames ≈ 9–16 ms is too close to noise; 1,000 schedules is 45–82 ms old vs ~1 ms new). **Median** release duration `< 20 µs` (0.21–1.10 new, 45.7–82.5 old). Plus a Linux-only `/proc/self/status` peak-thread assert `<= 16` — a resource assertion, not a timing one, and ubuntu is what CI runs. |
| 10 | C10 | Per-round `delta == usize::from(fired)`; `fires + claims == ROUNDS`; `fires > 0 && claims > 0`; delete `unregisters() <= ROUNDS` | The aggregate bound silently permits "twice in one round, zero in another" — the exact property claimed. Report the split via `eprintln!` so `--nocapture` shows coverage. |
| 11 | C11 | Spec update per AGENTS.md rule 2 — do **last**, so it describes what landed | §2.2 add `timer.rs`; §2.3 add `TeardownTimer`/`PendingTeardown`, rewrite `WatchGuard` invariants (153–154); §4.2 replace the spawn+sleep pseudocode (304); invert the trade-off rows (309–316); new subsection for the claim protocol, lock order and callback contract; §8.2 new tests; §9.1/9.2 record measurements and drop the stale `DashMap`/`AtomicUsize` claims; §14.1 known limitations. |

**Falsifiability check for Step 9 — do not skip:**

```
git worktree add /tmp/em-main origin/main
# paste the new test body into /tmp/em-main/sonos-event-manager/src/manager.rs
cargo test -p sonos-sdk-event-manager --locked test_immediate_mode_churn_costs_no_threads -- --exact --nocapture
# must FAIL on both the median-time and (on Linux) the thread-count assertion
git worktree remove /tmp/em-main
```

**Port ranges** (non-overlapping; 4000–5400 already taken): 5400–5500 C1, 5500–5600 C3, 5600–5700 C4, 5700–5800 C2, 5800–5900 C8.

---

## Verification gate

```
export PATH="$HOME/.cargo/bin:$PATH"
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --features sonos-sdk/test-support --locked -- -D warnings
cargo test --workspace --features sonos-sdk/test-support --locked
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked

for i in $(seq 1 20); do cargo test -p sonos-sdk-event-manager --locked || break; done
cargo test -p sonos-sdk-event-manager --locked -- --test-threads=1
```

Run clippy immediately after Step 4 — the `schedule` signature change touches every call site.

## Amend #108, do not stack

C1, C3 and C4 are **regressions relative to `main`**: main survives a panicking callback, main's `shutdown()` leaves the grace mechanism working, main's spawn failure panics rather than leaking at 540 releases/sec. Merging and fixing later puts a critical regression on `main` whose symptom — every later teardown silently dead, stale watched entries forever — will not be noticed until a TUI stops updating. The fixes are ~150 lines of source plus tests, all inside code this PR already rewrites; a follow-up branch would conflict with itself constantly. AGENTS.md rule 2 makes C11 mandatory for this PR.

Keep `ef0ba44` intact (the PR body quotes its measurements). Add one commit per step group:

1. `fix: contain registry panics and restart the teardown timer`
2. `fix: claim teardown tokens under the pending-map mutex on shutdown and drop`
3. `fix: shutdown must not latch the teardown timer off`
4. `fix: tear down inline when the timer thread is unavailable`
5. `fix: register the watch after the teardown claim resolves`
6. `docs: state the lock ordering the teardown path depends on`
7. `test: make the churn and race tests able to fail`
8. `docs: update sonos-event-manager spec for the shared teardown timer`

## Risks this plan introduces

- **`catch_unwind` hides implementor bugs.** A panicking registry logs and continues. Mitigated by `error!` with `(ip, service)` and the trait contract. Accepted: the alternative is a dead thread with no log at all.
- **Restart loop could mask a pathological callback** — one error line per release at 540/sec. Answer is rate-limited logging, not removing the restart.
- **The C2 decision keeps user code under a mutex.** `drop`, `shutdown()` and `acquire_watch`'s claim can each block for one callback. Step 8 makes it visible; the contract bounds it; §2d's sharding is the escape hatch.
- **Moving `register_watch` changes observable ordering for implementors** of a `pub trait` used across a crate boundary. `StateWatchRegistry` doesn't care. Note it in the spec.
- **Inline teardown runs a user callback from `Drop`** — only when thread spawning has already failed. Documented, not defended against.
- **Two timing-sensitive tests** (Step 9 median, Step 10 `fires > 0 && claims > 0`). First CI flake should retune the bound using the recorded numbers, not delete the assertion.

## Deliberately not doing

1. Offloading the registry callback — **declined outright** (§2c), not deferred.
2. Sharding `pending_unsubscribes` — deferred with a trigger (§2d).
3. The sibling-key register race — own PR; recorded in §14.1.
4. Making `GRACE_PERIOD` configurable — invariant 5.
5. Joining the timer thread on drop — declined by design; comment says why.
6. `acquire_watch` leaking a ref count and a registration when `send(Subscribe)` fails — pre-existing on `main`.
7. Post-`shutdown()` `Unsubscribe` commands accumulating in the unbounded channel — pre-existing, bounded by manager lifetime. §14.1.
8. A CI thread-count job or criterion benchmark — own PR.
