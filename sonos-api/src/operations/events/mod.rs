//! UPnP event subscription operations
//! 
//! This module provides operations for managing UPnP event subscriptions
//! across all Sonos services. These operations handle the HTTP-based
//! subscription protocol rather than SOAP.

pub mod subscribe;
pub mod unsubscribe;
pub mod renew;

pub use subscribe::{SubscribeOperation, SubscribeRequest, SubscribeResponse};
pub use unsubscribe::{UnsubscribeOperation, UnsubscribeRequest, UnsubscribeResponse};
pub use renew::{RenewOperation, RenewRequest, RenewResponse};