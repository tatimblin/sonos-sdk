//! # Sonos Stream - Event Streaming and Subscription Management
//!
//! This crate provides a modern event streaming and subscription management system for Sonos devices.
//! It integrates with SonosClient's subscription lifecycle management, provides transparent event/polling
//! fallback, and exposes events through an optimized iterator interface designed for state management.
//!
//! ## Key Features
//!
//! - **Transparent Event/Polling Switching**: Automatically switches between UPnP events and polling
//!   based on firewall detection and event availability
//! - **Proactive Firewall Detection**: Uses existing callback-server infrastructure to immediately
//!   detect firewall blocking and start polling without delay
//! - **Optimal State Management**: Provides both sync and async iterator interfaces, with sync being
//!   best practice for local state management
//! - **Changes Only Pattern**: Events contain only deltas, consumers handle initial state via queries
//! - **Automatic Resync**: System detects state drift and emits resync events with full state
//! - **Resource Efficient**: Shares HTTP clients and connection pools across operations
//!
//! ## Basic Usage
//!
//! ```rust,no_run
//! use sonos_stream::{EventBroker, BrokerConfig, Service};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let mut broker = EventBroker::new(BrokerConfig::default()).await?;
//!
//! // Register speakers with automatic duplicate protection
//! let reg1 = broker.register_speaker_service("192.168.1.100".parse()?, Service::AVTransport)?;
//!
//! // Process events with optimal sync iterator for state management
//! let mut events = broker.event_iterator();
//! for event in events.iter() {
//!     // Handle events - transparent switching between UPnP events and polling
//!     println!("Event: {:?}", event);
//! }
//! # Ok(())
//! # }
//! ```
//!
//! ## Architecture
//!
//! The crate is structured around several key components:
//!
//! - [`EventBroker`] - Main interface for registration and event streaming
//! - [`registry`] - Speaker/service pair registration with duplicate protection
//! - [`subscription`] - Integration with SonosClient's ManagedSubscription lifecycle
//! - [`polling`] - Intelligent polling system with service-specific strategies
//! - [`events`] - Event processing, enrichment, and iterator interfaces

pub mod broker;
pub mod config;
pub mod error;
pub mod events;
pub mod polling;
pub mod registry;
pub mod subscription;

// Re-export main types for easy access
pub use broker::{EventBroker, PollingReason, RegistrationResult};
pub use config::BrokerConfig;
pub use error::{BrokerError, PollingError, RegistryError, SubscriptionError};
pub use events::types::{EnrichedEvent, EventData, EventSource, ResyncReason};
pub use events::iterator::EventIterator;
pub use registry::{RegistrationId, SpeakerServicePair};

// Re-export types from dependencies that users commonly need
pub use sonos_api::Service;
pub use callback_server::firewall_detection::FirewallStatus;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crate_exports() {
        // Ensure all main types are properly exported
        let _config = BrokerConfig::default();

        // Test that we can create RegistrationId and SpeakerServicePair
        let _reg_id = RegistrationId::new(1);
        let _pair = SpeakerServicePair {
            speaker_ip: "192.168.1.100".parse().unwrap(),
            service: Service::AVTransport,
        };
    }
}