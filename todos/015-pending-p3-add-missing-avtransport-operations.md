---
status: complete
priority: p3
issue_id: "015"
tags: [code-review, agent-native]
dependencies: []
---

# Expose 5 missing AVTransport operations at SDK level

## Problem Statement

Five AVTransport operations available in `sonos-api` are not exposed as SDK methods: `get_device_capabilities`, `remove_track_range_from_queue`, `backup_queue`, `snooze_alarm`, `get_running_alarm_properties`. An agent building queue management or alarm workflows must drop to the lower-level crate.

## Findings

- **Source**: Agent-Native Reviewer
- **Impact**: Incomplete SDK coverage for queue range removal and alarm operations

## Proposed Solutions

Add 5 one-liner wrapper methods to Speaker, following the existing `exec()` pattern.

## Acceptance Criteria

- [ ] `remove_track_range_from_queue(update_id, starting_index, count)` added
- [ ] `backup_queue()` added
- [ ] `snooze_alarm(duration)` added
- [ ] `get_running_alarm_properties()` added
- [ ] `get_device_capabilities()` added
- [ ] Response types re-exported

## Resources

- PR #41: https://github.com/tatimblin/sonos-sdk/pull/41
