//! # sonos-stream
//!
//! A micro-crate for managing UPnP event subscriptions for Sonos speakers using a broker pattern.
//!
//! This crate provides a clean abstraction for subscribing to speaker events without needing to
//! understand the underlying UPnP protocol details. It uses the strategy pattern to delegate
//! service-specific event parsing to separate implementations, keeping the core broker logic
//! service-agnostic.

mod broker;
mod builder;
mod callback_server;
mod error;
mod event;
mod strategy;
mod subscription;
mod types;

pub use broker::{ActiveSubscription, EventBroker};
pub use builder::EventBrokerBuilder;
pub use callback_server::{CallbackServer, EventRouter, RawEvent};
pub use error::*;
pub use event::{Event, ParsedEvent};
pub use strategy::*;
pub use subscription::*;
pub use types::*;
