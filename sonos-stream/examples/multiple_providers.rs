//! Example demonstrating multiple service providers registration.
//!
//! This example shows how to:
//! 1. Register multiple service providers (AVTransport, RenderingControl, etc.)
//! 2. Handle events from different service types in a unified stream
//! 3. Demonstrate the provider pattern's extensibility
//! 4. Show how each provider encapsulates its service-specific logic
//!
//! Note: This example uses AVTransportProvider (real implementation) and 
//! mock providers for other services to demonstrate the pattern without
//! requiring full implementations of all services.
//!
//! Run with: cargo run --example multiple_providers

use sonos_stream::{
    ServiceStrategy, EventBrokerBuilder, Event, ServiceType, Speaker, SpeakerId, 
    Subscription, SubscriptionScope, SubscriptionConfig, AVTransportProvider,
    StrategyError, SubscriptionError, TypedEvent,
};
use std::net::IpAddr;
use sonos_parser::services::av_transport::AVTransportParser;
use std::time::{Duration, SystemTime};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use async_trait::async_trait;

/// Mock RenderingControl provider for demonstration
#[derive(Debug, Clone)]
struct MockRenderingControlProvider {
    counter: Arc<AtomicU32>,
}

impl MockRenderingControlProvider {
    fn new() -> Self {
        Self {
            counter: Arc::new(AtomicU32::new(0)),
        }
    }
}

#[async_trait]
impl ServiceStrategy for MockRenderingControlProvider {
    fn service_type(&self) -> ServiceType {
        ServiceType::RenderingControl
    }

    fn subscription_scope(&self) -> SubscriptionScope {
        SubscriptionScope::PerSpeaker
    }

    fn service_endpoint_path(&self) -> &'static str {
        "/MediaRenderer/RenderingControl/Event"
    }

    fn parse_event(
        &self,
        _speaker_id: &SpeakerId,
        event_xml: &str,
    ) -> Result<TypedEvent, StrategyError> {
        // For demo purposes, create a simple mock event
        // In a real implementation, this would parse RenderingControl XML
        if event_xml.contains("Volume") {
            // Create a mock volume event (in real implementation, would use RenderingControlParser)
            let mock_data = format!("Volume event from XML: {}", event_xml.len());
            Ok(TypedEvent::new_parser(
                mock_data,
                "rendering_control_event",
                ServiceType::RenderingControl,
            ))
        } else {
            Err(StrategyError::EventParseFailed(
                "RenderingControl event parsing not fully implemented in demo".to_string()
            ))
        }
    }

    async fn create_subscription(
        &self,
        speaker: &Speaker,
        _callback_url: String,
        config: &SubscriptionConfig,
    ) -> Result<Box<dyn Subscription>, StrategyError> {
        let count = self.counter.fetch_add(1, Ordering::Relaxed);
        let subscription_id = format!("rc-mock-{}-{}", speaker.id.as_str(), count);
        
        Ok(Box::new(MockSubscription {
            id: subscription_id,
            speaker_id: speaker.id.clone(),
            service_type: self.service_type(),
            created_at: SystemTime::now(),
            timeout: Duration::from_secs(config.timeout_seconds as u64),
        }))
    }
}

/// Mock ZoneGroupTopology provider for demonstration
#[derive(Debug, Clone)]
struct MockZoneGroupTopologyProvider {
    counter: Arc<AtomicU32>,
}

impl MockZoneGroupTopologyProvider {
    fn new() -> Self {
        Self {
            counter: Arc::new(AtomicU32::new(0)),
        }
    }
}

#[async_trait]
impl ServiceStrategy for MockZoneGroupTopologyProvider {
    fn service_type(&self) -> ServiceType {
        ServiceType::ZoneGroupTopology
    }

    fn subscription_scope(&self) -> SubscriptionScope {
        SubscriptionScope::NetworkWide  // ZoneGroupTopology is typically global
    }

    fn service_endpoint_path(&self) -> &'static str {
        "/ZoneGroupTopology/Event"
    }

    fn parse_event(
        &self,
        _speaker_id: &SpeakerId,
        event_xml: &str,
    ) -> Result<TypedEvent, StrategyError> {
        // For demo purposes, create a simple mock event
        // In a real implementation, this would parse ZoneGroupTopology XML
        if event_xml.contains("ZoneGroup") {
            let mock_data = format!("Zone topology event from XML: {}", event_xml.len());
            Ok(TypedEvent::new_parser(
                mock_data,
                "zone_group_topology_event",
                ServiceType::ZoneGroupTopology,
            ))
        } else {
            Err(StrategyError::EventParseFailed(
                "ZoneGroupTopology event parsing not fully implemented in demo".to_string()
            ))
        }
    }

    async fn create_subscription(
        &self,
        speaker: &Speaker,
        _callback_url: String,
        config: &SubscriptionConfig,
    ) -> Result<Box<dyn Subscription>, StrategyError> {
        let count = self.counter.fetch_add(1, Ordering::Relaxed);
        let subscription_id = format!("zgt-mock-{}-{}", speaker.id.as_str(), count);
        
        Ok(Box::new(MockSubscription {
            id: subscription_id,
            speaker_id: speaker.id.clone(),
            service_type: self.service_type(),
            created_at: SystemTime::now(),
            timeout: Duration::from_secs(config.timeout_seconds as u64),
        }))
    }
}

/// Generic mock subscription for demo providers
struct MockSubscription {
    id: String,
    speaker_id: SpeakerId,
    service_type: ServiceType,
    created_at: SystemTime,
    timeout: Duration,
}

#[async_trait]
impl Subscription for MockSubscription {
    fn subscription_id(&self) -> &str {
        &self.id
    }

    async fn renew(&mut self) -> Result<(), SubscriptionError> {
        self.created_at = SystemTime::now();
        Ok(())
    }

    async fn unsubscribe(&mut self) -> Result<(), SubscriptionError> {
        Ok(())
    }

    fn is_active(&self) -> bool {
        self.created_at.elapsed().unwrap_or(Duration::MAX) < self.timeout
    }

    fn time_until_renewal(&self) -> Option<Duration> {
        let renewal_time = self.timeout / 2;
        let elapsed = self.created_at.elapsed().unwrap_or(Duration::ZERO);
        
        if elapsed < renewal_time {
            Some(renewal_time - elapsed)
        } else {
            Some(Duration::ZERO)
        }
    }

    fn speaker_id(&self) -> &SpeakerId {
        &self.speaker_id
    }

    fn service_type(&self) -> ServiceType {
        self.service_type
    }
}

/// Event processor that handles multiple service types
struct MultiServiceEventProcessor {
    av_transport_count: u32,
    rendering_control_count: u32,
    zone_group_topology_count: u32,
    parse_error_count: u32,
}

impl MultiServiceEventProcessor {
    fn new() -> Self {
        Self {
            av_transport_count: 0,
            rendering_control_count: 0,
            zone_group_topology_count: 0,
            parse_error_count: 0,
        }
    }

    fn process_event(&mut self, speaker_id: &SpeakerId, event: &TypedEvent) {
        println!("Processing event from speaker: {}", speaker_id.as_str());
        println!("  Event type: {}", event.event_type());
        println!("  Service type: {:?}", event.service_type());

        match event.service_type() {
            ServiceType::AVTransport => {
                self.process_av_transport_event(event);
                self.av_transport_count += 1;
            }
            ServiceType::RenderingControl => {
                self.process_rendering_control_event(event);
                self.rendering_control_count += 1;
            }
            ServiceType::ZoneGroupTopology => {
                self.process_zone_group_topology_event(event);
                self.zone_group_topology_count += 1;
            }
        }
        println!();
    }

    fn process_av_transport_event(&self, event: &TypedEvent) {
        if let Some(av_event) = event.downcast_ref::<AVTransportParser>() {
            println!("  ✓ AVTransport event (real provider):");
            println!("    Transport State: {}", av_event.transport_state());
            
            if let Some(track_uri) = av_event.current_track_uri() {
                println!("    Track URI: {}", track_uri);
            }
            
            if let Some(duration) = av_event.current_track_duration() {
                println!("    Duration: {}", duration);
            }

            match av_event.transport_state() {
                "PLAYING" => println!("    → Music is currently playing"),
                "PAUSED_PLAYBACK" => println!("    → Music is paused"),
                "STOPPED" => println!("    → Playback is stopped"),
                _ => println!("    → Transport state: {}", av_event.transport_state()),
            }
        } else {
            println!("  ✗ Failed to downcast AVTransport event");
        }
    }

    fn process_rendering_control_event(&self, event: &TypedEvent) {
        if let Some(mock_data) = event.downcast_ref::<String>() {
            println!("  ✓ RenderingControl event (mock provider):");
            println!("    Mock data: {}", mock_data);
            println!("    → This would contain volume, mute, and EQ settings in a real implementation");
        } else {
            println!("  ✗ Failed to downcast RenderingControl event");
        }
    }

    fn process_zone_group_topology_event(&self, event: &TypedEvent) {
        if let Some(mock_data) = event.downcast_ref::<String>() {
            println!("  ✓ ZoneGroupTopology event (mock provider):");
            println!("    Mock data: {}", mock_data);
            println!("    → This would contain speaker grouping information in a real implementation");
        } else {
            println!("  ✗ Failed to downcast ZoneGroupTopology event");
        }
    }

    fn process_parse_error(&mut self, speaker_id: &SpeakerId, service_type: ServiceType, error: &str) {
        println!("Parse error from speaker: {}", speaker_id.as_str());
        println!("  Service: {:?}", service_type);
        println!("  Error: {}", error);
        self.parse_error_count += 1;
        println!();
    }

    fn get_stats(&self) -> (u32, u32, u32, u32) {
        (
            self.av_transport_count,
            self.rendering_control_count,
            self.zone_group_topology_count,
            self.parse_error_count,
        )
    }
}

/// Simulate sending events for different service types
async fn simulate_multi_service_events(callback_url: String, av_subscription_id: String) {
    tokio::time::sleep(Duration::from_millis(500)).await;

    let client = reqwest::Client::new();
    
    // Simulate AVTransport events (these will be parsed by the real provider)
    let av_events = vec![
        ("<event><PLAYING>true</PLAYING></event>", "AVTransport: Started playing"),
        ("<event><PAUSED>true</PAUSED></event>", "AVTransport: Paused playback"),
        ("<event><PLAYING>true</PLAYING></event>", "AVTransport: Resumed playing"),
    ];

    for (xml, description) in av_events {
        tokio::time::sleep(Duration::from_millis(1000)).await;
        
        println!("📤 Simulating event: {}", description);
        
        let url = format!("{}/notify/{}", callback_url, av_subscription_id);
        let result = client
            .post(&url)
            .header("SID", av_subscription_id.clone())
            .header("NT", "upnp:event")
            .header("NTS", "upnp:propchange")
            .header("Content-Type", "text/xml")
            .body(xml.to_string())
            .send()
            .await;
            
        match result {
            Ok(response) => {
                println!("   ✓ Event sent successfully (status: {})", response.status());
            }
            Err(e) => {
                eprintln!("   ✗ Failed to send event: {}", e);
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Starting multiple service providers example...\n");

    // Create multiple providers
    let av_provider = AVTransportProvider::new();
    let rc_provider = MockRenderingControlProvider::new();
    let zgt_provider = MockZoneGroupTopologyProvider::new();

    println!("Registering multiple service providers:");
    println!("  • AVTransportProvider (real implementation)");
    println!("  • MockRenderingControlProvider (demo implementation)");
    println!("  • MockZoneGroupTopologyProvider (demo implementation)");
    println!();

    // Create broker with multiple providers
    let mut broker = EventBrokerBuilder::new()
        .with_strategy(Box::new(av_provider))
        .with_strategy(Box::new(rc_provider))
        .with_strategy(Box::new(zgt_provider))
        .with_port_range(42000, 42100)
        .build()
        .await?;

    // Create test speakers
    let speaker1 = Speaker::new(
        SpeakerId::new("RINCON_MULTI_EXAMPLE_1"),
        "127.0.0.1".parse::<IpAddr>()?,
        "Living Room".to_string(),
        "Living Room".to_string(),
    );

    let speaker2 = Speaker::new(
        SpeakerId::new("RINCON_MULTI_EXAMPLE_2"),
        "127.0.0.2".parse::<IpAddr>()?,
        "Kitchen".to_string(),
        "Kitchen".to_string(),
    );

    println!("Subscribing to multiple services for multiple speakers:");
    
    // Subscribe to AVTransport for both speakers
    println!("  Subscribing to AVTransport for {}", speaker1.name);
    broker.subscribe(&speaker1, ServiceType::AVTransport).await?;
    
    println!("  Subscribing to AVTransport for {}", speaker2.name);
    broker.subscribe(&speaker2, ServiceType::AVTransport).await?;

    // Subscribe to RenderingControl for both speakers
    println!("  Subscribing to RenderingControl for {}", speaker1.name);
    broker.subscribe(&speaker1, ServiceType::RenderingControl).await?;
    
    println!("  Subscribing to RenderingControl for {}", speaker2.name);
    broker.subscribe(&speaker2, ServiceType::RenderingControl).await?;

    // Subscribe to ZoneGroupTopology (global service, so just once)
    println!("  Subscribing to ZoneGroupTopology (global service)");
    broker.subscribe(&speaker1, ServiceType::ZoneGroupTopology).await?;

    // Get the event stream
    let mut event_stream = broker.event_stream();
    let mut processor = MultiServiceEventProcessor::new();

    println!("\nListening for events from multiple providers...\n");

    // Wait for subscription establishment events
    let mut established_count = 0;
    let expected_subscriptions = 5; // 2 AVTransport + 2 RenderingControl + 1 ZoneGroupTopology
    let mut av_subscription_id = String::new();

    while established_count < expected_subscriptions {
        if let Some(Event::SubscriptionEstablished { subscription_id, service_type, speaker_id, .. }) = event_stream.recv().await {
            println!("✓ Subscription established:");
            println!("  Speaker: {}", speaker_id.as_str());
            println!("  Service: {:?}", service_type);
            println!("  ID: {}", subscription_id);
            
            // Capture an AVTransport subscription ID for simulation
            if service_type == ServiceType::AVTransport && av_subscription_id.is_empty() {
                av_subscription_id = subscription_id;
            }
            
            established_count += 1;
            println!();
        }
    }

    let callback_url = format!("http://127.0.0.1:42000");

    // Spawn task to simulate events (only for AVTransport since others are mock)
    if !av_subscription_id.is_empty() {
        let sim_url = callback_url.clone();
        let sim_sub_id = av_subscription_id.clone();
        tokio::spawn(async move {
            simulate_multi_service_events(sim_url, sim_sub_id).await;
        });
    }

    // Process events with timeout
    let mut service_event_count = 0;
    let max_events = 3; // Wait for 3 service events then exit
    let timeout_duration = Duration::from_secs(15);

    println!("Processing events from multiple providers (timeout: {}s)...\n", timeout_duration.as_secs());

    let start_time = std::time::Instant::now();

    loop {
        let remaining_time = timeout_duration.saturating_sub(start_time.elapsed());
        if remaining_time.is_zero() {
            println!("⏰ Demo timeout reached.");
            break;
        }

        tokio::select! {
            Some(event) = event_stream.recv() => {
                match event {
                    Event::ServiceEvent { speaker_id, service_type: _, event, .. } => {
                        processor.process_event(&speaker_id, &event);
                        service_event_count += 1;
                        
                        if service_event_count >= max_events {
                            println!("Processed {} service events, exiting...\n", max_events);
                            break;
                        }
                    }
                    Event::ParseError { speaker_id, service_type, error, .. } => {
                        processor.process_parse_error(&speaker_id, service_type, &error);
                    }
                    _ => {
                        // Handle other event types as needed
                    }
                }
            }
            _ = tokio::time::sleep(remaining_time) => {
                println!("⏰ Demo timeout reached.");
                break;
            }
        }
    }

    // Show processing statistics
    let (av_count, rc_count, zgt_count, error_count) = processor.get_stats();
    println!("Processing Statistics:");
    println!("  AVTransport events: {}", av_count);
    println!("  RenderingControl events: {}", rc_count);
    println!("  ZoneGroupTopology events: {}", zgt_count);
    println!("  Parse errors: {}", error_count);
    println!();

    // Cleanup - unsubscribe from all services
    println!("Demo complete! Cleaning up subscriptions...");
    
    // Unsubscribe from all services for both speakers
    broker.unsubscribe(&speaker1, ServiceType::AVTransport).await?;
    broker.unsubscribe(&speaker2, ServiceType::AVTransport).await?;
    broker.unsubscribe(&speaker1, ServiceType::RenderingControl).await?;
    broker.unsubscribe(&speaker2, ServiceType::RenderingControl).await?;
    broker.unsubscribe(&speaker1, ServiceType::ZoneGroupTopology).await?;

    // Wait briefly for cleanup
    tokio::time::sleep(Duration::from_millis(200)).await;

    broker.shutdown().await?;
    println!("✓ Broker shut down");
    
    println!("\nThis example demonstrated:");
    println!("• Registering multiple service providers in a single broker");
    println!("• Each provider encapsulating its service-specific logic");
    println!("• Handling events from different service types in a unified stream");
    println!("• Real provider (AVTransportProvider) alongside mock providers");
    println!("• Different subscription scopes (per-speaker vs global)");
    println!("• Type-safe event processing for multiple service types");
    println!("• The extensibility of the provider pattern");

    Ok(())
}