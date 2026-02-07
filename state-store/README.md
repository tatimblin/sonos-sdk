# state-store

A generic, type-safe state management library with change detection and blocking iteration.

## Overview

`state-store` provides a simple, synchronous API for managing typed property state across multiple entities. It handles:

- Type-erased storage with type-safe access
- Automatic change detection (only emits events when values actually change)
- Watch pattern for selective property monitoring
- Blocking iteration over change events

This crate is the generic foundation used by `sonos-state` but can be used independently for any stateful application.

## Features

- **Generic Entity IDs**: Use any hashable type as entity identifiers (strings, custom IDs, etc.)
- **Type-safe Properties**: Store and retrieve strongly-typed values via the `Property` trait
- **Change Detection**: Only emit events when values actually differ (via `PartialEq`)
- **Watch Pattern**: Register interest in specific properties to filter change events
- **Sync API**: All operations are synchronous - no async runtime required

## Quick Start

```rust
use state_store::{StateStore, Property};

// Define a property type
#[derive(Clone, PartialEq, Debug)]
struct Temperature(f32);

impl Property for Temperature {
    const KEY: &'static str = "temperature";
}

fn main() {
    // Create a store with String entity IDs
    let store = StateStore::<String>::new();
    let sensor_id = "sensor-1".to_string();

    // Watch for temperature changes
    store.watch(sensor_id.clone(), Temperature::KEY);

    // Set a value (emits change event since watched)
    store.set(&sensor_id, Temperature(72.5));

    // Get current value
    let temp = store.get::<Temperature>(&sensor_id);
    assert_eq!(temp, Some(Temperature(72.5)));

    // Iterate over change events
    for event in store.iter().try_iter() {
        println!("{} changed on {:?}", event.property_key, event.entity_id);
    }
}
```

## API Overview

### Property Trait

```rust
pub trait Property: Clone + Send + Sync + PartialEq + 'static {
    const KEY: &'static str;
}
```

### StateStore

```rust
let store = StateStore::<EntityId>::new();

// Get/set properties
store.set(&entity_id, MyProperty(value));
let value = store.get::<MyProperty>(&entity_id);

// Watch management
store.watch(entity_id, MyProperty::KEY);
store.unwatch(&entity_id, MyProperty::KEY);
store.is_watched(&entity_id, MyProperty::KEY);

// Entity management
store.entity_count();
store.entity_ids();
store.remove_entity(&entity_id);
```

### Change Iteration

```rust
// Blocking iteration
for event in store.iter() {
    println!("{} changed", event.property_key);
}

// Non-blocking
for event in store.iter().try_iter() { /* ... */ }

// With timeout
for event in store.iter().timeout_iter(Duration::from_secs(5)) { /* ... */ }

// Single event with timeout
if let Some(event) = store.iter().recv_timeout(Duration::from_secs(1)) { /* ... */ }
```

## Architecture

```text
StateStore<Id>
    |
    +-- entities: HashMap<Id, PropertyBag>
    |       |
    |       +-- PropertyBag: HashMap<TypeId, Box<dyn Any>>
    |
    +-- watched: HashSet<(Id, property_key)>
    |
    +-- event_channel: mpsc::channel<ChangeEvent<Id>>
            |
            +-- ChangeIterator<Id>
```

## Thread Safety

- `StateStore` is `Clone` and shares state across clones via `Arc`
- All operations are internally synchronized with `RwLock` and `Mutex`
- Safe to use from multiple threads

## License

MIT License
