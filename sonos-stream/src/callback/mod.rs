//! HTTP callback server for receiving UPnP event notifications.
//!
//! This module provides the infrastructure for receiving UPnP event notifications
//! from Sonos devices via HTTP callbacks. It consists of two main components:
//!
//! - `CallbackServer`: HTTP server that binds to a local port and receives incoming
//!   event notifications from Sonos devices
//! - `EventRouter`: Routes incoming events to the appropriate handlers based on
//!   subscription ID mappings
//!
//! # Architecture
//!
//! The callback module follows a separation of concerns:
//!
//! - **server.rs**: Handles HTTP server lifecycle, port binding, IP detection,
//!   and graceful shutdown
//! - **router.rs**: Manages subscription-to-handler mappings and routes incoming
//!   events to the event processor
//!
//! # Usage
//!
//! The callback server is typically created by the `EventBrokerBuilder` and is
//! not intended to be used directly by end users. It automatically:
//!
//! 1. Finds an available port in the configured range (default 3400-3500)
//! 2. Detects the local IP address for callback URLs
//! 3. Starts an HTTP server to receive UPnP NOTIFY requests
//! 4. Routes events to the event processor for parsing
//!
//! # Example
//!
//! ```no_run
//! use tokio::sync::mpsc;
//! use sonos_stream::CallbackServer;
//!
//! # async fn example() -> Result<(), String> {
//! let (event_tx, event_rx) = mpsc::unbounded_channel();
//! let server = CallbackServer::new((3400, 3500), event_tx).await?;
//!
//! println!("Callback server running at: {}", server.base_url());
//!
//! // Register subscriptions, handle events...
//!
//! server.shutdown().await?;
//! # Ok(())
//! # }
//! ```

mod router;
mod server;

pub use router::{EventRouter, RawEvent};
pub use server::CallbackServer;
