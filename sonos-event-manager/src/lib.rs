//! # Sonos Event Manager
//!
//! A high-level facade for intelligent Sonos event subscription management with automatic
//! reference counting and resource cleanup.
//!
//! ## Overview
//!
//! The Sonos Event Manager provides a simplified interface for working with Sonos device events
//! by implementing demand-driven subscription management. UPnP event subscriptions are only
//! created when there are active consumers and are automatically terminated when no consumers
//! remain, preventing resource waste.
//!
//! ## Key Features
//!
//! - **Automatic Lifecycle Management**: UPnP subscriptions created on first consumer, destroyed on last drop
//! - **Reference Counting**: Thread-safe tracking of active consumers per device/service pair
//! - **Resource Efficient**: Only subscribe to events that are actually being consumed
//! - **Discovery Integration**: Easy integration with `sonos-discovery` for adding devices
//! - **Clean API**: Hides `sonos-stream` complexity behind simple subscribe/consume patterns
//!
//! ## Usage
//!
//! ```rust
//! use sonos_event_manager::SonosEventManager;
//! use sonos_api::Service;
//! use sonos_discovery;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Create the event manager
//! let mut event_manager = SonosEventManager::new().await?;
//!
//! // Add discovered devices
//! let devices = sonos_discovery::get();
//! event_manager.add_devices(devices).await?;
//!
//! // Get a device (for this example, assume we have at least one)
//! let devices = event_manager.devices().await;
//! if let Some(device) = devices.first() {
//!     let device_ip = device.ip_address.parse()?;
//!
//!     // Subscribe to volume events - automatically creates UPnP subscription
//!     let volume_events = event_manager
//!         .subscribe(device_ip, Service::RenderingControl)
//!         .await?;
//!
//!     // Subscribe to transport events - reuses existing subscription if any
//!     let transport_events = event_manager
//!         .subscribe(device_ip, Service::AVTransport)
//!         .await?;
//!
//!     // Process events - when these consumers are dropped,
//!     // reference counts decrement and subscriptions are cleaned up automatically
//! }
//! # Ok(())
//! # }
//! ```
//!
//! ## Architecture
//!
//! The event manager implements the **Reference-Counted Observable** pattern:
//!
//! 1. **Device Registration**: Discovered devices are registered with the manager
//! 2. **Demand-Driven Subscriptions**: UPnP subscriptions created only when first consumer requests them
//! 3. **Reference Counting**: Each `EventConsumer` increments a reference count
//! 4. **Automatic Cleanup**: When reference count reaches zero, UPnP subscription is terminated
//! 5. **Event Distribution**: Events are converted and distributed to all active consumers
//!
//! This approach is similar to RxJS's `refCount()` operator or connection pooling with reference counting.

pub mod error;
pub mod manager;

// Re-export main types for convenience
pub use error::{EventManagerError, Result};
pub use manager::SonosEventManager;

// Re-export commonly used types from dependencies
pub use sonos_api::Service;
pub use sonos_discovery::Device;

/// Prelude module for convenient imports
///
/// Use this to import the most commonly used types and traits:
///
/// ```rust
/// use sonos_event_manager::prelude::*;
/// ```
pub mod prelude {
    pub use crate::{
        EventManagerError, Result, SonosEventManager,
    };
    pub use sonos_api::Service;
    pub use sonos_discovery::Device;
}