//! Change events for property updates
//!
//! When a watched property changes, a `ChangeEvent` is emitted
//! containing the entity ID and property key that changed.

use std::time::Instant;

/// A change event emitted when a watched property changes
///
/// Events only include the entity ID and property key, not the actual
/// value. Use `StateStore::get()` to retrieve the new value after
/// receiving an event.
///
/// # Example
///
/// ```rust,ignore
/// for event in store.iter() {
///     println!("{} changed on {:?}", event.property_key, event.entity_id);
///
///     // Get the new value
///     if event.property_key == Temperature::KEY {
///         if let Some(temp) = store.get::<Temperature>(&event.entity_id) {
///             println!("New temperature: {:?}", temp);
///         }
///     }
/// }
/// ```
#[derive(Debug, Clone)]
pub struct ChangeEvent<Id> {
    /// The entity whose property changed
    pub entity_id: Id,

    /// The property key that changed (matches `Property::KEY`)
    pub property_key: &'static str,

    /// When the change was detected
    pub timestamp: Instant,
}

impl<Id> ChangeEvent<Id> {
    /// Create a new change event
    pub fn new(entity_id: Id, property_key: &'static str) -> Self {
        Self {
            entity_id,
            property_key,
            timestamp: Instant::now(),
        }
    }

    /// Create a new change event with a specific timestamp
    pub fn with_timestamp(entity_id: Id, property_key: &'static str, timestamp: Instant) -> Self {
        Self {
            entity_id,
            property_key,
            timestamp,
        }
    }
}

impl<Id: PartialEq> PartialEq for ChangeEvent<Id> {
    fn eq(&self, other: &Self) -> bool {
        // Timestamp not included in equality
        self.entity_id == other.entity_id && self.property_key == other.property_key
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_change_event_creation() {
        let event = ChangeEvent::new("entity-1".to_string(), "temperature");

        assert_eq!(event.entity_id, "entity-1");
        assert_eq!(event.property_key, "temperature");
    }

    #[test]
    fn test_change_event_equality() {
        let event1 = ChangeEvent::new("entity-1".to_string(), "temperature");
        let event2 = ChangeEvent::new("entity-1".to_string(), "temperature");
        let event3 = ChangeEvent::new("entity-2".to_string(), "temperature");
        let event4 = ChangeEvent::new("entity-1".to_string(), "humidity");

        // Same entity and property
        assert_eq!(event1, event2);

        // Different entity
        assert_ne!(event1, event3);

        // Different property
        assert_ne!(event1, event4);
    }
}
