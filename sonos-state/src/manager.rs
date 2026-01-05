//! State manager - main orchestrator for state management

use std::net::IpAddr;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use crate::{
    cache::StateSnapshot, EventReceiver, Result, StateCache, StateChange, StateError,
    StateProcessor,
};

/// Configuration for StateManager
#[derive(Debug, Clone)]
pub struct StateManagerConfig {
    /// Number of state change events to buffer (channel capacity)
    pub change_buffer_size: usize,
}

impl Default for StateManagerConfig {
    fn default() -> Self {
        Self {
            change_buffer_size: 1000,
        }
    }
}

/// Main state manager that coordinates initialization, event processing, and change emission
///
/// The StateManager is the primary interface for managing Sonos device state.
/// It handles:
/// - Initialization from a single speaker IP
/// - Event processing from any EventReceiver implementation
/// - Emitting StateChange events for state updates
///
/// # Example
///
/// ```rust,ignore
/// use sonos_state::{StateManager, StateManagerConfig};
///
/// let mut manager = StateManager::new(StateManagerConfig::default());
///
/// // Initialize from a speaker IP
/// manager.initialize_from_ip("192.168.1.100".parse().unwrap())?;
///
/// // Get current state
/// let snapshot = manager.snapshot();
/// println!("Found {} speakers", snapshot.speaker_count());
///
/// // Start processing events
/// let receiver = // ... your EventReceiver implementation
/// manager.start_processing(receiver)?;
///
/// // Consume state changes
/// let changes = manager.take_change_receiver().unwrap();
/// while let Ok(change) = changes.recv() {
///     println!("State changed: {:?}", change);
/// }
/// ```
pub struct StateManager {
    cache: StateCache,
    change_sender: Sender<StateChange>,
    change_receiver: Option<Receiver<StateChange>>,
    processing_thread: Option<thread::JoinHandle<()>>,
    _config: StateManagerConfig,
}

impl StateManager {
    /// Create a new StateManager with the given configuration
    pub fn new(config: StateManagerConfig) -> Self {
        let cache = StateCache::new();
        let (change_sender, change_receiver) = mpsc::channel();

        Self {
            cache,
            change_sender,
            change_receiver: Some(change_receiver),
            processing_thread: None,
            _config: config,
        }
    }

    /// Initialize state from a single speaker IP
    ///
    /// This queries the topology from the given speaker and discovers all
    /// speakers and groups in the Sonos network.
    ///
    /// # Arguments
    ///
    /// * `ip` - IP address of any Sonos speaker in the network
    pub fn initialize_from_ip(&mut self, ip: IpAddr) -> Result<()> {
        let (speakers, groups) = crate::init::initialize_from_ip(ip)?;

        let speaker_count = speakers.len();
        let group_count = groups.len();

        // Initialize cache
        self.cache.initialize(speakers, groups);

        // Emit initialization event
        let _ = self.change_sender.send(StateChange::StateInitialized {
            speaker_count,
            group_count,
        });

        Ok(())
    }

    /// Start processing events from an EventReceiver
    ///
    /// This spawns a background thread that processes events and emits changes.
    /// Only one processing thread can be active at a time.
    ///
    /// # Arguments
    ///
    /// * `event_receiver` - The event source to process events from
    pub fn start_processing<E: EventReceiver + 'static>(&mut self, mut event_receiver: E) -> Result<()> {
        if self.processing_thread.is_some() {
            return Err(StateError::AlreadyRunning);
        }

        let change_sender = self.change_sender.clone();
        let cache = self.cache.clone();

        let handle = thread::spawn(move || {
            let mut processor = StateProcessor::new(cache);
            processor.rebuild_ip_mapping();

            while event_receiver.is_connected() {
                if let Some(event) = event_receiver.recv() {
                    let changes = processor.process_event(event);
                    for change in changes {
                        if change_sender.send(change).is_err() {
                            // Receiver dropped, stop processing
                            return;
                        }
                    }
                }
            }
        });

        self.processing_thread = Some(handle);
        Ok(())
    }

    /// Process a single event immediately (without background thread)
    ///
    /// This is useful for testing or when you want manual control over
    /// event processing.
    pub fn process_event(&self, event: crate::StateEvent) -> Vec<StateChange> {
        let mut processor = StateProcessor::new(self.cache.clone());
        processor.rebuild_ip_mapping();
        processor.process_event(event)
    }

    /// Get the change receiver for consuming state changes
    ///
    /// Can only be called once - moves ownership of the receiver.
    /// Returns `None` if already taken.
    pub fn take_change_receiver(&mut self) -> Option<Receiver<StateChange>> {
        self.change_receiver.take()
    }

    /// Get a snapshot of the current state
    pub fn snapshot(&self) -> StateSnapshot {
        self.cache.snapshot()
    }

    /// Get a reference to the underlying cache for direct queries
    pub fn cache(&self) -> &StateCache {
        &self.cache
    }

    /// Check if the processing thread is running
    pub fn is_processing(&self) -> bool {
        self.processing_thread.is_some()
    }

    /// Stop processing and clean up
    ///
    /// This will close the change channel and wait for the processing
    /// thread to finish.
    pub fn shutdown(self) -> Result<()> {
        // Drop the sender to signal shutdown
        drop(self.change_sender);

        if let Some(handle) = self.processing_thread {
            handle.join().map_err(|_| StateError::ShutdownFailed)?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Speaker, SpeakerId};
    use std::net::IpAddr;

    fn create_test_speaker() -> Speaker {
        Speaker {
            id: SpeakerId::new("RINCON_123"),
            name: "Test Speaker".to_string(),
            room_name: "Test Room".to_string(),
            ip_address: "192.168.1.100".parse::<IpAddr>().unwrap(),
            port: 1400,
            model_name: "Sonos One".to_string(),
            software_version: "56.0".to_string(),
            satellites: vec![],
        }
    }

    #[test]
    fn test_new() {
        let manager = StateManager::new(StateManagerConfig::default());
        assert!(manager.snapshot().is_empty());
        assert!(!manager.is_processing());
    }

    #[test]
    fn test_manual_initialization() {
        let manager = StateManager::new(StateManagerConfig::default());

        // Manually initialize cache for testing
        manager.cache.initialize(vec![create_test_speaker()], vec![]);

        assert_eq!(manager.snapshot().speaker_count(), 1);
    }

    #[test]
    fn test_take_change_receiver() {
        let mut manager = StateManager::new(StateManagerConfig::default());

        let receiver1 = manager.take_change_receiver();
        assert!(receiver1.is_some());

        let receiver2 = manager.take_change_receiver();
        assert!(receiver2.is_none());
    }

    #[test]
    fn test_process_event() {
        let manager = StateManager::new(StateManagerConfig::default());

        // Initialize with a speaker
        manager.cache.initialize(vec![create_test_speaker()], vec![]);

        // Process a volume event
        let event = crate::StateEvent::new(
            "192.168.1.100".parse().unwrap(),
            crate::StateEventPayload::Rendering {
                master_volume: Some(50),
                master_mute: None,
            },
        );

        let changes = manager.process_event(event);

        // Should have a volume change
        assert_eq!(changes.len(), 1);
        assert!(matches!(changes[0], StateChange::VolumeChanged { .. }));
    }
}
