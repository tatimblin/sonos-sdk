//! Event processing and display module.
//!
//! This module provides functionality for consuming events from the EventBroker,
//! handling different event types, and displaying them in a human-readable format
//! with proper timestamps and structured logging.

use anyhow::Result;
use chrono::{DateTime, Local};
use sonos_stream::{Event, TypedEvent};
// use sonos_parser::services::av_transport::AVTransportParser; // Removed - using direct event types now
use tokio::sync::mpsc;
use tokio::signal;
use tracing::{info, warn, error, debug};
use std::time::SystemTime;

/// Configuration for event processing
#[derive(Debug, Clone)]
pub struct EventProcessingConfig {
    /// Whether to display raw event data for debugging
    pub show_raw_data: bool,
    /// Whether to use colored output (if terminal supports it)
    pub use_colors: bool,
    /// Maximum number of events to buffer before dropping
    pub max_buffer_size: usize,
    /// Whether to log events to the tracing system
    pub enable_logging: bool,
}

impl Default for EventProcessingConfig {
    fn default() -> Self {
        Self {
            show_raw_data: false,
            use_colors: true,
            max_buffer_size: 1000,
            enable_logging: true,
        }
    }
}

/// Statistics about processed events
#[derive(Debug, Default, Clone)]
pub struct EventStats {
    /// Total number of events processed
    pub total_events: u64,
    /// Number of subscription established events
    pub subscriptions_established: u64,
    /// Number of subscription failed events
    pub subscriptions_failed: u64,
    /// Number of service events received
    pub service_events: u64,
    /// Number of parse errors encountered
    pub parse_errors: u64,
    /// Number of subscription renewals
    pub subscription_renewals: u64,
    /// Number of subscription expirations
    pub subscription_expirations: u64,
    /// Number of subscription removals
    pub subscription_removals: u64,
}

impl EventStats {
    /// Update statistics based on an event
    pub fn update(&mut self, event: &Event) {
        self.total_events += 1;
        
        match event {
            Event::SubscriptionEstablished { .. } => self.subscriptions_established += 1,
            Event::SubscriptionFailed { .. } => self.subscriptions_failed += 1,
            Event::ServiceEvent { .. } => self.service_events += 1,
            Event::ParseError { .. } => self.parse_errors += 1,
            Event::SubscriptionRenewed { .. } => self.subscription_renewals += 1,
            Event::SubscriptionExpired { .. } => self.subscription_expirations += 1,
            Event::SubscriptionRemoved { .. } => self.subscription_removals += 1,
        }
    }

    /// Get a summary string of the statistics
    pub fn summary(&self) -> String {
        format!(
            "Events: {} total, {} service, {} established, {} failed, {} errors",
            self.total_events,
            self.service_events,
            self.subscriptions_established,
            self.subscriptions_failed,
            self.parse_errors
        )
    }
}

/// Event stream consumer that processes events from the broker.
///
/// This struct handles the async event loop to consume events from the broker,
/// processes different event types, and provides graceful shutdown on termination signals.
pub struct EventStreamConsumer {
    /// Configuration for event processing
    config: EventProcessingConfig,
    /// Statistics about processed events
    stats: EventStats,
    /// Event formatter for display
    formatter: EventFormatter,
}

impl EventStreamConsumer {
    /// Create a new event stream consumer with default configuration.
    pub fn new() -> Self {
        Self::with_config(EventProcessingConfig::default())
    }

    /// Create a new event stream consumer with custom configuration.
    pub fn with_config(config: EventProcessingConfig) -> Self {
        Self {
            formatter: EventFormatter::new(config.clone()),
            config,
            stats: EventStats::default(),
        }
    }

    /// Start consuming events from the event stream.
    ///
    /// This function implements the async event loop to consume events from the broker,
    /// handles different event types (subscription, service events, errors), and
    /// implements graceful shutdown on termination signals.
    ///
    /// # Arguments
    ///
    /// * `event_receiver` - The event receiver from the EventBroker
    ///
    /// # Returns
    ///
    /// Returns Ok(()) when the event loop exits gracefully, or an error if something goes wrong.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use integration_example::event_processing::EventStreamConsumer;
    /// use sonos_stream::EventBroker;
    ///
    /// # async fn example(mut broker: EventBroker) -> anyhow::Result<()> {
    /// let mut consumer = EventStreamConsumer::new();
    /// let event_stream = broker.event_stream();
    /// consumer.consume_events(event_stream).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn consume_events(&mut self, mut event_receiver: mpsc::Receiver<Event>) -> Result<()> {
        info!("Starting event stream consumer");
        
        // Set up graceful shutdown handling
        let mut shutdown_signal = Box::pin(signal::ctrl_c());
        
        loop {
            tokio::select! {
                // Handle incoming events
                event = event_receiver.recv() => {
                    match event {
                        Some(event) => {
                            if let Err(e) = self.process_event(event).await {
                                error!("Error processing event: {}", e);
                            }
                        }
                        None => {
                            info!("Event stream closed, shutting down consumer");
                            break;
                        }
                    }
                }
                
                // Handle shutdown signal (Ctrl+C)
                _ = &mut shutdown_signal => {
                    info!("Received shutdown signal, stopping event consumer");
                    break;
                }
            }
        }

        // Print final statistics
        self.print_final_stats();
        
        Ok(())
    }

    /// Process a single event from the stream.
    async fn process_event(&mut self, event: Event) -> Result<()> {
        // Update statistics
        self.stats.update(&event);

        // Log the event if logging is enabled
        if self.config.enable_logging {
            self.log_event(&event);
        }

        // Format and display the event
        let formatted = self.formatter.format_event(&event);
        println!("{}", formatted);

        // Show raw data if requested
        if self.config.show_raw_data {
            self.display_raw_event_data(&event);
        }

        Ok(())
    }

    /// Log an event to the tracing system.
    fn log_event(&self, event: &Event) {
        match event {
            Event::SubscriptionEstablished { speaker_id, service_type, subscription_id, .. } => {
                info!(
                    speaker_id = %speaker_id.as_str(),
                    service_type = ?service_type,
                    subscription_id = %subscription_id,
                    "Subscription established"
                );
            }
            Event::SubscriptionFailed { speaker_id, service_type, error, .. } => {
                warn!(
                    speaker_id = %speaker_id.as_str(),
                    service_type = ?service_type,
                    error = %error,
                    "Subscription failed"
                );
            }
            Event::ServiceEvent { speaker_id, service_type, event, .. } => {
                debug!(
                    speaker_id = %speaker_id.as_str(),
                    service_type = ?service_type,
                    event_type = %event.event_type(),
                    "Service event received"
                );
            }
            Event::ParseError { speaker_id, service_type, error, .. } => {
                error!(
                    speaker_id = %speaker_id.as_str(),
                    service_type = ?service_type,
                    error = %error,
                    "Parse error"
                );
            }
            Event::SubscriptionRenewed { speaker_id, service_type, .. } => {
                debug!(
                    speaker_id = %speaker_id.as_str(),
                    service_type = ?service_type,
                    "Subscription renewed"
                );
            }
            Event::SubscriptionExpired { speaker_id, service_type, .. } => {
                warn!(
                    speaker_id = %speaker_id.as_str(),
                    service_type = ?service_type,
                    "Subscription expired"
                );
            }
            Event::SubscriptionRemoved { speaker_id, service_type, .. } => {
                info!(
                    speaker_id = %speaker_id.as_str(),
                    service_type = ?service_type,
                    "Subscription removed"
                );
            }
        }
    }

    /// Display raw event data for debugging.
    fn display_raw_event_data(&self, event: &Event) {
        match event {
            Event::ServiceEvent { event: typed_event, .. } => {
                println!("  Raw event data:");
                println!("    Event Type: {}", typed_event.event_type());
                println!("    Service Type: {:?}", typed_event.service_type());
                println!("    Debug: {:?}", typed_event);
            }
            _ => {
                println!("  Raw data: {:?}", event);
            }
        }
    }

    /// Print final statistics when shutting down.
    fn print_final_stats(&self) {
        println!("\n=== Event Processing Statistics ===");
        println!("Total events processed: {}", self.stats.total_events);
        println!("Subscriptions established: {}", self.stats.subscriptions_established);
        println!("Subscriptions failed: {}", self.stats.subscriptions_failed);
        println!("Service events: {}", self.stats.service_events);
        println!("Parse errors: {}", self.stats.parse_errors);
        println!("Subscription renewals: {}", self.stats.subscription_renewals);
        println!("Subscription expirations: {}", self.stats.subscription_expirations);
        println!("Subscription removals: {}", self.stats.subscription_removals);
        
        info!("Event processing completed: {}", self.stats.summary());
    }

    /// Get current event statistics.
    pub fn stats(&self) -> &EventStats {
        &self.stats
    }

    /// Get a copy of the current configuration.
    pub fn config(&self) -> &EventProcessingConfig {
        &self.config
    }
}

impl Default for EventStreamConsumer {
    fn default() -> Self {
        Self::new()
    }
}

/// Event formatter for creating human-readable event displays.
pub struct EventFormatter {
    /// Configuration for formatting
    config: EventProcessingConfig,
}

impl EventFormatter {
    /// Create a new event formatter with the given configuration.
    pub fn new(config: EventProcessingConfig) -> Self {
        Self { config }
    }

    /// Format an event for display.
    ///
    /// Creates formatted output for different event types with timestamps
    /// and structured logging. Displays AVTransport state changes in
    /// human-readable format.
    pub fn format_event(&self, event: &Event) -> String {
        let timestamp = self.format_timestamp();
        
        match event {
            Event::SubscriptionEstablished { speaker_id, service_type, subscription_id, .. } => {
                self.format_with_color(
                    &format!(
                        "[{}] SUBSCRIPTION_ESTABLISHED: {:?} on {} ({})",
                        timestamp,
                        service_type,
                        speaker_id.as_str(),
                        subscription_id
                    ),
                    "\x1b[32m", // Green
                )
            }
            Event::SubscriptionFailed { speaker_id, service_type, error, .. } => {
                self.format_with_color(
                    &format!(
                        "[{}] SUBSCRIPTION_FAILED: {:?} on {} - {}",
                        timestamp,
                        service_type,
                        speaker_id.as_str(),
                        error
                    ),
                    "\x1b[31m", // Red
                )
            }
            Event::ServiceEvent { speaker_id, service_type, event, .. } => {
                let formatted_event = self.format_service_event(event);
                self.format_with_color(
                    &format!(
                        "[{}] SERVICE_EVENT: {} - {:?} - {}",
                        timestamp,
                        speaker_id.as_str(),
                        service_type,
                        formatted_event
                    ),
                    "\x1b[36m", // Cyan
                )
            }
            Event::ParseError { speaker_id, service_type, error, .. } => {
                self.format_with_color(
                    &format!(
                        "[{}] PARSE_ERROR: {} - {:?} - {}",
                        timestamp,
                        speaker_id.as_str(),
                        service_type,
                        error
                    ),
                    "\x1b[33m", // Yellow
                )
            }
            Event::SubscriptionRenewed { speaker_id, service_type, .. } => {
                self.format_with_color(
                    &format!(
                        "[{}] SUBSCRIPTION_RENEWED: {:?} on {}",
                        timestamp,
                        service_type,
                        speaker_id.as_str()
                    ),
                    "\x1b[34m", // Blue
                )
            }
            Event::SubscriptionExpired { speaker_id, service_type, .. } => {
                self.format_with_color(
                    &format!(
                        "[{}] SUBSCRIPTION_EXPIRED: {:?} on {}",
                        timestamp,
                        service_type,
                        speaker_id.as_str()
                    ),
                    "\x1b[35m", // Magenta
                )
            }
            Event::SubscriptionRemoved { speaker_id, service_type, .. } => {
                self.format_with_color(
                    &format!(
                        "[{}] SUBSCRIPTION_REMOVED: {:?} on {}",
                        timestamp,
                        service_type,
                        speaker_id.as_str()
                    ),
                    "\x1b[37m", // White
                )
            }
        }
    }

    /// Format a service event with human-readable information.
    fn format_service_event(&self, event: &TypedEvent) -> String {
        match event.event_type() {
            "av_transport_event" => self.format_av_transport_event(event),
            _ => format!("{}: TypedEvent", event.event_type()),
        }
    }

    /// Format AVTransport events with specific state information.
    fn format_av_transport_event(&self, _event: &TypedEvent) -> String {
        // TODO: Update this method to work with new event types after sonos-stream refactoring
        // The integration-example is temporarily disabled during the parsing refactoring
        "AVTransport event".to_string()
    }

    /// Format text with color if colors are enabled.
    fn format_with_color(&self, text: &str, color_code: &str) -> String {
        if self.config.use_colors {
            format!("{}{}\x1b[0m", color_code, text)
        } else {
            text.to_string()
        }
    }

    /// Format the current timestamp.
    fn format_timestamp(&self) -> String {
        let now: DateTime<Local> = Local::now();
        now.format("%Y-%m-%d %H:%M:%S%.3f").to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sonos_stream::{ServiceType, SpeakerId, TypedEvent};

    fn create_test_service_event() -> Event {
        // TODO: Update test after sonos-stream refactoring is complete
        // Creating a minimal service event for testing
        use sonos_stream::{ServiceType, SpeakerId};

        Event::ServiceEvent {
            speaker_id: SpeakerId::new("RINCON_TEST123456"),
            service_type: ServiceType::AVTransport,
            event: TypedEvent::new_generic("test_event".to_string(), "av_transport_event", ServiceType::AVTransport),
            timestamp: SystemTime::now(),
        }
    }

    #[test]
    fn test_event_stats_update() {
        let mut stats = EventStats::default();
        
        let event = Event::SubscriptionEstablished {
            speaker_id: SpeakerId::new("test"),
            service_type: ServiceType::AVTransport,
            subscription_id: "sub123".to_string(),
            timestamp: SystemTime::now(),
        };
        
        stats.update(&event);
        
        assert_eq!(stats.total_events, 1);
        assert_eq!(stats.subscriptions_established, 1);
        assert_eq!(stats.service_events, 0);
    }

    #[test]
    fn test_event_stats_summary() {
        let mut stats = EventStats::default();
        stats.total_events = 10;
        stats.service_events = 5;
        stats.subscriptions_established = 2;
        stats.subscriptions_failed = 1;
        stats.parse_errors = 1;
        
        let summary = stats.summary();
        assert!(summary.contains("10 total"));
        assert!(summary.contains("5 service"));
        assert!(summary.contains("2 established"));
        assert!(summary.contains("1 failed"));
        assert!(summary.contains("1 errors"));
    }

    #[test]
    fn test_event_formatter_subscription_established() {
        let config = EventProcessingConfig {
            use_colors: false,
            ..Default::default()
        };
        let formatter = EventFormatter::new(config);
        
        let event = Event::SubscriptionEstablished {
            speaker_id: SpeakerId::new("RINCON_TEST123456"),
            service_type: ServiceType::AVTransport,
            subscription_id: "sub123".to_string(),
            timestamp: SystemTime::now(),
        };
        
        let formatted = formatter.format_event(&event);
        assert!(formatted.contains("SUBSCRIPTION_ESTABLISHED"));
        assert!(formatted.contains("AVTransport"));
        assert!(formatted.contains("RINCON_TEST123456"));
        assert!(formatted.contains("sub123"));
    }

    #[test]
    fn test_event_formatter_service_event() {
        let config = EventProcessingConfig {
            use_colors: false,
            ..Default::default()
        };
        let formatter = EventFormatter::new(config);
        
        let event = create_test_service_event();
        let formatted = formatter.format_event(&event);
        
        assert!(formatted.contains("SERVICE_EVENT"));
        assert!(formatted.contains("RINCON_TEST123456"));
        // TODO: Update assertions after sonos-stream refactoring
        assert!(formatted.contains("AVTransport event"));
    }

    #[test]
    fn test_event_formatter_av_transport_event() {
        let config = EventProcessingConfig::default();
        let formatter = EventFormatter::new(config);

        // TODO: Update test after sonos-stream refactoring is complete
        let typed_event = TypedEvent::new_generic("test_event".to_string(), "av_transport_event", ServiceType::AVTransport);
        let formatted = formatter.format_av_transport_event(&typed_event);

        // For now, just verify basic functionality
        assert!(formatted.contains("AVTransport event"));
    }

    #[test]
    fn test_event_formatter_colors_disabled() {
        let config = EventProcessingConfig {
            use_colors: false,
            ..Default::default()
        };
        let formatter = EventFormatter::new(config);
        
        let text = "test message";
        let formatted = formatter.format_with_color(text, "\x1b[32m");
        
        assert_eq!(formatted, text);
        assert!(!formatted.contains("\x1b["));
    }

    #[test]
    fn test_event_formatter_colors_enabled() {
        let config = EventProcessingConfig {
            use_colors: true,
            ..Default::default()
        };
        let formatter = EventFormatter::new(config);
        
        let text = "test message";
        let formatted = formatter.format_with_color(text, "\x1b[32m");
        
        assert!(formatted.contains("\x1b[32m"));
        assert!(formatted.contains("\x1b[0m"));
        assert!(formatted.contains(text));
    }

    #[test]
    fn test_event_processing_config_default() {
        let config = EventProcessingConfig::default();
        
        assert!(!config.show_raw_data);
        assert!(config.use_colors);
        assert_eq!(config.max_buffer_size, 1000);
        assert!(config.enable_logging);
    }

    #[test]
    fn test_event_stream_consumer_creation() {
        let consumer = EventStreamConsumer::new();
        assert_eq!(consumer.stats.total_events, 0);
        assert!(!consumer.config.show_raw_data);
        
        let custom_config = EventProcessingConfig {
            show_raw_data: true,
            use_colors: false,
            ..Default::default()
        };
        let consumer_with_config = EventStreamConsumer::with_config(custom_config);
        assert!(consumer_with_config.config.show_raw_data);
        assert!(!consumer_with_config.config.use_colors);
    }
}