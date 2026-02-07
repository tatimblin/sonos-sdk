//! Type-erased property storage and state management
//!
//! This module provides the core storage primitives for state management:
//! - `PropertyBag`: Type-erased storage for a single entity's properties
//! - `StateStore<Id>`: Collection of entities with their property bags

use std::any::{Any, TypeId};
use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::sync::{mpsc, Arc, Mutex, RwLock};
use std::time::Instant;

use crate::event::ChangeEvent;
use crate::iter::ChangeIterator;
use crate::property::Property;

// ============================================================================
// PropertyBag - type-erased property storage for a single entity
// ============================================================================

/// Type-erased storage for an entity's properties
///
/// Uses `TypeId` to store and retrieve strongly-typed values.
/// Change detection is built-in via `PartialEq` comparison.
///
/// # Example
///
/// ```rust,ignore
/// use state_store::{PropertyBag, Property};
///
/// #[derive(Clone, PartialEq, Debug)]
/// struct Volume(u8);
/// impl Property for Volume {
///     const KEY: &'static str = "volume";
/// }
///
/// let mut bag = PropertyBag::new();
/// assert!(bag.get::<Volume>().is_none());
///
/// // First set returns true (value changed)
/// assert!(bag.set(Volume(50)));
///
/// // Same value returns false (no change)
/// assert!(!bag.set(Volume(50)));
///
/// // Different value returns true
/// assert!(bag.set(Volume(75)));
///
/// assert_eq!(bag.get::<Volume>(), Some(Volume(75)));
/// ```
pub struct PropertyBag {
    values: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
}

impl PropertyBag {
    /// Create a new empty property bag
    pub fn new() -> Self {
        Self {
            values: HashMap::new(),
        }
    }

    /// Get a property value by type
    ///
    /// Returns `None` if the property has not been set.
    pub fn get<P: Property>(&self) -> Option<P> {
        let type_id = TypeId::of::<P>();
        self.values
            .get(&type_id)
            .and_then(|boxed| boxed.downcast_ref::<P>())
            .cloned()
    }

    /// Set a property value, returning whether the value changed
    ///
    /// Uses `PartialEq` comparison to detect actual changes.
    /// Returns `true` if the value was different (or newly set),
    /// `false` if the value was the same.
    pub fn set<P: Property>(&mut self, value: P) -> bool {
        let type_id = TypeId::of::<P>();
        let current = self
            .values
            .get(&type_id)
            .and_then(|boxed| boxed.downcast_ref::<P>());

        if current != Some(&value) {
            self.values.insert(type_id, Box::new(value));
            true
        } else {
            false
        }
    }

    /// Remove a property, returning whether it existed
    pub fn remove<P: Property>(&mut self) -> bool {
        let type_id = TypeId::of::<P>();
        self.values.remove(&type_id).is_some()
    }

    /// Check if a property exists
    pub fn contains<P: Property>(&self) -> bool {
        let type_id = TypeId::of::<P>();
        self.values.contains_key(&type_id)
    }

    /// Get the number of properties stored
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Check if the bag is empty
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Clear all properties
    pub fn clear(&mut self) {
        self.values.clear();
    }
}

impl Default for PropertyBag {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for PropertyBag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PropertyBag")
            .field("property_count", &self.values.len())
            .finish()
    }
}

// ============================================================================
// StateStore<Id> - generic state store for entities
// ============================================================================

/// Generic state store for managing entity properties with change detection
///
/// The store is generic over the entity ID type, allowing it to be used
/// with any identifier (strings, custom IDs, etc.).
///
/// # Features
///
/// - Type-safe property storage and retrieval
/// - Change detection (only emits events when values actually change)
/// - Watch pattern (register interest in property changes)
/// - Blocking iteration over change events
///
/// # Example
///
/// ```rust,ignore
/// use state_store::{StateStore, Property};
///
/// #[derive(Clone, PartialEq, Debug)]
/// struct Temperature(f32);
/// impl Property for Temperature {
///     const KEY: &'static str = "temperature";
/// }
///
/// let store = StateStore::<String>::new();
/// let sensor_id = "sensor-1".to_string();
///
/// // Watch for temperature changes on sensor-1
/// store.watch(sensor_id.clone(), Temperature::KEY);
///
/// // Set temperature (will emit change event since watched)
/// store.set(&sensor_id, Temperature(72.5));
///
/// // Get current value
/// let temp = store.get::<Temperature>(&sensor_id);
/// assert_eq!(temp, Some(Temperature(72.5)));
/// ```
pub struct StateStore<Id>
where
    Id: Clone + Eq + Hash + Send + Sync + 'static,
{
    /// Entity property storage: entity_id -> PropertyBag
    entities: Arc<RwLock<HashMap<Id, PropertyBag>>>,

    /// Watched properties: (entity_id, property_key)
    watched: Arc<RwLock<HashSet<(Id, &'static str)>>>,

    /// Channel sender for change events
    event_tx: mpsc::Sender<ChangeEvent<Id>>,

    /// Channel receiver for change events (wrapped for cloning)
    event_rx: Arc<Mutex<mpsc::Receiver<ChangeEvent<Id>>>>,
}

impl<Id> StateStore<Id>
where
    Id: Clone + Eq + Hash + Send + Sync + 'static,
{
    /// Create a new empty state store
    pub fn new() -> Self {
        let (event_tx, event_rx) = mpsc::channel();

        Self {
            entities: Arc::new(RwLock::new(HashMap::new())),
            watched: Arc::new(RwLock::new(HashSet::new())),
            event_tx,
            event_rx: Arc::new(Mutex::new(event_rx)),
        }
    }

    /// Get a property value for an entity
    ///
    /// Returns `None` if the entity doesn't exist or the property isn't set.
    pub fn get<P: Property>(&self, entity_id: &Id) -> Option<P> {
        let entities = self.entities.read().ok()?;
        entities.get(entity_id)?.get::<P>()
    }

    /// Set a property value for an entity
    ///
    /// If the value changes and the property is being watched,
    /// a change event is emitted.
    pub fn set<P: Property>(&self, entity_id: &Id, value: P) {
        let changed = {
            let mut entities = match self.entities.write() {
                Ok(e) => e,
                Err(_) => return,
            };
            let bag = entities.entry(entity_id.clone()).or_insert_with(PropertyBag::new);
            bag.set(value)
        };

        if changed {
            self.maybe_emit_change(entity_id, P::KEY);
        }
    }

    /// Register interest in a property for an entity
    ///
    /// After watching, changes to this property will appear in `iter()`.
    pub fn watch(&self, entity_id: Id, property_key: &'static str) {
        if let Ok(mut watched) = self.watched.write() {
            watched.insert((entity_id, property_key));
        }
    }

    /// Unregister interest in a property
    pub fn unwatch(&self, entity_id: &Id, property_key: &'static str) {
        if let Ok(mut watched) = self.watched.write() {
            watched.remove(&(entity_id.clone(), property_key));
        }
    }

    /// Check if a property is being watched
    pub fn is_watched(&self, entity_id: &Id, property_key: &'static str) -> bool {
        self.watched
            .read()
            .map(|w| w.contains(&(entity_id.clone(), property_key)))
            .unwrap_or(false)
    }

    /// Create a blocking iterator over change events
    ///
    /// Only emits events for properties that have been watched.
    pub fn iter(&self) -> ChangeIterator<Id> {
        ChangeIterator::new(Arc::clone(&self.event_rx))
    }

    /// Get the number of entities in the store
    pub fn entity_count(&self) -> usize {
        self.entities
            .read()
            .map(|e| e.len())
            .unwrap_or(0)
    }

    /// Check if the store is empty
    pub fn is_empty(&self) -> bool {
        self.entity_count() == 0
    }

    /// Get all entity IDs
    pub fn entity_ids(&self) -> Vec<Id> {
        self.entities
            .read()
            .map(|e| e.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// Remove an entity and all its properties
    pub fn remove_entity(&self, entity_id: &Id) -> bool {
        self.entities
            .write()
            .map(|mut e| e.remove(entity_id).is_some())
            .unwrap_or(false)
    }

    /// Clear all entities and properties
    pub fn clear(&self) {
        if let Ok(mut entities) = self.entities.write() {
            entities.clear();
        }
        if let Ok(mut watched) = self.watched.write() {
            watched.clear();
        }
    }

    /// Get the event sender for external event injection
    ///
    /// This is useful for testing or for injecting events from
    /// external sources (e.g., network callbacks).
    pub fn event_sender(&self) -> mpsc::Sender<ChangeEvent<Id>> {
        self.event_tx.clone()
    }

    /// Emit a change event if the property is being watched
    fn maybe_emit_change(&self, entity_id: &Id, property_key: &'static str) {
        let is_watched = self
            .watched
            .read()
            .map(|w| w.contains(&(entity_id.clone(), property_key)))
            .unwrap_or(false);

        if is_watched {
            let event = ChangeEvent {
                entity_id: entity_id.clone(),
                property_key,
                timestamp: Instant::now(),
            };
            let _ = self.event_tx.send(event);
        }
    }
}

impl<Id> Default for StateStore<Id>
where
    Id: Clone + Eq + Hash + Send + Sync + 'static,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<Id> Clone for StateStore<Id>
where
    Id: Clone + Eq + Hash + Send + Sync + 'static,
{
    fn clone(&self) -> Self {
        Self {
            entities: Arc::clone(&self.entities),
            watched: Arc::clone(&self.watched),
            event_tx: self.event_tx.clone(),
            event_rx: Arc::clone(&self.event_rx),
        }
    }
}

impl<Id> std::fmt::Debug for StateStore<Id>
where
    Id: Clone + Eq + Hash + Send + Sync + std::fmt::Debug + 'static,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StateStore")
            .field("entity_count", &self.entity_count())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, PartialEq, Debug)]
    struct TestProp(i32);

    impl Property for TestProp {
        const KEY: &'static str = "test";
    }

    #[derive(Clone, PartialEq, Debug)]
    struct OtherProp(String);

    impl Property for OtherProp {
        const KEY: &'static str = "other";
    }

    #[test]
    fn test_property_bag_basic() {
        let mut bag = PropertyBag::new();

        // Initially empty
        assert!(bag.is_empty());
        assert!(bag.get::<TestProp>().is_none());

        // Set returns true (value changed)
        assert!(bag.set(TestProp(42)));
        assert!(!bag.is_empty());
        assert_eq!(bag.get::<TestProp>(), Some(TestProp(42)));

        // Same value returns false
        assert!(!bag.set(TestProp(42)));

        // Different value returns true
        assert!(bag.set(TestProp(99)));
        assert_eq!(bag.get::<TestProp>(), Some(TestProp(99)));
    }

    #[test]
    fn test_property_bag_multiple_types() {
        let mut bag = PropertyBag::new();

        bag.set(TestProp(42));
        bag.set(OtherProp("hello".to_string()));

        assert_eq!(bag.len(), 2);
        assert_eq!(bag.get::<TestProp>(), Some(TestProp(42)));
        assert_eq!(bag.get::<OtherProp>(), Some(OtherProp("hello".to_string())));
    }

    #[test]
    fn test_state_store_basic() {
        let store = StateStore::<String>::new();

        // Initially empty
        assert!(store.is_empty());
        assert!(store.get::<TestProp>(&"entity-1".to_string()).is_none());

        // Set creates entity
        store.set(&"entity-1".to_string(), TestProp(42));
        assert_eq!(store.entity_count(), 1);
        assert_eq!(store.get::<TestProp>(&"entity-1".to_string()), Some(TestProp(42)));
    }

    #[test]
    fn test_state_store_watch() {
        let store = StateStore::<String>::new();
        let entity_id = "entity-1".to_string();

        // Not watched initially
        assert!(!store.is_watched(&entity_id, TestProp::KEY));

        // Watch
        store.watch(entity_id.clone(), TestProp::KEY);
        assert!(store.is_watched(&entity_id, TestProp::KEY));

        // Unwatch
        store.unwatch(&entity_id, TestProp::KEY);
        assert!(!store.is_watched(&entity_id, TestProp::KEY));
    }

    #[test]
    fn test_state_store_change_event() {
        let store = StateStore::<String>::new();
        let entity_id = "entity-1".to_string();

        // Watch the property
        store.watch(entity_id.clone(), TestProp::KEY);

        // Set value (should emit event)
        store.set(&entity_id, TestProp(42));

        // Get event via iter
        let iter = store.iter();
        let event = iter.recv_timeout(std::time::Duration::from_millis(100));
        assert!(event.is_some());

        let event = event.unwrap();
        assert_eq!(event.entity_id, entity_id);
        assert_eq!(event.property_key, TestProp::KEY);
    }

    #[test]
    fn test_state_store_no_event_when_not_watched() {
        let store = StateStore::<String>::new();
        let entity_id = "entity-1".to_string();

        // Set without watching
        store.set(&entity_id, TestProp(42));

        // No event should be emitted
        let iter = store.iter();
        let event = iter.recv_timeout(std::time::Duration::from_millis(50));
        assert!(event.is_none());
    }

    #[test]
    fn test_state_store_no_event_when_same_value() {
        let store = StateStore::<String>::new();
        let entity_id = "entity-1".to_string();

        store.watch(entity_id.clone(), TestProp::KEY);

        // First set emits event
        store.set(&entity_id, TestProp(42));

        let iter = store.iter();
        let event = iter.recv_timeout(std::time::Duration::from_millis(100));
        assert!(event.is_some());

        // Same value does not emit event
        store.set(&entity_id, TestProp(42));
        let event = iter.recv_timeout(std::time::Duration::from_millis(50));
        assert!(event.is_none());
    }

    #[test]
    fn test_state_store_clone() {
        let store = StateStore::<String>::new();
        let cloned = store.clone();

        // Both share the same state
        store.set(&"entity-1".to_string(), TestProp(42));
        assert_eq!(cloned.get::<TestProp>(&"entity-1".to_string()), Some(TestProp(42)));
    }
}
