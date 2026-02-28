---
status: complete
priority: p2
issue_id: "011"
tags: [code-review, architecture, agent-native]
dependencies: []
---

# Preserve structured ValidationError instead of flattening to String

## Problem Statement

The `exec()` helper converts `ValidationError` to `SdkError::OperationFailed(e.to_string())`, destroying the structured error data (parameter name, value, min/max range). This prevents agents from making programmatic retry decisions based on which parameter failed and what the valid range is.

## Findings

- **Source**: Agent-Native Reviewer, Code Simplicity Reviewer
- **Location**: `sonos-sdk/src/speaker.rs` line 176, `sonos-sdk/src/group.rs` line 203
- **Alternative noted by Simplicity Reviewer**: `ApiError` already has `From<ValidationError>` converting to `ApiError::InvalidParameter(String)`, so `OperationFailed` could be eliminated entirely by routing through `SdkError::ApiError(ApiError::from(e))`.

## Proposed Solutions

### Option A: Embed ValidationError directly in SdkError
```rust
#[error("Validation failed: {0}")]
ValidationFailed(#[from] sonos_api::operation::ValidationError),
```
- **Pros**: Full structured data preserved, agents can match on parameter/range
- **Cons**: Exposes sonos-api type in SDK error enum
- **Effort**: Small
- **Risk**: Low

### Option B: Remove OperationFailed, route through ApiError
```rust
let op = operation.map_err(|e| SdkError::ApiError(ApiError::from(e)))?;
```
- **Pros**: Fewer error variants, simpler SdkError enum
- **Cons**: Still loses structure (ApiError::InvalidParameter is a String)
- **Effort**: Small
- **Risk**: Low

### Option C: Keep OperationFailed as-is
- **Pros**: No change needed
- **Cons**: Opaque string errors, poor agent experience
- **Effort**: None
- **Risk**: None

## Recommended Action

Option A for best agent experience, or Option B for simplicity.

## Acceptance Criteria

- [ ] Validation errors preserve parameter name and valid range
- [ ] Tests updated to match on new error variant
- [ ] Agent can determine which parameter failed and what valid range is

## Resources

- PR #41: https://github.com/tatimblin/sonos-sdk/pull/41
