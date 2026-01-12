use std::net::IpAddr;
use std::sync::Arc;
use sonos_state::{StateManager, SpeakerId, Volume, PlaybackState, PropertyWatcher};
use sonos_api::{
    SonosClient,
    services::{
        rendering_control::{self, GetVolumeResponse},
        av_transport::{self, GetTransportInfoResponse}
    }
};
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

            /// Get cached property value
            pub fn get(&self) -> Option<$property_type> {
                self.state_manager.get_property::<$property_type>(&self.speaker_id)
            }

            /// Fetch fresh value from device + update cache
            pub async fn fetch(&self) -> Result<$property_type, SdkError> {
                // 1. Build operation using builder pattern
                let operation = $request_expr.build().map_err(|e| {
                    SdkError::ApiError(sonos_api::ApiError::ParseError(format!("Operation build failed: {}", e)))
                })?;

                // 2. Execute operation using enhanced API
                let response = self.api_client
                    .execute_enhanced(&self.speaker_ip.to_string(), operation)
                    .map_err(SdkError::ApiError)?;

                // 3. Convert response to property type
                let property_value = $convert_expr(response);

                // 4. Update state store (triggers watchers)
                self.state_manager.update_property(&self.speaker_id, property_value.clone());

                Ok(property_value)
            }

            /// Watch property with UPnP event streaming
            pub async fn watch(&self) -> Result<PropertyWatcher<$property_type>, SdkError> {
                self.state_manager
                    .watch_property::<$property_type>(self.speaker_id.clone())
                    .await
                    .map_err(SdkError::StateError)
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
                "PAUSED" => PlaybackState::Paused,
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