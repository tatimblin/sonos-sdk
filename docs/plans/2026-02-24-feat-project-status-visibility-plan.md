---
title: "feat: Add project status matrix and impact roadmap"
type: feat
status: completed
date: 2026-02-24
origin: docs/brainstorms/2026-02-24-project-visibility-brainstorm.md
---

# Add Project Status Matrix and Impact Roadmap

## Overview

Create `docs/STATUS.md` — a living document with two sections: (1) a service completion matrix tracking all 17+ known Sonos services across the 6 implementation checkpoints, and (2) an impact-prioritized roadmap of incomplete work. Reference it from CLAUDE.md and AGENTS.md so AI agents always check status before working on services.

(see brainstorm: docs/brainstorms/2026-02-24-project-visibility-brainstorm.md)

## Acceptance Criteria

- [x] `docs/STATUS.md` exists with a status matrix covering all known services
- [x] Matrix uses 6 columns: API, Stream Events, Stream Polling, State Decoder, SDK Handles, SDK Fetch
- [x] Status values are: Done, Partial (with notes), Stub, None, or blank (N/A)
- [x] All 6 started services have accurate per-checkpoint status with specific gap notes
- [x] All 12+ unstarted services are listed with blank/None status
- [x] Impact-prioritized roadmap section with 5 tiers (from brainstorm)
- [x] CLAUDE.md references STATUS.md in a visible location
- [x] AGENTS.md references STATUS.md with instruction to check before service work

## Implementation Steps

### Step 1: Create `docs/STATUS.md`

Write the file with two main sections:

**Section 1 — Service Completion Matrix**

Use the verified data below (corrected from brainstorm research):

```
Service               | API     | Stream Events | Stream Polling | State Decoder | SDK Handles | SDK Fetch
----------------------|---------|---------------|----------------|---------------|-------------|----------
AVTransport           | Done    | Done          | Partial [1]    | Done          | Done        | Partial [2]
RenderingControl      | Partial [3] | Done      | Partial [4]    | Done          | Done        | Partial [5]
GroupRenderingControl | Done    | Done          | Stub           | Partial [6]   | Partial [7] | Done
ZoneGroupTopology     | Done    | Done          | Stub           | Done          | Partial [8] | None [9]
GroupManagement       | Done    | Done          | Stub           | None          | None        | N/A
DeviceProperties      | None    | Partial [10]  | None           | None          | None        | N/A
AlarmClock            | None    | None          | None           | None          | None        | N/A
AudioIn               | None    | None          | None           | None          | None        | N/A
ConnectionManager     | None    | None          | None           | None          | None        | N/A
ContentDirectory      | None    | None          | None           | None          | None        | N/A
HTControl             | None    | None          | None           | None          | None        | N/A
MusicServices         | None    | None          | None           | None          | None        | N/A
Queue                 | None    | None          | None           | None          | None        | N/A
SystemProperties      | None    | None          | None           | None          | None        | N/A
VirtualLineIn         | None    | None          | None           | None          | None        | N/A
```

**Footnotes** (each explains what's missing):

1. Polling only calls GetTransportInfo; position/track data are TODOs
2. CurrentTrack has no fetch() — only Volume, PlaybackState, Position do
3. Only GetVolume, SetVolume, SetRelativeVolume — missing GetMute, GetBass, GetTreble, GetLoudness operations
4. Polling only queries volume; mute is hardcoded to `false`
5. Only Volume has fetch() — Mute, Bass, Treble, Loudness require Get operations first (see [3])
6. Only GroupVolume decoded; GroupMute and GroupVolumeChangeable not decoded despite event data
7. GroupVolume handle exists on Group; no GroupMute handle
8. GroupMembership on Speaker; Topology is system-level with no SDK handle
9. GroupMembership has no fetch(); could use GetZoneGroupState
10. DevicePropertiesEvent type exists in stream but no Service enum variant; uses ZoneGroupTopology as fallback

**Section 2 — Impact Roadmap**

Five tiers from brainstorm (with correction: GroupVolume fetch exists, remove from Tier 1):

- **Tier 1 — Users Can't Get Initial Values**: Missing fetch() for CurrentTrack, GroupMembership. Mute/Bass/Treble/Loudness fetch requires API operations first.
- **Tier 2 — Incomplete Existing Services**: GroupRenderingControl decoder gaps, GroupManagement state/SDK layers, polling hardcodes
- **Tier 3 — Reliability Under Firewall**: Stub polling strategies for ZoneGroupTopology, GroupRenderingControl, GroupManagement
- **Tier 4 — New Service Expansion**: DeviceProperties, Queue, ContentDirectory, AlarmClock, etc.
- **Tier 5 — Quality & Testing**: Fix 2 sonos-stream iterator test failures, polling integration tests

### Step 2: Add reference to CLAUDE.md

Add a short section after the "Project Overview" paragraph (before "Development Commands"):

```markdown
> **Project Status**: See [docs/STATUS.md](docs/STATUS.md) for the service completion matrix and development roadmap.
```

This keeps it near the top where it's immediately visible.

### Step 3: Add reference to AGENTS.md

Add a new rule to the "Standard Development Workflow" section:

```markdown
6. Before working on any service, check [docs/STATUS.md](/docs/STATUS.md) for the current implementation status across all layers.
7. After completing work on a service, update the status matrix in STATUS.md to reflect your changes.
```

### Step 4: Validate

- Re-read STATUS.md to confirm all footnotes match actual code state
- Verify CLAUDE.md and AGENTS.md references render correctly in markdown

## Sources

- **Origin brainstorm:** [docs/brainstorms/2026-02-24-project-visibility-brainstorm.md](docs/brainstorms/2026-02-24-project-visibility-brainstorm.md) — Key decisions: standalone STATUS.md location, all 17+ services in scope, impact-prioritized roadmap, manual maintenance, AI agent references
- **Existing tool:** `.claude/skills/add-service/scripts/service_status.py` — can scan source code for service status (useful for future validation)
- **Architecture reference:** `docs/adding-services.md` — defines the 4-layer, 6-checkpoint pattern
- **Code verification:** Exact per-layer status verified against source files in sonos-api, sonos-stream, sonos-state, sonos-sdk
