# Crate Replacement Audit Brainstorm

**Date**: 2026-08-15
**Status**: Draft
**Focus**: Replacing hand-rolled implementations with established public crates

## What We're Building

Not a feature — a debt inventory. This audit walks the whole workspace looking for
code that reimplements something a maintained crate already does well, plus the
dependency and tooling hygiene gaps that let that drift happen in the first place.

### The Problem

The SDK was built largely from scratch, before familiarity with the Rust ecosystem.
That produced a lot of correct-but-redundant machinery: an XML tokenizer, three XML
escapers, two SSDP header parsers, four independent shutdown mechanisms, a
from-scratch anymap. Some of it has real bugs. Some of it is dead. And because there
are **zero** configured lints and no dependency auditing in CI, nothing pushes back
on the accumulation.

Scope of findings: ~2,500 LOC of hand-rolled machinery with direct crate
replacements, 17 declared-but-unused dependencies, and two full TLS stacks compiled
into one binary.

### Method

Five parallel sub-agent sweeps (SOAP/XML layer, discovery, stream + callback-server,
state layers, cross-cutting hygiene), then crate versions verified against crates.io
rather than recalled, and the load-bearing claims spot-checked directly against the
source and `Cargo.lock`.

---

## Tier 1 — clear wins, low risk

### 1. `quick-xml::escape` replaces three hand-rolled escapers (~150 LOC)

- `sonos-api/src/operation/mod.rs:253` — `xml_escape()`, a char loop over the 5
  predefined entities, **18 call sites**.
- `sonos-state/src/decoder.rs:439-445` — a `.replace()` chain that unescapes in the
  **wrong order**. `&amp;` is expanded before `&apos;`/`&quot;`, so `&amp;apos;`
  decodes to `'` instead of the literal `&apos;`. Real, if narrow, bug.
- `sonos-api/src/events/xml_utils.rs` — see item 2.

`quick-xml` is *already* a dependency of `sonos-api`. `escape::escape` and
`escape::unescape` are drop-in replacements.

### 2. Delete `strip_namespaces()` — 121 LOC of hand-written XML tokenizer

`sonos-api/src/events/xml_utils.rs:39-159`. A character-level lexer with a peekable
loop that rewrites tags, skips `<?`/`<!`, drops `xmlns` attributes, and strips
prefixes from tag and attribute names. Called from 6 places (all 5 `from_xml` impls
plus `parse_zone_group_state_xml`).

It holds **8 of the crate's 17 non-test `unwrap()`s** (lines 50, 73, 86, 90, 94, 104,
139, 149 — all `chars.next().unwrap()` after a `peek()`).

This is the single largest hand-rolled artifact in the repo, and it lives in a file
that already imports `quick_xml`. `quick-xml`'s `Reader` has namespace resolution
built in; serde `rename` attributes cover the rest.

### 3. Two substring-based XML "parsers" that should be serde (~40 LOC)

- `sonos-api/src/events/types.rs:196` — `extract_xml_value()`, `find("<tag>")` +
  index slicing. Publicly re-exported at `lib.rs:183`. Its own comment calls it a
  "fallback when proper parsers are not available."
- `sonos-state/src/decoder.rs:430` — `extract_xml_element()`, same algorithm. Its own
  comment at `:419` says "could use quick-xml for more robust parsing."
- A third instance is inline at `sonos-stream/examples/basic_usage.rs:418-421`.

### 4. `url::Url` replaces two IP-from-URL parsers

- `sonos-discovery/src/device.rs:92` — `split("//").nth(1)?.split(':').next()`,
  returns `String`.
- `sonos-state/src/decoder.rs:370` — `strip_prefix("http://")?.split('/')...`,
  returns `IpAddr`.

Both mis-parse IPv6 literals and any `//` in the path. Discovery additionally
hardcodes `port: 1400` (`device.rs:68`) instead of reading it from the LOCATION
authority. `url 2.5` is already a declared — and unused — dep of `callback-server`.

### 5. Collapse the two HTTP stacks (biggest dependency win)

Two TLS implementations and two hyper trees compile into one binary. Verified in
`Cargo.lock`:

| Client | Declared by | Pulls |
|---|---|---|
| `ureq 2.9` (→2.12.1) | `soap-client` | rustls 0.23 + ring |
| `reqwest 0.11` (→0.11.27) | `sonos-discovery`, `callback-server` | hyper 0.14, http 0.2, native-tls, openssl-sys |

`sonos-sdk` depends on both paths, so both are always built. Lockfile confirms
`hyper` at 0.14.32 **and** 1.8.1, `http` at 0.2.12 **and** 1.4.0, plus `native-tls`,
`openssl-sys 0.9.111`, `rustls 0.23.35`, and `ring 0.17.14` all present.

Note: `reqwest` in `callback-server/Cargo.toml:20` has **zero uses in `src/`** — it is
test-only and mis-scoped as a runtime dependency.

Pick one client: `ureq 3.x` to stay blocking, or `reqwest 0.13` to go async.

### 6. Consolidate on one XML crate

`xmltree 0.10` and `quick-xml 0.31` both live inside `sonos-api`, split along an
operations/events seam:

- `xmltree` — SOAP responses. 7 files: `operation/{mod.rs:16, builder.rs:166,
  macros.rs:67/196/278}` plus 4 `services/*/operations.rs`.
- `quick-xml` — event payloads. 6 files: `events/xml_utils.rs` plus 5
  `services/*/events.rs`.

Consolidating on `quick-xml` (0.41 current) drops a dependency and removes the 11
hand-written `get_child().get_text().parse().unwrap_or_default()` chains, which
`quick_xml::de` + serde derives handle declaratively.

Also: `sonos-stream/Cargo.toml:23` declares `quick-xml` with the `serialize` feature
and has **0 references** to it anywhere.

### 7. `warp 0.3` → `axum`

30 `warp::` references in exactly one file (`callback-server/src/server.rs:7,290,291,314`),
serving one method-gated catch-all route. `warp` is the sole reason the workspace
carries the legacy `hyper 0.14` / `http 0.2` stack.

It also forces awkward code:
- `Method::from_bytes(b"NOTIFY").unwrap()` constructed **per request** (`server.rs:291`)
- 38 LOC of `InvalidUpnpHeaders` reject type + `handle_rejection` status mapping
  (`server.rs:427-464`) that axum's extractors make unnecessary
- Untyped `header::optional::<String>` extraction for SID/NT/NTS, then manual string
  comparison in `validate_upnp_headers` (`server.rs:405-424`) — where NT/NTS are only
  validated when *both* are present, so `NT` alone goes unchecked

`warp 0.4.3` exists now, but axum is the better destination given it's one route.

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

### 9. `state-store` is a from-scratch anymap that nothing uses

~650 of its ~700 non-test LOC are effectively dead. Three mechanisms:

- **A. TypeId-keyed heterogeneous map** (`src/store.rs:51-134`, ~84 LOC) —
  `HashMap<TypeId, Box<dyn Any + Send + Sync>>` with downcast get/set and a
  `PartialEq` change-detect returning `bool`. This is exactly `anymap` / `type-map`
  plus a thin wrapper.
- **B. Observer registry** (`src/store.rs:176-330`, ~155 LOC) —
  `HashSet<(Id, &'static str)>` of watched keys plus a single mpsc. Subscription by
  string key, delivery by scanning, no per-subscriber channel.
- **C. Blocking-iterator wrappers** (`src/iter.rs:36-123`, ~90 LOC) — newtypes over
  `Arc<Mutex<mpsc::Receiver<T>>>`. Because the receiver is shared behind a `Mutex`,
  clones **steal** events rather than each seeing them.

**The finding that matters more than any of the above:** the entire crate is consumed
by exactly one line —

```rust
// sonos-state/src/property.rs:15
pub use state_store::Property;
```

`StateStore`, `PropertyBag`, `ChangeEvent`, and `ChangeIterator` are unused outside
`state-store`'s own tests and example. Meanwhile `sonos-state/src/state.rs:259-293`
contains a **verbatim reimplementation of `PropertyBag`** (35 LOC, identical
`TypeId`/`downcast_ref`/`PartialEq` body) despite the crate depending on `state-store`.

**Decision needed:** either delete the crate and move the 13-LOC `Property` trait
(`state-store/src/property.rs:29-41`) into `sonos-state`, or make `sonos-state`
actually use what it depends on. Note the crate is published to crates.io as
`sonos-sdk-state-store`, so removal has a (minor) semver dimension.

### 10. `tokio::sync::watch` for the reactive layer

**Zero uses of `watch` in the workspace** — verified by grep. But `CLAUDE.md:99` and
`docs/SUMMARY.md:337` both state that properties use `tokio::sync::watch`. The docs
are wrong and should be fixed regardless of what we do with the code.

What exists instead: a watched-key set plus one shared mpsc, with near-identical emit
logic reimplemented at **three layers** (~120 LOC total):
- `state-store/src/store.rs:314` — `maybe_emit_change`
- `sonos-state/src/state.rs:663-678` — `maybe_emit_change`
- `sonos-state/src/event_worker.rs:298-325` — `apply_property_change`

`watch` is a latest-value, single-writer/multi-reader channel — precisely this shape —
and it gives multi-consumer for free. Today `event_iterator()` can only be called
once (`sonos-stream/src/broker.rs:660`), and all event channels are
`mpsc::unbounded_channel` despite `config.event_buffer_size` existing and being
validated (`config.rs:168`) but never used.

Related: **three hand-written receiver-wrapper iterator files** —
`state-store/src/iter.rs` (125 LOC), `sonos-state/src/iter.rs` (152), and
`sonos-event-manager/src/iter.rs` (108). ~385 LOC differing only in type parameter
and `tracing::trace!` lines.

Also in this area: `sonos-stream/src/events/iterator.rs` exposes five overlapping
consumption surfaces (344 LOC) over one receiver, including `FilteredEventIterator`
with a `Box<dyn Fn>` predicate that reimplements `StreamExt::filter`. Two of them
(`SyncEventIterator::next:236`, `FilteredSyncIterator::next:337`) call
`Handle::block_on` and panic if called from a runtime thread — the tests at `:506-560`
are written specifically to dodge that.

### 11. `tokio_util::sync::CancellationToken` replaces four shutdown mechanisms

- `sonos-stream/src/broker.rs:104,228` — `shutdown_signal: Arc<AtomicBool>`,
  `store(true)` on shutdown and **never read by anything**. Vestigial.
- `sonos-stream/src/polling/scheduler.rs:35,58,107,123` — `Arc<AtomicBool>` checked at
  loop top with `Ordering::Relaxed`.
- `sonos-stream/src/subscription/manager.rs:34,55` — `is_polling_active: Arc<AtomicBool>`.
- `sonos-event-manager/src/manager.rs:142` —
  `Mutex<HashMap<(IpAddr, Service), Arc<AtomicBool>>>`, i.e. hand-rolled cancellation
  tokens as a map of atomics.

Because the scheduler only observes its flag once per interval, `stop_polling().await`
blocks for up to a **full poll interval**. A `CancellationToken` makes it
`select!`-able.

Same crate spawns an **OS thread per unsubscribe** purely to `thread::sleep(50ms)`
(`sonos-event-manager/src/manager.rs:309-337`) despite tokio being a direct
dependency. `tokio::time::sleep` + token cancellation replaces the whole thing,
including the drain-on-shutdown logic duplicated across `shutdown()` (`:516-530`) and
`Drop` (`:533-548`).

Also worth folding in here: **three separate manual graceful-shutdown-over-channel
implementations** (`callback-server/src/server.rs:150,242-254`,
`sonos-stream/src/broker.rs:721-748`, `polling/scheduler.rs:305-316,463-478`), and
**three never-terminating `tokio::time::interval` sweep loops with no shutdown arm**
(`callback-server/src/firewall_detection.rs:287`,
`sonos-stream/src/subscription/event_detector.rs:161`, `broker.rs:395`) — all stopped
only by `abort()`.

### 12. `thiserror` for the three hand-written error types

`thiserror 1.0` is already declared by 6 members (2.0.20 is current), yet:

- `sonos-state/src/error.rs:50-85` — 36 LOC of manual `Display` + `source`, 14 arms.
- `sonos-discovery/src/error.rs:9-37` — 37 LOC of manual `Display` + `Error`.

Separately, `soap-client`/`sonos-api` errors are all `String` payloads constructed via
`.map_err(|e| E::X(e.to_string()))`, which discards the source chain entirely.
`#[from]` / `#[source]` would preserve it.

And the `SoapError` → `ApiError` match is written inline **5 more times**
(`client.rs:97`, `client.rs:162`, `services/events.rs:68`, `:127`, `:189`) despite the
canonical `From` impl already existing at `sonos-api/src/error.rs:67`.

### 13. Derive macro for the property boilerplate

Current cost per property, all mechanically derivable from the type plus 3 consts:

| Pattern | Count | Location |
|---|---|---|
| `impl Property for X { const KEY }` | 14 | `sonos-state/src/property.rs:71..456` |
| `impl SonosProperty for X { SCOPE, SERVICE }` | 14 | same file |
| `PropertyChange` match arms | **48** (4 blocks × 12) | `sonos-state/src/decoder.rs:73-165` |
| `impl Fetchable for X` | 9 (+3 variants) | `sonos-sdk/src/property/handles.rs:628..733` |
| `pub type XHandle = PropertyHandle<X>` | 12 | `handles.rs:812-836, 1077-1083` |

The four parallel 12-arm match blocks in `decoder.rs` recover per-type consts the
compiler already knows. Adding one property today touches **~9 files**.

A small proc-macro crate — `#[derive(SonosProperty)]` with
`#[sonos(key = "bass", scope = Speaker, service = RenderingControl)]` — plus
`enum_dispatch` or a blanket impl removes most of it.

Note `paste 1.0` is already doing adjacent work in
`sonos-api/src/operation/macros.rs:39,159,236`, but `:snake` mangles acronyms —
producing names like `set_a_v_transport_u_r_i_operation` (`operations.rs:747`), which
is why there are **46 hand-written `pub use x as y;` alias lines** across 4 modules.
`heck 0.5` handles acronyms correctly.

---

## Tier 3 — smaller / opportunistic

- **`moka` or `lru`** for `callback-server/src/router.rs:49-208` — a hand-rolled
  bounded TTL cache with O(n) min-by-timestamp eviction at cap 256 and a
  `swap_remove` loop with manual index bookkeeping ("Don't increment i"). 185 LOC of
  tests exist solely to pin this behavior.
- **`dirs 5` → `dirs 6` / `directories`** in `sonos-sdk/src/cache.rs:33`. The 73-LOC
  atomic-write + TTL disk cache itself is fine as-is.
- **Duration parsing duplicated verbatim** — `sonos-state/src/property.rs:338`
  (`parse_time_to_ms`) and `sonos-state/src/decoder.rs:378` (`parse_duration_ms`).
  Same HH:MM:SS(.mmm) algorithm, same file tree. Pick one.
- **Sonos bool encoding is inconsistent** — `"1"/"0"` in 7 places, `"true"/"false"` at
  `av_transport/operations.rs:673`, and the parse rule re-inlined 4× in the events
  layer instead of calling the existing `parse_sonos_bool` (`operation/mod.rs:243`).
- **`strum::EnumString`** for the PLAYING/PAUSED/STOPPED string→enum mapping, written
  **three times**: `sonos-state/src/property.rs:280`, `decoder.rs:233`,
  `sonos-sdk/src/property/handles.rs:652`.
- **`backon` or `tokio-retry`** for `sonos-stream/src/polling/scheduler.rs:222-235`,
  which sleeps **twice** per error iteration (loop sleep at `:133` + backoff sleep at
  `:235`), has no jitter, and can overflow-panic in debug via
  `current_interval * 2_u32.pow(6)`. A second, divergent doubling rule lives at `:262`.
- **`arc-swap`** for the two hand-rolled RCU sites: `sonos-sdk/src/system.rs:461-474`
  (builds a new `HashMap` off-lock, swaps under a brief write lock) and
  `sonos-state/src/event_worker.rs:169-226` (a manual two-phase commit across two
  `RwLock`s).
- **`bon` or `derive_builder`** — `StateManagerBuilder` (`sonos-state/src/state.rs:870-951`)
  is 82 LOC for exactly 2 fields.
- **`governor`** (or just leave it) for the `AtomicU64` epoch-seconds rediscovery
  cooldown at `sonos-sdk/src/system.rs:433-480`.

---

## Not worth replacing

- **`SonosOperation` trait + macro design.** This is good Rust. Do *not* reach for a
  generic UPnP crate here — the typed operations are better than what `rupnp` offers.
- **`if-addrs`, `paste`, `parking_lot`, `tracing`, `serde`** — all correct choices.
- **Zero `unsafe` in 42k LOC.** Worth adding `#![forbid(unsafe_code)]` to lock in.

---

## Tooling gaps (cheap, high leverage)

### Missing entirely

`rust-toolchain.toml`, `clippy.toml`, `deny.toml`, `rustfmt.toml`, `.cargo/config.toml`
— none exist.

**Zero** `[lints]` tables in any `Cargo.toml`, zero `[workspace.lints]`, and zero
crate-level `#![warn(...)]` / `#![deny(...)]` / `#![forbid(...)]`. All lint policy
lives in CI's `-D warnings`, which is exactly why `uninlined_format_args` bites
locally but not in CI (consistent with the known rustc version skew between local and
CI toolchains).

`#![deny(missing_docs)]` is absent. Docs are present in practice — every internal
crate's `lib.rs:1` opens with `//! Internal implementation detail of...` — but nothing
enforces it. `cargo doc -D warnings` catches broken intra-doc links only.

### CI coverage

`.github/workflows/ci.yml` has 4 jobs (fmt, clippy, test, doc), all `ubuntu-latest` on
`dtolnay/rust-toolchain@stable`. Gaps:

- **No MSRV job.** `rust-version = "1.80"` (`Cargo.toml:22`) has never been verified.
- No `cargo deny` / `cargo audit` / `cargo-udeps` / `cargo-semver-checks` (semver is
  only checked by release-plz at release time, and only for `sonos-api` + `sonos-sdk`).
- No `--no-default-features` job, so `sonos-stream`'s `firewall-detection` feature is
  never tested off.
- Single OS — nothing exercises `if-addrs`' Windows/macOS paths or the **four**
  `windows-sys` versions in the lock (0.48.0, 0.52.0, 0.60.2, 0.61.2).
- `--ignored` never runs, so all 26 `#[ignore]`d hardware tests are dead in CI.

### 17 declared-but-unused dependencies

Runtime deps unreferenced in their own crate:
- `callback-server`: `async-trait`, `thiserror`, `url`, `uuid`, `tracing-subscriber`
  (5 of 10); `reqwest` used only in `tests/` (mis-scoped, not a dev-dep)
- `sonos-stream`: `quick-xml` (unused anywhere), `tracing-subscriber` (examples only)

Dev-deps unreferenced (12): `callback-server` `proptest`/`tokio-test`; `sonos-api`
`rstest`/`mockito`; `sonos-stream` `rstest`/`proptest`/`tokio-test`/`mockall`;
`sonos-state` all 5 non-workspace dev-deps (`tokio`, `chrono`, `ctrlc`, `ratatui`,
`crossterm`); `sonos-sdk` `ratatui`/`crossterm`.

**`mockall 0.11` deserves a call-out:** 0 usages, yet it is the sole reason
`syn 1.0.109` compiles alongside `syn 2.0.111` (via `mockall_derive 0.11.4`).

`cargo-udeps` or `cargo machete` in CI catches all of these.

### Testing inconsistency

`rstest` is declared in 3 crates and `#[fixture]` is used **zero** times. Meanwhile
`sonos-event-manager/src/manager.rs:557` hand-writes a `MockRegistry` with
`AtomicUsize` call counters implementing `WatchRegistry` — exactly `mockall`'s job,
and `mockall` is declared (unused) in the sibling crate.

HTTP mocking is split: `mockito` is used only in
`sonos-discovery/tests/fixture_based_integration.rs:9`, while
`callback-server/tests/integration_tests.rs` (557 LOC) spins up the real `warp` server
and drives it with 21 raw `reqwest` NOTIFY requests across **hand-partitioned port
ranges** (50000-50100, 50200-50300, … 51400-51500) to avoid cross-test collisions.

Test totals: 541 `#[test]`, 53 `#[tokio::test]`, 5 `#[rstest]`, 18 `proptest!` blocks.
6 of 9 members have no `tests/` directory at all (100% inline). No `insta`, no
`wiremock`, no `tempfile`, no paused/mock time anywhere — several tests fabricate
backdated `Instant`s to force timeouts (`callback-server/src/router.rs:297`,
`sonos-stream/src/subscription/event_detector.rs:389`) and one sleeps a real 1200ms
because the sweeper ticks at 1 Hz (`firewall_detection.rs:409-427`).

### 32 transitive crates at 2+ versions

Beyond the hyper/http/TLS split already noted: `base64` 0.21.7 + 0.22.1, `mio` 0.8.11
+ 1.1.1, `socket2` 0.5.10 + 0.6.1, `getrandom` 0.2.16 + 0.3.4, `rand` 0.8.5 + 0.9.2,
`hashbrown` 0.15.5 + 0.16.1, `bitflags` 1.3.2 + 2.10.0, `windows-sys` at four
versions. Dev-only skew: `ratatui` 0.26.3 + 0.29.0, `crossterm` 0.27.0 + 0.28.1.

### `tokio = { features = ["full"] }` in 5 members (6 declarations)

`callback-server/Cargo.toml:16`, `sonos-event-manager:26`+`:32`, `sonos-stream:19`,
`sonos-api:30` (dev), `sonos-state:33` (dev).

Actual API surface used workspace-wide: `sync::mpsc`, `spawn`, `task::spawn_blocking`,
`time::{sleep,interval,timeout}`, `select`, `runtime`, `macros`. **No** `net`, `fs`,
`process`, `signal`, or `io-util` usage anywhere. `sonos-event-manager` uses exactly 3
distinct tokio paths and pulls `full`.

---

## Verified crate versions (crates.io, 2026-08-15)

| Crate | Latest | Relevant to |
|---|---|---|
| `quick-xml` | 0.41.0 | items 1, 2, 3, 6 |
| `url` | 2.5.8 | item 4 |
| `ureq` | 3.4.0 | item 5 |
| `reqwest` | 0.13.4 | item 5 |
| `axum` | 0.8.9 | item 7 |
| `warp` | 0.4.3 | item 7 (alternative) |
| `ssdp-client` | 2.1.0 | item 8 |
| `rupnp` | 3.0.0 | item 8 |
| `socket2` | 0.6.5 | item 8 (middle path) |
| `anymap` | 1.0.0-beta.2 | item 9 |
| `type-map` | 0.5.1 | item 9 |
| `tokio-util` | 0.7.19 | item 11 |
| `thiserror` | 2.0.20 | item 12 |
| `heck` | 0.5.0 | item 13 |
| `strum` | 0.28.0 | tier 3 |
| `moka` | 0.12.16 | tier 3 |
| `lru` | 0.18.2 | tier 3 |
| `backon` | 1.6.0 | tier 3 |
| `tokio-retry` | 0.3.2 | tier 3 |
| `arc-swap` | 1.9.2 | tier 3 |
| `bon` | 3.9.3 | tier 3 |
| `derive_builder` | 0.20.2 | tier 3 |
| `governor` | 0.10.4 | tier 3 |
| `humantime` | 2.4.0 | tier 3 |
| `dashmap` | 7.0.0-rc2 | (not recommended — pre-release) |

---

## Proposed sequencing

1. **Hygiene pass.** `cargo machete` + drop the 17 unused deps, add
   `[workspace.lints]`, `rust-toolchain.toml`, `deny.toml`, MSRV job. An afternoon,
   zero behavioral risk, and it stops further drift.
2. **Dependency consolidation.** One HTTP client, one XML crate, `thiserror 2.0`,
   `warp` → `axum`. Removes ~150 transitive packages.
3. **Delete the hand-rolled XML.** `strip_namespaces` + the three escapers + the two
   substring parsers, in favor of `quick-xml`. ~350 LOC and 8 `unwrap()`s gone.
4. **Reactive layer.** `watch` channels + `CancellationToken`, and resolve the
   `state-store` question. Biggest design change; also fixes `CLAUDE.md:99` and
   `docs/SUMMARY.md:337` being wrong.
5. **Property derive macro.** Most work, least urgent.

## Open Questions

- **Discovery:** accept async (`ssdp-client`/`rupnp`) and reshape the public API, or
  keep blocking and take the `socket2` + `httparse` middle path?
- **`state-store`:** delete the crate, or make `sonos-state` actually use it? It's
  published as `sonos-sdk-state-store`, so removal has a semver dimension.
- **HTTP client:** `ureq 3.x` (stay blocking, matches `soap-client`'s design) or
  `reqwest 0.13` (async, matches the rest of the stack)?
- Is the `firewall-detection` feature meant to be optional long-term? It's never
  tested off and all 5 `sonos-stream` examples require it.

## Notes

- No vendored or copied third-party code found anywhere in the workspace. An
  aggressive scan for attribution patterns returned exactly one hit, a false positive
  (`sonos-api/src/operation/mod.rs:300`, a panic message containing "derived from").
- **Zero `unsafe`** in all 42,801 LOC, including tests and examples.
- Non-test `unwrap()`/`expect()` totals are low (17 + 4 across 12,236 non-comment
  non-test src LOC) and fully accounted for: 8 in the XML tokenizer, 6 lock-poisoning
  in `sonos-api/src/subscription.rs`, 1 each in `system.rs:271` and `server.rs:291`.
  The 3 `expect()`s at `sonos-sdk/src/system.rs:312-320` are gated only by
  `#[cfg(feature = "test-support")]`, not `#[cfg(test)]`, so they ship whenever a
  downstream enables that feature.
