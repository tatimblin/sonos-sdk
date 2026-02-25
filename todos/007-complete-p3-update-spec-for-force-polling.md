---
status: complete
priority: p3
issue_id: "007"
tags: [code-review, architecture, documentation]
dependencies: []
---

# Update sonos-stream spec for force_polling_mode and firewall_simulation preset

## Problem Statement

Per AGENTS.md rule 2: "Always update the appropriate README.md or SPEC file when you make changes that impact the accuracy of these documents." The spec at `docs/specs/sonos-stream.md` does not mention `force_polling_mode` in the BrokerConfig data model (Section 5.1) or `firewall_simulation()` in Configuration Presets (Section 12.2).

## Findings

- **Architecture reviewer**: "The spec should reflect the new config option and preset."

## Proposed Solutions

### Option A: Update spec sections
- Add `force_polling_mode: bool` to BrokerConfig data model
- Add `firewall_simulation()` to Configuration Presets section
- **Effort**: Small (15 min)
- **Risk**: None

## Technical Details

**Affected files:**
- `docs/specs/sonos-stream.md` — Sections 5.1, 12.2

## Acceptance Criteria

- [ ] `force_polling_mode` documented in BrokerConfig data model
- [ ] `firewall_simulation()` preset documented
- [ ] Behavior description matches implementation

## Work Log

| Date | Action | Learnings |
|------|--------|-----------|
| 2026-02-24 | Identified during PR #36 review | Architecture reviewer — AGENTS.md compliance |

## Resources

- PR: #36
- Spec: `docs/specs/sonos-stream.md`
