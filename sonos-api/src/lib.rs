//! High-level Sonos API for device control
//! 
//! This crate provides a type-safe, trait-based API for controlling Sonos devices.
//! It uses the private `soap-client` crate for low-level SOAP communication.
//!
//! # Subscription Management
//!
//! The primary way to manage UPnP event subscriptions is through the `ManagedSubscription` API:
//!
//! ```rust
//! use sonos_api::{SonosClient, Service};
//!
//! let client = SonosClient::new();
//! let subscription = client.create_managed_subscription(
//!     "192.168.1.100",
//!     Service::AVTransport,
//!     "http://192.168.1.50:8080/callback",
//!     1800
//! )?;
//!
//! // Check if renewal is needed and renew if so
//! if subscription.needs_renewal() {
//!     subscription.renew()?;
//! }
//!
//! // Clean up when done
//! subscription.unsubscribe()?;
//! ```
//!
//! The `ManagedSubscription` handles all lifecycle management including expiration tracking,
//! renewal logic, and proper cleanup.

pub mod client;
pub mod error;
pub mod operation;
pub mod service;
pub mod operations;
pub mod subscription;

pub use client::SonosClient;
pub use error::{ApiError, Result};
pub use operation::SonosOperation;
pub use service::{Service, ServiceInfo};
pub use subscription::ManagedSubscription;