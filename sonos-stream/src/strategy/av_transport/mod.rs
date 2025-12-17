//! AVTransport strategy implementation and event types.
//!
//! This module provides the complete AVTransport strategy implementation including:
//! - Strategy implementation for subscription and event parsing
//! - Strongly-typed event data structures
//! - Utility functions for working with AVTransport events

pub mod event;
pub mod strategy;

pub use event::AVTransportEvent;
pub use strategy::AVTransportStrategy;