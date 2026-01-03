//! Subscription management and event detection
//!
//! This module handles subscription lifecycle management by integrating with SonosClient's
//! ManagedSubscription system and provides proactive firewall detection to enable immediate
//! polling fallback when needed.

pub mod event_detector;
pub mod manager;

pub use event_detector::{EventDetector, ResyncDetector};
pub use manager::{ManagedSubscriptionWrapper, SubscriptionManager};