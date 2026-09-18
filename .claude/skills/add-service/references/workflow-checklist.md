# Service Implementation Workflow Checklist

## Pre-Flight Checks

- [ ] Service documentation URL available
- [ ] Operations list identified
- [ ] Properties list identified
- [ ] Real Sonos speaker accessible for testing
- [ ] Clean git working directory (`git status`)

## Layer 1: API (sonos-api)

### Files to Create/Modify
- [ ] `sonos-api/src/service.rs` (add `Service` variant + `name()`/`info()` arms)
- [ ] `sonos-api/src/services/{service}/mod.rs`
- [ ] `sonos-api/src/services/{service}/operations.rs`
- [ ] `sonos-api/src/services/{service}/events.rs`
- [ ] `sonos-api/src/services/{service}/state.rs`
- [ ] `sonos-api/src/services/mod.rs` (add module)
- [ ] `sonos-api/src/lib.rs` (add re-export)

### Implementation Steps
- [ ] Create service directory
- [ ] Define operation structs with Request/Response
- [ ] Declare each operation with `define_upnp_operation!` /
      `define_operation_with_response!` (implements `UPnPOperation`)
- [ ] Add a `Validate` impl for each generated `{Op}Request`
- [ ] Use macros where applicable
- [ ] Add the variant to the `Service` enum in `sonos-api/src/service.rs`, plus its
      `name()` and `info()` match arms
- [ ] Write unit tests

### Verification
```bash
cargo test -p sonos-api
cargo run -p sonos-api --example cli_example -- <speaker_ip> <Service> <Operation>
```
- [ ] All tests pass
- [ ] CLI example works with real speaker

---

## Layer 2: Stream (sonos-stream)

### Files to Modify
- [ ] `sonos-api/src/services/{service}/state.rs`
- [ ] `sonos-api/src/services/{service}/events.rs`
- [ ] `sonos-stream/src/events/types.rs`
- [ ] `sonos-stream/src/events/processor.rs`
- [ ] `sonos-stream/src/polling/strategies.rs`

### Implementation Steps
- [ ] Define `{Service}State` in sonos-api with `Option<T>` fields, deriving
      `Debug, Clone, Serialize, Deserialize, PartialEq`
- [ ] Write `state::poll()` producing `{Service}State`
- [ ] Write `{Service}Event::into_state()` producing the same type
- [ ] Add `EventData::{Service}({Service}State)` variant — no `Event` suffix on the variant
- [ ] Add `convert_api_event_data()` case in processor: downcast, then `into_state()`
- [ ] Implement `{Service}Poller` with `poll_state`, `state_to_event_data`, `service_type`
- [ ] Register poller in `DeviceStatePoller::new()`

### Verification
```bash
cargo test -p sonos-sdk-stream
python .claude/skills/implement-service-stream/scripts/analyze_stream_events.py --validate
python .claude/skills/implement-service-stream/scripts/test_polling.py <speaker_ip> <Service>
```
- [ ] All tests pass
- [ ] Event types listed correctly
- [ ] Polling works with real speaker

---

## Layer 3: State (sonos-state)

### Files to Modify
- [ ] `sonos-state/src/property.rs`
- [ ] `sonos-state/src/decoder.rs`
- [ ] `sonos-state/src/lib.rs`

### Implementation Steps
- [ ] Define property structs with required derives
- [ ] Implement `Property` trait (KEY constant)
- [ ] Implement `SonosProperty` trait (SCOPE, SERVICE)
- [ ] Add constructor and accessor methods
- [ ] Add `PropertyChange` enum variants
- [ ] Update `key()` match arms
- [ ] Update `service()` match arms
- [ ] Implement `decode_{service}()` function
- [ ] Add case in `decode_event()`
- [ ] Re-export properties in lib.rs

### Verification
```bash
cargo test -p sonos-sdk-state
python .claude/skills/implement-service-state/scripts/analyze_properties.py --coverage
```
- [ ] All tests pass
- [ ] Coverage shows new properties

---

## Layer 4: SDK (sonos-sdk)

### Files to Modify
- [ ] `sonos-sdk/src/property/handles.rs`
- [ ] `sonos-sdk/src/speaker.rs`
- [ ] `sonos-sdk/src/lib.rs`

### Implementation Steps
- [ ] Add imports for new operation types
- [ ] Add imports for new property types
- [ ] Implement `Fetchable` trait (if applicable)
- [ ] Create type alias (`pub type {Property}Handle = PropertyHandle<{Property}>`)
- [ ] Add field to `Speaker` struct with doc comment
- [ ] Add handle import to speaker.rs
- [ ] Initialize field in `Speaker::new()`
- [ ] Re-export types in lib.rs

### Verification
```bash
cargo test -p sonos-sdk
python .claude/skills/implement-service-sdk/scripts/analyze_handles.py --coverage
```
- [ ] All tests pass
- [ ] Coverage shows new handles

---

## Integration Testing

### Full Test Suite
```bash
# The feature flag is required: sonos-sdk's tests/property_tests.rs
# does not compile without it. Bare `cargo test` fails.
cargo test --workspace --features sonos-sdk/test-support --locked
```
- [ ] All workspace tests pass

### Real Speaker Test
```bash
python .claude/skills/add-service/scripts/integration_test.py <Service> <speaker_ip>
```
- [ ] API operations work
- [ ] Events/polling work
- [ ] State updates work
- [ ] SDK handles work

### Example Application
```bash
cargo run -p sonos-sdk --example basic_usage_sdk
```
- [ ] Example compiles
- [ ] New properties accessible

---

## Post-Implementation

### Documentation
- [ ] Update crate-level documentation if significant
- [ ] Add examples if complex usage patterns

### Code Quality
- [ ] Run `cargo fmt`
- [ ] Run `cargo clippy`
- [ ] No warnings in new code

### Git
- [ ] Review all changes: `git diff`
- [ ] Commit with descriptive message
- [ ] Consider separate commits per layer

---

## Quick Reference: Files Per Layer

```
Layer 1: API
├── sonos-api/src/service.rs                        (Service enum)
├── sonos-api/src/services/{service}/mod.rs
├── sonos-api/src/services/{service}/operations.rs
├── sonos-api/src/services/{service}/events.rs      ({Service}Event, into_state)
├── sonos-api/src/services/{service}/state.rs       ({Service}State, poll)
├── sonos-api/src/services/mod.rs
└── sonos-api/src/lib.rs

Layer 2: Stream
├── sonos-stream/src/events/types.rs                (EventData variant)
├── sonos-stream/src/events/processor.rs
└── sonos-stream/src/polling/strategies.rs

Layer 3: State
├── sonos-state/src/property.rs
├── sonos-state/src/decoder.rs
└── sonos-state/src/lib.rs

Layer 4: SDK
├── sonos-sdk/src/property/handles.rs
├── sonos-sdk/src/speaker.rs
└── sonos-sdk/src/lib.rs
```

---

## Troubleshooting Quick Reference

| Problem | Check |
|---------|-------|
| Operation fails | API implementation, SOAP payload format |
| Events not received | EventData variant, processor case |
| Polling not working | ServicePoller impl, DeviceStatePoller registration |
| State not updating | PropertyChange variant, decoder function |
| Property None | decoder not parsing field, silent parse failure |
| fetch() missing | Fetchable trait not implemented |
| watch() CacheOnly | Event manager not configured |
