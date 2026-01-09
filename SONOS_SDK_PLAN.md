# Sonos SDK Implementation Plan

## Overview

The `sonos-sdk` crate will be the **single entry point** for developers using the Sonos SDK. It provides a clean, DOM-like API that abstracts all complexity while leveraging the existing robust infrastructure (sonos-state, sonos-api, sonos-discovery, etc.).

## Goals

- **Single Dependency**: Users only need `sonos-sdk`
- **Unified API**: Speakers and Groups have identical interface pattern
- **Reactive Ready**: Built on sonos-state's reactive foundation
- **Type Safe**: Leverages existing type-safe operations
- **Real-time**: Automatic updates via event streaming

## Target API

```rust
// Simple initialization
let system = sonos::new().await?;

// Speaker management
let speakers = system.get_speakers();
let speaker = system.get_speaker_by_name("Living Room")?;

// Group management - same API patterns as speakers
let groups = system.get_groups();
let group = system.get_group_by_name("Kitchen + Dining")?;

// Identical API for speakers and groups
speaker.play().await?;
group.play().await?;    // Routed to coordinator

let speaker_volume = speaker.get_volume(); // From sonos-state
let group_volume = group.get_volume();     // From sonos-state (GroupRenderingControl service not yet available)

// Group management
speaker.join_group(&group).await?;
speaker.leave_group().await?;
```

## Architecture Decisions

### Core Components
- **SonosSystem**: Main entry point, manages StateManager and device registry
- **Speaker**: Individual device abstraction
- **Group**: Speaker group abstraction
- **Operation Router**: Routes group operations to coordinators (lives in sonos-sdk)

### Key Integrations
- **sonos-state**: Reactive state management (current values + change watching)
- **sonos-api**: Stateless SOAP operations (play, pause, volume, etc.)
- **sonos-discovery**: Automatic device discovery
- **sonos-stream**: Event streaming for real-time updates

### Design Patterns
- **State-First**: Use `store.get::<Property>(&id)` for all queries
- **Stateless Operations**: Use sonos-api for all actions
- **Coordinator Routing**: Group operations → coordinator IP resolution → sonos-api

---

## Implementation Phases

### Phase 1: Foundation (Session 1)
**Goal**: Create basic sonos-sdk crate structure with core types

**Tasks:**
- [ ] **Task 1.1**: Create sonos-sdk crate
  - Add to workspace Cargo.toml
  - Create sonos-sdk/Cargo.toml with dependencies
  - Create basic lib.rs with module structure
  - **Dependencies**: sonos-state, sonos-api, sonos-discovery, sonos-stream, tokio
  - **Deliverable**: Compilable crate with stub types

- [ ] **Task 1.2**: Define core traits and types
  - Define `SonosPlayable` trait for common operations
  - Create `SonosSystem`, `Speaker`, `Group` structs (stub implementations)
  - Create error types and Result aliases
  - **Deliverable**: Type definitions compile, basic API surface defined

### Phase 2: Basic System and Discovery (Session 2)
**Goal**: Initialize system and discover devices

**Tasks:**
- [ ] **Task 2.1**: Implement SonosSystem initialization
  - `sonos::new()` function
  - Initialize StateManager integration
  - Set up device registry (HashMap<String, Speaker>)
  - **Deliverable**: `let system = sonos::new().await?` works

- [ ] **Task 2.2**: Integrate device discovery
  - Use sonos-discovery to find devices
  - Create Speaker instances from discovered devices
  - Implement `get_speakers()`, `get_speaker_by_name()`, `get_speaker_by_id()`
  - **Deliverable**: Can discover and retrieve speaker instances

### Phase 3: Speaker Operations (Session 3)
**Goal**: Basic speaker playback and volume control

**Tasks:**
- [ ] **Task 3.1**: Implement SonosPlayable for Speaker
  - `play()`, `pause()`, `stop()` using sonos-api operations
  - Direct IP routing (no coordinator logic yet)
  - **Deliverable**: Basic playback control works for individual speakers

- [ ] **Task 3.2**: Implement speaker volume operations
  - `set_volume()`, `get_volume()` methods
  - `get_volume()` should use sonos-state for current value
  - `set_volume()` should use sonos-api operations
  - **Deliverable**: Volume control works for individual speakers

### Phase 4: State Integration (Session 4)
**Goal**: Integrate sonos-state for reactive queries

**Tasks:**
- [ ] **Task 4.1**: Speaker state queries
  - `is_playing()` using sonos-state PlaybackState
  - `get_current_track()` using sonos-state CurrentTrack
  - Ensure StateManager processes events properly
  - **Deliverable**: State queries return current values from sonos-state

- [ ] **Task 4.2**: Event streaming integration
  - Integrate sonos-stream to feed events to StateManager
  - Set up proper event subscription for discovered devices
  - Test that state updates in real-time
  - **Deliverable**: State automatically updates when devices change

### Phase 5: Group Discovery and State (Session 5)
**Goal**: Understand and expose speaker groups

**Tasks:**
- [ ] **Task 5.1**: Group discovery from topology
  - Use sonos-state Topology to identify groups
  - Create Group instances with member speakers
  - Implement `get_groups()`, `get_group_by_name()`
  - **Deliverable**: Can discover and list speaker groups

- [ ] **Task 5.2**: Group state queries
  - Implement group volume using `store.get_group::<Volume>()`
  - Group playback state and current track
  - **Deliverable**: Group state queries work using sonos-state

### Phase 6: Group Operations Router (Session 6)
**Goal**: Route group operations to coordinators

**Tasks:**
- [ ] **Task 6.1**: Coordinator resolution
  - Create operation router component
  - Use Topology to find group coordinators
  - Map group operations to coordinator IPs
  - **Deliverable**: Can identify coordinator for any group

- [ ] **Task 6.2**: Implement SonosPlayable for Group
  - `play()`, `pause()`, `stop()` routed to coordinator
  - `set_volume()` for entire group
  - **Deliverable**: Group playback and volume control works

### Phase 7: Group Management (Session 7)
**Goal**: Create, modify, and dissolve groups

**Tasks:**
- [ ] **Task 7.1**: Basic group operations
  - `speaker.join_group(&group)`
  - `speaker.leave_group()`
  - Handle topology updates after group changes
  - **Deliverable**: Can modify group membership

- [ ] **Task 7.2**: Group creation
  - `system.create_group(&[speaker1, speaker2])`
  - Handle coordinator selection
  - **Deliverable**: Can create new groups programmatically

### Phase 8: Error Handling and Polish (Session 8)
**Goal**: Production-ready error handling and user experience

**Tasks:**
- [ ] **Task 8.1**: Comprehensive error handling
  - Network error handling with retries
  - Graceful fallbacks for state queries
  - User-friendly error messages
  - **Deliverable**: Robust error handling throughout

- [ ] **Task 8.2**: Documentation and examples
  - Comprehensive API documentation
  - Basic usage examples
  - Advanced group management examples
  - **Deliverable**: Well-documented public API

### Phase 9: Future Reactive Properties (Future Session)
**Goal**: Enable reactive property watching (future enhancement)

**Tasks:**
- [ ] **Task 9.1**: Property watching API
  - `speaker.watch_volume()` returning Stream/Receiver
  - `group.watch_playback_state()` for group changes
  - **Deliverable**: Reactive property updates for applications

---

## Success Criteria

### Phase 1-2 Complete
- [ ] Can initialize system and discover speakers
- [ ] Basic type structure in place

### Phase 3-4 Complete
- [ ] Individual speaker control works (play, pause, volume)
- [ ] State queries return live data from sonos-state

### Phase 5-6 Complete
- [ ] Group discovery and control works
- [ ] Group operations properly routed to coordinators

### Phase 7-8 Complete
- [ ] Full group management capabilities
- [ ] Production-ready error handling and docs

## Technical Notes

### Dependencies Management
Each phase should be careful about:
- Version compatibility across workspace crates
- Proper async/await patterns throughout
- Error propagation and conversion

### Testing Strategy
- Unit tests for each component
- Integration tests using CLI example pattern
- Mock tests for network operations

### Performance Considerations
- Leverage singleton SoapClient for efficiency
- Cache coordinator lookups appropriately
- Minimize StateManager queries

---

## Getting Started

1. Start with **Phase 1** to establish foundation
2. Each phase builds incrementally on previous phases
3. Each task should be completable in one focused session
4. Test thoroughly before moving to next phase

This plan ensures the sonos-sdk becomes the clean, powerful interface users need while leveraging all existing robust infrastructure.