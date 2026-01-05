//! Event receiver trait for pluggable event sources
//!
//! This module defines the `EventReceiver` trait which abstracts the event source,
//! allowing sonos-state to work with sonos-stream or any other event source.

use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use std::time::SystemTime;

/// An event that can be processed to update state
///
/// This is the unified event format that `EventReceiver` implementations
/// must produce. It abstracts the specific event format used by the
/// underlying event source (e.g., sonos-stream).
#[derive(Debug, Clone)]
pub struct StateEvent {
    /// IP address of the speaker that generated this event
    pub speaker_ip: IpAddr,

    /// Timestamp when this event occurred
    pub timestamp: SystemTime,

    /// The event payload
    pub payload: StateEventPayload,
}

impl StateEvent {
    /// Create a new StateEvent
    pub fn new(speaker_ip: IpAddr, payload: StateEventPayload) -> Self {
        Self {
            speaker_ip,
            timestamp: SystemTime::now(),
            payload,
        }
    }

    /// Create a StateEvent with a specific timestamp
    pub fn with_timestamp(
        speaker_ip: IpAddr,
        timestamp: SystemTime,
        payload: StateEventPayload,
    ) -> Self {
        Self {
            speaker_ip,
            timestamp,
            payload,
        }
    }
}

/// Event payload variants for state management
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StateEventPayload {
    /// AVTransport state (playback, track, position)
    Transport {
        /// Transport state (PLAYING, PAUSED_PLAYBACK, STOPPED, etc.)
        transport_state: Option<String>,
        /// Current track URI
        current_track_uri: Option<String>,
        /// Track duration (HH:MM:SS format)
        track_duration: Option<String>,
        /// Relative time position (HH:MM:SS format)
        rel_time: Option<String>,
        /// Track metadata (DIDL-Lite XML)
        track_metadata: Option<String>,
    },

    /// RenderingControl state (volume, mute)
    Rendering {
        /// Master volume (0-100)
        master_volume: Option<u8>,
        /// Master mute state
        master_mute: Option<bool>,
    },

    /// ZoneGroupTopology state (groups, speakers)
    Topology {
        /// Current zone groups
        zone_groups: Vec<TopologyZoneGroup>,
    },
}

/// Zone group information from topology events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopologyZoneGroup {
    /// Coordinator speaker UUID
    pub coordinator: String,
    /// Group ID
    pub id: String,
    /// Group members
    pub members: Vec<TopologyZoneMember>,
}

/// Zone member information from topology events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopologyZoneMember {
    /// Speaker UUID (RINCON_...)
    pub uuid: String,
    /// Speaker location URL
    pub location: String,
    /// Zone/room name
    pub zone_name: String,
    /// Software version
    pub software_version: String,
    /// Satellite speaker UUIDs
    pub satellites: Vec<String>,
}

/// Trait for event sources to implement
///
/// This allows sonos-state to be decoupled from specific event sources
/// like sonos-stream. Implementers convert their native event format
/// to `StateEvent` for processing.
///
/// # Example
///
/// ```rust,ignore
/// use sonos_state::{EventReceiver, StateEvent};
///
/// struct MyEventReceiver {
///     // Your event source...
/// }
///
/// impl EventReceiver for MyEventReceiver {
///     fn recv(&mut self) -> Option<StateEvent> {
///         // Convert your events to StateEvent
///         None
///     }
///
///     fn try_recv(&mut self) -> Option<StateEvent> {
///         None
///     }
///
///     fn is_connected(&self) -> bool {
///         true
///     }
/// }
/// ```
pub trait EventReceiver: Send {
    /// Receive the next event, blocking until one is available
    ///
    /// Returns `None` when the event source is closed/exhausted.
    fn recv(&mut self) -> Option<StateEvent>;

    /// Try to receive an event without blocking
    ///
    /// Returns `None` if no event is immediately available.
    fn try_recv(&mut self) -> Option<StateEvent>;

    /// Check if the receiver is still connected/valid
    fn is_connected(&self) -> bool;
}

/// A simple channel-based event receiver for testing and simple use cases
pub struct ChannelEventReceiver {
    receiver: std::sync::mpsc::Receiver<StateEvent>,
}

impl ChannelEventReceiver {
    /// Create a new channel-based event receiver
    pub fn new(receiver: std::sync::mpsc::Receiver<StateEvent>) -> Self {
        Self { receiver }
    }

    /// Create a channel pair (sender, receiver)
    pub fn channel() -> (std::sync::mpsc::Sender<StateEvent>, Self) {
        let (sender, receiver) = std::sync::mpsc::channel();
        (sender, Self { receiver })
    }
}

impl EventReceiver for ChannelEventReceiver {
    fn recv(&mut self) -> Option<StateEvent> {
        self.receiver.recv().ok()
    }

    fn try_recv(&mut self) -> Option<StateEvent> {
        self.receiver.try_recv().ok()
    }

    fn is_connected(&self) -> bool {
        // Check if channel is still open by trying a non-blocking receive
        // A disconnected channel returns Err(Disconnected), not Err(Empty)
        match self.receiver.try_recv() {
            Err(std::sync::mpsc::TryRecvError::Disconnected) => false,
            _ => true,
        }
    }
}

/// A mock event receiver for testing
#[cfg(test)]
pub struct MockEventReceiver {
    events: std::collections::VecDeque<StateEvent>,
    connected: bool,
}

#[cfg(test)]
impl MockEventReceiver {
    /// Create a new mock receiver with the given events
    pub fn new(events: Vec<StateEvent>) -> Self {
        Self {
            events: events.into_iter().collect(),
            connected: true,
        }
    }

    /// Disconnect the receiver
    pub fn disconnect(&mut self) {
        self.connected = false;
    }
}

#[cfg(test)]
impl EventReceiver for MockEventReceiver {
    fn recv(&mut self) -> Option<StateEvent> {
        if !self.connected {
            return None;
        }
        self.events.pop_front()
    }

    fn try_recv(&mut self) -> Option<StateEvent> {
        self.recv()
    }

    fn is_connected(&self) -> bool {
        self.connected && !self.events.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_state_event_creation() {
        let ip: IpAddr = "192.168.1.100".parse().unwrap();
        let payload = StateEventPayload::Rendering {
            master_volume: Some(50),
            master_mute: Some(false),
        };
        let event = StateEvent::new(ip, payload);

        assert_eq!(event.speaker_ip, ip);
        assert!(matches!(
            event.payload,
            StateEventPayload::Rendering { .. }
        ));
    }

    #[test]
    fn test_mock_receiver() {
        let ip: IpAddr = "192.168.1.100".parse().unwrap();
        let events = vec![
            StateEvent::new(
                ip,
                StateEventPayload::Rendering {
                    master_volume: Some(50),
                    master_mute: None,
                },
            ),
            StateEvent::new(
                ip,
                StateEventPayload::Rendering {
                    master_volume: Some(60),
                    master_mute: None,
                },
            ),
        ];

        let mut receiver = MockEventReceiver::new(events);

        assert!(receiver.is_connected());

        let event1 = receiver.recv();
        assert!(event1.is_some());

        let event2 = receiver.recv();
        assert!(event2.is_some());

        // No more events
        assert!(!receiver.is_connected());
    }

    #[test]
    fn test_channel_receiver() {
        let (sender, mut receiver) = ChannelEventReceiver::channel();
        let ip: IpAddr = "192.168.1.100".parse().unwrap();

        sender
            .send(StateEvent::new(
                ip,
                StateEventPayload::Rendering {
                    master_volume: Some(50),
                    master_mute: None,
                },
            ))
            .unwrap();

        let event = receiver.try_recv();
        assert!(event.is_some());
    }
}
