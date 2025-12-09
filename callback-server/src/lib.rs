//! Generic UPnP callback server for receiving event notifications.
//!
//! This crate provides a lightweight HTTP server for handling UPnP NOTIFY requests.
//! It is designed to be generic and has no knowledge of device-specific protocols.
//!
//! # Overview
//!
//! The callback server consists of three main components:
//!
//! - [`CallbackServer`]: HTTP server that binds to a local port and receives incoming
//!   UPnP event notifications via HTTP POST requests.
//! - [`EventRouter`]: Routes incoming events based on subscription IDs to registered
//!   handlers via channels.
//! - [`NotificationPayload`]: Generic data structure containing subscription ID and
//!   raw XML event body.
//!
//! # Architecture
//!
//! The callback server is designed to be a thin HTTP layer that:
//!
//! 1. Binds to an available port in a specified range
//! 2. Validates incoming UPnP NOTIFY requests
//! 3. Extracts subscription IDs and event XML
//! 4. Routes events to registered handlers via channels
//!
//! All device-specific logic (speaker IDs, service types, event parsing) should be
//! handled by the consuming crate through an adapter layer.
//!
//! # Example: Basic Usage
//!
//! ```no_run
//! use callback_server::{CallbackServer, NotificationPayload};
//! use tokio::sync::mpsc;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), String> {
//!     // Create a channel for receiving notifications
//!     let (tx, mut rx) = mpsc::unbounded_channel::<NotificationPayload>();
//!     
//!     // Create and start the callback server
//!     let server = CallbackServer::new((3400, 3500), tx).await?;
//!     
//!     println!("Callback server listening at: {}", server.base_url());
//!     
//!     // Register a subscription
//!     server.router().register("uuid:subscription-123".to_string()).await;
//!     
//!     // Handle incoming notifications
//!     tokio::spawn(async move {
//!         while let Some(notification) = rx.recv().await {
//!             println!("Received event for subscription: {}", notification.subscription_id);
//!             println!("Event XML: {}", notification.event_xml);
//!         }
//!     });
//!     
//!     // Server runs until shutdown is called
//!     // server.shutdown().await?;
//!     
//!     Ok(())
//! }
//! ```
//!
//! # Example: With Adapter Layer
//!
//! Device-specific crates should create an adapter layer that wraps the generic
//! types and adds domain-specific context:
//!
//! ```no_run
//! use callback_server::{CallbackServer, NotificationPayload};
//! use tokio::sync::mpsc;
//! use std::collections::HashMap;
//! use std::sync::Arc;
//! use tokio::sync::RwLock;
//!
//! // Device-specific event with additional context
//! #[derive(Debug, Clone)]
//! struct DeviceEvent {
//!     subscription_id: String,
//!     device_id: String,
//!     service_type: String,
//!     event_xml: String,
//! }
//!
//! #[tokio::main]
//! async fn main() -> Result<(), String> {
//!     // Create channels
//!     let (notification_tx, mut notification_rx) = mpsc::unbounded_channel::<NotificationPayload>();
//!     let (device_event_tx, mut device_event_rx) = mpsc::unbounded_channel::<DeviceEvent>();
//!     
//!     // Create callback server
//!     let server = CallbackServer::new((3400, 3500), notification_tx).await?;
//!     
//!     // Maintain mapping from subscription ID to device context
//!     let subscription_map: Arc<RwLock<HashMap<String, (String, String)>>> = 
//!         Arc::new(RwLock::new(HashMap::new()));
//!     
//!     // Spawn adapter task to add device-specific context
//!     let map_clone = subscription_map.clone();
//!     tokio::spawn(async move {
//!         while let Some(notification) = notification_rx.recv().await {
//!             let map = map_clone.read().await;
//!             if let Some((device_id, service_type)) = map.get(&notification.subscription_id) {
//!                 let device_event = DeviceEvent {
//!                     subscription_id: notification.subscription_id,
//!                     device_id: device_id.clone(),
//!                     service_type: service_type.clone(),
//!                     event_xml: notification.event_xml,
//!                 };
//!                 let _ = device_event_tx.send(device_event);
//!             }
//!         }
//!     });
//!     
//!     // Register subscription with device context
//!     let sub_id = "uuid:subscription-123".to_string();
//!     server.router().register(sub_id.clone()).await;
//!     subscription_map.write().await.insert(
//!         sub_id,
//!         ("device-001".to_string(), "AVTransport".to_string())
//!     );
//!     
//!     // Process device-specific events
//!     tokio::spawn(async move {
//!         while let Some(event) = device_event_rx.recv().await {
//!             println!("Device {} service {} event", event.device_id, event.service_type);
//!         }
//!     });
//!     
//!     Ok(())
//! }
//! ```
//!
//! # Private Workspace Crate
//!
//! This crate is intended for internal use within the workspace and is not published
//! to crates.io. It provides the foundation for device-specific event handling layers.

pub mod router;
mod server;

pub use router::{EventRouter, NotificationPayload};
pub use server::CallbackServer;
