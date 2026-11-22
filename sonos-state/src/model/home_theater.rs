//! Home theater satellites: surrounds and subs bonded to a main speaker.

use super::SpeakerId;
use serde::{Deserialize, Serialize};

/// The channel a satellite plays for its main speaker.
///
/// Read from the `HTSatChanMapSet` topology attribute, where the main speaker
/// plays `LF,RF`, the surrounds `LR` and `RR`, and a sub `SW`. Variants are
/// declared in display order, so sorting satellites lists the left surround,
/// then the right, then the sub.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum SatelliteRole {
    /// Left rear surround (`LR`).
    LeftSurround,
    /// Right rear surround (`RR`).
    RightSurround,
    /// Subwoofer (`SW`).
    Sub,
    /// A channel code this version doesn't recognise, kept verbatim (empty
    /// when the topology gave none).
    Other(String),
}

impl SatelliteRole {
    /// Map a channel code from `HTSatChanMapSet` to a role.
    pub fn from_channels(channels: &str) -> Self {
        match channels {
            "LR" => Self::LeftSurround,
            "RR" => Self::RightSurround,
            "SW" => Self::Sub,
            other => Self::Other(other.to_string()),
        }
    }

    /// A short human-readable label, such as "Left surround".
    pub fn label(&self) -> &str {
        match self {
            Self::LeftSurround => "Left surround",
            Self::RightSurround => "Right surround",
            Self::Sub => "Sub",
            Self::Other(_) => "Satellite",
        }
    }

    /// Whether this is a surround (left or right).
    pub fn is_surround(&self) -> bool {
        matches!(self, Self::LeftSurround | Self::RightSurround)
    }
}

/// A satellite bonded to a main speaker, as reported by the topology.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SatelliteMember {
    /// The satellite's own speaker ID.
    pub id: SpeakerId,
    /// The channel it plays.
    pub role: SatelliteRole,
}

impl SatelliteMember {
    pub fn new(id: SpeakerId, role: SatelliteRole) -> Self {
        Self { id, role }
    }
}

/// Find `uuid`'s channels in an `HTSatChanMapSet` value.
///
/// The value is `;`-separated `UUID:CHANNELS` entries, for example
/// `RINCON_A:LF,RF;RINCON_B:LR;RINCON_C:RR`. Returns `None` when `uuid` has no
/// entry.
pub(crate) fn channels_for<'a>(chan_map_set: &'a str, uuid: &str) -> Option<&'a str> {
    chan_map_set.split(';').find_map(|entry| {
        let (id, channels) = entry.split_once(':')?;
        (id.trim() == uuid).then(|| channels.trim())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Basement's live map (Playbar + two Sonos One surrounds, no sub).
    const BASEMENT: &str =
        "RINCON_5CAAFDAE58BD01400:LF,RF;RINCON_7828CA128F0001400:LR;RINCON_7828CAFB9D9C01400:RR";

    #[test]
    fn channels_for_finds_each_entry() {
        assert_eq!(
            channels_for(BASEMENT, "RINCON_5CAAFDAE58BD01400"),
            Some("LF,RF")
        );
        assert_eq!(
            channels_for(BASEMENT, "RINCON_7828CA128F0001400"),
            Some("LR")
        );
        assert_eq!(
            channels_for(BASEMENT, "RINCON_7828CAFB9D9C01400"),
            Some("RR")
        );
        assert_eq!(channels_for(BASEMENT, "RINCON_MISSING"), None);
        assert_eq!(channels_for("", "RINCON_A"), None);
    }

    #[test]
    fn roles_map_from_channel_codes() {
        assert_eq!(
            SatelliteRole::from_channels("LR"),
            SatelliteRole::LeftSurround
        );
        assert_eq!(
            SatelliteRole::from_channels("RR"),
            SatelliteRole::RightSurround
        );
        assert_eq!(SatelliteRole::from_channels("SW"), SatelliteRole::Sub);
        assert_eq!(
            SatelliteRole::from_channels("LH"),
            SatelliteRole::Other("LH".to_string())
        );
    }

    #[test]
    fn roles_sort_in_display_order() {
        let mut roles = vec![
            SatelliteRole::Sub,
            SatelliteRole::Other(String::new()),
            SatelliteRole::RightSurround,
            SatelliteRole::LeftSurround,
        ];
        roles.sort();
        assert_eq!(
            roles,
            vec![
                SatelliteRole::LeftSurround,
                SatelliteRole::RightSurround,
                SatelliteRole::Sub,
                SatelliteRole::Other(String::new()),
            ]
        );
    }
}
