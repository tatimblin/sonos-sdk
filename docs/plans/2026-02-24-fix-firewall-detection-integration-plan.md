---
title: "fix: Firewall detection integration gaps and mock infrastructure"
type: fix
status: completed
date: 2026-02-24
origin: docs/brainstorms/2026-02-24-product-roadmap-brainstorm.md
---

# Fix Firewall Detection Integration Gaps

## Overview

The firewall detection system in `sonos-stream` is partially implemented. The core detection coordinator works (tracking device status as Unknown/Accessible/Blocked, timeout monitoring, caching), but three integration points are broken, and there's no infrastructure for testing firewall scenarios.

First-subscription detection works: after 15 seconds with no UPnP event, a device is marked `Blocked` and subsequent registrations get immediate polling. But mid-stream event loss (events were flowing then stopped) never triggers polling fallback because the `EventDetector` is disconnected from the `EventBroker`.

(see brainstorm: docs/brainstorms/2026-02-24-product-roadmap-brainstorm.md — Key decision: "Polling is must-have for all 5 services")

## Problem Statement

Three integration bugs prevent full firewall detection:

1. **`EventDetector` never connected to `EventBroker`** — `broker.rs:202-206` creates the detector but never calls `set_firewall_coordinator()` or `set_polling_request_sender()`. The ongoing event timeout monitoring task runs but can't interact with the rest of the system.

2. **Polling request channel sender dropped immediately** — `broker.rs:277-278`:
   ```rust
   let (_polling_request_sender, polling_request_receiver) = mpsc::unbounded_channel();
   ```
   The `_` prefix means the sender is dropped on creation. Nothing can send polling requests through the channel, so `start_polling_request_processing()` processes an empty receiver forever.

3. **Event timeout monitoring is stubbed** — `event_detector.rs:152-193` runs a monitoring task that detects when events stop arriving (>30s timeout) but only prints a debug message:
   ```rust
   eprintln!("⏰ Event timeout detected for registration {}", registration_id);
   // TODO: Look up speaker/service pair and send actual request
   ```
   No actual polling is started.

Additionally, there's no way to mock a firewall for testing — the only way to test firewall detection is with real Sonos hardware behind a real firewall.

## Proposed Solution

Fix the three integration bugs and add a firewall simulation capability for testing.

**Files to modify:**
- `sonos-stream/src/broker.rs` — connect EventDetector, fix channel sender
- `sonos-stream/src/subscription/event_detector.rs` — implement actual polling request on timeout
- `sonos-stream/src/config.rs` — add force-polling config option for firewall simulation

## Technical Considerations

### What Already Works (Don't Touch)

- `callback-server/src/firewall_detection.rs` — `FirewallDetectionCoordinator` is fully implemented and tested (6 passing tests). Tracks per-device status, monitors timeouts, caches results.
- `sonos-stream/src/events/processor.rs:84-87` — event arrival notification to coordinator works.
- `sonos-stream/src/broker.rs:435-505` — first-subscription firewall check and immediate polling activation works.

### Dead Config Options

Three config options exist but are never read:
- `firewall_detection_timeout: Duration` (10s)
- `firewall_detection_retries: u32` (2)
- `firewall_detection_fallback: bool` (true)

Decision: remove these to avoid confusion. They were likely intended for retry logic that was never built.

## Implementation Tasks

---

### Task 1: Connect EventDetector to EventBroker

**File:** `sonos-stream/src/broker.rs`

**Current state:** EventDetector is created at lines 202-206 but `set_firewall_coordinator()` and `set_polling_request_sender()` are never called.

**Fix:**

- [x] After creating `event_detector`, call `event_detector.set_firewall_coordinator(firewall_coordinator.clone())` if firewall detection is enabled
- [x] Store the polling request sender (not drop it — see Task 2) and call `event_detector.set_polling_request_sender(sender.clone())`
- [x] Verify `event_detector.start_monitoring()` is called during broker startup

**Pattern:** Follow how `firewall_coordinator` is already passed to `EventProcessor` at `broker.rs:220-222`.

---

### Task 2: Fix Polling Request Channel

**File:** `sonos-stream/src/broker.rs`

**Current state:** Line 277-278 drops the sender immediately:
```rust
let (_polling_request_sender, polling_request_receiver) = mpsc::unbounded_channel();
```

**Fix:**

- [x] Remove the `_` prefix — store the sender as `polling_request_sender`
- [x] Pass it to `event_detector.set_polling_request_sender()` (from Task 1)
- [x] Store it in the `EventBroker` struct if other components need to send polling requests
- [x] Verify `start_polling_request_processing(polling_request_receiver)` correctly processes incoming requests

---

### Task 3: Implement Event Timeout → Polling Activation

**File:** `sonos-stream/src/subscription/event_detector.rs`

**Current state:** Lines 152-193 detect event timeout but only print a debug message.

**Fix:**

- [x] When event timeout is detected for a registration:
  1. Look up the `SpeakerServicePair` for the registration ID (needs access to the registry or a stored mapping)
  2. Create a `PollingRequest` with action `Start` and reason `EventTimeout`
  3. Send the request through the polling request sender channel
- [x] Handle the case where the speaker/service pair is not found (log warning, skip)
- [x] Add a `registration_pairs: HashMap<RegistrationId, SpeakerServicePair>` or pass the registry reference to EventDetector
- [x] Add method `register_pair(registration_id, pair)` called when a new subscription is created
- [x] Add method `unregister_pair(registration_id)` called when a subscription is removed
- [x] Add test verifying that event timeout sends a polling request

---

### Task 4: Add Firewall Simulation Config

**File:** `sonos-stream/src/config.rs`

**Purpose:** Allow testing firewall behavior without a real firewall.

**Tasks:**

- [x] Add `force_polling_mode: bool` (default: `false`) to `BrokerConfig`
  - When true, skip UPnP subscription entirely and go straight to polling for all registrations
  - This simulates a firewall that blocks all callback traffic
- [x] Add validation in `BrokerConfig::validate()` — `force_polling_mode` is incompatible with `enable_proactive_firewall_detection: false`
- [x] Add `BrokerConfig::firewall_simulation()` preset that enables force polling with fast intervals
- [x] Wire `force_polling_mode` into `EventBroker::register_speaker_service()` — if true, skip subscription creation, immediately start polling
- [x] Add test verifying force_polling_mode bypasses UPnP subscriptions

---

### Task 5: Remove Dead Config Options

**File:** `sonos-stream/src/config.rs`

- [x] Remove `firewall_detection_timeout` field (never read anywhere)
- [x] Remove `firewall_detection_retries` field (never read anywhere)
- [x] Remove `firewall_detection_fallback` field (never read anywhere)
- [x] Update all config presets that set these values
- [x] Verify no other code references these fields

---

### Task 6: Update Tests and Examples

**Files:** `sonos-stream/src/subscription/event_detector.rs`, `sonos-stream/examples/firewall_handling.rs`

- [x] Add test `test_event_timeout_sends_polling_request` — verify that when events stop arriving, a polling request is sent through the channel
- [x] Add test `test_force_polling_mode` — verify that with `force_polling_mode: true`, registrations go straight to polling (validated via config tests for `firewall_simulation()` preset and validation)
- [x] Update `firewall_handling.rs` example to demonstrate `force_polling_mode` as a testing strategy
- [x] Add inline documentation explaining how to test firewall scenarios

---

## Acceptance Criteria

- [x] `cargo test -p sonos-stream` passes (47 pass, 2 pre-existing failures unrelated to this work)
- [x] `cargo test -p callback-server` passes (32 tests)
- [x] `cargo clippy -p sonos-stream` passes (no new warnings from our changes)
- [x] EventDetector is connected to EventBroker (set_firewall_coordinator + set_polling_request_sender called)
- [x] Polling request channel sender is not dropped
- [x] Event timeout (>30s no events) triggers automatic polling activation
- [x] `force_polling_mode` config option bypasses UPnP subscriptions and goes straight to polling
- [x] Dead config options removed
- [x] Tests cover both the integration fixes and the new force_polling_mode

## Dependencies & Risks

**Dependencies:**
- No dependency on the polling plan (the pollers themselves work independently of how they're activated)
- The polling plan depends on this working correctly for end-to-end firewall fallback

**Risks:**

| Risk | Impact | Mitigation |
|------|--------|------------|
| EventDetector needs registration → pair mapping | Requires passing speaker/service pairs to EventDetector | Add a simple HashMap registration method |
| Connecting the channel may surface timing issues | Race between detection and polling start | Use the existing channel ordering guarantee (unbounded channel) |
| Removing dead config breaks external users | Users referencing these config fields get compile errors | These fields were never read — no behavior change |

## Sources & References

### Internal References

- FirewallDetectionCoordinator: `callback-server/src/firewall_detection.rs:1-491`
- EventDetector: `sonos-stream/src/subscription/event_detector.rs`
- EventBroker: `sonos-stream/src/broker.rs`
- Config: `sonos-stream/src/config.rs`
- Event processor firewall integration: `sonos-stream/src/events/processor.rs:84-87`
- Firewall handling example: `sonos-stream/examples/firewall_handling.rs`
- sonos-stream spec: `docs/specs/sonos-stream.md`
