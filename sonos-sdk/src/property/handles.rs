use std::net::IpAddr;
use std::sync::Arc;

use sonos_api::{
    services::{
        av_transport::{self, GetTransportInfoResponse},
        rendering_control::{self, GetVolumeResponse},
    },
    SonosClient,
};
use sonos_state::{PlaybackState, SpeakerId, StateManager, Volume};

use crate::SdkError;

/// Macro to generate property handles with minimal duplication
macro_rules! define_property_handle {
    (
        $(#[$attr:meta])*
        $handle_name:ident for $property_type:ty {
            operation: $operation_type:ty,
            request: $request_expr:expr,
            convert_response: $convert_expr:expr,
        }
    ) => {
        $(#[$attr])*
        #[derive(Clone)]
        pub struct $handle_name {
            speaker_id: SpeakerId,
            speaker_ip: IpAddr,
            state_manager: Arc<StateManager>,
            api_client: SonosClient,
        }

        impl $handle_name {
            pub fn new(
                speaker_id: SpeakerId,
                speaker_ip: IpAddr,
                state_manager: Arc<StateManager>,
                api_client: SonosClient,
            ) -> Self {
                Self {
                    speaker_id,
                    speaker_ip,
                    state_manager,
                    api_client,
                }
            }

            /// Get cached property value (sync)
            pub fn get(&self) -> Option<$property_type> {
                self.state_manager.get_property::<$property_type>(&self.speaker_id)
            }

            /// Fetch fresh value from device + update cache (sync)
            ///
            /// This makes a synchronous UPnP call to the device and updates
            /// the local state cache with the result.
            pub fn fetch(&self) -> Result<$property_type, SdkError> {
                // 1. Build operation using builder pattern
                let operation = $request_expr.build().map_err(|e| {
                    SdkError::ApiError(sonos_api::ApiError::ParseError(format!("Operation build failed: {}", e)))
                })?;

                // 2. Execute operation using enhanced API (sync call)
                let response = self.api_client
                    .execute_enhanced(&self.speaker_ip.to_string(), operation)
                    .map_err(SdkError::ApiError)?;

                // 3. Convert response to property type
                let property_value = $convert_expr(response);

                // 4. Update state store
                self.state_manager.set_property(&self.speaker_id, property_value.clone());

                Ok(property_value)
            }

            /// Start watching this property for changes (sync)
            ///
            /// Registers this property for change notifications. After calling
            /// `watch()`, changes to this property will appear in `system.iter()`.
            ///
            /// When an event manager is configured, this will automatically
            /// subscribe to the UPnP service for this property.
            ///
            /// Returns the current value if available.
            pub fn watch(&self) -> Result<Option<$property_type>, SdkError> {
                // Register for changes via state manager
                // This will also subscribe via the event manager if configured
                use sonos_state::property::Property;
                self.state_manager.register_watch(&self.speaker_id, <$property_type as Property>::KEY);

                // Return current cached value
                Ok(self.get())
            }

            /// Stop watching this property (sync)
            ///
            /// Unregisters this property from change notifications.
            /// When an event manager is configured, this will release
            /// the UPnP service subscription if no other watchers remain.
            pub fn unwatch(&self) {
                use sonos_state::property::Property;
                self.state_manager.unregister_watch(&self.speaker_id, <$property_type as Property>::KEY);
            }

            /// Check if this property is currently being watched
            pub fn is_watched(&self) -> bool {
                use sonos_state::property::Property;
                self.state_manager.is_watched(&self.speaker_id, <$property_type as Property>::KEY)
            }
        }
    };
}

// Generate all property handles using the macro
define_property_handle! {
    /// Handle for speaker volume (0-100)
    VolumeHandle for Volume {
        operation: GetVolumeOperation,
        request: rendering_control::get_volume_operation("Master".to_string()),
        convert_response: |response: GetVolumeResponse| Volume::new(response.current_volume),
    }
}

define_property_handle! {
    /// Handle for playback state (Playing/Paused/Stopped)
    PlaybackStateHandle for PlaybackState {
        operation: GetTransportInfoOperation,
        request: av_transport::get_transport_info_operation(),
        convert_response: |response: GetTransportInfoResponse| {
            match response.current_transport_state.as_str() {
                "PLAYING" => PlaybackState::Playing,
                "PAUSED" | "PAUSED_PLAYBACK" => PlaybackState::Paused,
                "STOPPED" => PlaybackState::Stopped,
                _ => PlaybackState::Transitioning,
            }
        },
    }
}

// TODO: Add more properties as needed:
// - Mute (requires GetMuteOperation to be implemented in sonos-api)
// - Position (requires GetPositionInfoOperation)
// - CurrentTrack (requires GetPositionInfoOperation or similar)
// - Bass, Treble, Loudness (require corresponding Get operations)
