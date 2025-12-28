//! Managed UPnP subscription with lifecycle management
//!
//! This module provides a higher-level subscription API that handles the complete
//! lifecycle of UPnP subscriptions with manual renewal and proper cleanup.

use crate::{ApiError, Result, Service};
use crate::operations::events::{
    SubscribeOperation, SubscribeRequest,
    UnsubscribeOperation, UnsubscribeRequest, UnsubscribeResponse,
    RenewOperation, RenewRequest, RenewResponse,
};
use soap_client::SoapClient;
use std::time::{Duration, SystemTime};
use std::sync::{Arc, Mutex};

/// A managed UPnP subscription with lifecycle management
///
/// This struct wraps the low-level subscription operations and provides:
/// - Expiration tracking
/// - Manual renewal with proper state updates
/// - Proper cleanup on drop
/// - Thread-safe state management
///
/// # Example
/// ```rust
/// use sonos_api::{SonosClient, Service};
///
/// let client = SonosClient::new();
/// let subscription = client.create_managed_subscription(
///     "192.168.1.100",
///     Service::AVTransport,
///     "http://192.168.1.50:8080/callback",
///     1800
/// )?;
///
/// // Check if renewal is needed
/// if subscription.needs_renewal() {
///     subscription.renew()?;
/// }
///
/// // Clean up when done
/// subscription.unsubscribe()?;
/// ```
#[derive(Debug)]
pub struct ManagedSubscription {
    /// UPnP subscription ID (SID) returned by the device
    sid: String,
    /// Device IP address
    device_ip: String,
    /// Service being subscribed to
    service: Service,
    /// Subscription state (protected by mutex)
    state: Arc<Mutex<SubscriptionState>>,
    /// SOAP client for making requests
    soap_client: SoapClient,
}

#[derive(Debug)]
struct SubscriptionState {
    /// When this subscription expires
    expires_at: SystemTime,
    /// Whether the subscription is currently active
    active: bool,
    /// Timeout duration for this subscription
    timeout_seconds: u32,
}

impl ManagedSubscription {
    /// Create a new managed subscription by performing the initial subscribe operation
    pub(crate) fn create(
        device_ip: String,
        service: Service,
        callback_url: String,
        timeout_seconds: u32,
        soap_client: SoapClient,
    ) -> Result<Self> {
        let request = SubscribeRequest {
            callback_url,
            timeout_seconds,
        };
        
        let response = SubscribeOperation::execute(&soap_client, &device_ip, service, &request)?;
        
        let state = SubscriptionState {
            expires_at: SystemTime::now() + Duration::from_secs(response.timeout_seconds as u64),
            active: true,
            timeout_seconds: response.timeout_seconds,
        };
        
        Ok(Self {
            sid: response.sid,
            device_ip,
            service,
            state: Arc::new(Mutex::new(state)),
            soap_client,
        })
    }
    
    /// Send a UPnP unsubscribe request (internal use only)
    fn unsubscribe_internal(
        soap_client: &SoapClient,
        device_ip: &str,
        service: Service,
        request: &UnsubscribeRequest,
    ) -> Result<UnsubscribeResponse> {
        UnsubscribeOperation::execute(soap_client, device_ip, service, request)
    }
    
    /// Send a UPnP renewal request (internal use only)
    fn renew_internal(
        soap_client: &SoapClient,
        device_ip: &str,
        service: Service,
        request: &RenewRequest,
    ) -> Result<RenewResponse> {
        RenewOperation::execute(soap_client, device_ip, service, request)
    }
    
    /// Get the subscription ID
    pub fn subscription_id(&self) -> &str {
        &self.sid
    }
    
    /// Check if the subscription is still active and not expired
    pub fn is_active(&self) -> bool {
        let state = self.state.lock().unwrap();
        state.active && SystemTime::now() < state.expires_at
    }
    
    /// Check if the subscription needs renewal
    ///
    /// Returns true if the subscription is active and will expire within
    /// the renewal threshold (5 minutes by default).
    pub fn needs_renewal(&self) -> bool {
        self.time_until_renewal().is_some()
    }
    
    /// Get the time until renewal is needed
    ///
    /// Returns `Some(duration)` if renewal is needed within the threshold,
    /// `None` if renewal is not needed or subscription is inactive.
    pub fn time_until_renewal(&self) -> Option<Duration> {
        let state = self.state.lock().unwrap();
        
        if !state.active {
            return None;
        }
        
        let now = SystemTime::now();
        if now >= state.expires_at {
            return Some(Duration::ZERO);
        }
        
        let time_until_expiry = state.expires_at.duration_since(now).ok()?;
        let renewal_threshold = Duration::from_secs(300); // 5 minutes
        
        if time_until_expiry <= renewal_threshold {
            Some(time_until_expiry)
        } else {
            None
        }
    }
    
    /// Get when the subscription expires
    pub fn expires_at(&self) -> SystemTime {
        let state = self.state.lock().unwrap();
        state.expires_at
    }
    
    /// Manually renew the subscription
    ///
    /// This sends a renewal request to the device and updates the internal
    /// expiration time based on the response.
    ///
    /// # Returns
    /// `Ok(())` if renewal succeeded, `Err(ApiError)` if it failed.
    ///
    /// # Errors
    /// - `ApiError::SubscriptionExpired` if the subscription has already expired
    /// - Network or device errors from the renewal request
    pub fn renew(&self) -> Result<()> {
        let current_timeout = {
            let state = self.state.lock().unwrap();
            if !state.active {
                return Err(ApiError::SubscriptionExpired);
            }
            state.timeout_seconds
        };
        
        let request = RenewRequest {
            sid: self.sid.clone(),
            timeout_seconds: current_timeout,
        };
        
        let response = Self::renew_internal(&self.soap_client, &self.device_ip, self.service, &request)?;
        
        // Update state with new expiration time
        {
            let mut state = self.state.lock().unwrap();
            state.expires_at = SystemTime::now() + Duration::from_secs(response.timeout_seconds as u64);
            state.timeout_seconds = response.timeout_seconds;
        }
        
        Ok(())
    }
    
    /// Unsubscribe and clean up the subscription
    ///
    /// This sends an unsubscribe request to the device and marks the
    /// subscription as inactive. After calling this method, the subscription
    /// should not be used for any further operations.
    ///
    /// # Returns
    /// `Ok(())` if unsubscribe succeeded, `Err(ApiError)` if it failed.
    /// Note that the subscription is marked inactive regardless of the result.
    pub fn unsubscribe(&self) -> Result<()> {
        // Mark as inactive first
        {
            let mut state = self.state.lock().unwrap();
            state.active = false;
        }
        
        // Send unsubscribe request
        let request = UnsubscribeRequest {
            sid: self.sid.clone(),
        };
        
        Self::unsubscribe_internal(&self.soap_client, &self.device_ip, self.service, &request).map(|_| ())
    }
}

impl Drop for ManagedSubscription {
    fn drop(&mut self) {
        // Mark as inactive
        if let Ok(mut state) = self.state.lock() {
            if state.active {
                state.active = false;
                
                // Attempt to unsubscribe, but don't panic if it fails
                let request = UnsubscribeRequest {
                    sid: self.sid.clone(),
                };
                
                if let Err(e) = Self::unsubscribe_internal(&self.soap_client, &self.device_ip, self.service, &request) {
                    eprintln!("⚠️  Failed to unsubscribe {} during drop: {}", self.sid, e);
                }
            }
        }
    }
}

