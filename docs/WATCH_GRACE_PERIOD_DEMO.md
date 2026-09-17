# WatchHandle Grace Period Demo

`sonos-sdk/examples/watch_grace_period_demo.rs` demonstrates the RAII `WatchHandle` and
the 50 ms teardown grace period against real speakers.

## The Model

`watch()` returns a `WatchHandle<P>`. The handle is a **lease on the UPnP subscription**,
not a snapshot of the value:

- It is `#[must_use]`. Dropping it starts a 50 ms grace period, after which the
  subscription is torn down.
- If another handle for the same `(speaker, service)` is acquired inside that window, the
  pending teardown is cancelled and the existing subscription is reused.
- `value()` re-reads the store on every call, so **one handle held across a loop reports
  every change**. There is no need to re-watch to refresh a value.
- Overlapping handles share one subscription. It is released only when the last one drops.

The grace period is hysteresis for the cases where a handle genuinely must be dropped and
reacquired. It is not a licence to acquire one per frame — `release`/`acquire` churn is
work the subscription layer then has to undo.

## Running It

Requires Sonos speakers on the local network, awake and discoverable.

```bash
cargo run -p sonos-sdk --example watch_grace_period_demo
```

## What Each Demo Shows

**Demo 1 — Normal usage.** Acquire a handle, read `mode()` and `value()`, hold it for two
seconds, drop it. Dropping is what starts the grace period.

**Demo 2 — TUI pattern.** Simulates ten draw calls at ~60 FPS. The handle is acquired
**once, outside the loop**; each frame does a live read through `value()`. One
subscription covers the whole loop and no frame sees a stale value.

**Demo 3 — Grace period timing.** Acquire a handle and drop it, wait 25 ms (inside the
window), then acquire a new one — the subscription is reused and was never interrupted.
Then drop it and wait 60 ms so the grace period expires and the subscription is cleaned up.

**Demo 4 — Subscription sharing.** Two overlapping volume handles plus a mute handle.
Dropping the first volume handle leaves the subscription up, because the second still
holds it. Only when the last handle drops does the grace period start.

## Reading a Handle

```rust
let handle = speaker.volume.watch()?;

if let Some(vol) = handle.value() {
    println!("Volume: {}%", vol.0);
}

println!("Watch mode: {}", handle.mode());
println!("Has realtime events: {}", handle.has_realtime_events());
```

`mode()` is one of `Events` (UPnP NOTIFY), `Polling` (callbacks blocked, value polled on
an interval) or `CacheOnly` (neither; the store is read but nothing refreshes it).

## Validating Changes

```bash
cargo test -p sonos-sdk --features test-support
cargo build -p sonos-sdk --examples
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --features sonos-sdk/test-support --locked -- -D warnings
```

The teardown timer's own coverage lives in `sonos-event-manager/src/manager.rs` — notably
`test_expiry_racing_reacquire_keeps_the_watch_registered`,
`test_cancel_and_expiry_have_exactly_one_winner` and
`test_immediate_mode_churn_costs_no_threads`.

Beyond that, run the demo against real speakers and, optionally, watch network traffic to
confirm no SUBSCRIBE/UNSUBSCRIBE churn during Demo 2.

## How It Works Underneath

Subscriptions are reference counted in `sonos-event-manager`:

- First holder creates the UPnP subscription (ref count 0→1)
- Additional holders share it without duplication
- Last holder dropping schedules the teardown (ref count 1→0)

The delay is not a thread per drop. A single OS thread per manager
(`sonos-event-manager/src/timer.rs`, named `sonos-teardown-timer`) services a
deadline-ordered queue, so a release costs a mutex, a heap push and a condvar notify. It
runs off the Tokio runtime deliberately: the worker runtime is single-threaded and the
subscribe path makes blocking calls, so a `tokio::time::sleep` there would not fire while
a SUBSCRIBE to an unreachable speaker was in flight.

Cancellation and expiry race by design, and exactly one wins. Each pending teardown owns
an `AtomicBool` claim token; whoever swaps it from `false` to `true` — the re-acquiring
watcher or the expiring timer — owns the outcome. Both swap while holding the pending
map's mutex, so they cannot interleave.

```
SDK user code
    ↓
WatchHandle<P>            RAII lease, live read
    ↓
StateManager              watched-key set, property store
    ↓
SonosEventManager         reference counting + teardown timer
    ↓
sonos-stream              UPnP events / polling fallback
    ↓
UPnP SOAP operations
```
