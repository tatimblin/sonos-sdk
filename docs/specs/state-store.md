# state-store Specification

---

## 1. Purpose & Motivation

### 1.1 Problem Statement

State management for IoT devices and similar applications requires:

1. **Type-safe storage**: Store various property types without runtime type errors
2. **Change detection**: Only notify when values actually change (not on every set)
3. **Watch pattern**: Allow consumers to register interest in specific properties
4. **Synchronous API**: Simple blocking iteration without async complexity

The state-store crate provides these primitives as a generic, reusable foundation that can be used by domain-specific state management layers like `sonos-state`.

### 1.2 Design Goals

| Priority | Goal | Rationale |
|----------|------|-----------|
| P0 | Type-safe property storage | Strongly typed access prevents runtime errors |
| P0 | Change detection | Emit events only when values differ (via PartialEq) |
| P0 | Generic entity IDs | Support any hashable type as identifiers |
| P1 | Blocking iteration | Simple sync API without async runtime requirements |
| P1 | Thread-safe | Safe concurrent access from multiple threads |
| P2 | Minimal dependencies | Zero external dependencies beyond std |

### 1.3 Non-Goals

- **Persistence**: This is an in-memory store only
- **Async API**: Blocking iteration is intentional for simplicity
- **Networking**: No network communication - pure state management
- **Domain-specific types**: Generic primitives only, no Sonos-specific code

### 1.4 Success Criteria

- [x] Type-erased storage with type-safe access
- [x] Change detection via PartialEq comparison
- [x] Watch/unwatch pattern for selective notification
- [x] Blocking, timeout, and non-blocking iteration
- [x] Thread-safe with Arc-based cloning

---

## 2. Architecture

### 2.1 High-Level Design

```
┌─────────────────────────────────────────────────────────────────┐
│                         Public API                               │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │  StateStore<Id>::new()                                    │   │
│  │  store.set(&id, property) / store.get::<P>(&id)          │   │
│  │  store.watch(id, key) / store.unwatch(&id, key)          │   │
│  │  store.iter() -> ChangeIterator                           │   │
│  └──────────────────────────────────────────────────────────┘   │
├─────────────────────────────────────────────────────────────────┤
│                   Internal Components                            │
│  ┌────────────────────┐  ┌─────────────────────────────────┐   │
│  │    PropertyBag     │  │         WatchSet                 │   │
│  │  HashMap<TypeId,   │  │  HashSet<(Id, &'static str)>    │   │
│  │  Box<dyn Any>>     │  │                                  │   │
│  └────────────────────┘  └─────────────────────────────────┘   │
│                                                                  │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │              mpsc::channel<ChangeEvent<Id>>              │   │
│  │         (Sender for store, Receiver for iter)            │   │
│  └─────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────┘
```

### 2.2 Data Flow

```
1. User calls store.watch(entity_id, PropertyKey)
   └── Adds (entity_id, key) to watched set

2. User calls store.set(&entity_id, property)
   ├── PropertyBag.set() compares old vs new value
   ├── If changed AND watched:
   │   └── Send ChangeEvent to mpsc channel
   └── If unchanged:
       └── No event emitted

3. User calls store.iter() (or iter().try_recv(), etc.)
   └── Receives ChangeEvent from mpsc channel
```

### 2.3 Core Types

```rust
// Marker trait for storable properties
pub trait Property: Clone + Send + Sync + PartialEq + 'static {
    const KEY: &'static str;
}

// Type-erased property storage for one entity
pub struct PropertyBag {
    values: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
}

// Main state store generic over entity ID type
pub struct StateStore<Id: Clone + Eq + Hash + Send + Sync + 'static> {
    entities: Arc<RwLock<HashMap<Id, PropertyBag>>>,
    watched: Arc<RwLock<HashSet<(Id, &'static str)>>>,
    event_tx: mpsc::Sender<ChangeEvent<Id>>,
    event_rx: Arc<Mutex<mpsc::Receiver<ChangeEvent<Id>>>>,
}

// Change notification
pub struct ChangeEvent<Id> {
    pub entity_id: Id,
    pub property_key: &'static str,
    pub timestamp: Instant,
}
```

---

## 3. API Reference

### 3.1 Property Trait

```rust
pub trait Property: Clone + Send + Sync + PartialEq + 'static {
    const KEY: &'static str;
}
```

Example implementation:
```rust
#[derive(Clone, PartialEq)]
struct Temperature(f32);

impl Property for Temperature {
    const KEY: &'static str = "temperature";
}
```

### 3.2 StateStore Methods

| Method | Description |
|--------|-------------|
| `new()` | Create empty store |
| `set(&id, value)` | Set property, emit event if changed and watched |
| `get::<P>(&id)` | Get property value, returns `Option<P>` |
| `watch(id, key)` | Register interest in property changes |
| `unwatch(&id, key)` | Unregister interest |
| `is_watched(&id, key)` | Check if property is being watched |
| `iter()` | Get blocking change iterator |
| `entity_count()` | Number of entities |
| `entity_ids()` | List all entity IDs |
| `remove_entity(&id)` | Remove entity and all properties |

### 3.3 ChangeIterator Methods

| Method | Description |
|--------|-------------|
| `recv()` | Block until next event |
| `recv_timeout(duration)` | Block with timeout |
| `try_recv()` | Non-blocking check |
| `try_iter()` | Non-blocking iterator over available events |
| `timeout_iter(duration)` | Blocking iterator with per-event timeout |

---

## 4. Thread Safety

- `StateStore<Id>` is `Clone` - clones share state via `Arc`
- Internal state protected by `RwLock` (entities, watched) and `Mutex` (receiver)
- Safe to call from multiple threads concurrently
- Event channel uses `mpsc` - single receiver semantics

---

## 5. Usage by sonos-state

The `sonos-state` crate builds on `state-store` by:

1. **Extending Property trait**: `SonosProperty` adds `SCOPE` and `SERVICE` for UPnP integration
2. **Adding Sonos types**: `SpeakerId`, `GroupId`, `SpeakerInfo`, etc.
3. **Event decoding**: Converting UPnP events to typed property changes
4. **Service integration**: Connection to `sonos-event-manager` for live updates

```rust
// sonos-state extends the base Property trait
pub trait SonosProperty: state_store::Property {
    const SCOPE: Scope;       // Speaker, Group, or System
    const SERVICE: Service;   // UPnP service for subscriptions
}
```

---

## 6. Testing Strategy

### Unit Tests (22 tests)
- `PropertyBag`: set/get, change detection, multiple types
- `StateStore`: basic operations, watch/unwatch, event emission
- `ChangeIterator`: blocking, timeout, non-blocking modes
- `ChangeEvent`: creation, equality

### Integration
- Full workflow: create store, set properties, watch, iterate events
- Clone behavior: verify shared state across clones
- Multiple entities: test with many entities and properties

---

## 7. File Structure

```
state-store/
├── Cargo.toml
├── README.md
└── src/
    ├── lib.rs         # Exports and prelude
    ├── property.rs    # Property trait
    ├── store.rs       # PropertyBag and StateStore
    ├── event.rs       # ChangeEvent
    └── iter.rs        # ChangeIterator
```

---

## 8. Dependencies

**None** - state-store has zero external dependencies, using only the Rust standard library.
