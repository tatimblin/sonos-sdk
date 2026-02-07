//! Property handles for DOM-like property access
//!
//! This module provides the `PropertyHandle<P>` generic type and type aliases
//! for all supported property types.

mod handles;

// Re-export the generic PropertyHandle and Fetchable trait
pub use handles::{Fetchable, PropertyHandle};

// Re-export type aliases for common property handles (only those currently used)
pub use handles::{PlaybackStateHandle, VolumeHandle};
