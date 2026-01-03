//! Speaker and service registration with duplicate protection
//!
//! This module provides thread-safe registration and management of speaker/service
//! pairs, ensuring that duplicate registrations are prevented and providing
//! efficient lookup capabilities.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::error::{RegistryError, RegistryResult};

/// Unique identifier for a speaker/service registration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RegistrationId(u64);

impl RegistrationId {
    /// Create a new RegistrationId with the given value
    pub fn new(id: u64) -> Self {
        Self(id)
    }

    /// Get the raw ID value
    pub fn as_u64(&self) -> u64 {
        self.0
    }
}

impl std::fmt::Display for RegistrationId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "reg-{}", self.0)
    }
}

/// A speaker and service type pair
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct SpeakerServicePair {
    /// IP address of the speaker
    pub speaker_ip: IpAddr,
    /// UPnP service type
    pub service: sonos_api::Service,
}

impl SpeakerServicePair {
    /// Create a new SpeakerServicePair
    pub fn new(speaker_ip: IpAddr, service: sonos_api::Service) -> Self {
        Self {
            speaker_ip,
            service,
        }
    }
}

impl std::fmt::Display for SpeakerServicePair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{:?}", self.speaker_ip, self.service)
    }
}

/// Thread-safe registry for speaker/service pairs with duplicate protection
///
/// This registry maintains bidirectional mappings between registration IDs and
/// speaker/service pairs, allowing for efficient lookups in both directions
/// while preventing duplicate registrations.
pub struct SpeakerServiceRegistry {
    /// Mapping from registration ID to speaker/service pair
    registrations: Arc<RwLock<HashMap<RegistrationId, SpeakerServicePair>>>,

    /// Reverse mapping from speaker/service pair to registration ID for duplicate detection
    pair_to_registration: Arc<RwLock<HashMap<SpeakerServicePair, RegistrationId>>>,

    /// Atomic counter for generating unique registration IDs
    next_id: Arc<AtomicU64>,

    /// Maximum number of registrations allowed
    max_registrations: usize,
}

impl SpeakerServiceRegistry {
    /// Create a new registry with the specified maximum registrations
    pub fn new(max_registrations: usize) -> Self {
        Self {
            registrations: Arc::new(RwLock::new(HashMap::new())),
            pair_to_registration: Arc::new(RwLock::new(HashMap::new())),
            next_id: Arc::new(AtomicU64::new(1)),
            max_registrations,
        }
    }

    /// Register a speaker/service pair, returning either a new or existing registration ID
    ///
    /// If the pair is already registered, returns the existing registration ID.
    /// If the pair is new, creates a new registration ID.
    ///
    /// # Arguments
    /// * `speaker_ip` - IP address of the speaker
    /// * `service` - UPnP service type
    ///
    /// # Returns
    /// * `Ok(RegistrationId)` - The registration ID (new or existing)
    /// * `Err(RegistryError)` - If registration fails
    pub async fn register(
        &self,
        speaker_ip: IpAddr,
        service: sonos_api::Service,
    ) -> RegistryResult<RegistrationId> {
        let pair = SpeakerServicePair::new(speaker_ip, service);

        // First check if this pair is already registered (read lock)
        {
            let pair_lookup = self.pair_to_registration.read().await;
            if let Some(existing_id) = pair_lookup.get(&pair) {
                return Ok(*existing_id);
            }
        }

        // Need to register new pair (write locks)
        let mut registrations = self.registrations.write().await;
        let mut pair_lookup = self.pair_to_registration.write().await;

        // Double-check in case another task registered it between locks
        if let Some(existing_id) = pair_lookup.get(&pair) {
            return Ok(*existing_id);
        }

        // Check registration limit
        if registrations.len() >= self.max_registrations {
            return Err(RegistryError::RegistryFull {
                max_registrations: self.max_registrations,
            });
        }

        // Generate new registration ID
        let registration_id = RegistrationId::new(self.next_id.fetch_add(1, Ordering::Relaxed));

        // Insert into both mappings
        registrations.insert(registration_id, pair.clone());
        pair_lookup.insert(pair, registration_id);

        Ok(registration_id)
    }

    /// Unregister a registration ID and return the associated pair
    ///
    /// # Arguments
    /// * `registration_id` - The registration ID to unregister
    ///
    /// # Returns
    /// * `Ok(SpeakerServicePair)` - The pair that was unregistered
    /// * `Err(RegistryError::NotFound)` - If the registration ID is not found
    pub async fn unregister(&self, registration_id: RegistrationId) -> RegistryResult<SpeakerServicePair> {
        let mut registrations = self.registrations.write().await;
        let mut pair_lookup = self.pair_to_registration.write().await;

        // Remove from primary mapping
        let pair = registrations
            .remove(&registration_id)
            .ok_or(RegistryError::NotFound(registration_id))?;

        // Remove from reverse mapping
        pair_lookup.remove(&pair);

        Ok(pair)
    }

    /// Check if a speaker/service pair is already registered
    ///
    /// # Arguments
    /// * `speaker_ip` - IP address of the speaker
    /// * `service` - UPnP service type
    ///
    /// # Returns
    /// * `true` if the pair is registered, `false` otherwise
    pub async fn is_registered(&self, speaker_ip: IpAddr, service: sonos_api::Service) -> bool {
        let pair = SpeakerServicePair::new(speaker_ip, service);
        let pair_lookup = self.pair_to_registration.read().await;
        pair_lookup.contains_key(&pair)
    }

    /// Get the registration ID for a speaker/service pair
    ///
    /// # Arguments
    /// * `speaker_ip` - IP address of the speaker
    /// * `service` - UPnP service type
    ///
    /// # Returns
    /// * `Some(RegistrationId)` if the pair is registered
    /// * `None` if the pair is not registered
    pub async fn get_registration_id(
        &self,
        speaker_ip: IpAddr,
        service: sonos_api::Service,
    ) -> Option<RegistrationId> {
        let pair = SpeakerServicePair::new(speaker_ip, service);
        let pair_lookup = self.pair_to_registration.read().await;
        pair_lookup.get(&pair).copied()
    }

    /// Get the speaker/service pair for a registration ID
    ///
    /// # Arguments
    /// * `registration_id` - The registration ID to look up
    ///
    /// # Returns
    /// * `Some(SpeakerServicePair)` if the registration ID is found
    /// * `None` if the registration ID is not found
    pub async fn get_pair(&self, registration_id: RegistrationId) -> Option<SpeakerServicePair> {
        let registrations = self.registrations.read().await;
        registrations.get(&registration_id).cloned()
    }

    /// List all current registrations
    ///
    /// # Returns
    /// A vector of (RegistrationId, SpeakerServicePair) tuples
    pub async fn list_registrations(&self) -> Vec<(RegistrationId, SpeakerServicePair)> {
        let registrations = self.registrations.read().await;
        registrations
            .iter()
            .map(|(id, pair)| (*id, pair.clone()))
            .collect()
    }

    /// Get the number of current registrations
    pub async fn count(&self) -> usize {
        let registrations = self.registrations.read().await;
        registrations.len()
    }

    /// Get the maximum number of registrations allowed
    pub fn max_registrations(&self) -> usize {
        self.max_registrations
    }

    /// Clear all registrations (useful for testing or shutdown)
    pub async fn clear(&self) {
        let mut registrations = self.registrations.write().await;
        let mut pair_lookup = self.pair_to_registration.write().await;
        registrations.clear();
        pair_lookup.clear();
    }

    /// Get statistics about the registry
    pub async fn stats(&self) -> RegistryStats {
        let registrations = self.registrations.read().await;
        let count = registrations.len();

        // Count services
        let mut service_counts = HashMap::new();
        for pair in registrations.values() {
            *service_counts.entry(pair.service).or_insert(0) += 1;
        }

        RegistryStats {
            total_registrations: count,
            max_registrations: self.max_registrations,
            service_breakdown: service_counts,
        }
    }
}

/// Statistics about the registry state
#[derive(Debug, Clone)]
pub struct RegistryStats {
    pub total_registrations: usize,
    pub max_registrations: usize,
    pub service_breakdown: HashMap<sonos_api::Service, usize>,
}

impl std::fmt::Display for RegistryStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Registry Stats:")?;
        writeln!(f, "  Total: {}/{}", self.total_registrations, self.max_registrations)?;
        writeln!(f, "  Service breakdown:")?;
        for (service, count) in &self.service_breakdown {
            writeln!(f, "    {:?}: {}", service, count)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_registration_basic() {
        let registry = SpeakerServiceRegistry::new(100);
        let ip: IpAddr = "192.168.1.100".parse().unwrap();
        let service = sonos_api::Service::AVTransport;

        // First registration should succeed
        let reg_id = registry.register(ip, service).await.unwrap();
        assert!(registry.is_registered(ip, service).await);
        assert_eq!(registry.count().await, 1);

        // Same pair should return same ID
        let reg_id2 = registry.register(ip, service).await.unwrap();
        assert_eq!(reg_id, reg_id2);
        assert_eq!(registry.count().await, 1); // Should not increase
    }

    #[tokio::test]
    async fn test_duplicate_detection() {
        let registry = SpeakerServiceRegistry::new(100);
        let ip: IpAddr = "192.168.1.100".parse().unwrap();
        let service = sonos_api::Service::AVTransport;

        let reg_id1 = registry.register(ip, service).await.unwrap();
        let reg_id2 = registry.register(ip, service).await.unwrap();

        assert_eq!(reg_id1, reg_id2);
        assert_eq!(registry.count().await, 1);
    }

    #[tokio::test]
    async fn test_different_services() {
        let registry = SpeakerServiceRegistry::new(100);
        let ip: IpAddr = "192.168.1.100".parse().unwrap();

        let av_reg = registry.register(ip, sonos_api::Service::AVTransport).await.unwrap();
        let rc_reg = registry.register(ip, sonos_api::Service::RenderingControl).await.unwrap();

        assert_ne!(av_reg, rc_reg);
        assert_eq!(registry.count().await, 2);
    }

    #[tokio::test]
    async fn test_unregistration() {
        let registry = SpeakerServiceRegistry::new(100);
        let ip: IpAddr = "192.168.1.100".parse().unwrap();
        let service = sonos_api::Service::AVTransport;

        let reg_id = registry.register(ip, service).await.unwrap();
        assert_eq!(registry.count().await, 1);

        let pair = registry.unregister(reg_id).await.unwrap();
        assert_eq!(pair.speaker_ip, ip);
        assert_eq!(pair.service, service);
        assert_eq!(registry.count().await, 0);
        assert!(!registry.is_registered(ip, service).await);
    }

    #[tokio::test]
    async fn test_registration_limit() {
        let registry = SpeakerServiceRegistry::new(2);
        let ip1: IpAddr = "192.168.1.100".parse().unwrap();
        let ip2: IpAddr = "192.168.1.101".parse().unwrap();
        let ip3: IpAddr = "192.168.1.102".parse().unwrap();
        let service = sonos_api::Service::AVTransport;

        // First two should succeed
        assert!(registry.register(ip1, service).await.is_ok());
        assert!(registry.register(ip2, service).await.is_ok());

        // Third should fail
        let result = registry.register(ip3, service).await;
        assert!(matches!(result, Err(RegistryError::RegistryFull { .. })));
    }

    #[tokio::test]
    async fn test_lookups() {
        let registry = SpeakerServiceRegistry::new(100);
        let ip: IpAddr = "192.168.1.100".parse().unwrap();
        let service = sonos_api::Service::AVTransport;

        let reg_id = registry.register(ip, service).await.unwrap();

        // Test bidirectional lookups
        assert_eq!(registry.get_registration_id(ip, service).await, Some(reg_id));
        assert_eq!(registry.get_pair(reg_id).await, Some(SpeakerServicePair::new(ip, service)));

        // Test non-existent lookups
        assert_eq!(registry.get_registration_id(ip, sonos_api::Service::RenderingControl).await, None);
        assert_eq!(registry.get_pair(RegistrationId::new(999)).await, None);
    }

    #[tokio::test]
    async fn test_list_and_stats() {
        let registry = SpeakerServiceRegistry::new(100);
        let ip: IpAddr = "192.168.1.100".parse().unwrap();

        let av_reg = registry.register(ip, sonos_api::Service::AVTransport).await.unwrap();
        let rc_reg = registry.register(ip, sonos_api::Service::RenderingControl).await.unwrap();

        let registrations = registry.list_registrations().await;
        assert_eq!(registrations.len(), 2);

        let stats = registry.stats().await;
        assert_eq!(stats.total_registrations, 2);
        assert_eq!(stats.service_breakdown.get(&sonos_api::Service::AVTransport), Some(&1));
        assert_eq!(stats.service_breakdown.get(&sonos_api::Service::RenderingControl), Some(&1));
    }

    #[tokio::test]
    async fn test_concurrent_access() {
        let registry = Arc::new(SpeakerServiceRegistry::new(100));
        let ip: IpAddr = "192.168.1.100".parse().unwrap();
        let service = sonos_api::Service::AVTransport;

        // Simulate concurrent registration attempts
        let handles: Vec<_> = (0..10)
            .map(|_| {
                let registry = Arc::clone(&registry);
                tokio::spawn(async move {
                    registry.register(ip, service).await
                })
            })
            .collect();

        let results: Vec<_> = futures::future::join_all(handles)
            .await
            .into_iter()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
            .into_iter()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        // All should return the same registration ID
        let first_id = results[0];
        for result in &results {
            assert_eq!(*result, first_id);
        }

        // Should only have one registration
        assert_eq!(registry.count().await, 1);
    }
}