//! High-level device controller for Sonos operations

use soap_client::SoapClient;
use crate::{Result, SonosOperation};
use crate::operations::av_transport::{
    PlayOperation, PlayRequest, PauseOperation, PauseRequest, 
    StopOperation, StopRequest, GetTransportInfoOperation, 
    GetTransportInfoRequest, GetTransportInfoResponse
};
use crate::operations::rendering_control::{
    GetVolumeOperation, GetVolumeRequest,
    SetVolumeOperation, SetVolumeRequest, SetRelativeVolumeOperation,
    SetRelativeVolumeRequest
};
use crate::ApiError;

/// Base trait for all service controllers
/// 
/// This trait provides a common interface for executing operations on Sonos devices.
/// All service-specific controllers implement this trait to provide a consistent API.
pub trait Controller {
    /// Execute a generic operation on a device
    /// 
    /// # Arguments
    /// * `ip` - The IP address of the target device
    /// * `request` - The typed request data for the operation
    /// 
    /// # Returns
    /// The typed response data or an error if the operation fails
    fn execute_operation<T: SonosOperation>(
        &self,
        ip: &str,
        request: T::Request,
    ) -> Result<T::Response>;
}

/// High-level controller for Sonos device operations
/// 
/// This controller provides a unified interface for controlling Sonos devices
/// by composing service-specific controllers and handling cross-service operations.
pub struct DeviceController {
    client: SoapClient,
}

impl DeviceController {
    /// Create a new device controller
    pub fn new() -> Self {
        Self {
            client: SoapClient::new(),
        }
    }
}

impl Controller for DeviceController {
    /// Execute a generic operation on a device
    fn execute_operation<T: SonosOperation>(
        &self,
        ip: &str,
        request: T::Request,
    ) -> Result<T::Response> {
        let service_info = T::SERVICE.info();
        
        // Build the payload for the operation
        let payload = T::build_payload(&request);
        
        // Execute the SOAP request
        let response_xml = self.client.call(
            ip,
            service_info.endpoint,
            service_info.service_uri,
            T::ACTION,
            &payload,
        )?;
        
        // Parse the response
        T::parse_response(&response_xml)
    }
}

impl DeviceController {
    /// Get access to the SOAP client for advanced operations
    pub fn client(&self) -> &SoapClient {
        &self.client
    }
}

/// Controller for AVTransport service operations
/// 
/// Provides high-level methods for controlling audio playback on Sonos devices.
/// This includes play, pause, stop operations and querying transport state.
pub struct AVTransportController {
    client: SoapClient,
}

impl AVTransportController {
    /// Create a new AVTransport controller
    pub fn new() -> Self {
        Self {
            client: SoapClient::new(),
        }
    }

    /// Play audio on the device
    /// 
    /// # Arguments
    /// * `ip` - The IP address of the target device
    /// 
    /// # Returns
    /// Ok(()) if successful, or an error if the operation fails
    /// 
    /// # Note
    /// This operation requires the device to be a group coordinator for grouped speakers
    pub fn play(&self, ip: &str) -> Result<()> {
        let request = PlayRequest {
            instance_id: 0,
            speed: "1".to_string(),
        };
        
        self.execute_operation::<PlayOperation>(ip, request)?;
        Ok(())
    }

    /// Pause audio on the device
    /// 
    /// # Arguments
    /// * `ip` - The IP address of the target device
    /// 
    /// # Returns
    /// Ok(()) if successful, or an error if the operation fails
    /// 
    /// # Note
    /// This operation requires the device to be a group coordinator for grouped speakers
    pub fn pause(&self, ip: &str) -> Result<()> {
        let request = PauseRequest {
            instance_id: 0,
        };
        
        self.execute_operation::<PauseOperation>(ip, request)?;
        Ok(())
    }

    /// Stop audio on the device
    /// 
    /// # Arguments
    /// * `ip` - The IP address of the target device
    /// 
    /// # Returns
    /// Ok(()) if successful, or an error if the operation fails
    /// 
    /// # Note
    /// This operation requires the device to be a group coordinator for grouped speakers
    pub fn stop(&self, ip: &str) -> Result<()> {
        let request = StopRequest {
            instance_id: 0,
        };
        
        self.execute_operation::<StopOperation>(ip, request)?;
        Ok(())
    }

    /// Get the current transport information from the device
    /// 
    /// # Arguments
    /// * `ip` - The IP address of the target device
    /// 
    /// # Returns
    /// The transport information including play state, status, and speed
    pub fn get_transport_info(&self, ip: &str) -> Result<GetTransportInfoResponse> {
        let request = GetTransportInfoRequest {
            instance_id: 0,
        };
        
        self.execute_operation::<GetTransportInfoOperation>(ip, request)
    }
}

impl Controller for AVTransportController {
    fn execute_operation<T: SonosOperation>(
        &self,
        ip: &str,
        request: T::Request,
    ) -> Result<T::Response> {
        let service_info = T::SERVICE.info();
        
        // Build the payload for the operation
        let payload = T::build_payload(&request);
        
        // Execute the SOAP request
        let response_xml = self.client.call(
            ip,
            service_info.endpoint,
            service_info.service_uri,
            T::ACTION,
            &payload,
        )?;
        
        // Parse the response
        T::parse_response(&response_xml)
    }
}

impl Default for AVTransportController {
    fn default() -> Self {
        Self::new()
    }
}

/// Controller for RenderingControl service operations
/// 
/// Provides high-level methods for controlling volume and audio rendering on Sonos devices.
/// This includes getting and setting volume levels.
pub struct RenderingControlController {
    client: SoapClient,
}

impl RenderingControlController {
    /// Create a new RenderingControl controller
    pub fn new() -> Self {
        Self {
            client: SoapClient::new(),
        }
    }

    /// Get the current volume of the device
    /// 
    /// # Arguments
    /// * `ip` - The IP address of the target device
    /// 
    /// # Returns
    /// The current volume level (0-100)
    pub fn get_volume(&self, ip: &str) -> Result<u8> {
        let request = GetVolumeRequest {
            instance_id: 0,
            channel: "Master".to_string(),
        };
        
        let response = self.execute_operation::<GetVolumeOperation>(ip, request)?;
        Ok(response.current_volume)
    }

    /// Set the volume of the device
    /// 
    /// # Arguments
    /// * `ip` - The IP address of the target device
    /// * `volume` - The desired volume level (0-100)
    /// 
    /// # Returns
    /// Ok(()) if successful, or an error if the operation fails
    /// 
    /// # Errors
    /// Returns `ApiError::InvalidVolume` if the volume is greater than 100
    pub fn set_volume(&self, ip: &str, volume: u8) -> Result<()> {
        if volume > 100 {
            return Err(ApiError::InvalidVolume(volume));
        }

        let request = SetVolumeRequest {
            instance_id: 0,
            channel: "Master".to_string(),
            desired_volume: volume,
        };
        
        self.execute_operation::<SetVolumeOperation>(ip, request)?;
        Ok(())
    }

    /// Adjust the volume of the device by a relative amount
    /// 
    /// # Arguments
    /// * `ip` - The IP address of the target device
    /// * `adjustment` - The volume adjustment (-100 to +100)
    /// 
    /// # Returns
    /// The new volume level after adjustment
    pub fn set_relative_volume(&self, ip: &str, adjustment: i8) -> Result<u8> {
        let request = SetRelativeVolumeRequest {
            instance_id: 0,
            channel: "Master".to_string(),
            adjustment,
        };
        
        let response = self.execute_operation::<SetRelativeVolumeOperation>(ip, request)?;
        Ok(response.new_volume)
    }
}

impl Controller for RenderingControlController {
    fn execute_operation<T: SonosOperation>(
        &self,
        ip: &str,
        request: T::Request,
    ) -> Result<T::Response> {
        let service_info = T::SERVICE.info();
        
        // Build the payload for the operation
        let payload = T::build_payload(&request);
        
        // Execute the SOAP request
        let response_xml = self.client.call(
            ip,
            service_info.endpoint,
            service_info.service_uri,
            T::ACTION,
            &payload,
        )?;
        
        // Parse the response
        T::parse_response(&response_xml)
    }
}

impl Default for RenderingControlController {
    fn default() -> Self {
        Self::new()
    }
}

/// Controller for GroupRenderingControl service operations
/// 
/// Provides high-level methods for controlling group-wide volume and audio rendering
/// on Sonos devices. This controller manages volume operations that affect entire
/// speaker groups rather than individual devices.
pub struct GroupRenderingControlController {
    client: SoapClient,
}

impl GroupRenderingControlController {
    /// Create a new GroupRenderingControl controller
    pub fn new() -> Self {
        Self {
            client: SoapClient::new(),
        }
    }

    // TODO: Add group volume operations when they are implemented
    // Example methods that will be added:
    // - get_group_volume()
    // - set_group_volume()
    // - set_group_relative_volume()
}

impl Controller for GroupRenderingControlController {
    fn execute_operation<T: SonosOperation>(
        &self,
        ip: &str,
        request: T::Request,
    ) -> Result<T::Response> {
        let service_info = T::SERVICE.info();
        
        // Build the payload for the operation
        let payload = T::build_payload(&request);
        
        // Execute the SOAP request
        let response_xml = self.client.call(
            ip,
            service_info.endpoint,
            service_info.service_uri,
            T::ACTION,
            &payload,
        )?;
        
        // Parse the response
        T::parse_response(&response_xml)
    }
}

impl Default for GroupRenderingControlController {
    fn default() -> Self {
        Self::new()
    }
}

/// Controller for ZoneGroupTopology service operations
/// 
/// Provides high-level methods for querying zone and group information on Sonos devices.
/// This includes getting zone group state, topology information, and speaker grouping details.
pub struct ZoneGroupTopologyController {
    client: SoapClient,
}

impl ZoneGroupTopologyController {
    /// Create a new ZoneGroupTopology controller
    pub fn new() -> Self {
        Self {
            client: SoapClient::new(),
        }
    }

    // TODO: Add zone group topology operations when they are implemented
    // Example methods that will be added:
    // - get_zone_group_state()
    // - get_zone_info()
    // - get_zone_attributes()
}

impl Controller for ZoneGroupTopologyController {
    fn execute_operation<T: SonosOperation>(
        &self,
        ip: &str,
        request: T::Request,
    ) -> Result<T::Response> {
        let service_info = T::SERVICE.info();
        
        // Build the payload for the operation
        let payload = T::build_payload(&request);
        
        // Execute the SOAP request
        let response_xml = self.client.call(
            ip,
            service_info.endpoint,
            service_info.service_uri,
            T::ACTION,
            &payload,
        )?;
        
        // Parse the response
        T::parse_response(&response_xml)
    }
}

impl Default for ZoneGroupTopologyController {
    fn default() -> Self {
        Self::new()
    }
}

/// Controller for DeviceProperties service operations
/// 
/// Provides high-level methods for querying device properties and metadata on Sonos devices.
/// This includes device information, capabilities, and configuration details.
pub struct DevicePropertiesController {
    client: SoapClient,
}

impl DevicePropertiesController {
    /// Create a new DeviceProperties controller
    pub fn new() -> Self {
        Self {
            client: SoapClient::new(),
        }
    }

    // TODO: Add device properties operations when they are implemented
    // Example methods that will be added:
    // - get_zone_info()
    // - get_zone_attributes()
    // - get_householdid()
}

impl Controller for DevicePropertiesController {
    fn execute_operation<T: SonosOperation>(
        &self,
        ip: &str,
        request: T::Request,
    ) -> Result<T::Response> {
        let service_info = T::SERVICE.info();
        
        // Build the payload for the operation
        let payload = T::build_payload(&request);
        
        // Execute the SOAP request
        let response_xml = self.client.call(
            ip,
            service_info.endpoint,
            service_info.service_uri,
            T::ACTION,
            &payload,
        )?;
        
        // Parse the response
        T::parse_response(&response_xml)
    }
}

impl Default for DevicePropertiesController {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for DeviceController {
    fn default() -> Self {
        Self::new()
    }
}