//! Property trait for typed, watchable state values
//!
//! The Property trait defines the contract for values that can be stored,
//! watched, and tracked for changes in a StateStore.
//!
//! # Example
//!
//! ```rust
//! use state_store::Property;
//!
//! #[derive(Clone, PartialEq, Debug)]
//! pub struct Temperature(pub f32);
//!
//! impl Property for Temperature {
//!     const KEY: &'static str = "temperature";
//! }
//! ```

/// Marker trait for properties that can be stored and watched
///
/// Properties must be:
/// - Clone: For copying values to watchers
/// - Send + Sync: For thread-safe access
/// - PartialEq: For change detection (only emit when value actually changes)
/// - 'static: For type-erased storage using TypeId
///
/// The KEY constant provides a human-readable identifier for debugging,
/// logging, and event filtering.
pub trait Property: Clone + Send + Sync + PartialEq + 'static {
    /// Unique key identifying this property type
    ///
    /// Used for debugging, logging, and filtering change events.
    /// Should be unique within your application domain.
    ///
    /// # Examples
    ///
    /// - `"volume"` for audio volume
    /// - `"temperature"` for sensor readings
    /// - `"connection_state"` for network status
    const KEY: &'static str;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, PartialEq, Debug)]
    struct TestProperty(i32);

    impl Property for TestProperty {
        const KEY: &'static str = "test_property";
    }

    #[test]
    fn test_property_key() {
        assert_eq!(TestProperty::KEY, "test_property");
    }

    #[test]
    fn test_property_equality() {
        let a = TestProperty(42);
        let b = TestProperty(42);
        let c = TestProperty(99);

        assert_eq!(a, b);
        assert_ne!(a, c);
    }
}
