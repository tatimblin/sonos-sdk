//! Speaker information type

use super::SpeakerId;
use serde::{Deserialize, Serialize};
use std::net::IpAddr;

/// Information about a Sonos speaker device
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Speaker {
    /// Unique speaker identifier
    pub id: SpeakerId,
    /// Friendly name of the speaker
    pub name: String,
    /// Room name where the speaker is located
    pub room_name: String,
    /// IP address of the speaker
    pub ip_address: IpAddr,
    /// Port number (typically 1400)
    pub port: u16,
    /// Model name (e.g., "Sonos One", "Sonos Play:1")
    pub model_name: String,
    /// Software/firmware version
    pub software_version: String,
    /// Satellite speaker IDs (for home theater setups)
    pub satellites: Vec<SpeakerId>,
}

impl Speaker {
    /// Get the speaker ID
    pub fn get_id(&self) -> &SpeakerId {
        &self.id
    }

    /// Get the full address (ip:port) for this speaker
    pub fn address(&self) -> String {
        format!("{}:{}", self.ip_address, self.port)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_speaker() -> Speaker {
        Speaker {
            id: SpeakerId::new("RINCON_123"),
            name: "Living Room".to_string(),
            room_name: "Living Room".to_string(),
            ip_address: "192.168.1.100".parse().unwrap(),
            port: 1400,
            model_name: "Sonos One".to_string(),
            software_version: "56.0-76060".to_string(),
            satellites: vec![],
        }
    }

    #[test]
    fn test_get_id() {
        let speaker = create_test_speaker();
        assert_eq!(speaker.get_id().as_str(), "RINCON_123");
    }

    #[test]
    fn test_address() {
        let speaker = create_test_speaker();
        assert_eq!(speaker.address(), "192.168.1.100:1400");
    }
}
