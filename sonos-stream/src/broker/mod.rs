//! Event broker module for managing UPnP event subscriptions.
//!
//! This module provides the core broker functionality split into focused submodules:
//!
//! - `core`: The main EventBroker struct and public API
//! - `event_processor`: Event routing and parsing
//! - `callback_adapter`: Converts generic callback notifications to Sonos-specific events
//!
//! # Architecture
//!
//! The broker uses a manager pattern to separate concerns:
//!
//! - **EventBroker** (core): Coordinates between managers and exposes the public API
//! - **EventProcessor**: Runs a background task to route events to strategies for parsing
//! - **CallbackAdapter**: Converts generic callback notifications to Sonos-specific events
//!
//! All managers share access to subscription state via `Arc<RwLock<HashMap>>` and communicate
//! via channels for events and shutdown signals.
//!
//! Note: Subscription creation and management is now handled by the `sonos-api` crate's
//! `SonosClient` and `ManagedSubscription` types.

// Module declarations
mod callback_adapter;
mod core;
mod event_processor;

// Re-exports
pub use core::EventBroker;

// Internal re-exports
pub(crate) use callback_adapter::CallbackAdapter;
pub(crate) use event_processor::EventProcessor;

