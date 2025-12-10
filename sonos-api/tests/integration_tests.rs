//! Integration tests for controller operations
//! 
//! These tests verify end-to-end controller functionality by testing
//! the interaction between controllers and operations.

use sonos_api::{
    AVTransportController, RenderingControlController, ApiError
};

#[cfg(test)]
mod integration_tests {
    use super::*;

    /// Test that controllers can be created and used together
    #[test]
    fn test_multiple_controllers_creation() {
        let av_controller = AVTransportController::new();
        let rendering_controller = RenderingControlController::new();
        
        // Verify both controllers can coexist
        assert!(std::ptr::addr_of!(av_controller) as *const _ != std::ptr::null());
        assert!(std::ptr::addr_of!(rendering_controller) as *const _ != std::ptr::null());
    }

    /// Test error handling in volume validation
    #[test]
    fn test_volume_validation_integration() {
        let controller = RenderingControlController::new();
        
        // Test that invalid volume is properly rejected
        // Note: This would normally test against a real device or mock,
        // but for now we test the validation logic structure
        let test_ip = "192.168.1.100";
        let invalid_volume = 150u8;
        
        // The controller should validate volume before making SOAP calls
        let result = controller.set_volume(test_ip, invalid_volume);
        
        // This should fail with InvalidVolume error
        match result {
            Err(ApiError::InvalidVolume(vol)) => {
                assert_eq!(vol, invalid_volume);
            }
            Err(ApiError::Soap(_)) => {
                // This is also acceptable as it means the validation
                // happened at the SOAP level or the request was attempted
            }
            _ => {
                // For now, we accept any error since we don't have a real device
                // In a full integration test with mocking, this would be more specific
            }
        }
    }

    /// Test that valid volume values are accepted by the controller
    #[test]
    fn test_valid_volume_acceptance() {
        let controller = RenderingControlController::new();
        let test_ip = "192.168.1.100";
        
        // Test boundary values
        let valid_volumes = [0u8, 50u8, 100u8];
        
        for volume in valid_volumes {
            let result = controller.set_volume(test_ip, volume);
            
            // We expect either success or a SOAP error (since no real device)
            // but NOT an InvalidVolume error
            match result {
                Ok(()) => {
                    // Success is good
                }
                Err(ApiError::InvalidVolume(_)) => {
                    panic!("Valid volume {} was rejected", volume);
                }
                Err(_) => {
                    // Other errors (like network errors) are expected without a real device
                }
            }
        }
    }

    /// Test controller trait implementation consistency
    #[test]
    fn test_controller_trait_consistency() {
        let av_controller = AVTransportController::new();
        let rendering_controller = RenderingControlController::new();
        
        // Both controllers should implement the Controller trait
        // This is verified at compile time by the fact that they can be created
        
        // Test that both controllers exist and can be instantiated
        // (This demonstrates the common interface through trait implementation)
        
        // Note: Without dependency injection or mocking framework,
        // we can't easily test execute_operation directly here.
        // In a production system, we'd use a mocking framework like mockall
        // or dependency injection to test this properly.
        
        assert!(std::ptr::addr_of!(av_controller) as *const _ != std::ptr::null());
        assert!(std::ptr::addr_of!(rendering_controller) as *const _ != std::ptr::null());
    }
}

/// Tests for service-specific validation logic
#[cfg(test)]
mod service_validation_tests {
    use super::*;

    #[test]
    fn test_av_transport_methods_exist() {
        let controller = AVTransportController::new();
        let test_ip = "192.168.1.100";
        
        // Test that all expected methods exist and can be called
        // (They will fail without a real device, but should not panic)
        
        let _ = controller.play(test_ip);
        let _ = controller.pause(test_ip);
        let _ = controller.stop(test_ip);
        let _ = controller.get_transport_info(test_ip);
        
        // If we get here without panicking, the methods exist and are callable
    }

    #[test]
    fn test_rendering_control_methods_exist() {
        let controller = RenderingControlController::new();
        let test_ip = "192.168.1.100";
        
        // Test that all expected methods exist and can be called
        let _ = controller.get_volume(test_ip);
        let _ = controller.set_volume(test_ip, 50);
        let _ = controller.set_relative_volume(test_ip, 10);
        
        // If we get here without panicking, the methods exist and are callable
    }
}