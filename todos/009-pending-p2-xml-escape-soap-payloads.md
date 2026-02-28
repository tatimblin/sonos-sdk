---
status: complete
priority: p2
issue_id: "009"
tags: [code-review, security]
dependencies: []
---

# Add XML escaping to SOAP payload string interpolation

## Problem Statement

All string parameters flowing into SOAP XML payloads are interpolated via `format!()` with zero XML escaping. This is a pre-existing issue in `sonos-api`, but PR #41 significantly expands the attack surface by exposing 10+ string-accepting methods to SDK consumers.

## Findings

- **Source**: Security Sentinel agent
- **Affected layer**: `sonos-api` (not `sonos-sdk` directly)
- **Affected files**: `sonos-api/src/services/av_transport/operations.rs` (payload macros), `soap-client/src/lib.rs` lines 89-100
- **Affected SDK methods**: `set_av_transport_uri`, `set_next_av_transport_uri`, `seek`, `configure_sleep_timer`, `add_uri_to_queue`, `remove_track_from_queue`, `save_queue`, `create_saved_queue`, `delegate_coordination_to`
- **Impact**: A caller passing `</CurrentURI><Injected>payload</Injected><CurrentURI>` as a URI would produce malformed XML. Practical risk is low (local-network SDK, requires caller cooperation), but defense-in-depth warrants escaping.

## Proposed Solutions

### Option A: XML escape helper in sonos-api payload construction
- Add `fn xml_escape(s: &str) -> String` using `quick-xml` (already a dependency)
- Apply to all string fields in `define_upnp_operation!` macro payload closures
- **Pros**: Fixes at the source, protects all consumers
- **Cons**: Requires modifying the macro
- **Effort**: Medium
- **Risk**: Low

### Option B: Escape at SDK boundary before passing to builders
- **Pros**: Contained to sonos-sdk
- **Cons**: Doesn't protect direct sonos-api users; easy to miss new methods
- **Effort**: Medium
- **Risk**: Medium (incomplete coverage)

## Recommended Action

Option A — fix at the source in `sonos-api`.

## Technical Details

- **Affected crate**: `sonos-api`
- **Key file**: `sonos-api/src/operation/macros.rs` (payload generation)

## Acceptance Criteria

- [ ] All string parameters in SOAP payloads are XML-escaped before interpolation
- [ ] Test with XML special characters (`<`, `>`, `&`, `"`, `'`) in string params
- [ ] No regression in existing tests

## Resources

- PR #41: https://github.com/tatimblin/sonos-sdk/pull/41
