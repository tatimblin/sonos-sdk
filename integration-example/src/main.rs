use anyhow::{Context, Result};
use clap::Parser;
use std::time::Duration;
use tracing::{info, warn, error};

pub mod conversion;
pub mod device_selection;
pub mod event_broker;
pub mod event_processing;
pub mod subscription;

use device_selection::DeviceSelector;
use event_broker::{BrokerConfig, create_event_broker};
use event_processing::{EventStreamConsumer, EventProcessingConfig};
use subscription::{create_av_transport_subscription, SubscriptionConfig, SubscriptionResult};

/// Sonos SDK Integration Example
/// 
/// Demonstrates the complete Sonos SDK workflow by discovering devices,
/// selecting a target device, establishing an event subscription, and
/// consuming real-time events from the device.
#[derive(Parser, Debug)]
#[command(name = "integration-example")]
#[command(about = "Sonos SDK Integration Example - Complete workflow demonstration")]
#[command(version = "0.1.0")]
pub struct Args {
    /// Target device name to subscribe to
    #[arg(short, long, default_value = "Sonos Roam 2")]
    pub target_device: String,

    /// Discovery timeout in seconds
    #[arg(short = 'd', long, default_value = "3")]
    pub discovery_timeout: u64,

    /// Callback server port range start
    #[arg(long, default_value = "3400")]
    pub port_start: u16,

    /// Callback server port range end
    #[arg(long, default_value = "3500")]
    pub port_end: u16,

    /// Subscription timeout in seconds
    #[arg(short = 's', long, default_value = "1800")]
    pub subscription_timeout: u64,

    /// Maximum retry attempts for operations
    #[arg(short = 'r', long, default_value = "3")]
    pub max_retries: u32,

    /// Show raw event data for debugging
    #[arg(long)]
    pub show_raw_data: bool,

    /// Disable colored output
    #[arg(long)]
    pub no_colors: bool,

    /// Use first available device if target not found
    #[arg(long)]
    pub use_first_available: bool,

    /// List discovered devices and exit
    #[arg(long)]
    pub list_devices: bool,

    /// Log level (error, warn, info, debug, trace)
    #[arg(long, default_value = "info")]
    pub log_level: String,
}

impl Args {
    /// Get discovery timeout as Duration
    pub fn discovery_timeout_duration(&self) -> Duration {
        Duration::from_secs(self.discovery_timeout)
    }

    /// Get subscription timeout as Duration
    pub fn subscription_timeout_duration(&self) -> Duration {
        Duration::from_secs(self.subscription_timeout)
    }

    /// Validate command line arguments
    pub fn validate(&self) -> Result<()> {
        if self.port_start == 0 || self.port_end == 0 {
            return Err(anyhow::anyhow!("Port range must not include port 0"));
        }

        if self.port_start > self.port_end {
            return Err(anyhow::anyhow!(
                "Invalid port range: start ({}) > end ({})",
                self.port_start,
                self.port_end
            ));
        }

        if self.discovery_timeout == 0 {
            return Err(anyhow::anyhow!("Discovery timeout must be positive"));
        }

        if self.subscription_timeout == 0 {
            return Err(anyhow::anyhow!("Subscription timeout must be positive"));
        }

        if self.max_retries == 0 {
            return Err(anyhow::anyhow!("Max retries must be at least 1"));
        }

        // Validate log level
        match self.log_level.to_lowercase().as_str() {
            "error" | "warn" | "info" | "debug" | "trace" => {}
            _ => {
                return Err(anyhow::anyhow!(
                    "Invalid log level '{}'. Valid levels: error, warn, info, debug, trace",
                    self.log_level
                ));
            }
        }

        Ok(())
    }
}

/// Configuration derived from command line arguments and environment variables
#[derive(Debug, Clone)]
pub struct Config {
    pub target_device: String,
    pub discovery_timeout: Duration,
    pub port_range: (u16, u16),
    pub subscription_timeout: Duration,
    pub max_retries: u32,
    pub show_raw_data: bool,
    pub use_colors: bool,
    pub use_first_available: bool,
    pub list_devices: bool,
    pub log_level: String,
}

impl From<Args> for Config {
    fn from(args: Args) -> Self {
        let discovery_timeout = args.discovery_timeout_duration();
        let subscription_timeout = args.subscription_timeout_duration();
        
        Self {
            target_device: args.target_device,
            discovery_timeout,
            port_range: (args.port_start, args.port_end),
            subscription_timeout,
            max_retries: args.max_retries,
            show_raw_data: args.show_raw_data,
            use_colors: !args.no_colors,
            use_first_available: args.use_first_available,
            list_devices: args.list_devices,
            log_level: args.log_level,
        }
    }
}

impl Config {
    /// Create configuration from command line arguments and environment variables
    pub fn from_env() -> Result<Self> {
        let mut args = Args::parse();

        // Override with environment variables if present
        if let Ok(target) = std::env::var("SONOS_TARGET_DEVICE") {
            args.target_device = target;
        }

        if let Ok(timeout) = std::env::var("SONOS_DISCOVERY_TIMEOUT") {
            args.discovery_timeout = timeout.parse()
                .context("Invalid SONOS_DISCOVERY_TIMEOUT environment variable")?;
        }

        if let Ok(port_start) = std::env::var("SONOS_PORT_START") {
            args.port_start = port_start.parse()
                .context("Invalid SONOS_PORT_START environment variable")?;
        }

        if let Ok(port_end) = std::env::var("SONOS_PORT_END") {
            args.port_end = port_end.parse()
                .context("Invalid SONOS_PORT_END environment variable")?;
        }

        if let Ok(sub_timeout) = std::env::var("SONOS_SUBSCRIPTION_TIMEOUT") {
            args.subscription_timeout = sub_timeout.parse()
                .context("Invalid SONOS_SUBSCRIPTION_TIMEOUT environment variable")?;
        }

        if let Ok(retries) = std::env::var("SONOS_MAX_RETRIES") {
            args.max_retries = retries.parse()
                .context("Invalid SONOS_MAX_RETRIES environment variable")?;
        }

        if let Ok(log_level) = std::env::var("SONOS_LOG_LEVEL") {
            args.log_level = log_level;
        }

        if std::env::var("SONOS_SHOW_RAW_DATA").is_ok() {
            args.show_raw_data = true;
        }

        if std::env::var("SONOS_NO_COLORS").is_ok() {
            args.no_colors = true;
        }

        if std::env::var("SONOS_USE_FIRST_AVAILABLE").is_ok() {
            args.use_first_available = true;
        }

        if std::env::var("SONOS_LIST_DEVICES").is_ok() {
            args.list_devices = true;
        }

        // Validate arguments
        args.validate()?;

        Ok(Config::from(args))
    }

    /// Print configuration summary
    pub fn print_summary(&self) {
        info!("Configuration:");
        info!("  Target device: {}", self.target_device);
        info!("  Discovery timeout: {}s", self.discovery_timeout.as_secs());
        info!("  Port range: {}-{}", self.port_range.0, self.port_range.1);
        info!("  Subscription timeout: {}s", self.subscription_timeout.as_secs());
        info!("  Max retries: {}", self.max_retries);
        info!("  Show raw data: {}", self.show_raw_data);
        info!("  Use colors: {}", self.use_colors);
        info!("  Use first available: {}", self.use_first_available);
        info!("  List devices only: {}", self.list_devices);
        info!("  Log level: {}", self.log_level);
    }
}

/// Initialize tracing/logging with the specified log level
fn init_tracing(log_level: &str) -> Result<()> {
    let filter = match log_level.to_lowercase().as_str() {
        "error" => "error",
        "warn" => "warn", 
        "info" => "info",
        "debug" => "debug",
        "trace" => "trace",
        _ => "info", // fallback
    };

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(filter))
        )
        .init();

    Ok(())
}

/// Print application banner and information
fn print_banner() {
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║                 Sonos SDK Integration Example               ║");
    println!("║                                                              ║");
    println!("║  Demonstrates complete Sonos SDK workflow:                  ║");
    println!("║  • Device discovery via SSDP/UPnP                          ║");
    println!("║  • Device selection and conversion                          ║");
    println!("║  • Event subscription establishment                         ║");
    println!("║  • Real-time event stream consumption                       ║");
    println!("║                                                              ║");
    println!("║  Press Ctrl+C to stop the example                          ║");
    println!("╚══════════════════════════════════════════════════════════════╝");
    println!();
}

/// Print help information about environment variables
fn print_env_help() {
    println!("Environment Variables:");
    println!("  SONOS_TARGET_DEVICE         Target device name (default: 'Sonos Roam 2')");
    println!("  SONOS_DISCOVERY_TIMEOUT     Discovery timeout in seconds (default: 3)");
    println!("  SONOS_PORT_START            Callback port range start (default: 3400)");
    println!("  SONOS_PORT_END              Callback port range end (default: 3500)");
    println!("  SONOS_SUBSCRIPTION_TIMEOUT  Subscription timeout in seconds (default: 1800)");
    println!("  SONOS_MAX_RETRIES           Max retry attempts (default: 3)");
    println!("  SONOS_LOG_LEVEL             Log level (default: info)");
    println!("  SONOS_SHOW_RAW_DATA         Show raw event data (set to enable)");
    println!("  SONOS_NO_COLORS             Disable colored output (set to enable)");
    println!("  SONOS_USE_FIRST_AVAILABLE   Use first device if target not found (set to enable)");
    println!("  SONOS_LIST_DEVICES          List devices and exit (set to enable)");
    println!();
}

/// Run the complete integration workflow
/// 
/// Orchestrates the discovery → selection → subscription → event processing flow
/// with proper error handling at each stage and resource cleanup on shutdown.
async fn run_integration_workflow(config: Config) -> Result<()> {
    info!("Starting integration workflow");

    // Phase 1: Device Discovery
    info!("Phase 1: Discovering Sonos devices on the network...");
    let discovered_devices = discover_devices(&config).await
        .context("Failed during device discovery phase")?;

    // Handle list-devices-only mode
    if config.list_devices {
        let device_list = DeviceSelector::list_devices(&discovered_devices);
        println!("\n{}", device_list);
        return Ok(());
    }

    // Phase 2: Device Selection
    info!("Phase 2: Selecting target device...");
    let target_speaker = select_target_device(&config, discovered_devices)
        .context("Failed during device selection phase")?;

    info!(
        "Selected device: '{}' ({}) at {}",
        target_speaker.name, target_speaker.room, target_speaker.ip
    );

    // Phase 3: Event Broker Setup
    info!("Phase 3: Setting up event broker...");
    let mut event_broker = setup_event_broker(&config).await
        .context("Failed during event broker setup phase")?;

    // Phase 4: Subscription Establishment
    info!("Phase 4: Establishing AVTransport subscription...");
    let subscription_result = establish_subscription(&mut event_broker, &target_speaker, &config).await
        .context("Failed during subscription establishment phase")?;

    // Handle subscription failure
    if let SubscriptionResult::Failed { .. } | SubscriptionResult::Timeout { .. } = subscription_result {
        let error_report = subscription::generate_subscription_error_report(&subscription_result, &target_speaker);
        error!("Subscription failed:\n{}", error_report);
        return Err(anyhow::anyhow!("Failed to establish subscription"));
    }

    info!("Subscription established successfully");

    // Phase 5: Event Processing
    info!("Phase 5: Starting event stream consumption...");
    let event_stream_result = consume_event_stream(&mut event_broker, &config).await;

    // Phase 6: Cleanup
    info!("Phase 6: Cleaning up resources...");
    cleanup_resources(&mut event_broker, &target_speaker).await
        .context("Failed during resource cleanup")?;

    // Shutdown the event broker
    info!("Shutting down event broker...");
    if let Err(e) = event_broker.shutdown().await {
        warn!("Failed to shutdown event broker: {}", e);
    }

    // Check event stream result
    event_stream_result.context("Failed during event stream consumption")?;

    info!("Integration workflow completed successfully");
    Ok(())
}

/// Phase 1: Discover Sonos devices on the network
async fn discover_devices(config: &Config) -> Result<Vec<sonos_discovery::Device>> {
    info!(
        "Discovering devices with timeout of {}s...",
        config.discovery_timeout.as_secs()
    );

    let devices = tokio::task::spawn_blocking({
        let timeout = config.discovery_timeout;
        move || sonos_discovery::get_with_timeout(timeout)
    }).await.context("Discovery task failed")?;
    
    info!("Discovered {} Sonos device(s)", devices.len());

    if devices.is_empty() {
        warn!("No Sonos devices found on the network");
        println!("\nNo Sonos devices found. Please check:");
        println!("• Network connectivity");
        println!("• Devices are powered on");
        println!("• Devices are on the same network");
        println!("• Firewall settings allow multicast traffic");
        return Err(anyhow::anyhow!("No devices found"));
    }

    // Log discovered devices
    for device in &devices {
        info!(
            "Found device: '{}' ({}) at {} [{}]",
            device.name, device.room_name, device.ip_address, device.model_name
        );
    }

    Ok(devices)
}

/// Phase 2: Select the target device from discovered devices
fn select_target_device(
    config: &Config,
    discovered_devices: Vec<sonos_discovery::Device>,
) -> Result<sonos_stream::Speaker> {
    info!("Looking for target device: '{}'", config.target_device);

    let speaker = if config.use_first_available {
        info!("Using first available device as requested");
        DeviceSelector::get_first_available(discovered_devices)
    } else {
        DeviceSelector::find_device_with_fallback(&config.target_device, discovered_devices)
    };

    match speaker {
        Ok(speaker) => {
            info!(
                "Successfully selected device: '{}' ({})",
                speaker.name, speaker.id.as_str()
            );
            Ok(speaker)
        }
        Err(e) => {
            error!("Device selection failed: {}", e);
            Err(anyhow::anyhow!("Device selection failed: {}", e))
        }
    }
}

/// Phase 3: Set up the event broker with AVTransport strategy
async fn setup_event_broker(config: &Config) -> Result<sonos_stream::EventBroker> {
    info!(
        "Creating event broker with port range {}-{}",
        config.port_range.0, config.port_range.1
    );

    let broker_config = BrokerConfig {
        callback_port_range: config.port_range,
        subscription_timeout: config.subscription_timeout,
        max_retry_attempts: config.max_retries,
        ..Default::default()
    };

    let broker = create_event_broker(broker_config).await
        .context("Failed to create event broker")?;

    info!("Event broker created successfully");
    Ok(broker)
}

/// Phase 4: Establish AVTransport subscription to the target device
async fn establish_subscription(
    broker: &mut sonos_stream::EventBroker,
    speaker: &sonos_stream::Speaker,
    config: &Config,
) -> Result<SubscriptionResult> {
    info!(
        "Creating AVTransport subscription for device '{}'",
        speaker.name
    );

    let subscription_config = SubscriptionConfig {
        establishment_timeout: Duration::from_secs(30),
        max_retry_attempts: config.max_retries,
        retry_delay: Duration::from_secs(2),
        retry_on_network_errors: true,
    };

    let result = create_av_transport_subscription(broker, speaker, subscription_config).await
        .context("Failed to create subscription")?;

    match &result {
        SubscriptionResult::Success { .. } => {
            info!("AVTransport subscription established successfully");
        }
        SubscriptionResult::Failed { error, attempts, .. } => {
            warn!(
                "Subscription failed after {} attempts: {}",
                attempts, error
            );
        }
        SubscriptionResult::Timeout { timeout, .. } => {
            warn!(
                "Subscription timed out after {}s",
                timeout.as_secs()
            );
        }
    }

    Ok(result)
}

/// Phase 5: Consume events from the event stream
async fn consume_event_stream(
    broker: &mut sonos_stream::EventBroker,
    config: &Config,
) -> Result<()> {
    info!("Starting event stream consumption");

    let event_processing_config = EventProcessingConfig {
        show_raw_data: config.show_raw_data,
        use_colors: config.use_colors,
        enable_logging: true,
        ..Default::default()
    };

    let mut consumer = EventStreamConsumer::with_config(event_processing_config);
    let event_receiver = broker.event_stream();

    println!("\n=== Event Stream Started ===");
    println!("Listening for AVTransport events... (Press Ctrl+C to stop)");
    println!();

    consumer.consume_events(event_receiver).await
        .context("Event stream consumption failed")?;

    Ok(())
}

/// Phase 6: Clean up resources and unsubscribe
async fn cleanup_resources(
    broker: &mut sonos_stream::EventBroker,
    speaker: &sonos_stream::Speaker,
) -> Result<()> {
    info!("Cleaning up resources...");

    // Unsubscribe from AVTransport service
    if let Err(e) = subscription::unsubscribe_av_transport(broker, speaker).await {
        warn!("Failed to unsubscribe from AVTransport: {}", e);
    }

    info!("Resource cleanup completed");
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    // Parse configuration from command line and environment
    let config = Config::from_env().context("Failed to parse configuration")?;

    // Initialize tracing with the specified log level
    init_tracing(&config.log_level).context("Failed to initialize logging")?;

    // Print banner
    print_banner();

    // Print configuration summary
    config.print_summary();

    info!("Starting Sonos SDK Integration Example");

    // Check if user wants help with environment variables
    if std::env::args().any(|arg| arg == "--env-help") {
        print_env_help();
        return Ok(());
    }

    // Execute the complete integration workflow
    match run_integration_workflow(config).await {
        Ok(()) => {
            info!("Integration example completed successfully");
        }
        Err(e) => {
            error!("Integration example failed: {}", e);
            std::process::exit(1);
        }
    }
    
    Ok(())
}