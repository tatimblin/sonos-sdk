//! Polling system for transparent event fallback
//!
//! This module implements an intelligent polling system that can replace UPnP events
//! when firewall blocking is detected or events fail to arrive. It uses service-specific
//! strategies to poll device state and generate synthetic events.

pub mod scheduler;
pub mod strategies;

pub use scheduler::{PollingScheduler, PollingTask};
pub use strategies::{
    AVTransportPoller, DeviceStatePoller, RenderingControlPoller, ServicePoller, StateChange,
};