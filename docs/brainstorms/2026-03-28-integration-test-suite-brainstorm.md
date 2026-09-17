# Integration Test Suite for Real Speaker Validation

**Date:** 2026-03-28
**Status:** Landed

## What It Explored

A fast smoke-test suite run against real speakers before PR submission, to catch breaking
changes that unit tests cannot reach.

## What Shipped

A `tests/` integration target, not a binary.

- **File:** `sonos-sdk/tests/integration_real_speakers.rs`
- **Command:** `cargo test --package sonos-sdk --test integration_real_speakers -- --ignored --nocapture`
- **Tests:** `test_api_operations`, `test_save_queue_element_names`, `test_event_streaming`,
  `test_group_lifecycle`, `test_property_watching`, `test_event_integration` — each
  `#[test] #[ignore]`
- **Sync, not async.** `sonos-sdk` has no `tokio` dependency at all, so there is no
  `#[tokio::main]` and no `.await` anywhere in the suite.
- **Fails loudly on missing hardware:** `require_real_speakers()` returns an error rather
  than silently passing. `test_group_lifecycle` is the one exception — it skips with a
  warning when fewer than two standalone speakers are present, since bonded pairs and home
  theatre setups legitimately cannot satisfy it.

## Where To Read About It Now

- [docs/INTEGRATION_TESTS.md](../INTEGRATION_TESTS.md) — how to run the suite, what each
  test validates, and troubleshooting
- `docs/plans/2026-03-28-feat-integration-test-suite-plan.md`
- `docs/plans/2026-05-01-feat-data-freshness-completeness-tests-plan.md` — the follow-on
  freshness suite, `sonos-sdk/tests/data_freshness.rs`
