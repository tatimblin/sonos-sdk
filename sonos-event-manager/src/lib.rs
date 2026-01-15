//! # Sonos Event Manager
//!
//! A sync-first facade for intelligent Sonos event subscription management with automatic
//! reference counting and resource cleanup.
//!
//! ## Overview
//!
//! The Sonos Event Manager provides a fully synchronous interface for working with Sonos
//! device events. All async operations are hidden in a background worker thread, so users
//! never need to use async/await.
//!
//! ## Key Features
//!
//! - **Sync-First API**: All methods are synchronous - no async/await required
//! - **Automatic Lifecycle Management**: UPnP subscriptions created on first consumer, destroyed on last drop
//! - **Reference Counting**: Thread-safe tracking of active consumers per device/service pair
//! - **Resource Efficient**: Only subscribe to events that are actually being consumed
//! - **Discovery Integration**: Easy integration with `sonos-discovery` for adding devices
//! - **Background Processing**: Async event handling hidden in dedicated worker thread
//!
//! ## Usage
//!
//! ```rust,ignore
//! use sonos_event_manager::SonosEventManager;
//! use sonos_api::Service;
//! use sonos_discovery;
//!
//! // Create the event manager (sync - no .await!)
//! let manager = SonosEventManager::new()?;
//!
//! // Add discovered devices
//! let devices = sonos_discovery::get();
//! manager.add_devices(devices)?;
//!
//! // Get a device
//! let devices = manager.devices();
//! if let Some(device) = devices.first() {
//!     let device_ip = device.ip_address.parse()?;
//!
//!     // Subscribe to events (sync, ref-counted)
//!     manager.ensure_service_subscribed(device_ip, Service::RenderingControl)?;
//!
//!     // Iterate over events (blocking)
//!     for event in manager.iter() {
//!         println!("Event from {:?}: {:?}", event.speaker_ip, event.service);
//!     }
//!
//!     // Release subscription when done
//!     manager.release_service_subscription(device_ip, Service::RenderingControl)?;
//! }
//! ```
//!
//! ## Architecture
//!
//! The event manager implements the **Reference-Counted Observable** pattern:
//!
//! 1. **Device Registration**: Discovered devices are registered with the manager
//! 2. **Demand-Driven Subscriptions**: UPnP subscriptions created only when first consumer requests them
//! 3. **Reference Counting**: Each subscription call increments a reference count
//! 4. **Automatic Cleanup**: When reference count reaches zero, UPnP subscription is terminated
//! 5. **Background Processing**: All async operations handled by dedicated worker thread
//!
//! This approach is similar to RxJS's `refCount()` operator or connection pooling with reference counting.

pub mod error;
pub mod iter;
pub mod manager;
pub mod worker;

// Re-export main types for convenience
pub use error::{EventManagerError, Result};
pub use iter::EventManagerIterator;
pub use manager::SonosEventManager;

// Re-export commonly used types from dependencies
pub use sonos_api::Service;
pub use sonos_discovery::Device;
pub use sonos_stream::events::EnrichedEvent;
pub use sonos_stream::BrokerConfig;

/// Prelude module for convenient imports
///
/// Use this to import the most commonly used types and traits:
///
/// ```rust
/// use sonos_event_manager::prelude::*;
/// ```
pub mod prelude {
    pub use crate::{
        BrokerConfig, Device, EnrichedEvent, EventManagerError, EventManagerIterator, Result,
        Service, SonosEventManager,
    };
}
