# Crate Replacement Audit

**Date**: 2026-08-15
**Status**: Partially executed — this file tracks what remains
**Focus**: Replacing hand-rolled implementations with established public crates

## What This Is

Not a feature — a debt inventory. It lists workspace code that reimplements something a
maintained crate already does well, plus the dependency and tooling hygiene gaps that let
that drift happen.

The Tier 1 sweep (XML escaping, the hand-written XML tokenizer and substring parsers, URL
parsing, the HTTP/TLS stack collapse, single XML crate, `warp` → `axum`), the `state-store`
deletion, and the `thiserror` migration are all in the tree. So is the tooling half:
`rust-toolchain.toml`, `clippy.toml`, `deny.toml`, `[workspace.lints]` with ~35 lints,
`#![forbid(unsafe_code)]` in every member, and CI grown from 4 jobs to 10 including
`cargo-deny`, `cargo-machete`, `msrv` and `no-default-features`. Declared-but-unused
dependencies went to zero, and transitive crates at 2+ versions went from 32 to 7.

What follows is what has not been done.

---

## Tier 2 — meaningful, needs design thought

### 8. `sonos-discovery` SSDP layer: ~425 LOC of protocol boilerplate

`sonos-discovery/src/ssdp.rs` — 236 LOC of implementation plus 189 LOC of tests that
exist solely to cover the two hand-rolled parse functions.

What's hand-built:
- M-SEARCH request as a `format!` string with literal `\r\n` (`:71-79`)
- Raw `UdpSocket::bind` per interface, one `std::thread::spawn` each, `join()` in a
  loop, panics coerced to strings (`:81-136`)
- Fixed `[0u8; 2048]` recv buffer, no truncation handling (`:138-161`)
- `parse_ssdp_response` (`:198-227`) — line-by-line if/else chain for
  LOCATION/ST/USN/SERVER; status line never validated, duplicate headers last-wins,
  header folding unsupported
- `extract_header_value` (`:230-236`) — manual case-insensitive prefix compare

Protocol gaps a crate would close:
- No `IP_MULTICAST_TTL` set (relies on OS default, typically 1)
- No `SO_REUSEADDR` / `SO_REUSEPORT`
- No `join_multicast_v4` — send-only, so `ssdp:alive`/`byebye` NOTIFYs are invisible
- **Timeout resets on every received packet** (`:141-161`), so worst-case wall clock
  is unbounded on a chatty network, not `timeout`
- `MX: 2` hardcoded and decoupled from the caller's timeout (`:75`)
- Dedup keyed on the raw LOCATION string, not UDN/USN (`discovery.rs:156-160`)
- `DiscoveryError::Timeout` is declared (`error.rs:16`) but never constructed
- No retry: one M-SEARCH burst per interface, no re-probe

**Options.** `ssdp-client 2.1` is the focused choice; `rupnp 3.0` bundles SSDP +
device description + service control. Both are async, which is a real cost — this
crate is deliberately blocking and that shapes the public API (`get()`, `get_iter()`).

**Middle path worth considering:** keep the loop, but use `socket2 0.6` for socket
options and `httparse` for header parsing. ~60 of the 236 LOC go away and the
protocol bugs get fixed without an async rewrite.

### 11. `tokio_util::sync::CancellationToken` replaces four shutdown mechanisms

- `sonos-stream/src/broker.rs:104,228` — `shutdown_signal: Arc<AtomicBool>`,
  `store(true)` on shutdown and **never read by anything**. Vestigial.
- `sonos-stream/src/polling/scheduler.rs:35,58,107,123` — `Arc<AtomicBool>` checked at
  loop top with `Ordering::Relaxed`.
- `sonos-stream/src/subscription/manager.rs:34,55` — `is_polling_active: Arc<AtomicBool>`.
- `sonos-event-manager/src/manager.rs:142` —
  `Mutex<HashMap<(IpAddr, Service), Arc<AtomicBool>>>`, i.e. hand-rolled cancellation
  tokens as a map of atomics.

The scheduler's own responsiveness gap is closed: `ShutdownSignal { requested: AtomicBool,
wakeup: Notify }` with `sleep_or_shutdown` wakes both sleeps
(`sonos-stream/src/polling/scheduler.rs`). The remaining value of a `CancellationToken` is
uniformity — one cancellation primitive instead of four ad-hoc ones — not latency.

The event-manager's per-unsubscribe OS thread is also gone: `sonos-event-manager/src/timer.rs`
is a single shared timer thread over a deadline-ordered heap. It must stay off the tokio
runtime (the worker runtime is `new_current_thread` and the subscribe path makes blocking
calls), so `tokio::time::sleep` is **not** a candidate there.

`broker.rs`'s `shutdown_signal` is still vestigial: stored, never loaded.

Also worth folding in here: **three separate manual graceful-shutdown-over-channel
implementations** (`callback-server/src/server.rs:150,242-254`,
`sonos-stream/src/broker.rs:721-748`, `polling/scheduler.rs:305-316,463-478`), and
**three never-terminating `tokio::time::interval` sweep loops with no shutdown arm**
(`callback-server/src/firewall_detection.rs:287`,
`sonos-stream/src/subscription/event_detector.rs:161`, `broker.rs:395`) — all stopped
only by `abort()`.

### 13. Derive macro for the property boilerplate

Current cost per property, all mechanically derivable from the type plus 3 consts:

| Pattern | Count | Location |
|---|---|---|
| `impl Property for X { const KEY }` | 13 | `sonos-state/src/property.rs` |
| `impl SonosProperty for X { SCOPE, SERVICE, to_change }` | 13 | same file |
| `PropertyChange` variants + parallel match arms | 12 variants across 4 match blocks | `sonos-state/src/decoder.rs` |
| `impl Fetchable` / `GroupFetchable` / `FetchableWithContext` | 11 | `sonos-sdk/src/property/handles.rs` |
| `pub type XHandle = (Group)PropertyHandle<X>` | 12 | same file |

The four parallel match blocks in `decoder.rs` recover per-type consts the compiler
already knows. Adding one property today touches **~9 files**, and forgetting the
`to_change()` override in any of them yields a property that stores correctly and emits
nothing.

A small proc-macro crate — `#[derive(SonosProperty)]` with
`#[sonos(key = "bass", scope = Speaker, service = RenderingControl)]` — plus
`enum_dispatch` or a blanket impl removes most of it.

Note `paste` is already doing adjacent work in `sonos-api/src/operation/macros.rs`, but
`:snake` mangles acronyms — producing names like `set_a_v_transport_u_r_i_operation` — which
is why there are **52 hand-written `pub use x as y;` alias lines** across 5 service
modules. `heck` handles acronyms correctly. `paste` is also unmaintained
(RUSTSEC-2024-0436), so this item retires an advisory as well.

---

## Tier 3 — smaller / opportunistic

- **`moka` or `lru`** for `callback-server/src/router.rs` — a hand-rolled bounded TTL
  cache with O(n) min-by-timestamp eviction at `MAX_PENDING_EVENTS = 256` and a
  `swap_remove` loop with manual index bookkeeping ("Don't increment i"). A large block of
  tests exists solely to pin this behavior.
- **Duration parsing duplicated verbatim** — `parse_time_to_ms` in
  `sonos-state/src/property.rs` and `parse_duration_ms` in `sonos-state/src/decoder.rs`.
  Same HH:MM:SS(.mmm) algorithm, same crate. Pick one.
- **Sonos bool parsing is re-inlined in the events layer.** `parse_sonos_bool`
  (`sonos-api/src/operation/mod.rs`) accepts both `"1"/"0"` and `"true"/"false"` and is
  tested, but the events layer still open-codes the rule — e.g.
  `sonos-api/src/services/group_management/events.rs`.
- **`strum::EnumString`** for the PLAYING/PAUSED/STOPPED string→enum mapping, written
  **three times**: `sonos-state/src/property.rs`, `sonos-state/src/decoder.rs`,
  `sonos-sdk/src/property/handles.rs`.
- **`backon` or `tokio-retry`** for `sonos-stream/src/polling/scheduler.rs`, which sleeps
  **twice** per error iteration (loop sleep plus backoff sleep) and has no jitter. A
  second, divergent doubling rule lives further down the same file. The overflow hazard is
  fixed — the exponent is clamped with `.min(6)`.
- **`arc-swap`** for the two hand-rolled RCU sites:
  `SonosSystem::rebuild_speakers_excluding_satellites` (`sonos-sdk/src/system.rs`, builds a
  new `HashMap` off-lock and swaps under a brief write lock) and
  `sonos-state/src/event_worker.rs` (a manual two-phase commit across two `RwLock`s).
- **`bon` or `derive_builder`** — `StateManagerBuilder` (`sonos-state/src/state.rs`) is
  ~82 LOC for exactly 2 fields.
- **`governor`** (or just leave it) for the `AtomicU64` epoch-seconds rediscovery cooldown
  in `sonos-sdk/src/system.rs`.

---

## Not worth replacing

- **The `UPnPOperation` trait + macro design.** This is good Rust. Do *not* reach for a
  generic UPnP crate here — the typed operations are better than what `rupnp` offers.
- **`if-addrs`, `paste`, `parking_lot`, `tracing`, `serde`** — all correct choices, with
  one caveat: `paste` is unmaintained (RUSTSEC-2024-0436, ignored in `deny.toml` with a
  removal note), which strengthens the case for item 13.

---

## Tooling gaps still open

- **`rustfmt.toml`** — absent. Formatting is whatever the toolchain default is, so a
  rustfmt behaviour change lands as an unrelated diff. The toolchain is pinned, which
  bounds the damage but does not document intent.
- **`.cargo/config.toml`** — absent. No shared aliases, no target-dir or linker config.
- **CI runs on `ubuntu-latest` only.** All 14 `runs-on:` lines across the three workflows
  are Linux. The workspace compiles platform-conditional code (`if-addrs`, `windows-sys`
  is in the lock at two versions) and is developed on macOS, so the one platform CI proves
  is not the one it is written on. A `macos-latest` leg on the `test` job is the cheap
  version.
- **`cargo-semver-checks`** — not run directly. Semver is release-plz's, and only for
  `sonos-api` and `sonos-sdk`.
- **`--ignored` tests never run in CI.** 26 `#[ignore]` tests require real hardware, so
  they can only ever be a manual gate — but nothing records whether they were run for a
  given release.

---

## Duplicate dependency versions

Seven pairs remain out of ~233 packages, and three of them trace to one deliberate split:

| Crate | Versions | Why |
|---|---|---|
| `ureq` | 2.12.1, 3.4.0 | `soap-client` pins 2.x (3.x removes `Agent::request`, `Error::Status`, `Error::Transport`, all still used); `sonos-discovery` is on 3.x |
| `base64` | 0.22.1, 0.23.1 | pulled by the two `ureq` majors |
| `webpki-roots` | 0.26.11, 1.0.9 | pulled by the two `ureq` majors |
| `syn` | 2.0.119, 3.0.3 | `async-trait` |
| `getrandom` | 0.2.17, 0.3.4, 0.4.3 | upstream |
| `r-efi` | 5.3.0, 6.0.0 | upstream |
| `windows-sys` | 0.52.0, 0.61.2 | upstream |

The `ureq` split is documented in `soap-client/Cargo.toml`, the root `Cargo.toml` and
`deny.toml`, and it is what keeps three of the seven pairs alive. Migrating `soap-client`
to `ureq` 3.x collapses all three at once — the single highest-leverage dependency change
left on this list.

---

## Sequencing

1. Item 13 (property derive macro) — largest ongoing cost per new property, and it also
   retires `paste`, clearing RUSTSEC-2024-0436.
2. `soap-client` → `ureq` 3.x — collapses three duplicate pairs.
3. Tier 3 deduplication (duration parsing, `strum`, the re-inlined bool rule) — small,
   independent, no design risk.
4. Item 8 (SSDP) — largest single body of code, and the one with the most protocol
   subtlety to get wrong. Wants its own plan.
5. Item 11 (`CancellationToken`) — uniformity, not correctness. Lowest urgency.
6. `rustfmt.toml` and a CI OS matrix — cheap, do alongside anything.
