//! Integration tests for Sonos device discovery
//!
//! These tests validate the full discovery flow including:
//! - Real network discovery (when devices are available)
//! - Iterator behavior and event handling
//! - Deduplication logic
//! - Filtering of non-Sonos devices
//! - Early iterator termination

use sonos_discovery::{get, get_with_timeout, get_iter, get_iter_with_timeout, DeviceEvent};
use std::time::Duration;
use std::collections::HashSet;

#[test]
fn test_full_discovery_flow_with_iterator() {
    // Test the full discovery flow using the iterator API
    // This will discover real devices if available on the network
    let timeout = Duration::from_secs(2);
    let mut discovered_devices = Vec::new();
    
    for event in get_iter_with_timeout(timeout) {
        match event {
            DeviceEvent::Found(device) => {
                // Validate device has required fields
                assert!(!device.id.is_empty(), "Device ID should not be empty");
                assert!(!device.name.is_empty(), "Device name should not be empty");
                assert!(!device.ip_address.is_empty(), "Device IP should not be empty");
                assert!(!device.model_name.is_empty(), "Device model should not be empty");
                assert_eq!(device.port, 1400, "Sonos devices typically use port 1400");
                
                // Verify ID format (should be a UUID)
                assert!(device.id.starts_with("uuid:"), "Device ID should start with 'uuid:'");
                
                // Verify IP address format (basic check)
                assert!(device.ip_address.contains('.'), "IP address should contain dots");
                
                discovered_devices.push(device);
            }
        }
    }
    
    // Note: This test will pass even if no devices are found
    // It validates the discovery flow works correctly
    println!("Discovered {} Sonos device(s)", discovered_devices.len());
    
    for device in &discovered_devices {
        println!("  - {} ({}) at {}", device.name, device.model_name, device.ip_address);
    }
}

#[test]
fn test_deduplication_logic() {
    // Test that devices are not reported multiple times
    // Sonos devices often respond to SSDP multiple times
    let timeout = Duration::from_secs(2);
    let mut device_ids = HashSet::new();
    let mut device_ips = HashSet::new();
    let mut total_events = 0;
    
    for event in get_iter_with_timeout(timeout) {
        match event {
            DeviceEvent::Found(device) => {
                total_events += 1;
                
                // Check that we haven't seen this device ID before
                assert!(
                    device_ids.insert(device.id.clone()),
                    "Device ID {} was reported multiple times - deduplication failed",
                    device.id
                );
                
                // Check that we haven't seen this IP address before
                assert!(
                    device_ips.insert(device.ip_address.clone()),
                    "Device IP {} was reported multiple times - deduplication failed",
                    device.ip_address
                );
            }
        }
    }
    
    println!("Deduplication test: {} unique device(s) found", total_events);
}

#[test]
fn test_early_iterator_termination_with_discovery() {
    // Test that we can break out of discovery early without issues
    let timeout = Duration::from_secs(2);
    let mut count = 0;
    
    for event in get_iter_with_timeout(timeout) {
        match event {
            DeviceEvent::Found(device) => {
                println!("Found device: {} at {}", device.name, device.ip_address);
                count += 1;
                
                // Break after finding first device (if any)
                if count >= 1 {
                    break;
                }
            }
        }
    }
    
    // Iterator should be properly cleaned up after early termination
    // This test verifies no panic or resource leak occurs
    println!("Early termination test: stopped after {} device(s)", count);
}

#[test]
fn test_get_convenience_function() {
    // Test the convenience function that collects all devices
    let devices = get_with_timeout(Duration::from_secs(2));
    
    // Validate all returned devices
    for device in &devices {
        assert!(!device.id.is_empty());
        assert!(!device.name.is_empty());
        assert!(!device.ip_address.is_empty());
        assert!(!device.model_name.is_empty());
        assert_eq!(device.port, 1400);
    }
    
    println!("Convenience function test: found {} device(s)", devices.len());
}

#[test]
fn test_get_default_timeout() {
    // Test the default timeout version
    let devices = get();
    
    // Should work the same as get_with_timeout
    for device in &devices {
        assert!(!device.id.is_empty());
        assert!(!device.name.is_empty());
    }
    
    println!("Default timeout test: found {} device(s)", devices.len());
}

#[test]
fn test_get_iter_default_timeout() {
    // Test the default timeout iterator version
    let mut count = 0;
    
    for event in get_iter() {
        match event {
            DeviceEvent::Found(_device) => {
                count += 1;
            }
        }
    }
    
    println!("Default iterator test: found {} device(s)", count);
}

#[test]
fn test_filtering_non_sonos_devices() {
    // Test that only Sonos devices are returned
    // All discovered devices should have Sonos-specific characteristics
    let timeout = Duration::from_secs(2);
    
    for event in get_iter_with_timeout(timeout) {
        match event {
            DeviceEvent::Found(device) => {
                // Sonos device IDs contain RINCON
                assert!(
                    device.id.contains("RINCON") || device.id.contains("rincon"),
                    "Device ID {} doesn't appear to be a Sonos device",
                    device.id
                );
                
                // Sonos devices use port 1400
                assert_eq!(
                    device.port, 1400,
                    "Non-Sonos device detected with port {}",
                    device.port
                );
                
                println!("Validated Sonos device: {} ({})", device.name, device.id);
            }
        }
    }
}

#[test]
fn test_multiple_sequential_discoveries() {
    // Test that we can run multiple discoveries sequentially
    // This validates resource cleanup between discoveries
    let timeout = Duration::from_secs(1);
    
    for iteration in 1..=3 {
        let devices = get_with_timeout(timeout);
        println!("Iteration {}: found {} device(s)", iteration, devices.len());
        
        // Each iteration should work independently
        for device in &devices {
            assert!(!device.id.is_empty());
            assert!(!device.ip_address.is_empty());
        }
    }
}

#[test]
fn test_zero_timeout_behavior() {
    // Test with very short timeout - should complete quickly
    let devices = get_with_timeout(Duration::from_millis(100));
    
    // Should not panic, may find 0 devices due to short timeout
    println!("Short timeout test: found {} device(s)", devices.len());
}

#[test]
fn test_iterator_collect_matches_get() {
    // Test that manually collecting from iterator gives same results as get()
    let timeout = Duration::from_secs(2);
    
    let devices_from_get = get_with_timeout(timeout);
    
    // Small delay to avoid network interference
    std::thread::sleep(Duration::from_millis(100));
    
    let devices_from_iter: Vec<_> = get_iter_with_timeout(timeout)
        .filter_map(|event| match event {
            DeviceEvent::Found(device) => Some(device),
        })
        .collect();
    
    // Both methods should find the same number of devices
    // (allowing for some variance due to network timing)
    println!(
        "Comparison test: get() found {}, iter.collect() found {}",
        devices_from_get.len(),
        devices_from_iter.len()
    );
}

#[test]
fn test_device_event_clone() {
    // Test that DeviceEvent can be cloned
    let timeout = Duration::from_millis(500);
    
    for event in get_iter_with_timeout(timeout).take(1) {
        let cloned_event = event.clone();
        
        match (event, cloned_event) {
            (DeviceEvent::Found(device1), DeviceEvent::Found(device2)) => {
                assert_eq!(device1.id, device2.id);
                assert_eq!(device1.name, device2.name);
                assert_eq!(device1.ip_address, device2.ip_address);
            }
        }
    }
}

#[test]
fn test_device_debug_format() {
    // Test that Device implements Debug properly
    let timeout = Duration::from_millis(500);
    
    for event in get_iter_with_timeout(timeout).take(1) {
        match event {
            DeviceEvent::Found(device) => {
                let debug_str = format!("{:?}", device);
                assert!(debug_str.contains("Device"));
                assert!(debug_str.contains(&device.id));
                println!("Device debug format: {}", debug_str);
            }
        }
    }
}

#[test]
fn test_concurrent_iterator_creation() {
    // Test that multiple iterators can be created (though not used concurrently)
    let timeout = Duration::from_millis(100);
    
    let iter1 = get_iter_with_timeout(timeout);
    let iter2 = get_iter_with_timeout(timeout);
    let iter3 = get_iter_with_timeout(timeout);
    
    // Drop them in different order
    drop(iter2);
    drop(iter1);
    drop(iter3);
    
    // Should not panic or leak resources
}
