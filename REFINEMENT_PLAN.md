# Reactive Architecture Refinement Plan

## 🎯 Executive Summary

**Problem**: The current reactive architecture is massively over-engineered with multiple EventConsumers, complex fan-out logic, and disconnected channels.

**Solution**: Simplify to use ONE EventConsumer reading the multiplexed stream from EventBroker, with PropertyWatchers watching StateStore channels.

**Impact**: ~90% reduction in complexity, eliminates event distribution bugs, matches EventBroker's actual design.

---

## 🚨 Current Architecture Problems

### Problem 1: Multiple EventConsumers Per Service
```rust
// Current: Creates 2 EventConsumers for same RenderingControl service!
let volume_watcher = state_manager.watch_property::<Volume>(speaker_id).await?;
let mute_watcher = state_manager.watch_property::<Mute>(speaker_id).await?;
```

### Problem 2: Disconnected Channels
```rust
// In manager.rs:235 - These channels receive no events!
let (_event_sender, event_receiver) = mpsc::unbounded_channel();
```

### Problem 3: Misaligned with EventBroker Design
EventBroker provides **ONE multiplexed stream** but we're trying to create individual streams.

---

## ✅ Target Architecture

### Current (Broken):
```
EventBroker
    ↓
Multiple EventConsumers (N×M per speaker×service)
    ↓
Complex fan-out distribution
    ↓
PropertyWatchers (receive raw events)
```

### Target (Simple):
```
EventBroker
    ↓
EventManager (reference counting facade)
    ↓
ONE EventConsumer (single multiplexed stream)
    ↓
ONE Event Processor Task
    ↓
StateStore Updates (by speaker_ip + service routing)
    ↓
PropertyWatchers (watch StateStore channels)
```

---

## 📋 Detailed Refinement Steps

### Step 1: Analyze EventBroker's Single Stream Design
**File**: `sonos-stream/src/broker.rs`

**Key Insight**: EventBroker.event_iterator() provides ONE stream for ALL events:
```rust
pub fn event_iterator(&mut self) -> BrokerResult<EventIterator> {
    // Returns single iterator for ALL speakers, ALL services
}
```

Each event is tagged with:
- `event.speaker_ip` - Which speaker
- `event.service` - Which UPnP service
- `event.event_data` - The actual data

### Step 2: Simplify EventManager API
**File**: `sonos-event-manager/src/manager.rs`

**Changes Needed**:

1. **Remove per-service subscribe()** - Replace with single stream access
2. **Add reference counting for services** - Track which services are needed
3. **Simplify to match EventBroker pattern**

```rust
impl SonosEventManager {
    // OLD: Multiple subscribe() calls
    // pub async fn subscribe(&self, device_ip: IpAddr, service: Service) -> Result<EventConsumer>

    // NEW: Single event stream with service tracking
    pub async fn get_event_stream(&self) -> Result<EventIterator> {
        // Return the EventBroker's single multiplexed iterator
    }

    pub async fn ensure_service_subscribed(&self, device_ip: IpAddr, service: Service) -> Result<()> {
        // Reference counting: register device+service with EventBroker if first time
    }

    pub async fn release_service_subscription(&self, device_ip: IpAddr, service: Service) -> Result<()> {
        // Reference counting: unregister from EventBroker if last reference
    }
}
```

### Step 3: Redesign ReactiveStateManager
**File**: `sonos-state/src/reactive.rs`

**Complete Rewrite Strategy**:

```rust
pub struct ReactiveStateManager {
    state_manager: StateManager,
    event_manager: Arc<SonosEventManager>,
    // Track which services are subscribed per speaker
    service_refs: Arc<DashMap<(SpeakerId, Service), AtomicUsize>>,
    // Single event processor handle
    _event_processor: JoinHandle<()>,
}

impl ReactiveStateManager {
    pub async fn new() -> Result<Self> {
        let state_manager = StateManager::new();
        let event_manager = Arc::new(SonosEventManager::new().await?);
        let service_refs = Arc::new(DashMap::new());

        // Create ONE event processor for ALL events
        let event_processor = Self::start_event_processor(
            Arc::clone(&event_manager),
            state_manager.clone(), // Needs StateManager API adjustment
            Arc::clone(&service_refs),
        ).await?;

        Ok(Self {
            state_manager,
            event_manager,
            service_refs,
            _event_processor: event_processor,
        })
    }

    async fn start_event_processor(
        event_manager: Arc<SonosEventManager>,
        state_manager: StateManager,
        service_refs: Arc<DashMap<(SpeakerId, Service), AtomicUsize>>,
    ) -> Result<JoinHandle<()>> {
        // Get the single multiplexed event stream
        let mut events = event_manager.get_event_stream().await?;

        let handle = tokio::spawn(async move {
            while let Some(enriched_event) = events.next_async().await {
                if let Some(raw_event) = convert_event(&enriched_event) {
                    // Process event and update StateStore
                    let update_count = state_manager.process(raw_event);

                    if update_count > 0 {
                        tracing::debug!(
                            "Updated {} properties for speaker {:?} service {:?}",
                            update_count,
                            enriched_event.speaker_ip,
                            enriched_event.service
                        );
                    }
                }
            }
        });

        Ok(handle)
    }

    pub async fn watch_property<P: Property>(&self, speaker_id: SpeakerId) -> Result<PropertyWatcher<P>> {
        // Ensure this service is subscribed
        self.ensure_service_subscription(speaker_id.clone(), P::SERVICE).await?;

        // Create watcher that monitors StateStore
        Ok(PropertyWatcher::from_state_store(
            self.state_manager.store().watch::<P>(&speaker_id),
            speaker_id,
            P::SERVICE,
            Arc::clone(&self.service_refs),
        ))
    }

    async fn ensure_service_subscription(&self, speaker_id: SpeakerId, service: Service) -> Result<()> {
        let key = (speaker_id.clone(), service);

        // Get speaker IP
        let speaker_ip = self.state_manager.store()
            .get_speaker_ip(&speaker_id)
            .ok_or_else(|| StateError::SpeakerNotFound(speaker_id.clone()))?;

        // Atomic increment reference count
        let old_count = if let Some(counter) = self.service_refs.get(&key) {
            counter.fetch_add(1, Ordering::SeqCst)
        } else {
            self.service_refs.insert(key.clone(), AtomicUsize::new(1));
            0
        };

        // If first reference, subscribe with EventManager
        if old_count == 0 {
            self.event_manager.ensure_service_subscribed(speaker_ip, service).await?;
            tracing::debug!("Subscribed to {:?} for speaker {:?}", service, speaker_id);
        }

        Ok(())
    }
}
```

### Step 4: Simplify PropertyWatcher
**File**: `sonos-state/src/reactive.rs`

**New PropertyWatcher Design**:

```rust
pub struct PropertyWatcher<P: Property> {
    // Watch StateStore channel directly
    state_watcher: watch::Receiver<Option<P>>,
    speaker_id: SpeakerId,
    service: Service,
    service_refs: Arc<DashMap<(SpeakerId, Service), AtomicUsize>>,
    _phantom: PhantomData<P>,
}

impl<P: Property> PropertyWatcher<P> {
    fn from_state_store(
        state_watcher: watch::Receiver<Option<P>>,
        speaker_id: SpeakerId,
        service: Service,
        service_refs: Arc<DashMap<(SpeakerId, Service), AtomicUsize>>,
    ) -> Self {
        Self {
            state_watcher,
            speaker_id,
            service,
            service_refs,
            _phantom: PhantomData,
        }
    }

    pub fn current(&self) -> Option<P> {
        self.state_watcher.borrow().clone()
    }

    pub async fn changed(&mut self) -> Result<()> {
        self.state_watcher.changed().await
            .map_err(|_| StateError::ChannelClosed)
    }

    pub async fn next(&mut self) -> Option<P> {
        if self.changed().await.is_ok() {
            self.current()
        } else {
            None
        }
    }
}

impl<P: Property> Drop for PropertyWatcher<P> {
    fn drop(&mut self) {
        // Decrement reference count
        let key = (self.speaker_id.clone(), self.service);

        if let Some(counter) = self.service_refs.get(&key) {
            let old_count = counter.fetch_sub(1, Ordering::SeqCst);
            let new_count = old_count.saturating_sub(1);

            if new_count == 0 {
                // Last reference - remove from map and unsubscribe
                self.service_refs.remove(&key);

                // Note: In practice, you'd need a channel to signal the ReactiveStateManager
                // to unsubscribe from the EventManager. This is a design detail to work out.
                tracing::debug!("Last PropertyWatcher dropped for {:?} {:?}",
                               self.speaker_id, self.service);
            }
        }
    }
}
```

### Step 5: Remove Unnecessary Components
**Files to Simplify/Remove**:

1. **`sonos-event-manager/src/consumer.rs`** - No longer needed
2. **`sonos-event-manager/src/subscription.rs`** - Simplify to just reference counting
3. **`PropertySubscriptionManager`** - Replace with simple service tracking

### Step 6: Update StateManager Integration
**File**: `sonos-state/src/state_manager.rs`

**Required Changes**:
- Ensure StateManager is Clone or has shared access
- Add method to get speaker IP from SpeakerId
- Ensure thread-safe access to StateStore

---

## 🧪 Testing Strategy

### Phase 1: Unit Tests
- Test service reference counting
- Test PropertyWatcher creation/cleanup
- Test event routing by speaker_ip + service

### Phase 2: Integration Tests
- Test with mock EventBroker
- Verify single event processor handles all events
- Verify PropertyWatchers receive updates

### Phase 3: End-to-End Tests
- Test with actual Sonos devices
- Multiple speakers, multiple properties
- Verify subscription lifecycle

---

## 📊 Success Metrics

### Complexity Reduction:
- **Before**: N×M EventConsumers (9 for 3 speakers × 3 services)
- **After**: 1 EventConsumer total

### Code Reduction:
- **Remove**: ~200 lines of fan-out logic
- **Simplify**: PropertyWatcher from ~100 to ~50 lines
- **Eliminate**: Complex channel distribution

### Functionality:
- ✅ Reference counting still works
- ✅ Automatic subscription management
- ✅ PropertyWatchers receive real updates
- ✅ Proper cleanup on drop

---

## 🚀 Implementation Order

1. **Start with EventManager simplification** - Remove multiple EventConsumer pattern
2. **Redesign ReactiveStateManager** - Single event processor
3. **Simplify PropertyWatcher** - Watch StateStore channels
4. **Update examples** - Verify end-to-end functionality
5. **Remove dead code** - Clean up unused fan-out components
6. **Test thoroughly** - Ensure no regressions

---

## 🎯 Key Design Principles

1. **Match EventBroker's Design** - Use the single multiplexed stream as intended
2. **StateStore as Source of Truth** - PropertyWatchers watch StateStore, not raw events
3. **Simple Reference Counting** - Track service subscriptions, not individual consumers
4. **One Event Processor** - Single point of event handling logic
5. **Minimal Complexity** - Eliminate unnecessary abstractions

This refinement transforms the reactive system from a complex, buggy architecture into a simple, correct implementation that matches how the underlying EventBroker actually works.