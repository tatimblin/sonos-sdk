---
status: complete
priority: p2
issue_id: "001"
tags: [code-review, architecture, quality]
dependencies: []
---

# Remove force_polling_mode / firewall_detection validation coupling

## Problem Statement

The validation rule in `config.rs:199-203` rejects `force_polling_mode: true` when `enable_proactive_firewall_detection: false`. But the force-polling code path in `broker.rs:447-475` never uses the `FirewallDetectionCoordinator` — it hardcodes `FirewallStatus::Blocked` and starts polling directly. This is a phantom dependency that creates a confusing API contract.

4 of 5 review agents independently flagged this as incorrect.

## Findings

- **Simplicity reviewer**: "The stated rationale is that force_polling_mode 'requires' firewall detection. But the force-polling branch never reads firewall_coordinator or touches the firewall detection system at all."
- **Architecture reviewer**: "force_polling_mode is conceptually orthogonal to enable_proactive_firewall_detection."
- **Security reviewer**: "force_polling_mode by definition skips all firewall detection logic."
- **Agent-native reviewer**: "An agent constructing a minimal config would get a confusing validation error for what seems like a perfectly reasonable combination."

## Proposed Solutions

### Option A: Remove the validation rule (Recommended)
- Remove the `if self.force_polling_mode && !self.enable_proactive_firewall_detection` check
- Remove the `invalid_force_polling` test case
- **Pros**: Simple, removes confusion, allows valid configs
- **Cons**: None — there is no actual runtime dependency
- **Effort**: Small (10 min)
- **Risk**: None

### Option B: Document the rationale
- Keep the rule but add a detailed doc comment explaining why
- **Pros**: No code change
- **Cons**: There is no real rationale to document — the dependency doesn't exist
- **Effort**: Small
- **Risk**: Perpetuates the confusion

## Technical Details

**Affected files:**
- `sonos-stream/src/config.rs` — lines 199-203 (validation), lines 271-276 (test)

## Acceptance Criteria

- [ ] Validation rule removed from `BrokerConfig::validate()`
- [ ] Test `invalid_force_polling` removed or updated
- [ ] `BrokerConfig { force_polling_mode: true, enable_proactive_firewall_detection: false, ..Default::default() }` validates successfully

## Work Log

| Date | Action | Learnings |
|------|--------|-----------|
| 2026-02-24 | Identified during PR #36 review | Consensus finding across 4 agents |

## Resources

- PR: #36
- File: `sonos-stream/src/config.rs:199-203`
