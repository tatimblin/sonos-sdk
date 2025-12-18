//! Advanced example demonstrating type-safe event handling with TypedEvent.
//!
//! This example shows how to:
//! 1. Handle multiple event types safely using downcasting
//! 2. Create custom event processors for different service types
//! 3. Implement event filtering and routing based on event types
//! 4. Demonstrate error handling for invalid downcasts
//!
//! Run with: cargo run --example typed_event_handling

use sonos_stream::{
    BaseStrategy, EventBrokerBuilder, Event, ServiceType, Speaker, SpeakerId, 
    Subscription, SubscriptionScope, SubscriptionConfig,
    StrategyError, SubscriptionError, TypedEvent,
};
use std::net::IpAddr;
use sonos_parser::services::av_transport::AVTransportParser;
use std::time::{Duration, SystemTime};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use async_trait::async_trait;

/// Event processor that handles different event types with type safety
struct TypedEventProcessor {
    av_transport_count: u32,
    unknown_count: u32,
}

impl TypedEventProcessor {
    fn new() -> Self {
        Self {
            av_transport_count: 0,
            unknown_count: 0,
        }
    }

    /// Process a typed event with type-safe downcasting
    fn process_event(&mut self, speaker_id: &SpeakerId, event: &TypedEvent) {
        println!("Processing event from speaker: {}", speaker_id.as_str());
        println!("  Event type: {}", event.event_type());
        println!("  Service type: {:?}", event.service_type());

        // Demonstrate type-safe event handling
        match event.service_type() {
            ServiceType::AVTransport => {
                self.process_av_transport_event(event);
                self.av_transport_count += 1;
            }
            ServiceType::RenderingControl => {
                self.process_rendering_control_event(event);
            }
            ServiceType::ZoneGroupTopology => {
                self.process_zone_group_event(event);
            }
        }
        println!();
    }

    /// Process AVTransport events with full type safety
    fn process_av_transport_event(&self, event: &TypedEvent) {
        if let Some(av_event) = event.downcast_ref::<AVTransportParser>() {
            println!("  ✓ Successfully downcast to AVTransportParser");
            println!("    Transport State: {}", av_event.transport_state());
            
            // Demonstrate accessing optional fields safely
            if let Some(track_uri) = av_event.current_track_uri() {
                println!("    Track URI: {}", track_uri);
            }
            
            if let Some(duration) = av_event.current_track_duration() {
                println!("    Duration: {}", duration);
            }
            
            if let Some(track_num) = av_event.property.last_change.instance.current_track.as_ref().and_then(|v| v.val.parse::<u32>().ok()) {
                if let Some(total_tracks) = av_event.property.last_change.instance.number_of_tracks.as_ref().and_then(|v| v.val.parse::<u32>().ok()) {
                    println!("    Track: {} of {}", track_num, total_tracks);
                }
            }
            
            if let Some(play_mode) = av_event.property.last_change.instance.current_play_mode.as_ref() {
                println!("    Play Mode: {}", play_mode.val);
            }

            // Demonstrate conditional logic based on transport state
            match av_event.transport_state() {
                "PLAYING" => println!("    → Music is currently playing"),
                "PAUSED_PLAYBACK" => println!("    → Music is paused"),
                "STOPPED" => println!("    → Playback is stopped"),
                "TRANSITIONING" => println!("    → Transport state is changing"),
                _ => println!("    → Unknown transport state: {}", av_event.transport_state()),
            }
        } else {
            println!("  ✗ Failed to downcast AVTransport event - unexpected event type");
        }
    }

    /// Process RenderingControl events (would need RenderingControlEvent type)
    fn process_rendering_control_event(&self, event: &TypedEvent) {
        println!("  RenderingControl event processing (type-safe implementation would go here)");
        println!("    Event type: {}", event.event_type());
        // In a real implementation, we would downcast to RenderingControlEvent
        // if let Some(rc_event) = event.downcast_ref::<RenderingControlEvent>() { ... }
    }

    /// Process ZoneGroupTopology events (would need ZoneGroupTopologyEvent type)
    fn process_zone_group_event(&self, event: &TypedEvent) {
        println!("  ZoneGroupTopology event processing (type-safe implementation would go here)");
        println!("    Event type: {}", event.event_type());
        // In a real implementation, we would downcast to ZoneGroupTopologyEvent
        // if let Some(zgt_event) = event.downcast_ref::<ZoneGroupTopologyEvent>() { ... }
    }

    /// Process unknown events using generic debug output
    fn process_unknown_event(&self, event: &TypedEvent) {
        println!("  Generic event processing:");
        println!("    Debug: {:?}", event.debug());
    }

    /// Get processing statistics
    fn get_stats(&self) -> (u32, u32) {
        (self.av_transport_count, self.unknown_count)
    }
}

/// Enhanced mock strategy that creates different types of events
#[derive(Clone)]
struct EnhancedMockStrategy {
    service_type: ServiceType,
    counter: Arc<AtomicU32>,
}

impl EnhancedMockStrategy {
    fn new(service_type: ServiceType) -> Self {
        Self {
            service_type,
            counter: Arc::new(AtomicU32::new(0)),
        }
    }
}

#[async_trait]
impl BaseStrategy for EnhancedMockStrategy {
    fn service_type(&self) -> ServiceType {
        self.service_type
    }

    fn subscription_scope(&self) -> SubscriptionScope {
        SubscriptionScope::PerSpeaker
    }

    fn service_endpoint_path(&self) -> &'static str {
        match self.service_type {
            ServiceType::AVTransport => "/MediaRenderer/AVTransport/Event",
            ServiceType::RenderingControl => "/MediaRenderer/RenderingControl/Event",
            ServiceType::ZoneGroupTopology => "/ZoneGroupTopology/Event",
        }
    }

    async fn create_subscription(
        &self,
        speaker: &Speaker,
        _callback_url: String,
        config: &SubscriptionConfig,
    ) -> Result<Box<dyn Subscription>, StrategyError> {
        let count = self.counter.fetch_add(1, Ordering::Relaxed);
        let subscription_id = format!("enhanced-mock-{}-{}-{}", 
            self.service_type as u32, speaker.id.as_str(), count);
        
        Ok(Box::new(EnhancedMockSubscription {
            id: subscription_id,
            speaker_id: speaker.id.clone(),
            service_type: self.service_type,
            created_at: SystemTime::now(),
            timeout: Duration::from_secs(config.timeout_seconds as u64),
        }))
    }

    fn parse_event(
        &self,
        _speaker_id: &SpeakerId,
        event_xml: &str,
    ) -> Result<TypedEvent, StrategyError> {
        match self.service_type {
            ServiceType::AVTransport => {
                // Create realistic AVTransport events based on XML content
                let (transport_state, track_info) = if event_xml.contains("PLAYING") {
                    ("PLAYING", Some(("Bohemian Rhapsody", "Queen", "A Night at the Opera")))
                } else if event_xml.contains("PAUSED") {
                    ("PAUSED_PLAYBACK", Some(("Hotel California", "Eagles", "Hotel California")))
                } else if event_xml.contains("STOPPED") {
                    ("STOPPED", None)
                } else {
                    ("TRANSITIONING", None)
                };

                let xml = format!(
                    r#"<e:propertyset xmlns:e="urn:schemas-upnp-org:event-1-0"><e:property><LastChange>&lt;Event xmlns=&quot;urn:schemas-upnp-org:metadata-1-0/AVT/&quot;&gt;&lt;InstanceID val=&quot;0&quot;&gt;&lt;TransportState val=&quot;{}&quot;/&gt;{}&lt;/InstanceID&gt;&lt;/Event&gt;</LastChange></e:property></e:propertyset>"#,
                    transport_state,
                    track_info.map(|_| "&lt;CurrentTrackURI val=&quot;x-sonos-spotify:track123&quot;/&gt;").unwrap_or("")
                );
                let av_event = AVTransportParser::from_xml(&xml).unwrap();

                Ok(TypedEvent::new(Box::new(av_event)))
            }
            _ => {
                // For other service types, we would create their specific event types
                // For now, return an error to demonstrate error handling
                Err(StrategyError::EventParseFailed(
                    format!("Event parsing not implemented for {:?}", self.service_type)
                ))
            }
        }
    }
}

/// Enhanced mock subscription
struct EnhancedMockSubscription {
    id: String,
    speaker_id: SpeakerId,
    service_type: ServiceType,
    created_at: SystemTime,
    timeout: Duration,
}

#[async_trait]
impl Subscription for EnhancedMockSubscription {
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

/// Simulate sending different types of events
async fn simulate_typed_events(callback_url: String, subscription_id: String) {
    tokio::time::sleep(Duration::from_millis(500)).await;

    let client = reqwest::Client::new();
    
    let events = vec![
        ("<event><PLAYING>true</PLAYING></event>", "Started playing"),
        ("<event><PAUSED>true</PAUSED></event>", "Paused playback"),
        ("<event><PLAYING>true</PLAYING></event>", "Resumed playing"),
        ("<event><STOPPED>true</STOPPED></event>", "Stopped playback"),
    ];

    for (xml, description) in events {
        tokio::time::sleep(Duration::from_millis(1000)).await;
        
        println!("📤 Simulating event: {}", description);
        
        let url = format!("{}/notify/{}", callback_url, subscription_id);
        let result = client
            .post(&url)
            .header("SID", subscription_id.clone())
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
    println!("Starting typed event handling example...\n");

    // Create broker with enhanced mock strategy
    let strategy = EnhancedMockStrategy::new(ServiceType::AVTransport);
    let mut broker = EventBrokerBuilder::new()
        .with_strategy(Box::new(strategy))
        .with_port_range(41000, 41100)
        .build()
        .await?;

    // Create a test speaker
    let speaker = Speaker::new(
        SpeakerId::new("RINCON_TYPED_EXAMPLE"),
        "127.0.0.1".parse::<IpAddr>()?,
        "Typed Event Speaker".to_string(),
        "Demo Room".to_string(),
    );

    println!("Subscribing to speaker: {}", speaker.name);
    
    // Subscribe to the speaker
    broker.subscribe(&speaker, ServiceType::AVTransport).await?;

    // Get the event stream
    let mut event_stream = broker.event_stream();

    // Create event processor
    let mut processor = TypedEventProcessor::new();

    println!("Listening for typed events...\n");

    // Capture the subscription ID from the first event
    let subscription_id = if let Some(Event::SubscriptionEstablished { subscription_id, .. }) = event_stream.recv().await {
        println!("✓ Subscription established");
        println!("  ID: {}\n", subscription_id);
        subscription_id
    } else {
        return Err("Failed to establish subscription".into());
    };

    let callback_url = format!("http://127.0.0.1:41000");

    // Spawn task to simulate events
    let sim_url = callback_url.clone();
    let sim_sub_id = subscription_id.clone();
    tokio::spawn(async move {
        simulate_typed_events(sim_url, sim_sub_id).await;
    });

    // Process events with timeout
    let mut event_count = 0;
    let max_events = 4; // Wait for 4 service events then exit
    let timeout_duration = Duration::from_secs(15);

    println!("Processing typed events (timeout: {}s)...\n", timeout_duration.as_secs());

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
                    Event::ServiceEvent { speaker_id, service_type: _, event } => {
                        processor.process_event(&speaker_id, &event);
                        event_count += 1;
                        
                        if event_count >= max_events {
                            println!("Processed {} events, exiting...\n", max_events);
                            break;
                        }
                    }
                    Event::ParseError { speaker_id, service_type, error } => {
                        println!("✗ Parse error (demonstrating error handling):");
                        println!("  Speaker: {}", speaker_id.as_str());
                        println!("  Service: {:?}", service_type);
                        println!("  Error: {}\n", error);
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
    let (av_count, unknown_count) = processor.get_stats();
    println!("Processing Statistics:");
    println!("  AVTransport events: {}", av_count);
    println!("  Unknown events: {}", unknown_count);
    println!();

    // Cleanup
    println!("Demo complete! Cleaning up...");
    broker.unsubscribe(&speaker, ServiceType::AVTransport).await?;
    
    tokio::select! {
        Some(Event::SubscriptionRemoved { .. }) = event_stream.recv() => {
            println!("✓ Unsubscribed successfully");
        }
        _ = tokio::time::sleep(Duration::from_millis(100)) => {
            println!("✓ Unsubscribe initiated");
        }
    }

    broker.shutdown().await?;
    println!("✓ Broker shut down");
    println!("\nThis example demonstrated:");
    println!("• Type-safe event downcasting with AVTransportParser");
    println!("• Conditional processing based on event and service types");
    println!("• Safe handling of optional fields in typed events");
    println!("• Error handling for unsupported event types");
    println!("• Event processing statistics and monitoring");

    Ok(())
}