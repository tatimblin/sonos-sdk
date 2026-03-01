//! Example demonstrating the ManagedSubscription API
//!
//! This example shows how to use the new ManagedSubscription API that provides
//! automatic lifecycle management for UPnP subscriptions.

use sonos_api::{Result, Service, SonosClient};
use std::thread;
use std::time::Duration;

fn main() -> Result<()> {
    println!("🎵 Sonos ManagedSubscription Example");

    // Create a client
    let client = SonosClient::new();

    // Create a managed subscription
    // Note: This will fail without a real device, but demonstrates the API
    match client.create_managed_subscription(
        "192.168.1.100",
        Service::AVTransport,
        "http://192.168.1.50:8080/callback",
        1800, // 30 minutes
    ) {
        Ok(subscription) => {
            println!(
                "✅ Created subscription: {}",
                subscription.subscription_id()
            );

            // Check if it's active
            println!("📊 Subscription active: {}", subscription.is_active());

            // Check if renewal is needed
            if subscription.needs_renewal() {
                println!("🔄 Subscription needs renewal");

                // Renew it
                match subscription.renew() {
                    Ok(()) => println!("✅ Subscription renewed successfully"),
                    Err(e) => println!("❌ Failed to renew subscription: {e}"),
                }
            } else {
                println!("⏰ Subscription doesn't need renewal yet");
                if let Some(time_until) = subscription.time_until_renewal() {
                    println!("   Time until renewal needed: {time_until:?}");
                }
            }

            // Simulate some work
            println!("💤 Simulating work for 2 seconds...");
            thread::sleep(Duration::from_secs(2));

            // Clean up
            match subscription.unsubscribe() {
                Ok(()) => println!("✅ Unsubscribed successfully"),
                Err(e) => println!("❌ Failed to unsubscribe: {e}"),
            }

            // Check status after unsubscribe
            println!(
                "📊 Subscription active after unsubscribe: {}",
                subscription.is_active()
            );
        }
        Err(e) => {
            println!("❌ Failed to create subscription: {e}");
            println!("💡 This is expected without a real Sonos device");
        }
    }

    println!("🏁 Example completed");
    Ok(())
}
