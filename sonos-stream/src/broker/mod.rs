//! Event broker module for managing UPnP event subscriptions.
//!
//! This module provides the core broker functionality split into focused submodules:
//!
//! - `core`: The main EventBroker struct and public API
//! - `subscription_manager`: Subscription lifecycle management
//! - `renewal_manager`: Automatic subscription renewal with retry logic
//! - `event_processor`: Event routing and parsing
//!
//! # Architecture
//!
//! The broker uses a manager pattern to separate concerns:
//!
//! - **EventBroker** (core): Coordinates between managers and exposes the public API
//! - **SubscriptionManager**: Handles subscription creation, removal, and validation
//! - **RenewalManager**: Runs a background task for automatic renewal with exponential backoff
//! - **EventProcessor**: Runs a background task to route events to strategies for parsing
//!
//! All managers share access to subscription state via `Arc<RwLock<HashMap>>` and communicate
//! via channels for events and shutdown signals.

// Module declarations
// These modules will be populated in subsequent tasks
mod core;
mod event_processor;
mod renewal_manager;
mod subscription_manager;

// Re-exports
pub use core::EventBroker;
pub use subscription_manager::ActiveSubscription;

// Internal re-exports
pub(crate) use event_processor::EventProcessor;
pub(crate) use renewal_manager::RenewalManager;
pub(crate) use subscription_manager::SubscriptionManager;
