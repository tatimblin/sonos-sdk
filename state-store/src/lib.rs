//! Generic State Management Library
//!
//! A type-safe, generic state management library with change detection
//! and blocking iteration patterns.
//!
//! # Features
//!
//! - **Type-safe Storage**: Store and retrieve strongly-typed properties
//! - **Change Detection**: Only emit events when values actually change
//! - **Watch Pattern**: Register interest in specific properties
//! - **Blocking Iteration**: Consume change events via blocking iterators
//! - **Generic Entity IDs**: Use any hashable type as entity identifiers
//!
//! # Quick Start
//!
//! ```rust
//! use state_store::{StateStore, Property};
//!
//! // Define a property type
//! #[derive(Clone, PartialEq, Debug)]
//! struct Temperature(f32);
//!
//! impl Property for Temperature {
//!     const KEY: &'static str = "temperature";
//! }
//!
//! // Create a store with String entity IDs
//! let store = StateStore::<String>::new();
//!
//! // Watch for temperature changes on sensor-1
//! store.watch("sensor-1".to_string(), Temperature::KEY);
//!
//! // Set a value (will emit change event since watched)
//! store.set(&"sensor-1".to_string(), Temperature(72.5));
//!
//! // Get current value
//! let temp = store.get::<Temperature>(&"sensor-1".to_string());
//! assert_eq!(temp, Some(Temperature(72.5)));
//! ```
//!
//! # Iteration Patterns
//!
//! ```rust,ignore
//! // Blocking iteration (waits for events)
//! for event in store.iter() {
//!     println!("{} changed on {:?}", event.property_key, event.entity_id);
//! }
//!
//! // Non-blocking (processes available events)
//! for event in store.iter().try_iter() {
//!     println!("Event: {:?}", event);
//! }
//!
//! // With timeout
//! use std::time::Duration;
//! if let Some(event) = store.iter().recv_timeout(Duration::from_secs(1)) {
//!     println!("Got event: {:?}", event);
//! }
//! ```
//!
//! # Architecture
//!
//! ```text
//! StateStore<Id>
//!     │
//!     ├── entities: HashMap<Id, PropertyBag>
//!     │       │
//!     │       └── PropertyBag: HashMap<TypeId, Box<dyn Any>>
//!     │
//!     ├── watched: HashSet<(Id, property_key)>
//!     │
//!     └── event_channel: mpsc::channel<ChangeEvent<Id>>
//!             │
//!             └── ChangeIterator<Id>
//! ```

// Modules
pub mod event;
pub mod iter;
pub mod property;
pub mod store;

// Re-exports - Public API
pub use event::ChangeEvent;
pub use iter::{ChangeIterator, TimeoutIter, TryIter};
pub use property::Property;
pub use store::{PropertyBag, StateStore};

/// Prelude for convenient imports
pub mod prelude {
    pub use crate::event::ChangeEvent;
    pub use crate::iter::ChangeIterator;
    pub use crate::property::Property;
    pub use crate::store::{PropertyBag, StateStore};
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, PartialEq, Debug)]
    struct Volume(u8);

    impl Property for Volume {
        const KEY: &'static str = "volume";
    }

    #[derive(Clone, PartialEq, Debug)]
    struct Mute(bool);

    impl Property for Mute {
        const KEY: &'static str = "mute";
    }

    #[test]
    fn test_full_workflow() {
        // Create store
        let store = StateStore::<String>::new();

        // Add some properties
        store.set(&"speaker-1".to_string(), Volume(50));
        store.set(&"speaker-1".to_string(), Mute(false));

        // Verify values
        assert_eq!(store.get::<Volume>(&"speaker-1".to_string()), Some(Volume(50)));
        assert_eq!(store.get::<Mute>(&"speaker-1".to_string()), Some(Mute(false)));

        // Watch and verify events
        store.watch("speaker-1".to_string(), Volume::KEY);

        // Change value
        store.set(&"speaker-1".to_string(), Volume(75));

        // Get event
        let event = store.iter().recv_timeout(std::time::Duration::from_millis(100));
        assert!(event.is_some());
        assert_eq!(event.unwrap().property_key, Volume::KEY);
    }

    #[test]
    fn test_multiple_entities() {
        let store = StateStore::<String>::new();

        store.set(&"speaker-1".to_string(), Volume(50));
        store.set(&"speaker-2".to_string(), Volume(75));
        store.set(&"speaker-3".to_string(), Volume(100));

        assert_eq!(store.entity_count(), 3);
        assert_eq!(store.get::<Volume>(&"speaker-1".to_string()), Some(Volume(50)));
        assert_eq!(store.get::<Volume>(&"speaker-2".to_string()), Some(Volume(75)));
        assert_eq!(store.get::<Volume>(&"speaker-3".to_string()), Some(Volume(100)));
    }

    #[test]
    fn test_store_clone_shares_state() {
        let store1 = StateStore::<String>::new();
        let store2 = store1.clone();

        store1.set(&"speaker-1".to_string(), Volume(50));

        // Both clones see the same data
        assert_eq!(store2.get::<Volume>(&"speaker-1".to_string()), Some(Volume(50)));
    }
}
