# Principal Engineer Review: SDK_CLEANUP_PLAN.md

**Reviewer**: Principal Engineer
**Date**: 2026-01-13
**Verdict**: Not Ready for Implementation

---

## Executive Summary

The plan has a good north star (simplify sonos-sdk to a thin wrapper), but contains several architectural inconsistencies, underdeveloped sections, and potential implementation pitfalls. Another revision is needed before implementation.

---

## Critical Issues

### 1. Duplicate Types Already Exist - Plan Contradicts Reality

The plan states sonos-sdk should "re-export Speaker from sonos-state," but examining the code:

- `sonos-state/src/speaker_handle.rs:48-84` already has a `Speaker` type with **all 9 properties**
- `sonos-sdk/src/speaker.rs:9-24` has a different `Speaker` type with **only 2 properties**

The plan doesn't acknowledge this divergence or explain how to reconcile them. Which `Speaker` is the source of truth? The sonos-state version is more complete, so the plan's direction is correct, but this needs explicit callout.

**Author Response:**

The plan's direction is correct: sonos-sdk's `Speaker` should be deleted and sonos-state's `Speaker` should be re-exported. The concern here is not about confusion over which type is the source of truth.

The concern is that the plan does not acknowledge the **current state of divergence** between the two types, which represents a significant breaking change:

- sonos-sdk's current `Speaker`: 2 properties (the legacy, minimal version)
- sonos-state's `Speaker`: 9 properties (the complete, reactive version)

The plan should be updated to:

1. **Acknowledge this divergence explicitly** - Document that the types currently differ significantly in scope
2. **Document as a breaking change** - Any existing sonos-sdk consumers will see a dramatically different `Speaker` API (2 properties to 9 properties)
3. **Provide a migration guide** covering:
   - Field name differences (e.g., `ip` vs `ip_address` if applicable)
   - New properties consumers will now have access to (Volume, Mute, Bass, Treble, Loudness, PlaybackState, Position, CurrentTrack, GroupMembership)
   - Any behavioral differences between the two implementations
   - Import path changes

This is primarily a documentation concern, not an architectural one. The implementation direction is sound.

### 2. WatchedPropertyRegistry vs WatchCache - Unclear Relationship

The plan proposes a new `WatchedPropertyRegistry` to track which properties have been watched for `iter()` filtering. But `WatchCache` (`sonos-state/src/watch_cache.rs`) already:
- Tracks `(SpeakerId, property_key)` pairs
- Has `has_watch()` for checking if a property is being watched
- Manages cleanup timers

Why not extend `WatchCache`? The plan needs to clarify:
- Are these complementary or redundant?
- Why can't `iter()` filter based on what's in `WatchCache`?

**Author Response:**

`WatchCache` and `WatchedPropertyRegistry` are intended to be the same thing. The plan should be updated to extend `WatchCache` rather than introduce a new `WatchedPropertyRegistry` type.

`WatchCache` already tracks `(SpeakerId, property_key)` pairs and has the infrastructure needed for `iter()` filtering:
- The `has_watch()` method can be used to check if events should be emitted
- The existing `HashMap<(SpeakerId, &'static str), WatchEntry>` structure provides the lookup needed
- Cleanup timer management is already handled

The plan should be updated to specify what extensions to `WatchCache` are needed to support `iter()` filtering:
1. A method to iterate over all currently watched `(SpeakerId, property_key)` pairs
2. A method to check if a given `ChangeEvent` matches any watched property (for filtering)
3. Any thread-safety considerations for accessing this from the blocking `iter()` context

This simplifies the architecture by avoiding a second registry, reducing code duplication, and maintaining a single source of truth for "what properties are being watched."

### 3. Blocking `iter()` in Async Context - Design Smell

```rust
pub fn iter(&self) -> impl Iterator<Item = ChangeEvent>; // blocking
```

This is described as "for ratatui render loops," but the rest of the system is async. How does a blocking iterator consume events from async UPnP subscriptions? The plan doesn't address:

- How does `iter()` bridge async -> sync?
- Does it use `tokio::sync::mpsc` with `blocking_recv()`?
- What happens when no events are available - does it block forever?
- Can it be interrupted for graceful shutdown?

**Author Response:**

The synchronous `iter()` design is intentional and correct, not a design smell. This is a deliberate architectural choice driven by the primary use case: ratatui render loops require synchronous iteration.

More broadly, the author favors synchronous APIs wherever possible over async. The codebase can and should make anything down the code path synchronous as needed to support this requirement. Async complexity should be confined to internal implementation details, not exposed in the public API surface.

That said, the questions raised are valid implementation details that should be addressed in the plan:

1. **Async-to-sync bridging**: The plan should specify the bridging mechanism (e.g., `std::sync::mpsc::Receiver` fed by an async task, or `tokio::sync::mpsc` with `blocking_recv()`)
2. **Blocking behavior**: Document whether `iter()` blocks indefinitely or has a timeout variant, and specify channel buffer sizing
3. **Graceful shutdown**: Define the shutdown protocol - how does dropping `SonosSystem` or calling a shutdown method signal the iterator to terminate?

The recommendation to add `iter()` implementation details (R3) is accepted. The plan should be updated to specify these mechanics while maintaining the synchronous API requirement as a non-negotiable constraint.

**Reframing**: This issue should be retitled from "Design Smell" to "Implementation Details Needed" - the synchronous design is architecturally sound, but the plan requires additional specification of the underlying implementation.

### 4. `fetch()` Implementation - Acceptable for Initial Scope

The plan shows:
```rust
pub async fn fetch(&self) -> Result<P> {
    // 1. Build operation based on P::SERVICE and P::KEY
    // ...
}
```

But this requires a dispatch table mapping properties to operations. The plan lists three examples (Volume, Mute, PlaybackState) but there are 9 properties. More critically:
- Where does this mapping live?
- How do you handle properties with non-trivial fetch semantics (e.g., `CurrentTrack` requires parsing DIDL-Lite)?
- How is error handling different from `watch()`?

**Author Response:**

Acknowledged that `fetch()` is under-specified in the current plan. However, the complexity here is low - calling sonos-api operations and extracting values is straightforward. The mapping between properties and their corresponding API operations is well-defined by the UPnP service specifications, and sonos-api already provides the typed operations needed.

This can be figured out incrementally during implementation. For the initial implementation, just getting `Volume` working is sufficient to prove out the pattern. Other properties can be added as needed following the same approach:

1. Match on property type
2. Call the appropriate sonos-api operation
3. Extract and return the value

Properties with more complex parsing requirements (e.g., `CurrentTrack` with DIDL-Lite, `GroupMembership` with XML topology) can be addressed in subsequent iterations once the basic pattern is established and working.

This is a reasonable scope reduction for the first iteration. The focus should be on getting the core architecture right (`watch()`, `iter()`, async-to-sync bridging) rather than exhaustively implementing all property fetchers upfront.

**Reframing**: This issue has been retitled from "Severely Under-Specified" to "Acceptable for Initial Scope" - the gap is acknowledged, but it does not block initial implementation.

### 5. Missing `Topology` Property in Speaker Struct

The plan's "Final API Surface" shows `Speaker` with "all 9 properties" but the existing sonos-state `Speaker` also doesn't include `Topology`. Is topology a speaker-level property or system-level? This affects the API design.

**Author Response:**

Topology will be handled in a later iteration. Topology relates to groups, not individual speakers, and is intentionally out of scope for the current cleanup plan. Group-related functionality will be addressed separately once the core speaker-level API is stabilized.

---

## Architecture Concerns

### 6. Service-Level vs Property-Level Mismatch Not Fully Addressed

The plan acknowledges the insight that UPnP subscriptions are service-level, but `watch()` and `iter()` filtering are property-level. However:

- What happens if you `watch()` volume, then later `watch()` mute (same service)?
- Does the second `watch()` add "mute" to the registry without touching subscriptions?
- When the service subscription times out, do both properties lose their registry entries?

The lifecycle binding between `WatchedPropertyRegistry` and actual subscriptions needs clarification.

**Author Response:**

The lifecycle binding between WatchCache entries and service subscriptions is managed through reference counting in `PropertySubscriptionManager`. Here's how the mechanism works:

1. **Property watch creates registry entry**: When a user calls `watch()` on a property (e.g., Volume), the property gets added to the WatchCache registry.

2. **Reference counting increments**: When a property is watched, it increments `subscription_refs` in `PropertySubscriptionManager` for the corresponding service (e.g., RenderingControl).

3. **Shared subscriptions via ref count**: When another property using the same service is watched (e.g., Mute, which also uses RenderingControl), the ref count increments to 2. Both properties share the same underlying UPnP subscription.

4. **Cleanup on ref count zero**: Unsubscribing from the service only happens when the ref count drops to 0. If you stop watching Volume but still have an active Mute watcher, the RenderingControl subscription remains active (ref count = 1).

5. **Service failure handling**: If a service stops for some reason (subscription timeout, network error, etc.), WatchCache should clear out the values for all properties associated with that service. This ensures stale data isn't returned when the underlying subscription is no longer valid.

This reference counting mechanism ensures efficient subscription management - multiple property watchers share a single service subscription, and cleanup only occurs when no properties for that service are being watched. The plan should be updated to explicitly document this lifecycle and the relationship between WatchCache entries and `PropertySubscriptionManager` ref counts.

### 7. Cleanup Timeout Configurability - Layering Violation

The plan adds `StateManagerBuilder` with `cleanup_timeout` to sonos-state, and `SonosSystemBuilder` with the same to sonos-sdk. But if sonos-sdk is a "thin wrapper," why does it need its own builder? Just expose `StateManagerBuilder` via re-export.

**Author Response:**

Agreed - will remove the redundant `SonosSystemBuilder`. Will re-export `StateManagerBuilder` from sonos-state instead. This maintains the "thin wrapper" principle.

### 8. Error Type Strategy Incomplete

The plan says `sonos-sdk/src/error.rs` should "simplify, re-export sonos-state errors." But what about sonos-api errors during `fetch()`? The error hierarchy across crates needs thought:

```
SdkError
  StateError (from sonos-state)
  ApiError (from sonos-api)
  DiscoveryError (from sonos-discovery)
```

**Author Response:**

Agreed - will define a proper error hierarchy. Will document how errors from sonos-state, sonos-api, and sonos-discovery are handled/wrapped.

---

## Missing Details

### 9. No Thread Safety Analysis

- `WatchedPropertyRegistry` uses `Arc<RwLock<HashSet<...>>>`
- `WatchCache` uses `Arc<RwLock<HashMap<...>>>`
- `iter()` needs to read from these in a blocking context

What's the locking strategy? Can `iter()` deadlock if called from within a `watch()` callback?

### 10. No Migration Path

The plan deletes `sonos-sdk/src/speaker.rs` and `property/` directory. If there are existing consumers of `sonos_sdk::Speaker`, how do they migrate? The API surface changes (9 properties vs 2).

### 11. Testing Strategy is Too Vague

Phase 3 lists:
- "Test `fetch()` updates state and returns value"
- "Test `watch()` registers property and returns current value"
- "Test `iter()` only emits events for watched properties"

These are acceptance criteria, not test strategies. Where are the unit tests? Integration tests? Mock strategies for UPnP services?

### 12. No Consideration of Drop Semantics

What happens when:
- A `Speaker` is dropped - do its property handles clean up?
- A `PropertyHandle` is dropped mid-watch?
- The `SonosSystem` is dropped while `iter()` is blocking?

---

## Recommendations

### R1. Add an Architecture Diagram

Show the data flow between `watch()` -> `WatchCache` -> `WatchedPropertyRegistry` -> `iter()` -> consumers

### R2. Consolidate or Explicitly Delineate Cache Responsibilities

`WatchCache` and `WatchedPropertyRegistry` responsibilities need clear separation or consolidation.

### R3. Specify the `iter()` Implementation

Likely needs:
```rust
// Internal: async event stream -> sync channel -> blocking iterator
struct ChangeIterator {
    rx: std::sync::mpsc::Receiver<ChangeEvent>,
}
```

Document:
- Channel buffer size
- Blocking vs non-blocking variant
- Shutdown/interrupt behavior

### R4. Add `fetch()` Operation Mapping Table

Show all 9 properties and their corresponding API operations:

| Property | Service | API Operation | Notes |
|----------|---------|---------------|-------|
| Volume | RenderingControl | GetVolume | |
| Mute | RenderingControl | GetMute | |
| Bass | RenderingControl | GetBass | |
| Treble | RenderingControl | GetTreble | |
| Loudness | RenderingControl | GetLoudness | |
| PlaybackState | AVTransport | GetTransportInfo | |
| Position | AVTransport | GetPositionInfo | |
| CurrentTrack | AVTransport | GetPositionInfo | Requires DIDL-Lite parsing |
| GroupMembership | ZoneGroupTopology | GetZoneGroupState | XML parsing |

### R5. Remove Redundant Builder

Either `SonosSystemBuilder` or `StateManagerBuilder`, not both. If sonos-sdk is a thin wrapper, just re-export the sonos-state builder.

### R6. Add Breaking Change Documentation

List API differences for existing consumers:
- `Speaker` now has 9 properties instead of 2
- Property handles have different method signatures
- Import paths change

### R7. Document Drop/Cleanup Lifecycle

Add a section explaining:
- When subscriptions are cleaned up
- What triggers registry unregistration
- How graceful shutdown works

---

## Questions for Author

1. Is there a reason `WatchedPropertyRegistry` can't be merged into `WatchCache`?
2. What's the expected throughput of `iter()` - events per second?
3. Should `iter()` be `try_iter()` (non-blocking) or have a timeout variant?
4. Are there any consumers of the current `sonos_sdk::Speaker` API that need migration support?

---

## Approval Criteria

Before implementation, the plan should address:

- [ ] Clarify `WatchCache` vs `WatchedPropertyRegistry` relationship
- [ ] Specify `iter()` implementation details (async->sync bridge)
- [ ] Complete `fetch()` operation mapping for all 9 properties
- [ ] Add architecture diagram
- [ ] Document drop/cleanup semantics
- [ ] Remove redundant builder or justify both
