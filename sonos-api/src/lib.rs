//! High-level Sonos API for device control
//!
//! This crate provides a type-safe, trait-based API for controlling Sonos devices.
//! It uses the private `soap-client` crate for low-level SOAP communication.
//!
//! # UPnP Architecture
//!
//! This API correctly implements the UPnP architecture used by Sonos devices:
//!
//! - **Control Operations**: Commands like Play, Pause, SetVolume are sent to `/Control` endpoints
//! - **Event Subscriptions**: Subscriptions for state change notifications are sent to `/Event` endpoints
//!
//! These are completely separate and independent concepts that work together to provide
//! both control and monitoring capabilities.
//!
//! # Enhanced Operation Framework
//!
//! The operation framework provides composable, declarative UPnP operations with validation,
//! retry policies, and timeouts:
//!
//! ```rust,ignore
//! use sonos_api::{SonosClient, services::av_transport, services::rendering_control};
//! use sonos_api::operation::ValidationLevel;
//!
//! let client = SonosClient::new();
//!
//! // Simple operation execution
//! let play_op = av_transport::play("1".to_string())
//!     .with_validation(ValidationLevel::Comprehensive)
//!     .build()?;
//!
//! client.execute_enhanced("192.168.1.100", play_op)?;
//!
//! // Composed operations
//! let sequence = av_transport::play("1".to_string())
//!     .build()?
//!     .and_then(rendering_control::set_volume("Master".to_string(), 75).build()?);
//!
//! client.execute_sequence("192.168.1.100", sequence)?;
//! ```
//!
//! # Event Subscription Management
//!
//! UPnP event subscriptions allow you to receive real-time state change notifications
//! from Sonos devices. Subscriptions are managed separately from control operations:
//!
//! ## Client-Level Subscriptions
//!
//! Subscribe directly to any service using the client:
//!
//! ```rust,ignore
//! use sonos_api::{SonosClient, Service};
//!
//! let client = SonosClient::new();
//!
//! // Subscribe to AVTransport events (play/pause state changes)
//! let av_subscription = client.subscribe(
//!     "192.168.1.100",
//!     Service::AVTransport,
//!     "http://192.168.1.50:8080/callback"
//! )?;
//!
//! // Subscribe to RenderingControl events (volume changes)
//! let rc_subscription = client.subscribe(
//!     "192.168.1.100",
//!     Service::RenderingControl,
//!     "http://192.168.1.50:8080/callback"
//! )?;
//!
//! // Custom timeout (default is 1800 seconds)
//! let long_subscription = client.subscribe_with_timeout(
//!     "192.168.1.100",
//!     Service::AVTransport,
//!     "http://192.168.1.50:8080/callback",
//!     7200  // 2 hours
//! )?;
//! ```
//!
//! ## Service-Level Subscription Helpers
//!
//! Each service module provides convenient subscription helpers:
//!
//! ```rust,ignore
//! use sonos_api::{SonosClient, services::av_transport, services::rendering_control};
//!
//! let client = SonosClient::new();
//!
//! // Subscribe to specific services using module helpers
//! let av_subscription = av_transport::subscribe(
//!     &client,
//!     "192.168.1.100",
//!     "http://192.168.1.50:8080/callback"
//! )?;
//!
//! let rc_subscription = rendering_control::subscribe(
//!     &client,
//!     "192.168.1.100",
//!     "http://192.168.1.50:8080/callback"
//! )?;
//!
//! // With custom timeout
//! let long_av_subscription = av_transport::subscribe_with_timeout(
//!     &client,
//!     "192.168.1.100",
//!     "http://192.168.1.50:8080/callback",
//!     3600
//! )?;
//! ```
//!
//! ## Subscription Lifecycle Management
//!
//! All subscriptions return a `ManagedSubscription` that handles lifecycle management:
//!
//! ```rust,ignore
//! let subscription = client.subscribe(
//!     "192.168.1.100",
//!     Service::AVTransport,
//!     "http://192.168.1.50:8080/callback"
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
//!
//! ## Control Operations and Subscriptions Together
//!
//! Control operations and subscriptions work independently but can be used together:
//!
//! ```rust,ignore
//! use sonos_api::{SonosClient, Service, services::av_transport};
//!
//! let client = SonosClient::new();
//!
//! // Set up event subscription first
//! let subscription = client.subscribe(
//!     "192.168.1.100",
//!     Service::AVTransport,
//!     "http://192.168.1.50:8080/callback"
//! )?;
//!
//! // Execute control operations
//! let play_op = av_transport::play("1".to_string()).build()?;
//! client.execute_enhanced("192.168.1.100", play_op)?;
//!
//! // The subscription will receive events about state changes
//! // caused by the control operations
//! ```

pub mod client;
pub mod error;
pub mod operation; // Enhanced operation framework
pub mod service;
pub mod services; // Enhanced services
pub mod subscription;
pub mod events; // New event handling framework

// Legacy exports for backward compatibility
pub use client::SonosClient;
pub use error::{ApiError, Result};
pub use operation::SonosOperation; // Legacy trait
pub use service::{Service, ServiceInfo, ServiceScope};
pub use subscription::ManagedSubscription;


// New enhanced operation framework exports
pub use operation::{
    UPnPOperation, OperationBuilder,
    ValidationLevel, ValidationError, Validate,
    OperationMetadata,
};

// New event handling framework exports
pub use events::{
    EnrichedEvent, EventSource, EventParser, EventParserRegistry, EventProcessor,
    extract_xml_value,
};

// Enhanced services are available through the services module