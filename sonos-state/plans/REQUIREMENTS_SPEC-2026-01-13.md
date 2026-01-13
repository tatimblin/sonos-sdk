# Requirements Specification: Sonos SDK TUI Example

## Executive Summary

This project demonstrates a Terminal User Interface (TUI) application built with `ratatui` that showcases the reactive state management capabilities of the `sonos-sdk`. The TUI follows a **React Redux architecture pattern** where state is held at the top level, but property queries occur at the lowest function level possible, demonstrating efficient and scalable state access patterns.

## Project Goals

The primary goal is to create a **pedagogical example** that demonstrates:

1. **Redux-style State Architecture**: Hold state manager reference at top level, query properties where needed
2. **Reactive UI Updates**: Automatically re-render when watched properties change
3. **Efficient Property Watching**: Only watch properties that are actively displayed
4. **Clean Separation of Concerns**: State management separate from UI rendering logic

This is NOT a full-featured Sonos controller, but rather a focused example of architectural patterns.

## Target Users

- Developers learning to integrate `sonos-sdk` into applications
- Engineers studying reactive state management patterns
- Contributors understanding the SDK's design philosophy

## Core Requirements

### FR-1: System Initialization

**Description**: Initialize the Sonos system and discover all available speakers on the network.

**Acceptance Criteria**:
- Application initializes `SonosSystem` using `SonosSystem::new().await`
- Performs automatic device discovery on startup
- Holds `SonosSystem` reference at the application's top level
- Handles discovery errors gracefully

**State Management Pattern**:
```rust
struct App {
    system: SonosSystem,  // Held at top level
    selected_index: usize,
    // ... UI state
}
```

### FR-2: Speaker List Display

**Description**: Display all discovered speakers in a vertical list format.

**Acceptance Criteria**:
- Show all speakers returned by `system.speakers().await`
- Display each speaker's **name** (from `speaker.name` metadata)
- List is rendered in a scrollable view
- Current selection is visually highlighted

**State Management Pattern**:
- Speaker list retrieval happens at the list component level
- Only speaker **names** (metadata) are queried initially
- No property watching occurs for non-selected speakers

**Visual Example**:
```
┌─ Sonos Speakers ─────────────┐
│ > Living Room                │
│   Kitchen                    │
│   Bedroom                    │
│   Office                     │
└──────────────────────────────┘
```

### FR-3: Navigation

**Description**: Allow users to navigate through the speaker list using keyboard controls.

**Acceptance Criteria**:
- **Up Arrow (↑)**: Move selection to previous speaker (wrap to bottom if at top)
- **Down Arrow (↓)**: Move selection to next speaker (wrap to top if at bottom)
- **Escape/Q**: Exit application
- Selection state is maintained in application state (not in state manager)

### FR-4: Expanded View with Volume Display

**Description**: Show additional details for the currently selected speaker, specifically the current volume level.

**Acceptance Criteria**:
- When a speaker is selected, display its current volume
- Volume is retrieved using `speaker.volume.get()`
- Volume is displayed as a numeric value (0-100)
- Expanded view is rendered adjacent to or below the speaker name

**State Management Pattern** (Critical - React Redux Style):
- Volume receiver is stored in `App` state (not called on every render)
- `watch()` is called once when speaker is selected (in selection handler)
- Render function reads from the stored receiver using `.borrow()`
- When selection changes, old receiver is dropped (streaming stops automatically)
- `system.iter()` triggers redraws when watched properties change

**Lifecycle**:
1. **Selection changes** → Store `speaker.volume.watch().await` in App state
2. **Property changes** → `system.iter()` detects change → Trigger redraw
3. **Render** → Read current value via `volume_rx.borrow()`
4. **Selection changes** → Old receiver dropped → Streaming stops

- Example:
  ```rust
  struct App {
      system: SonosSystem,
      speakers: Vec<Speaker>,
      selected_index: usize,
      volume_rx: Option<Receiver<Option<Volume>>>, // Stored here!
  }
  
  impl App {
      async fn select_speaker(&mut self, index: usize) {
          self.selected_index = index;
          
          // Drop old receiver (streaming stops)
          self.volume_rx = None;
          
          // Watch new speaker (streaming starts)
          let speaker = &self.speakers[index];
          self.volume_rx = Some(speaker.volume.watch().await.unwrap());
      }
  }
  
  fn render_speaker_item(
      speaker: &Speaker, 
      volume_rx: &Option<Receiver<Option<Volume>>>
  ) -> Widget {
      // Just read current value - no async, no watch setup
      let volume = volume_rx
          .as_ref()
          .and_then(|rx| rx.borrow().clone())
          .map(|v| v.value().to_string())
          .unwrap_or_else(|| "N/A".to_string());
      
      // Render volume...
  }
  ```

**Visual Example**:
```
┌─ Sonos Speakers ─────────────┐
│ > Living Room                │
│   └─ Volume: 45              │
│   Kitchen                    │
│   Bedroom                    │
└──────────────────────────────┘
```

### FR-5: Reactive Volume Updates

**Description**: Automatically update the displayed volume when it changes on the device.

**Acceptance Criteria**:
- Application calls `speaker.volume.watch()` for the **currently selected speaker only**
- `watch()` immediately returns a receiver with the current value (via `.borrow()`)
- `watch()` also sets up streaming for future updates automatically
- When volume changes (via UPnP events or manual updates), the TUI re-renders
- Volume updates are reflected in real-time without user interaction
- When selection changes:
  - Stop watching the previously selected speaker's volume (drop the receiver)
  - Start watching the newly selected speaker's volume

**Key Pattern - `watch()` is Interchangeable with `get()`**:
- `get()` returns `Option<Volume>` - current value only
- `watch()` returns `Receiver<Option<Volume>>` - current value (via `.borrow()`) + streaming
- Using `watch()` everywhere simplifies the API - one call for both needs
- Behind the scenes, `watch()` activates UPnP event subscriptions automatically

**State Management Pattern**:
- Watch setup occurs when speaker is selected (in render function)
- Watch cleanup occurs automatically when receiver is dropped (selection changes)
- Application responds to watch channel notifications to trigger re-render
- Demonstrates efficient resource usage (only one volume watcher active at a time)

**Reactive Flow with `system.iter()`**:
```
1. User selects "Living Room"
2. App stores living_room.volume.watch().await in App.volume_rx
3. Receiver is alive → UPnP subscription active
4. Volume changes on device (e.g., via Sonos app)
5. system.iter() yields event → Main loop triggers redraw
6. Render reads rx.borrow() → New value displayed
7. User selects "Kitchen"
8. App.volume_rx = None → Living Room receiver dropped
9. App stores kitchen.volume.watch().await → New subscription
```

**No manual task spawning needed** - `system.iter()` handles the event detection!

### FR-6: Empty State Handling

**Description**: Display appropriate message when no speakers are discovered.

**Acceptance Criteria**:
- If `system.speakers().await` returns empty list, show message
- Message: "No Sonos speakers found on network"
- Application remains responsive (can exit with Escape/Q)
- Does not crash or panic on empty speaker list

**Visual Example**:
```
┌─ Sonos Speakers ─────────────┐
│                              │
│  No Sonos speakers found     │
│  on network                  │
│                              │
└──────────────────────────────┘
```

### FR-7: Debounced Watch Cleanup (Scalability Pattern)

**Description**: Implement a debounced cleanup mechanism for property watches to support scaling to dozens of properties without manual management.

**Problem Statement**: 
- With dozens of properties (volume, mute, bass, treble, playback_state, position, etc.), storing individual receivers at the top level becomes unwieldy
- Immediately dropping watches on deselection causes latency when re-selecting the same speaker
- Manual cleanup of many watches defeats the purpose of a state management system

**Solution**: 
Watch cache with debounced cleanup - watches persist for a configurable timeframe after deselection, then automatically clean up.

**Acceptance Criteria**:
- When a speaker is deselected, watches are marked for cleanup but NOT immediately dropped
- Cleanup timer starts (default: 5 seconds configurable)
- If speaker is re-selected within timeout, existing watch is reused (no latency)
- If timeout expires, watch is automatically cleaned up
- Supports arbitrary number of properties without code changes
- Works transparently with `system.iter()` event loop

**Architecture Pattern**:
```rust
struct App {
    system: SonosSystem,
    speakers: Vec<Speaker>,
    selected_index: usize,
    watch_cache: WatchCache, // Centralized watch management
}

struct WatchCache {
    // Key: (SpeakerId, PropertyKey)
    watches: HashMap<(SpeakerId, &'static str), CachedWatch>,
    cleanup_timeout: Duration, // Default: 5 seconds
}

struct CachedWatch {
    receiver: Box<dyn Any>, // Receiver<Option<P>>
    cleanup_timer: Option<tokio::task::JoinHandle<()>>,
    last_accessed: Instant,
}

impl WatchCache {
    /// Get or create a watch for a property
    async fn get_or_watch<P: Property>(
        &mut self,
        speaker: &Speaker
    ) -> Result<&Receiver<Option<P>>, Error> {
        let key = (speaker.id.clone(), P::KEY);
        
        if let Some(cached) = self.watches.get_mut(&key) {
            // Cancel cleanup timer - still in use
            if let Some(timer) = cached.cleanup_timer.take() {
                timer.abort();
            }
            cached.last_accessed = Instant::now();
            
            return Ok(cached.receiver
                .downcast_ref::<Receiver<Option<P>>>()
                .unwrap());
        }
        
        // Create new watch
        let rx = speaker.volume.watch().await?; // Or generic property handle
        self.watches.insert(key, CachedWatch {
            receiver: Box::new(rx),
            cleanup_timer: None,
            last_accessed: Instant::now(),
        });
        
        Ok(self.watches[&key].receiver
            .downcast_ref::<Receiver<Option<P>>>()
            .unwrap())
    }
    
    /// Mark watches for a speaker as eligible for cleanup
    fn schedule_cleanup(&mut self, speaker_id: &SpeakerId) {
        let timeout = self.cleanup_timeout;
        
        for ((id, prop_key), cached) in self.watches.iter_mut() {
            if id == speaker_id && cached.cleanup_timer.is_none() {
                let key = (id.clone(), *prop_key);
                let watches_ref = /* shared reference to self.watches */;
                
                cached.cleanup_timer = Some(tokio::spawn(async move {
                    tokio::time::sleep(timeout).await;
                    watches_ref.remove(&key);
                }));
            }
        }
    }
}
```

**Usage in App**:
```rust
impl App {
    async fn select_speaker(&mut self, index: usize) {
        let old_speaker_id = self.speakers[self.selected_index].id.clone();
        self.selected_index = index;
        
        // Schedule cleanup for old speaker's watches (debounced)
        self.watch_cache.schedule_cleanup(&old_speaker_id);
        
        // Get or reuse watch for new speaker
        let speaker = &self.speakers[index];
        let volume_rx = self.watch_cache
            .get_or_watch::<Volume>(speaker)
            .await
            .unwrap();
        
        // No latency if we recently viewed this speaker!
    }
    
    fn render(&self) {
        let speaker = &self.speakers[self.selected_index];
        
        // Access cached watch
        if let Ok(rx) = self.watch_cache.get_watch::<Volume>(&speaker.id) {
            let volume = rx.borrow().clone();
            // Render...
        }
    }
}
```

**Benefits**:
1. **Scales to dozens of properties** - Single cache manages all watches
2. **No manual cleanup** - Automatic timeout-based cleanup
3. **Performance** - Reuses watches for recently viewed speakers
4. **User Experience** - No latency when navigating back to recent speakers
5. **Memory Efficient** - Only active/recent watches are kept
6. **Transparent** - Works with existing `system.iter()` event loop

**Configuration**:
- Default cleanup timeout: 5 seconds (configurable)
- Maximum cache size: Unlimited (or configurable limit)
- Per-property or global timeout

**Edge Cases Handled**:
- Rapid navigation between speakers (timers canceled, watches reused)
- App idle with speaker selected (watch stays alive)
- Exiting app (all watches dropped via `App` drop)
- Memory concerns (optional max cache size with LRU eviction)

## Non-Functional Requirements

### NFR-1: Performance

- UI must remain responsive (<100ms for navigation actions)
- Property queries (`get()`) must be non-blocking
- Watch updates should trigger re-render within 200ms of change

### NFR-2: Code Quality

- Code should clearly demonstrate the Redux-style pattern
- Comments should explain **why** queries happen at specific levels
- Example should be simple enough to understand in <15 minutes

### NFR-3: Dependencies

- **Required**: `ratatui` (latest stable version)
- **Required**: `tokio` (async runtime)
- **Required**: `crossterm` (terminal backend for ratatui)
- **Required**: Your existing `sonos-sdk` crates

### NFR-4: Error Handling

- Gracefully handle initialization failures (log and exit)
- Handle missing/unavailable properties (show "N/A" or similar)
- No panics during normal operation

## Technical Architecture

### Component Hierarchy

```
App (holds SonosSystem)
 └─ render()
     └─ SpeakerListWidget
         └─ for each speaker:
             SpeakerItemWidget
              └─ if selected: query volume.get()
```

### State Management Layers

| Layer | Responsibility | Example |
|-------|---------------|---------|
| **App State** | UI state, selection index | `selected_index: usize` |
| **SonosSystem** | Device discovery, speaker handles | `system: SonosSystem` |
| **Speaker Handle** | Property handles | `speaker.volume`, `speaker.playback_state` |
| **Property Handle** | Get/watch specific property | `volume.get()`, `volume.watch()` |

### Data Flow (React Redux Pattern)

1. **Top Level**: `App` holds `SonosSystem` reference
2. **List Level**: Query `system.speakers()` when rendering list
3. **Item Level**: Query `speaker.name` for all items
4. **Expanded Item Level**: Query `speaker.volume.get()` ONLY for selected item
5. **Reactive Level**: Watch `speaker.volume.watch()` ONLY for selected item

This mirrors Redux's pattern:
- Store at top (SonosSystem)
- Selectors at component level (`.get()` calls)
- Subscriptions for reactive updates (`.watch()` calls)

## User Stories

### US-1: As a developer, I want to see all speakers on my network
**Given** I have Sonos speakers on my network  
**When** I launch the TUI application  
**Then** I see a list of all discovered speakers by name  

### US-2: As a developer, I want to view speaker volume
**Given** The speaker list is displayed  
**When** I navigate to a speaker (currently selected)  
**Then** I see the current volume level for that speaker  

### US-3: As a developer, I want to see live volume updates
**Given** A speaker is selected and displaying volume  
**When** The volume changes on the device (via another app/controller)  
**Then** The TUI automatically updates to show the new volume  
**And** I did not need to press any keys to refresh  

### US-4: As a developer, I want to navigate efficiently
**Given** Multiple speakers are displayed  
**When** I press the down arrow key  
**Then** Selection moves to the next speaker  
**And** Volume watching switches to the new speaker  

### US-5: As a developer studying the code, I want to understand the pattern
**Given** I open the source code  
**When** I read through the rendering logic  
**Then** I can clearly see where state queries occur  
**And** I understand why properties are queried at specific levels  

## Scope Boundaries

### In Scope
- Display speaker names
- Display volume for selected speaker
- Reactive volume updates via `watch()`
- Keyboard navigation (up/down/quit)
- Empty state handling
- Single-select model (one speaker selected at a time)

### Out of Scope (Future Enhancements)
- Volume control (adjusting volume)
- Playback control (play/pause/skip)
- Multiple property display (mute, bass, treble, current track)
- Multi-speaker operations
- Group/zone management
- Search/filter functionality
- Mouse input
- Configuration file
- Logging UI

## Success Metrics

This example is successful if:

1. **Educational Value**: Developers can understand the Redux pattern from the code
2. **Functionality**: All speakers are discovered and volumes update reactively
3. **Code Clarity**: Architecture pattern is obvious from code structure
4. **Performance**: UI remains responsive with 10+ speakers
5. **Reusability**: Pattern can be applied to other properties (playback_state, mute, etc.)

## Open Questions

None - requirements are complete based on discussion.

## Appendix: Key Code Patterns

### Pattern 1: Top-Level State Holding (with Receiver Storage)
```rust
struct App {
    system: SonosSystem,       // Holds state manager
    speakers: Vec<Speaker>,     // Cached speaker list
    selected_index: usize,      // UI state
    volume_rx: Option<Receiver<Option<Volume>>>, // Stored receiver!
}
```

### Pattern 2: Watch on Selection Change (Not on Every Render)
```rust
impl App {
    async fn select_speaker(&mut self, index: usize) {
        self.selected_index = index;
        
        // Drop old receiver → stops streaming previous speaker
        self.volume_rx = None;
        
        // Watch new speaker → starts streaming
        let speaker = &self.speakers[index];
        self.volume_rx = Some(speaker.volume.watch().await.unwrap());
    }
}
```

### Pattern 3: Render Reads from Stored Receiver (Synchronous)
```rust
fn render_speaker_item(
    speaker: &Speaker, 
    volume_rx: &Option<Receiver<Option<Volume>>>
) -> Widget {
    // Just read current value - no async, no setup
    let volume_text = volume_rx
        .as_ref()
        .and_then(|rx| rx.borrow().clone())  // Get current value
        .map(|v| format!("Volume: {}", v.value()))
        .unwrap_or_else(|| "Volume: N/A".to_string());
    
    // Render using ratatui widgets...
}
```

### Pattern 4: Event Loop with system.iter()
```rust
#[tokio::main]
async fn main() {
    let mut app = App::new().await;
    
    // Main event loop
    for event in app.system.iter() {
        match event {
            SystemEvent::PropertyChanged => {
                // Redraw TUI - will read new value via rx.borrow()
                app.render()?;
            }
            SystemEvent::KeyPress(key) => {
                match key {
                    Key::Down => app.select_next().await,
                    Key::Up => app.select_prev().await,
                    Key::Escape => break,
                    _ => {}
                }
            }
        }
    }
}
```

**Key Insights:**
1. ✅ **Receiver stored in App** → Keeps streaming alive
2. ✅ **watch() called once per selection** → Not on every render
3. ✅ **Render just reads .borrow()** → No async in render function
4. ✅ **system.iter() triggers redraws** → No manual task spawning
5. ✅ **Drop = cleanup** → Rust's ownership handles unsubscribe

### Pattern 5: WatchCache for Scalable Property Management (Recommended)
```rust
use std::collections::HashMap;
use std::time::{Duration, Instant};

struct App {
    system: SonosSystem,
    speakers: Vec<Speaker>,
    selected_index: usize,
    watch_cache: WatchCache, // Manages all property watches
}

struct WatchCache {
    watches: HashMap<(SpeakerId, &'static str), CachedWatch>,
    cleanup_timeout: Duration,
}

impl WatchCache {
    fn new(cleanup_timeout: Duration) -> Self {
        Self {
            watches: HashMap::new(),
            cleanup_timeout,
        }
    }
    
    /// Get existing watch or create new one
    async fn get_or_watch<P: Property>(
        &mut self,
        speaker: &Speaker,
    ) -> Result<&Receiver<Option<P>>, Error> {
        let key = (speaker.id.clone(), P::KEY);
        
        // Check if already watching
        if let Some(cached) = self.watches.get_mut(&key) {
            // Cancel pending cleanup - still needed
            if let Some(timer) = cached.cleanup_timer.take() {
                timer.abort();
            }
            cached.last_accessed = Instant::now();
            
            return Ok(cached.receiver
                .downcast_ref::<Receiver<Option<P>>>()
                .unwrap());
        }
        
        // Create new watch
        let rx = speaker.property_handle::<P>().watch().await?;
        self.watches.insert(key.clone(), CachedWatch {
            receiver: Box::new(rx),
            cleanup_timer: None,
            last_accessed: Instant::now(),
        });
        
        Ok(self.watches[&key].receiver
            .downcast_ref::<Receiver<Option<P>>>()
            .unwrap())
    }
    
    /// Schedule cleanup for all watches of a speaker (debounced)
    fn schedule_cleanup(&mut self, speaker_id: &SpeakerId) {
        let timeout = self.cleanup_timeout;
        
        for ((id, _), cached) in self.watches.iter_mut() {
            if id == speaker_id && cached.cleanup_timer.is_none() {
                // Start cleanup timer
                let key = (id.clone(), /* prop_key */);
                
                cached.cleanup_timer = Some(tokio::spawn(async move {
                    tokio::time::sleep(timeout).await;
                    // Remove from cache (actual impl needs Arc<Mutex<>>)
                }));
            }
        }
    }
}

// Usage in App
impl App {
    async fn select_speaker(&mut self, index: usize) {
        let old_speaker = &self.speakers[self.selected_index];
        self.selected_index = index;
        
        // Schedule cleanup for old speaker (debounced)
        self.watch_cache.schedule_cleanup(&old_speaker.id);
        
        // Get or reuse watch for new speaker
        let new_speaker = &self.speakers[index];
        let _volume_rx = self.watch_cache
            .get_or_watch::<Volume>(new_speaker)
            .await
            .unwrap();
    }
    
    fn render(&self) {
        let speaker = &self.speakers[self.selected_index];
        
        // Access from cache - instant if recently viewed!
        if let Ok(rx) = self.watch_cache.get::<Volume>(&speaker.id) {
            let volume = rx.borrow();
            // Render volume...
        }
    }
}
```

**Why This Pattern?**
- ✅ Scales to **dozens of properties** without code changes
- ✅ **Zero latency** when returning to recently viewed speakers
- ✅ **Automatic cleanup** - no manual management needed
- ✅ **Memory efficient** - watches timeout after 5 seconds of inactivity
- ✅ **User-friendly** - fast navigation, smooth experience

## Revision History

| Date | Version | Changes |
|------|---------|---------|
| 2026-01-12 | 1.0 | Initial requirements specification |
