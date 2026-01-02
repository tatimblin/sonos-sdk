//! Declarative UPnP Operations Example
//!
//! This example demonstrates the new enhanced operation framework that provides
//! declarative, composable UPnP operations with minimal boilerplate.
//!
//! The new framework provides:
//! - Builder pattern for fluent operation construction
//! - Operation composition (sequencing, batching, conditional execution)
//! - Dual validation strategy (boundary vs comprehensive)
//! - Enhanced error handling with context
//! - Retry policies and timeout configuration
//!
//! Compare this to the existing cli_example.rs to see the dramatic reduction
//! in boilerplate and improved composability.

use sonos_api::{
    SonosClient, Result as SonosResult,
    services::{av_transport, rendering_control},
    operation::{ValidationLevel, RetryPolicy},
    error::{WithContext, OperationContext, BatchStatistics},
    BatchResult, ConditionalResult,
};
use std::time::Duration;

#[tokio::main]
async fn main() -> SonosResult<()> {
    let device_ip = "192.168.1.100"; // Replace with your Sonos device IP
    let client = SonosClient::new();

    println!("🎵 Sonos API Declarative Operations Demo");
    println!("=======================================\n");

    // Example 1: Simple Operation Execution
    println!("1. Simple Operation Execution");
    println!("------------------------------");

    // Get current transport info using builder pattern
    let get_info_op = av_transport::get_transport_info()
        .with_validation(ValidationLevel::Boundary)
        .with_timeout(Duration::from_secs(10))
        .build()?;

    match client.execute_enhanced(device_ip, get_info_op) {
        Ok(response) => {
            println!("✅ Current state: {}", response.current_transport_state);
            println!("   Status: {}", response.current_transport_status);
            println!("   Speed: {}\n", response.current_speed);
        }
        Err(e) => {
            println!("❌ Failed to get transport info: {}\n", e);
        }
    }

    // Example 2: Operation Sequencing
    println!("2. Operation Sequencing");
    println!("-----------------------");

    // Create a sequence: Set volume to 50, then start playing
    let sequence = rendering_control::set_volume(50, "Master".to_string())
        .with_validation(ValidationLevel::Comprehensive)
        .build()?
        .and_then(
            av_transport::play("1".to_string())
                .with_retry(RetryPolicy::fixed(3, Duration::from_millis(500)))
                .build()?
        );

    match client.execute_sequence(device_ip, sequence) {
        Ok(result) => {
            println!("✅ Sequence completed successfully!");
            println!("   Volume set and playback started\n");
        }
        Err(e) => {
            println!("❌ Sequence failed: {}\n", e);
        }
    }

    // Example 3: Batch Operations (Concurrent)
    println!("3. Batch Operations");
    println!("-------------------");

    // Get volume and transport info concurrently
    let batch = rendering_control::get_volume("Master".to_string())
        .build()?
        .concurrent_with(
            av_transport::get_transport_info()
                .with_timeout(Duration::from_secs(5))
                .build()?
        );

    match client.execute_batch(device_ip, batch) {
        Ok(BatchResult::Complete((volume_result, info_result))) => {
            match (volume_result, info_result) {
                (Ok(volume), Ok(info)) => {
                    println!("✅ Batch completed successfully!");
                    println!("   Volume: {}", volume.current_volume);
                    println!("   State: {}\n", info.current_transport_state);
                }
                _ => println!("❌ Some operations in batch failed\n"),
            }
        }
        Err(e) => {
            println!("❌ Batch failed: {}\n", e);
        }
    }

    // Example 4: Conditional Operations
    println!("4. Conditional Operations");
    println!("-------------------------");

    // Only pause if currently playing (using a simple condition for demo)
    let conditional = av_transport::pause()
        .build()?
        .condition(|| true); // In real usage, this would check actual device state

    match client.execute_conditional(device_ip, conditional) {
        Ok(result) => {
            match result {
                ConditionalResult::Executed(_) => {
                    println!("✅ Device was playing, paused successfully");
                }
                ConditionalResult::Skipped => {
                    println!("ℹ️  Device was not playing, no action taken");
                }
            }
        }
        Err(e) => {
            println!("❌ Conditional operation failed: {}\n", e);
        }
    }

    // Example 5: Enhanced Validation
    println!("5. Enhanced Validation");
    println!("----------------------");

    // Try to set an invalid volume (will be caught by validation)
    let invalid_volume_op = rendering_control::set_volume(150, "Master".to_string())
        .with_validation(ValidationLevel::Comprehensive)
        .build();

    match invalid_volume_op {
        Ok(_) => unreachable!("Should have failed validation"),
        Err(e) => {
            println!("✅ Validation correctly caught invalid volume: {}\n", e);
        }
    }

    // Example 6: Error Context and Statistics
    println!("6. Error Context and Statistics");
    println!("-------------------------------");

    // Demonstrate enhanced error handling with context
    let context = OperationContext::new(
        "demo_operation",
        "RenderingControl",
        "SetVolume"
    ).with_metadata("device_type", "PLAY:5");

    // Simulate an operation with context
    let result: SonosResult<()> = Err(
        sonos_api::error::ApiError::InvalidParameter("simulated error".to_string())
    );

    match result.with_context(context) {
        Ok(_) => println!("Operation succeeded"),
        Err((error, context)) => {
            println!("❌ Operation failed with context:");
            println!("   Operation: {}", context.operation_name);
            println!("   Service: {}", context.service);
            println!("   Action: {}", context.action);
            println!("   Error: {}", error);

            if let Some(device_type) = context.metadata.get("device_type") {
                println!("   Device Type: {}", device_type);
            }
        }
    }

    // Demonstrate batch statistics
    let stats = BatchStatistics::new(10, 7); // 7 out of 10 operations succeeded
    println!("\n📊 Batch Statistics:");
    println!("   Total operations: {}", stats.total_operations);
    println!("   Successful: {}", stats.successful_operations);
    println!("   Failed: {}", stats.failed_operations);
    println!("   Success rate: {:.1}%", stats.success_rate);

    if stats.is_partial_success() {
        println!("   Result: Partial success");
    }

    // Example 7: Complex Workflow
    println!("\n7. Complex Workflow");
    println!("-------------------");

    // Demonstrate a complex workflow combining multiple patterns
    let complex_workflow = async {
        // Step 1: Get current volume and state concurrently
        let initial_batch = rendering_control::get_volume("Master".to_string())
            .build()?
            .concurrent_with(av_transport::get_transport_info().build()?);

        let initial_batch_result = client.execute_batch(device_ip, initial_batch)?;

        if let BatchResult::Complete((volume_result, info_result)) = initial_batch_result {
            match (volume_result, info_result) {
                (Ok(volume), Ok(info)) => {
                    println!("📍 Initial state - Volume: {}, Transport: {}",
                            volume.current_volume, info.current_transport_state);

                    // Step 2: Conditional sequence based on current state
                    if info.current_transport_state == "STOPPED" {
                        let start_sequence = rendering_control::set_volume(30, "Master".to_string())
                            .with_validation(ValidationLevel::Comprehensive)
                            .build()?
                            .and_then(av_transport::play("1".to_string()).build()?);

                        client.execute_sequence(device_ip, start_sequence)?;
                        println!("🎵 Started playback at volume 30");
                    } else {
                        // Just adjust volume if already playing
                        let volume_op = rendering_control::set_volume(40, "Master".to_string())
                            .with_retry(RetryPolicy::exponential(3, Duration::from_millis(100)))
                            .build()?;

                        client.execute_enhanced(device_ip, volume_op)?;
                        println!("🔊 Adjusted volume to 40");
                    }
                }
                _ => println!("❌ Failed to get initial state"),
            }
        }

        Ok::<(), sonos_api::error::ApiError>(())
    };

    match complex_workflow.await {
        Ok(_) => println!("✅ Complex workflow completed successfully!"),
        Err(e) => println!("❌ Complex workflow failed: {}", e),
    }

    println!("\n🎉 Demo completed! This example shows how the new declarative");
    println!("   operation framework makes Sonos device control more intuitive,");
    println!("   composable, and maintainable compared to the traditional approach.\n");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sonos_api::services::{av_transport, rendering_control};
    use sonos_api::operation::ValidationLevel;

    #[test]
    fn test_operation_builder_compilation() {
        // Test that operations compile correctly with builder pattern
        let _play_op = av_transport::play("1".to_string())
            .with_validation(ValidationLevel::Boundary)
            .build()
            .expect("Should build play operation");

        let _volume_op = rendering_control::set_volume(50, "Master".to_string())
            .with_validation(ValidationLevel::Comprehensive)
            .build()
            .expect("Should build volume operation");
    }

    #[test]
    fn test_operation_composition() {
        // Test that operations can be composed
        let play_op = av_transport::play("1".to_string())
            .build()
            .expect("Should build play operation");

        let pause_op = av_transport::pause()
            .build()
            .expect("Should build pause operation");

        let _sequence = play_op.and_then(pause_op);
        // Just testing compilation for now
    }

    #[test]
    fn test_batch_statistics() {
        let stats = BatchStatistics::new(10, 7);
        assert_eq!(stats.total_operations, 10);
        assert_eq!(stats.successful_operations, 7);
        assert_eq!(stats.failed_operations, 3);
        assert_eq!(stats.success_rate, 70.0);
        assert!(stats.is_partial_success());
        assert!(!stats.is_complete_success());
        assert!(!stats.is_complete_failure());
    }

    #[test]
    fn test_operation_context() {
        let context = OperationContext::new("test_op", "TestService", "TestAction")
            .with_metadata("key", "value");

        assert_eq!(context.operation_name, "test_op");
        assert_eq!(context.service, "TestService");
        assert_eq!(context.action, "TestAction");
        assert_eq!(context.metadata.get("key"), Some(&"value".to_string()));
    }
}