//! High-level Sonos API for device control
//!
//! This crate provides a type-safe, trait-based API for controlling Sonos devices.
//! It uses the private `soap-client` crate for low-level SOAP communication.
//!
//! # New Enhanced Operation Framework
//!
//! The new operation framework provides composable, declarative UPnP operations:
//!
//! ```rust,ignore
//! use sonos_api::operation::{OperationBuilder, ValidationLevel};
//! use sonos_api::services::av_transport;
//!
//! // Simple operation execution
//! let play_op = av_transport::play("1".to_string())
//!     .with_validation(ValidationLevel::Comprehensive)
//!     .build()?;
//!
//! // Composed operations
//! let sequence = av_transport::play("1".to_string())
//!     .build()?
//!     .and_then(rendering_control::set_volume(75).build()?);
//!
//! // Execute with enhanced client
//! let client = SonosClient::new();
//! client.execute_sequence("192.168.1.100", sequence)?;
//! ```
//!
//! # Legacy Subscription Management
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
pub mod operation; // Enhanced operation framework
pub mod service;
pub mod operations; // Legacy operations
pub mod services; // New enhanced services
pub mod subscription;

// Legacy exports for backward compatibility
pub use client::SonosClient;
pub use error::{ApiError, Result};
pub use operation::SonosOperation; // Legacy trait
pub use service::{Service, ServiceInfo};
pub use subscription::ManagedSubscription;

// Enhanced error handling exports
pub use error::{
    OperationContext, ContextualResult, WithContext, BatchStatistics
};

// New enhanced operation framework exports
pub use operation::{
    UPnPOperation, OperationBuilder, ComposableOperation,
    ValidationLevel, ValidationError, Validate,
    OperationSequence, OperationBatch, ConditionalOperation,
    RetryPolicy, OperationMetadata,
    SequenceResult, BatchResult, ConditionalResult,
};

// Enhanced services are available through the services module