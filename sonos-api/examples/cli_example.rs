//! # Sonos API CLI Example
//!
//! This is a minimal, interactive CLI example that demonstrates the core functionality
//! of the sonos-api crate. It provides device discovery, selection, and operation
//! execution through a simple numbered menu interface.
//!
//! ## Features
//!
//! - **Device Discovery**: Automatically discovers Sonos devices on your network
//! - **Interactive Menus**: Simple numbered menus for device and operation selection
//! - **Operation Execution**: Execute AVTransport and RenderingControl operations
//! - **Parameter Collection**: Dynamic parameter collection with validation
//! - **Error Handling**: Comprehensive error handling with user-friendly messages
//! - **Graceful Recovery**: Continue operation even when individual commands fail
//!
//! ## Usage
//!
//! Run the example with:
//! ```bash
//! cargo run --example cli_example
//! ```
//!
//! The CLI will guide you through:
//! 1. Device discovery (automatically finds Sonos speakers)
//! 2. Device selection (choose which speaker to control)
//! 3. Operation selection (choose what action to perform)
//! 4. Parameter input (provide any required parameters)
//! 5. Execution and results (see the operation results)
//!
//! ## Supported Operations
//!
//! ### AVTransport Service
//! - **Play**: Start playback with optional speed parameter
//! - **Pause**: Pause current playback
//! - **Stop**: Stop current playback
//! - **GetTransportInfo**: Get current playback state and information
//!
//! ### RenderingControl Service
//! - **GetVolume**: Get current volume level for a channel
//! - **SetVolume**: Set volume to a specific level (0-100)
//! - **SetRelativeVolume**: Adjust volume relatively
//! - **GetMute/SetMute**: Get or set mute state
//! - **GetBass/SetBass**: Get or set bass level (-10 to +10)
//! - **GetTreble/SetTreble**: Get or set treble level (-10 to +10)
//! - **GetLoudness/SetLoudness**: Get or set loudness compensation
//! - **SetRelativeVolume**: Adjust volume by a relative amount (-128 to +127)
//!
//! ## Requirements
//!
//! - Sonos speakers must be powered on and connected to the same network
//! - Network discovery must be allowed (check firewall settings)
//! - The computer running this example must be on the same network as the speakers
//!
//! ## Error Handling
//!
//! The CLI handles various error conditions gracefully:
//! - Network connectivity issues
//! - Device discovery timeouts
//! - Invalid user input
//! - SOAP operation failures
//! - Parameter validation errors
//!
//! Most errors allow you to retry or return to the previous menu.

use std::collections::HashMap;
use std::io::{self, Write};
use std::time::Duration;

use sonos_api::operation::{OperationBuilder, ValidationError};
use sonos_api::services::av_transport::{
    GetTransportInfoOperation, GetTransportInfoOperationRequest, PauseOperation,
    PauseOperationRequest, PlayOperation, PlayOperationRequest, StopOperation,
    StopOperationRequest,
};
use sonos_api::services::rendering_control::{
    GetBassOperation, GetBassOperationRequest, GetLoudnessOperation, GetLoudnessOperationRequest,
    GetMuteOperation, GetMuteOperationRequest, GetTrebleOperation, GetTrebleOperationRequest,
    GetVolumeOperation, GetVolumeOperationRequest, SetBassOperation, SetBassOperationRequest,
    SetLoudnessOperation, SetLoudnessOperationRequest, SetMuteOperation, SetMuteOperationRequest,
    SetRelativeVolumeOperation, SetRelativeVolumeOperationRequest, SetTrebleOperation,
    SetTrebleOperationRequest, SetVolumeOperation, SetVolumeOperationRequest,
};
use sonos_api::{ApiError, SonosClient};
use sonos_discovery::{get_with_timeout, Device, DiscoveryError};

/// CLI-specific error types for better error handling and user experience
#[derive(Debug, thiserror::Error)]
pub enum CliError {
    #[error("Device discovery error: {0}")]
    Discovery(#[from] DiscoveryError),

    #[error("API operation error: {0}")]
    Api(#[from] ApiError),

    #[error("Validation error: {0}")]
    Validation(#[from] ValidationError),

    #[error("Input/output error: {0}")]
    Io(#[from] io::Error),

    #[error("Input validation error: {0}")]
    Input(String),

    #[error("No devices found on the network")]
    NoDevicesFound,

    #[error("Operation not supported: {0}")]
    UnsupportedOperation(String),

    #[error("Missing required parameter: {0}")]
    MissingParameter(String),

    #[error("Invalid parameter value: {0}")]
    InvalidParameter(String),
}

/// Result type alias for CLI operations
pub type Result<T> = std::result::Result<T, CliError>;

/// Information about an available SOAP operation
#[derive(Debug, Clone)]
pub struct OperationInfo {
    pub name: String,
    pub service: String,
    pub description: String,
    pub parameters: Vec<ParameterInfo>,
}

/// Information about an operation parameter
#[derive(Debug, Clone)]
pub struct ParameterInfo {
    pub name: String,
    pub param_type: String,
    pub required: bool,
    pub default_value: Option<String>,
}

impl OperationInfo {
    pub fn new(name: &str, service: &str, description: &str) -> Self {
        Self {
            name: name.to_string(),
            service: service.to_string(),
            description: description.to_string(),
            parameters: Vec::new(),
        }
    }

    pub fn with_required_param(mut self, name: &str, param_type: &str) -> Self {
        self.parameters.push(ParameterInfo {
            name: name.to_string(),
            param_type: param_type.to_string(),
            required: true,
            default_value: None,
        });
        self
    }

    pub fn with_optional_param(mut self, name: &str, param_type: &str, default: &str) -> Self {
        self.parameters.push(ParameterInfo {
            name: name.to_string(),
            param_type: param_type.to_string(),
            required: false,
            default_value: Some(default.to_string()),
        });
        self
    }
}

/// Registry of available operations for the CLI
pub struct OperationRegistry {
    operations: Vec<OperationInfo>,
}

impl Default for OperationRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl OperationRegistry {
    pub fn new() -> Self {
        Self {
            operations: vec![
                // AVTransport operations
                OperationInfo::new("Play", "AVTransport", "Start playback")
                    .with_optional_param("speed", "String", "1"),
                OperationInfo::new("Pause", "AVTransport", "Pause playback"),
                OperationInfo::new("Stop", "AVTransport", "Stop playback"),
                OperationInfo::new(
                    "GetTransportInfo",
                    "AVTransport",
                    "Get current playback state",
                ),
                // RenderingControl operations
                OperationInfo::new("GetVolume", "RenderingControl", "Get current volume")
                    .with_optional_param("channel", "String", "Master"),
                OperationInfo::new("SetVolume", "RenderingControl", "Set volume level")
                    .with_required_param("volume", "u8")
                    .with_optional_param("channel", "String", "Master"),
                OperationInfo::new(
                    "SetRelativeVolume",
                    "RenderingControl",
                    "Adjust volume relatively",
                )
                .with_required_param("adjustment", "i8")
                .with_optional_param("channel", "String", "Master"),
                OperationInfo::new("GetMute", "RenderingControl", "Get mute state")
                    .with_optional_param("channel", "String", "Master"),
                OperationInfo::new("SetMute", "RenderingControl", "Set mute state")
                    .with_required_param("mute", "bool")
                    .with_optional_param("channel", "String", "Master"),
                OperationInfo::new("GetBass", "RenderingControl", "Get bass level (-10 to +10)"),
                OperationInfo::new("SetBass", "RenderingControl", "Set bass level (-10 to +10)")
                    .with_required_param("bass", "i8"),
                OperationInfo::new(
                    "GetTreble",
                    "RenderingControl",
                    "Get treble level (-10 to +10)",
                ),
                OperationInfo::new(
                    "SetTreble",
                    "RenderingControl",
                    "Set treble level (-10 to +10)",
                )
                .with_required_param("treble", "i8"),
                OperationInfo::new(
                    "GetLoudness",
                    "RenderingControl",
                    "Get loudness compensation state",
                )
                .with_optional_param("channel", "String", "Master"),
                OperationInfo::new(
                    "SetLoudness",
                    "RenderingControl",
                    "Set loudness compensation",
                )
                .with_required_param("loudness", "bool")
                .with_optional_param("channel", "String", "Master"),
            ],
        }
    }

    pub fn get_operations(&self) -> &[OperationInfo] {
        &self.operations
    }

    pub fn get_by_service(&self) -> HashMap<String, Vec<&OperationInfo>> {
        let mut grouped = HashMap::new();
        for op in &self.operations {
            grouped
                .entry(op.service.clone())
                .or_insert_with(Vec::new)
                .push(op);
        }
        grouped
    }
}

/// Discover Sonos devices on the network with timeout handling
///
/// This function wraps the sonos-discovery crate with CLI-specific error handling
/// and timeout management. It validates that at least one device is found.
///
/// # Arguments
///
/// * `timeout` - Maximum duration to wait for device discovery
///
/// # Returns
///
/// * `Ok(Vec<Device>)` - List of discovered devices
/// * `Err(CliError::NoDevicesFound)` - No devices found within timeout
/// * `Err(CliError::Discovery(_))` - Network or other discovery errors
///
/// # Requirements
///
/// Validates requirements 1.1, 1.3, 1.4 from the specification
pub async fn discover_devices() -> Result<Vec<Device>> {
    discover_devices_with_timeout(Duration::from_secs(5)).await
}

/// Discover Sonos devices with a custom timeout
///
/// # Arguments
///
/// * `timeout` - Maximum duration to wait for device discovery
pub async fn discover_devices_with_timeout(timeout: Duration) -> Result<Vec<Device>> {
    println!("Discovering Sonos devices on the network...");
    println!("This may take up to {} seconds...", timeout.as_secs());

    // Use tokio::task::spawn_blocking to run the blocking discovery in a separate thread
    let devices = tokio::task::spawn_blocking(move || get_with_timeout(timeout))
        .await
        .map_err(|e| {
            CliError::Discovery(DiscoveryError::NetworkError(format!(
                "Task join error: {e}"
            )))
        })?;

    if devices.is_empty() {
        println!("No Sonos devices found on the network.");
        println!("Please ensure:");
        println!("  - Your Sonos speakers are powered on");
        println!("  - You're connected to the same network as your speakers");
        println!("  - Your firewall allows network discovery");
        return Err(CliError::NoDevicesFound);
    }

    println!("✓ Found {} Sonos device(s)", devices.len());
    Ok(devices)
}

/// Display discovered devices in a formatted, numbered list
///
/// This function formats the device list according to requirement 1.2,
/// showing device name, room name, and IP address in a user-friendly format.
///
/// # Arguments
///
/// * `devices` - List of discovered devices to display
///
/// # Requirements
///
/// Validates requirement 1.2 from the specification
pub fn display_devices(devices: &[Device]) {
    println!("\nDiscovered Sonos Devices:");
    println!("{}", "=".repeat(25));

    for (i, device) in devices.iter().enumerate() {
        println!("{}. {} ({})", i + 1, device.name, device.room_name);
        println!(
            "   IP: {} | Model: {}",
            device.ip_address, device.model_name
        );
        if i < devices.len() - 1 {
            println!();
        }
    }

    println!();
}

/// Display a numbered menu with consistent formatting
///
/// This function provides a consistent interface for displaying numbered menus
/// throughout the CLI application. It formats items using a provided formatter
/// function and includes standard menu headers and exit options.
///
/// # Arguments
///
/// * `title` - The menu title to display
/// * `items` - List of items to display in the menu
/// * `formatter` - Function to format each item for display
///
/// # Requirements
///
/// Validates requirements 2.1, 5.1, 5.2 from the specification
pub fn display_menu<T>(title: &str, items: &[T], formatter: impl Fn(&T) -> String) {
    println!("\n{title}");
    println!("{}", "=".repeat(title.len()));

    for (i, item) in items.iter().enumerate() {
        println!("{}. {}", i + 1, formatter(item));
    }
    println!("0. Exit");
    println!();
}

/// Get user selection from a numbered menu with input validation
///
/// This function handles user input for menu selection with comprehensive
/// validation. It ensures the input is numeric and within the valid range,
/// providing clear error messages for invalid input.
///
/// # Arguments
///
/// * `max_value` - Maximum valid selection (number of menu items)
///
/// # Returns
///
/// * `Ok(Some(index))` - Valid selection (0-based index)
/// * `Ok(None)` - User chose to exit (selected 0)
/// * `Err(CliError)` - Invalid input with appropriate error message
///
/// # Requirements
///
/// Validates requirements 2.2, 2.3, 2.4, 2.5, 5.4 from the specification
pub fn get_user_selection(max_value: usize) -> Result<Option<usize>> {
    loop {
        print!("Enter your choice (0-{max_value}): ");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;

        let trimmed = input.trim();

        // Handle empty input
        if trimmed.is_empty() {
            println!("Please enter a number between 0 and {max_value}");
            continue;
        }

        // Parse the input as a number
        let choice: usize = match trimmed.parse() {
            Ok(num) => num,
            Err(_) => {
                println!("Invalid input: '{trimmed}' is not a valid number");
                println!("Please enter a number between 0 and {max_value}");
                continue;
            }
        };

        // Check if choice is 0 (exit)
        if choice == 0 {
            return Ok(None);
        }

        // Validate range
        if choice > max_value {
            println!("Invalid selection: {choice} is out of range");
            println!("Please enter a number between 0 and {max_value}");
            continue;
        }

        // Return valid selection (convert to 0-based index)
        return Ok(Some(choice - 1));
    }
}

/// Get user selection with a custom prompt message
///
/// This is a convenience function that allows for custom prompt messages
/// while maintaining the same validation logic.
///
/// # Arguments
///
/// * `prompt` - Custom prompt message to display
/// * `max_value` - Maximum valid selection (number of menu items)
pub fn get_user_selection_with_prompt(prompt: &str, max_value: usize) -> Result<Option<usize>> {
    print!("{prompt} (0-{max_value}): ");
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;

    let trimmed = input.trim();

    // Handle empty input
    if trimmed.is_empty() {
        return Err(CliError::Input("Please enter a valid number".to_string()));
    }

    // Parse the input as a number
    let choice: usize = trimmed
        .parse()
        .map_err(|_| CliError::Input(format!("'{trimmed}' is not a valid number")))?;

    // Check if choice is 0 (exit)
    if choice == 0 {
        return Ok(None);
    }

    // Validate range
    if choice > max_value {
        return Err(CliError::Input(format!(
            "Selection {choice} is out of range (1-{max_value})"
        )));
    }

    // Return valid selection (convert to 0-based index)
    Ok(Some(choice - 1))
}

/// Select a device from the discovered devices list
///
/// This function combines device display and selection into a single
/// user interaction, handling all validation and error cases.
///
/// # Arguments
///
/// * `devices` - List of discovered devices
///
/// # Returns
///
/// * `Ok(Some(Device))` - Selected device
/// * `Ok(None)` - User chose to exit
/// * `Err(CliError)` - Error during selection process
///
/// # Requirements
///
/// Validates requirements 2.1, 2.2, 2.3, 2.4, 2.5 from the specification
pub fn select_device(devices: &[Device]) -> Result<Option<&Device>> {
    if devices.is_empty() {
        return Err(CliError::NoDevicesFound);
    }

    display_menu("Select a Sonos Device", devices, |device| {
        format!(
            "{} ({}) - {}",
            device.name, device.room_name, device.ip_address
        )
    });

    match get_user_selection(devices.len())? {
        Some(index) => Ok(Some(&devices[index])),
        None => Ok(None),
    }
}

/// Collect parameters for a SOAP operation from user input
///
/// This function dynamically determines what parameters are required for an operation
/// and prompts the user for each parameter with appropriate validation. It handles
/// both required and optional parameters, providing default values where appropriate.
///
/// # Arguments
///
/// * `operation` - The operation information containing parameter definitions
///
/// # Returns
///
/// * `Ok(HashMap<String, String>)` - Collected parameters as key-value pairs
/// * `Err(CliError)` - Error during parameter collection or validation
///
/// # Requirements
///
/// Validates requirements 4.2, 4.3 from the specification
pub fn collect_parameters(operation: &OperationInfo) -> Result<HashMap<String, String>> {
    let mut params = HashMap::new();

    if operation.parameters.is_empty() {
        println!("This operation requires no parameters.");
        return Ok(params);
    }

    println!("\nCollecting parameters for operation: {}", operation.name);
    println!("{}", "=".repeat(40));

    for param in &operation.parameters {
        if param.required {
            // Required parameter - must collect from user
            let value = prompt_for_parameter(param)?;
            params.insert(param.name.clone(), value);
        } else {
            // Optional parameter - ask user if they want to provide it
            if should_prompt_optional_parameter(param)? {
                let value = prompt_for_parameter(param)?;
                params.insert(param.name.clone(), value);
            } else if let Some(default) = &param.default_value {
                // Use default value for optional parameter
                params.insert(param.name.clone(), default.clone());
                println!(
                    "Using default value '{}' for parameter '{}'",
                    default, param.name
                );
            }
        }
    }

    println!();
    Ok(params)
}

/// Prompt the user for a specific parameter value with type-based validation
///
/// This function handles the interactive collection of a single parameter value,
/// including input validation based on the parameter type and user-friendly
/// error messages for invalid input.
///
/// # Arguments
///
/// * `param` - Parameter information including name, type, and requirements
///
/// # Returns
///
/// * `Ok(String)` - Valid parameter value as string
/// * `Err(CliError)` - Error during input collection or validation
///
/// # Requirements
///
/// Validates requirement 4.3 from the specification
pub fn prompt_for_parameter(param: &ParameterInfo) -> Result<String> {
    loop {
        // Display parameter information
        print!("Enter {} ({})", param.name, param.param_type);

        if !param.required {
            if let Some(default) = &param.default_value {
                print!(" [default: {default}]");
            } else {
                print!(" [optional]");
            }
        }

        print!(": ");
        io::stdout().flush()?;

        // Read user input
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let value = input.trim().to_string();

        // Handle empty input for optional parameters
        if value.is_empty() && !param.required {
            if let Some(default) = &param.default_value {
                return Ok(default.clone());
            } else {
                return Ok(String::new());
            }
        }

        // Validate required parameters are not empty
        if value.is_empty() && param.required {
            println!("Error: {} is required and cannot be empty", param.name);
            continue;
        }

        // Validate parameter value based on type
        match validate_parameter_value(&value, &param.param_type) {
            Ok(()) => return Ok(value),
            Err(e) => {
                println!("Error: {e}");
                println!("Please try again.");
                continue;
            }
        }
    }
}

/// Ask user if they want to provide an optional parameter
///
/// This function prompts the user to decide whether to provide a value
/// for an optional parameter or use the default value.
///
/// # Arguments
///
/// * `param` - Parameter information for the optional parameter
///
/// # Returns
///
/// * `Ok(true)` - User wants to provide a custom value
/// * `Ok(false)` - User wants to use default value or skip
/// * `Err(CliError)` - Error during input collection
fn should_prompt_optional_parameter(param: &ParameterInfo) -> Result<bool> {
    if param.required {
        return Ok(true);
    }

    let default_text = if let Some(default) = &param.default_value {
        format!(" (default: {default})")
    } else {
        " (no default)".to_string()
    };

    loop {
        print!(
            "Provide custom value for optional parameter '{}'{}? (y/n): ",
            param.name, default_text
        );
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let response = input.trim().to_lowercase();

        match response.as_str() {
            "y" | "yes" => return Ok(true),
            "n" | "no" => return Ok(false),
            "" => return Ok(false), // Default to no for empty input
            _ => {
                println!("Please enter 'y' for yes or 'n' for no");
                continue;
            }
        }
    }
}

/// Validate a parameter value based on its expected type
///
/// This function performs type-based validation on parameter values,
/// ensuring they conform to the expected format before being used
/// in SOAP operations.
///
/// # Arguments
///
/// * `value` - The parameter value to validate
/// * `param_type` - The expected parameter type (e.g., "u8", "String", "i8")
///
/// # Returns
///
/// * `Ok(())` - Value is valid for the specified type
/// * `Err(CliError)` - Value is invalid with descriptive error message
///
/// # Requirements
///
/// Validates requirement 4.3 from the specification
pub fn validate_parameter_value(value: &str, param_type: &str) -> Result<()> {
    match param_type {
        "String" => {
            // String parameters are always valid (any text is acceptable)
            Ok(())
        }
        "u8" => {
            // Unsigned 8-bit integer (0-255)
            match value.parse::<u8>() {
                Ok(_) => Ok(()),
                Err(_) => Err(CliError::InvalidParameter(format!(
                    "'{value}' is not a valid u8 value (must be 0-255)"
                ))),
            }
        }
        "i8" => {
            // Signed 8-bit integer (-128 to 127)
            match value.parse::<i8>() {
                Ok(_) => Ok(()),
                Err(_) => Err(CliError::InvalidParameter(format!(
                    "'{value}' is not a valid i8 value (must be -128 to 127)"
                ))),
            }
        }
        "u16" => {
            // Unsigned 16-bit integer (0-65535)
            match value.parse::<u16>() {
                Ok(_) => Ok(()),
                Err(_) => Err(CliError::InvalidParameter(format!(
                    "'{value}' is not a valid u16 value (must be 0-65535)"
                ))),
            }
        }
        "i16" => {
            // Signed 16-bit integer (-32768 to 32767)
            match value.parse::<i16>() {
                Ok(_) => Ok(()),
                Err(_) => Err(CliError::InvalidParameter(format!(
                    "'{value}' is not a valid i16 value (must be -32768 to 32767)"
                ))),
            }
        }
        "u32" => {
            // Unsigned 32-bit integer
            match value.parse::<u32>() {
                Ok(_) => Ok(()),
                Err(_) => Err(CliError::InvalidParameter(format!(
                    "'{value}' is not a valid u32 value"
                ))),
            }
        }
        "i32" => {
            // Signed 32-bit integer
            match value.parse::<i32>() {
                Ok(_) => Ok(()),
                Err(_) => Err(CliError::InvalidParameter(format!(
                    "'{value}' is not a valid i32 value"
                ))),
            }
        }
        "bool" => {
            // Boolean values
            match value.to_lowercase().as_str() {
                "true" | "false" | "1" | "0" | "yes" | "no" => Ok(()),
                _ => Err(CliError::InvalidParameter(format!(
                    "'{value}' is not a valid boolean value (use true/false, 1/0, or yes/no)"
                ))),
            }
        }
        _ => {
            // Unknown type - treat as string but warn
            println!(
                "Warning: Unknown parameter type '{param_type}', treating as string"
            );
            Ok(())
        }
    }
}

/// Execute a SOAP operation on the selected device
///
/// This function takes an operation definition and collected parameters,
/// maps them to the appropriate SOAP operation, executes it using the
/// SonosClient, and returns a formatted result string.
///
/// # Arguments
///
/// * `client` - The SonosClient instance to use for execution
/// * `device` - The target Sonos device
/// * `operation` - The operation information containing name and service
/// * `params` - The collected parameters as key-value pairs
///
/// # Returns
///
/// * `Ok(String)` - Formatted result message from the operation
/// * `Err(CliError)` - Error during operation execution or parameter mapping
///
/// # Requirements
///
/// Validates requirements 4.4, 4.5, 4.6 from the specification
pub fn execute_operation(
    client: &SonosClient,
    device: &Device,
    operation: &OperationInfo,
    params: HashMap<String, String>,
) -> Result<String> {
    let device_ip = &device.ip_address;

    println!(
        "Executing {} operation on {} ({})...",
        operation.name, device.name, device.room_name
    );

    match (operation.service.as_str(), operation.name.as_str()) {
        // AVTransport operations
        ("AVTransport", "Play") => {
            let speed = params.get("speed").unwrap_or(&"1".to_string()).clone();
            let request = PlayOperationRequest {
                instance_id: 0,
                speed,
            };

            let operation = OperationBuilder::<PlayOperation>::new(request).build()?;
            client.execute_enhanced(device_ip, operation)?;
            Ok(format!("✓ Playback started on {}", device.name))
        }

        ("AVTransport", "Pause") => {
            let request = PauseOperationRequest { instance_id: 0 };
            let operation = OperationBuilder::<PauseOperation>::new(request).build()?;
            client.execute_enhanced(device_ip, operation)?;
            Ok(format!("✓ Playback paused on {}", device.name))
        }

        ("AVTransport", "Stop") => {
            let request = StopOperationRequest { instance_id: 0 };
            let operation = OperationBuilder::<StopOperation>::new(request).build()?;
            client.execute_enhanced(device_ip, operation)?;
            Ok(format!("✓ Playback stopped on {}", device.name))
        }

        ("AVTransport", "GetTransportInfo") => {
            let request = GetTransportInfoOperationRequest { instance_id: 0 };
            let operation = OperationBuilder::<GetTransportInfoOperation>::new(request).build()?;
            let response = client.execute_enhanced(device_ip, operation)?;

            Ok(format!(
                "✓ Transport Info for {}:\n  State: {}\n  Status: {}\n  Speed: {}",
                device.name,
                response.current_transport_state,
                response.current_transport_status,
                response.current_speed
            ))
        }

        // RenderingControl operations
        ("RenderingControl", "GetVolume") => {
            let channel = params
                .get("channel")
                .unwrap_or(&"Master".to_string())
                .clone();
            let request = GetVolumeOperationRequest {
                instance_id: 0,
                channel: channel.clone(),
            };

            let operation = OperationBuilder::<GetVolumeOperation>::new(request).build()?;
            let response = client.execute_enhanced(device_ip, operation)?;
            Ok(format!(
                "✓ Current volume on {} ({}): {}",
                device.name, channel, response.current_volume
            ))
        }

        ("RenderingControl", "SetVolume") => {
            let volume_str = params
                .get("volume")
                .ok_or_else(|| CliError::MissingParameter("volume".to_string()))?;
            let volume: u8 = volume_str.parse().map_err(|_| {
                CliError::InvalidParameter(format!(
                    "Volume must be a number between 0-100, got '{volume_str}'"
                ))
            })?;

            if volume > 100 {
                return Err(CliError::InvalidParameter(format!(
                    "Volume must be between 0-100, got {volume}"
                )));
            }

            let channel = params
                .get("channel")
                .unwrap_or(&"Master".to_string())
                .clone();
            let request = SetVolumeOperationRequest {
                instance_id: 0,
                channel: channel.clone(),
                desired_volume: volume,
            };

            let operation = OperationBuilder::<SetVolumeOperation>::new(request).build()?;
            client.execute_enhanced(device_ip, operation)?;
            Ok(format!(
                "✓ Volume set to {} on {} ({})",
                volume, device.name, channel
            ))
        }

        ("RenderingControl", "SetRelativeVolume") => {
            let adjustment_str = params
                .get("adjustment")
                .ok_or_else(|| CliError::MissingParameter("adjustment".to_string()))?;
            let adjustment: i8 = adjustment_str.parse().map_err(|_| {
                CliError::InvalidParameter(format!(
                    "Adjustment must be a number between -128 to 127, got '{adjustment_str}'"
                ))
            })?;

            let channel = params
                .get("channel")
                .unwrap_or(&"Master".to_string())
                .clone();
            let request = SetRelativeVolumeOperationRequest {
                instance_id: 0,
                channel: channel.clone(),
                adjustment,
            };

            let operation = OperationBuilder::<SetRelativeVolumeOperation>::new(request).build()?;
            let response = client.execute_enhanced(device_ip, operation)?;
            let direction = if adjustment > 0 {
                "increased"
            } else if adjustment < 0 {
                "decreased"
            } else {
                "unchanged"
            };

            Ok(format!(
                "✓ Volume {} by {} on {} ({})\n  New volume: {}",
                direction,
                adjustment.abs(),
                device.name,
                channel,
                response.new_volume
            ))
        }

        ("RenderingControl", "GetMute") => {
            let channel = params
                .get("channel")
                .unwrap_or(&"Master".to_string())
                .clone();
            let request = GetMuteOperationRequest {
                instance_id: 0,
                channel: channel.clone(),
            };

            let operation = OperationBuilder::<GetMuteOperation>::new(request).build()?;
            let response = client.execute_enhanced(device_ip, operation)?;
            Ok(format!(
                "✓ Mute state on {} ({}): {}",
                device.name,
                channel,
                if response.current_mute {
                    "MUTED"
                } else {
                    "unmuted"
                }
            ))
        }

        ("RenderingControl", "SetMute") => {
            let mute_str = params
                .get("mute")
                .ok_or_else(|| CliError::MissingParameter("mute".to_string()))?;
            let mute: bool = match mute_str.to_lowercase().as_str() {
                "true" | "1" | "yes" => true,
                "false" | "0" | "no" => false,
                _ => {
                    return Err(CliError::InvalidParameter(format!(
                        "Mute must be true/false, got '{mute_str}'"
                    )))
                }
            };

            let channel = params
                .get("channel")
                .unwrap_or(&"Master".to_string())
                .clone();
            let request = SetMuteOperationRequest {
                instance_id: 0,
                channel: channel.clone(),
                desired_mute: mute,
            };

            let operation = OperationBuilder::<SetMuteOperation>::new(request).build()?;
            client.execute_enhanced(device_ip, operation)?;
            Ok(format!(
                "✓ {} on {} ({})",
                if mute { "Muted" } else { "Unmuted" },
                device.name,
                channel
            ))
        }

        ("RenderingControl", "GetBass") => {
            let request = GetBassOperationRequest { instance_id: 0 };
            let operation = OperationBuilder::<GetBassOperation>::new(request).build()?;
            let response = client.execute_enhanced(device_ip, operation)?;
            Ok(format!(
                "✓ Bass on {}: {} (range: -10 to +10)",
                device.name, response.current_bass
            ))
        }

        ("RenderingControl", "SetBass") => {
            let bass_str = params
                .get("bass")
                .ok_or_else(|| CliError::MissingParameter("bass".to_string()))?;
            let bass: i8 = bass_str.parse().map_err(|_| {
                CliError::InvalidParameter(format!(
                    "Bass must be a number between -10 and 10, got '{bass_str}'"
                ))
            })?;

            let request = SetBassOperationRequest {
                instance_id: 0,
                desired_bass: bass,
            };

            let operation = OperationBuilder::<SetBassOperation>::new(request).build()?;
            client.execute_enhanced(device_ip, operation)?;
            Ok(format!("✓ Bass set to {} on {}", bass, device.name))
        }

        ("RenderingControl", "GetTreble") => {
            let request = GetTrebleOperationRequest { instance_id: 0 };
            let operation = OperationBuilder::<GetTrebleOperation>::new(request).build()?;
            let response = client.execute_enhanced(device_ip, operation)?;
            Ok(format!(
                "✓ Treble on {}: {} (range: -10 to +10)",
                device.name, response.current_treble
            ))
        }

        ("RenderingControl", "SetTreble") => {
            let treble_str = params
                .get("treble")
                .ok_or_else(|| CliError::MissingParameter("treble".to_string()))?;
            let treble: i8 = treble_str.parse().map_err(|_| {
                CliError::InvalidParameter(format!(
                    "Treble must be a number between -10 and 10, got '{treble_str}'"
                ))
            })?;

            let request = SetTrebleOperationRequest {
                instance_id: 0,
                desired_treble: treble,
            };

            let operation = OperationBuilder::<SetTrebleOperation>::new(request).build()?;
            client.execute_enhanced(device_ip, operation)?;
            Ok(format!("✓ Treble set to {} on {}", treble, device.name))
        }

        ("RenderingControl", "GetLoudness") => {
            let channel = params
                .get("channel")
                .unwrap_or(&"Master".to_string())
                .clone();
            let request = GetLoudnessOperationRequest {
                instance_id: 0,
                channel: channel.clone(),
            };

            let operation = OperationBuilder::<GetLoudnessOperation>::new(request).build()?;
            let response = client.execute_enhanced(device_ip, operation)?;
            Ok(format!(
                "✓ Loudness on {} ({}): {}",
                device.name,
                channel,
                if response.current_loudness {
                    "ON"
                } else {
                    "off"
                }
            ))
        }

        ("RenderingControl", "SetLoudness") => {
            let loudness_str = params
                .get("loudness")
                .ok_or_else(|| CliError::MissingParameter("loudness".to_string()))?;
            let loudness: bool = match loudness_str.to_lowercase().as_str() {
                "true" | "1" | "yes" => true,
                "false" | "0" | "no" => false,
                _ => {
                    return Err(CliError::InvalidParameter(format!(
                        "Loudness must be true/false, got '{loudness_str}'"
                    )))
                }
            };

            let channel = params
                .get("channel")
                .unwrap_or(&"Master".to_string())
                .clone();
            let request = SetLoudnessOperationRequest {
                instance_id: 0,
                channel: channel.clone(),
                desired_loudness: loudness,
            };

            let operation = OperationBuilder::<SetLoudnessOperation>::new(request).build()?;
            client.execute_enhanced(device_ip, operation)?;
            Ok(format!(
                "✓ Loudness {} on {} ({})",
                if loudness { "enabled" } else { "disabled" },
                device.name,
                channel
            ))
        }

        _ => Err(CliError::UnsupportedOperation(format!(
            "Operation {}.{} is not supported",
            operation.service, operation.name
        ))),
    }
}

/// Run the operation menu loop for a selected device
///
/// This function displays the available operations, handles user selection,
/// collects parameters, executes the operation, and displays results.
/// It continues in a loop until the user chooses to exit.
///
/// # Arguments
///
/// * `client` - The SonosClient instance to use for operations
/// * `device` - The selected Sonos device
/// * `registry` - The operation registry containing available operations
///
/// # Returns
///
/// * `Ok(bool)` - true to continue the loop, false to exit
/// * `Err(CliError)` - Error during operation execution
///
/// # Requirements
///
/// Validates requirements 3.1, 4.1, 4.4, 4.5, 4.6 from the specification
pub fn run_operation_menu(
    client: &SonosClient,
    device: &Device,
    registry: &OperationRegistry,
) -> Result<bool> {
    let _operations = registry.get_operations();
    let grouped_operations = registry.get_by_service();

    // Display operations grouped by service
    println!(
        "\nAvailable Operations for {} ({}):",
        device.name, device.room_name
    );
    println!("{}", "=".repeat(50));

    let mut operation_list = Vec::new();
    for (service, ops) in &grouped_operations {
        println!("\n{service}:");
        for op in ops {
            operation_list.push(*op);
            println!(
                "  {}. {} - {}",
                operation_list.len(),
                op.name,
                op.description
            );
        }
    }

    println!("\n0. Return to device selection");
    println!();

    // Get user selection
    match get_user_selection(operation_list.len())? {
        Some(index) => {
            let selected_operation = &operation_list[index];

            println!(
                "\nSelected operation: {} - {}",
                selected_operation.name, selected_operation.description
            );

            // Collect parameters for the operation
            let params = collect_parameters(selected_operation)?;

            // Execute the operation
            match execute_operation(client, device, selected_operation, params) {
                Ok(result) => {
                    println!("\n{result}");
                    println!("\nPress Enter to continue...");
                    let mut input = String::new();
                    io::stdin().read_line(&mut input)?;
                }
                Err(e) => {
                    eprintln!("\nOperation failed: {e}");
                    println!("Press Enter to continue...");
                    let mut input = String::new();
                    io::stdin().read_line(&mut input)?;
                }
            }

            Ok(true) // Continue the loop
        }
        None => Ok(false), // User chose to exit
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Display welcome message and setup information
    display_welcome_message();

    // Initialize the operation registry and client
    // Note: SonosClient::new() now uses a shared SOAP client for resource efficiency
    let registry = OperationRegistry::new();
    let client = SonosClient::new();
    println!("✓ Sonos API client initialized (using shared HTTP connection pool)");
    println!(
        "✓ Operation registry loaded with {} operations",
        registry.get_operations().len()
    );

    // Setup graceful shutdown handling
    setup_signal_handling();

    println!();

    // Discover devices with enhanced error handling
    let devices = match discover_devices_with_enhanced_error_handling().await {
        Ok(devices) => devices,
        Err(e) => {
            display_discovery_error(&e);
            return Err(e);
        }
    };

    display_devices(&devices);

    // Main application loop with enhanced error recovery
    println!("🎵 Ready to control your Sonos speakers!");
    println!("   Use Ctrl+C at any time to exit gracefully");
    println!();

    loop {
        // Device selection with enhanced prompts
        match select_device_with_enhanced_prompts(&devices)? {
            Some(device) => {
                println!(
                    "\n✓ Selected device: {} ({})",
                    device.name, device.room_name
                );
                println!("  IP Address: {}", device.ip_address);
                println!("  Model: {}", device.model_name);

                // Operation menu loop for the selected device
                loop {
                    match run_operation_menu_with_enhanced_error_handling(
                        &client, device, &registry,
                    ) {
                        Ok(should_continue) => {
                            if !should_continue {
                                println!("\n← Returning to device selection...");
                                break; // Return to device selection
                            }
                        }
                        Err(e) => {
                            display_operation_error(&e);
                            if !should_retry_after_error(&e)? {
                                break; // Return to device selection
                            }
                        }
                    }
                }
            }
            None => {
                display_goodbye_message();
                break; // Exit the application
            }
        }
    }

    Ok(())
}

/// Display a comprehensive welcome message with usage instructions
fn display_welcome_message() {
    println!("🎵 Sonos API CLI Example");
    println!("========================");
    println!();
    println!("This interactive CLI demonstrates the sonos-api crate functionality.");
    println!("You can discover Sonos devices and execute various control operations.");
    println!();
    println!("📋 What you can do:");
    println!("   • Discover Sonos speakers on your network");
    println!("   • Control playback (play, pause, stop)");
    println!("   • Adjust volume settings");
    println!("   • Get device status information");
    println!();
    println!("🔧 Requirements:");
    println!("   • Sonos speakers must be powered on");
    println!("   • Connected to the same network as this computer");
    println!("   • Network discovery allowed (check firewall)");
    println!();
}

/// Setup signal handling for graceful shutdown
fn setup_signal_handling() {
    // Note: In a real application, you might want to use tokio::signal
    // For this example, we'll rely on the default Ctrl+C handling
    println!("✓ Signal handling configured (Ctrl+C to exit)");
}

/// Enhanced device discovery with better error messages and retry options
async fn discover_devices_with_enhanced_error_handling() -> Result<Vec<Device>> {
    const MAX_RETRIES: u32 = 3;
    let mut attempt = 1;

    loop {
        println!(
            "🔍 Discovering Sonos devices... (attempt {attempt}/{MAX_RETRIES})"
        );

        match discover_devices().await {
            Ok(devices) => return Ok(devices),
            Err(CliError::NoDevicesFound) if attempt < MAX_RETRIES => {
                println!("   No devices found on attempt {attempt}");
                if should_retry_discovery()? {
                    attempt += 1;
                    continue;
                } else {
                    return Err(CliError::NoDevicesFound);
                }
            }
            Err(e) if attempt < MAX_RETRIES => {
                println!("   Discovery failed: {e}");
                if should_retry_discovery()? {
                    attempt += 1;
                    continue;
                } else {
                    return Err(e);
                }
            }
            Err(e) => return Err(e),
        }
    }
}

/// Ask user if they want to retry device discovery
fn should_retry_discovery() -> Result<bool> {
    println!();
    println!("Would you like to try discovering devices again?");
    println!("This might help if devices are still starting up or network is slow.");

    loop {
        print!("Retry discovery? (y/n): ");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let response = input.trim().to_lowercase();

        match response.as_str() {
            "y" | "yes" => return Ok(true),
            "n" | "no" => return Ok(false),
            "" => return Ok(false), // Default to no for empty input
            _ => {
                println!("Please enter 'y' for yes or 'n' for no");
                continue;
            }
        }
    }
}

/// Display comprehensive error information for discovery failures
fn display_discovery_error(error: &CliError) {
    println!();
    println!("❌ Device Discovery Failed");
    println!("{}", "=".repeat(25));
    println!();

    match error {
        CliError::NoDevicesFound => {
            println!("No Sonos devices were found on your network.");
            println!();
            println!("💡 Troubleshooting tips:");
            println!("   1. Ensure your Sonos speakers are powered on");
            println!("   2. Check that you're on the same WiFi network as your speakers");
            println!("   3. Verify your firewall allows network discovery");
            println!("   4. Try opening the Sonos app to ensure speakers are responsive");
            println!("   5. Wait a moment and try running the example again");
        }
        CliError::Discovery(discovery_error) => {
            println!("Network discovery error: {discovery_error}");
            println!();
            println!("💡 This might be a temporary network issue.");
            println!("   Try running the example again in a few moments.");
        }
        _ => {
            println!("Unexpected error during discovery: {error}");
        }
    }

    println!();
    println!("This CLI example requires Sonos devices to demonstrate the");
    println!("full operation execution functionality of the sonos-api crate.");
}

/// Enhanced device selection with better prompts and help text
fn select_device_with_enhanced_prompts(devices: &[Device]) -> Result<Option<&Device>> {
    if devices.is_empty() {
        return Err(CliError::NoDevicesFound);
    }

    println!("📱 Select a Sonos Device to Control");
    println!("{}", "=".repeat(35));

    for (i, device) in devices.iter().enumerate() {
        println!("{}. {} ({})", i + 1, device.name, device.room_name);
        println!("   📍 {} | 🔧 {}", device.ip_address, device.model_name);
        if i < devices.len() - 1 {
            println!();
        }
    }

    println!();
    println!("0. Exit application");
    println!();
    println!("💡 Tip: Choose the device you want to control");

    match get_user_selection(devices.len())? {
        Some(index) => Ok(Some(&devices[index])),
        None => Ok(None),
    }
}

/// Enhanced operation menu with better error handling and recovery
fn run_operation_menu_with_enhanced_error_handling(
    client: &SonosClient,
    device: &Device,
    registry: &OperationRegistry,
) -> Result<bool> {
    let _operations = registry.get_operations();
    let grouped_operations = registry.get_by_service();

    // Display operations grouped by service with enhanced formatting
    println!(
        "\n🎛️  Available Operations for {} ({})",
        device.name, device.room_name
    );
    println!("{}", "=".repeat(60));

    let mut operation_list = Vec::new();
    for (service, ops) in &grouped_operations {
        println!("\n📂 {service}:");
        for op in ops {
            operation_list.push(*op);
            println!(
                "  {}. {} - {}",
                operation_list.len(),
                op.name,
                op.description
            );
        }
    }

    println!();
    println!("0. ← Return to device selection");
    println!();
    println!("💡 Tip: Select an operation to execute on {}", device.name);

    // Get user selection
    match get_user_selection(operation_list.len())? {
        Some(index) => {
            let selected_operation = &operation_list[index];

            println!(
                "\n🚀 Executing: {} - {}",
                selected_operation.name, selected_operation.description
            );
            println!("   Target device: {} ({})", device.name, device.room_name);

            // Collect parameters for the operation
            let params = collect_parameters_with_enhanced_prompts(selected_operation)?;

            // Execute the operation with enhanced error handling
            match execute_operation_with_enhanced_feedback(
                client,
                device,
                selected_operation,
                params,
            ) {
                Ok(result) => {
                    display_operation_success(&result);
                }
                Err(e) => {
                    display_operation_error(&e);
                    // Don't return error here - let user continue trying operations
                }
            }

            // Always continue the loop after an operation attempt
            Ok(true)
        }
        None => Ok(false), // User chose to exit
    }
}

/// Enhanced parameter collection with better prompts and help
fn collect_parameters_with_enhanced_prompts(
    operation: &OperationInfo,
) -> Result<HashMap<String, String>> {
    let mut params = HashMap::new();

    if operation.parameters.is_empty() {
        println!("✓ This operation requires no parameters - ready to execute!");
        return Ok(params);
    }

    println!("\n📝 Parameter Collection for: {}", operation.name);
    println!("{}", "=".repeat(50));
    println!("Please provide the following parameters:");
    println!();

    for (i, param) in operation.parameters.iter().enumerate() {
        println!("Parameter {} of {}:", i + 1, operation.parameters.len());

        if param.required {
            // Required parameter - must collect from user
            let value = prompt_for_parameter_with_enhanced_help(param)?;
            params.insert(param.name.clone(), value);
        } else {
            // Optional parameter - ask user if they want to provide it
            if should_prompt_optional_parameter_with_enhanced_help(param)? {
                let value = prompt_for_parameter_with_enhanced_help(param)?;
                params.insert(param.name.clone(), value);
            } else if let Some(default) = &param.default_value {
                // Use default value for optional parameter
                params.insert(param.name.clone(), default.clone());
                println!(
                    "✓ Using default value '{}' for parameter '{}'",
                    default, param.name
                );
            }
        }

        if i < operation.parameters.len() - 1 {
            println!();
        }
    }

    println!("\n✓ All parameters collected successfully!");
    Ok(params)
}

/// Enhanced parameter prompting with help text and examples
fn prompt_for_parameter_with_enhanced_help(param: &ParameterInfo) -> Result<String> {
    // Display parameter help information
    println!("  📋 Parameter: {}", param.name);
    println!("     Type: {}", param.param_type);

    // Add type-specific help and examples
    match param.param_type.as_str() {
        "u8" => println!("     Range: 0-255 (e.g., volume: 0-100)"),
        "i8" => println!("     Range: -128 to 127 (e.g., volume adjustment: -10 to +10)"),
        "String" => println!("     Text value (e.g., 'Master' for channel)"),
        _ => {}
    }

    if param.required {
        println!("     ⚠️  Required parameter");
    } else if let Some(default) = &param.default_value {
        println!("     💡 Default: {default}");
    }

    loop {
        // Display parameter information
        print!("     Enter {} ({})", param.name, param.param_type);

        if !param.required {
            if let Some(default) = &param.default_value {
                print!(" [default: {default}]");
            } else {
                print!(" [optional]");
            }
        }

        print!(": ");
        io::stdout().flush()?;

        // Read user input
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let value = input.trim().to_string();

        // Handle empty input for optional parameters
        if value.is_empty() && !param.required {
            if let Some(default) = &param.default_value {
                return Ok(default.clone());
            } else {
                return Ok(String::new());
            }
        }

        // Validate required parameters are not empty
        if value.is_empty() && param.required {
            println!(
                "     ❌ Error: {} is required and cannot be empty",
                param.name
            );
            continue;
        }

        // Validate parameter value based on type
        match validate_parameter_value(&value, &param.param_type) {
            Ok(()) => {
                println!("     ✓ Valid {} value: {}", param.param_type, value);
                return Ok(value);
            }
            Err(e) => {
                println!("     ❌ Error: {e}");
                println!(
                    "     Please try again with a valid {} value.",
                    param.param_type
                );
                continue;
            }
        }
    }
}

/// Enhanced optional parameter prompting with better explanations
fn should_prompt_optional_parameter_with_enhanced_help(param: &ParameterInfo) -> Result<bool> {
    if param.required {
        return Ok(true);
    }

    println!("  🔧 Optional Parameter: {}", param.name);

    let default_text = if let Some(default) = &param.default_value {
        format!(" (default: {default})")
    } else {
        " (no default)".to_string()
    };

    println!("     Type: {}{}", param.param_type, default_text);

    loop {
        print!("     Provide custom value? (y/n): ");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let response = input.trim().to_lowercase();

        match response.as_str() {
            "y" | "yes" => {
                println!("     → Will prompt for custom value");
                return Ok(true);
            }
            "n" | "no" | "" => {
                if let Some(default) = &param.default_value {
                    println!("     → Will use default value: {default}");
                } else {
                    println!("     → Will skip this parameter");
                }
                return Ok(false);
            }
            _ => {
                println!("     Please enter 'y' for yes or 'n' for no");
                continue;
            }
        }
    }
}

/// Enhanced operation execution with detailed feedback
fn execute_operation_with_enhanced_feedback(
    client: &SonosClient,
    device: &Device,
    operation: &OperationInfo,
    params: HashMap<String, String>,
) -> Result<String> {
    println!("\n⚡ Executing operation...");
    println!("   Operation: {}", operation.name);
    println!("   Service: {}", operation.service);
    println!("   Target: {} ({})", device.name, device.ip_address);

    if !params.is_empty() {
        println!("   Parameters:");
        for (key, value) in &params {
            println!("     {key}: {value}");
        }
    }

    println!();

    // Execute the operation (reuse existing implementation)
    execute_operation(client, device, operation, params)
}

/// Display operation success with enhanced formatting
fn display_operation_success(result: &str) {
    println!("✅ Operation Completed Successfully!");
    println!("{}", "=".repeat(35));
    println!();
    println!("{result}");
    println!();
    println!("Press Enter to continue...");
    let mut input = String::new();
    let _ = io::stdin().read_line(&mut input);
}

/// Display operation errors with enhanced formatting and recovery suggestions
fn display_operation_error(error: &CliError) {
    println!();
    println!("❌ Operation Failed");
    println!("{}", "=".repeat(18));
    println!();

    match error {
        CliError::Api(api_error) => {
            println!("SOAP API Error: {api_error}");
            println!();
            println!("💡 This might be because:");
            println!("   • The device is busy with another operation");
            println!("   • The requested operation is not supported in current state");
            println!("   • Network connectivity issues");
            println!("   • The device needs to be restarted");
        }
        CliError::InvalidParameter(msg) => {
            println!("Parameter Error: {msg}");
            println!();
            println!("💡 Please check your parameter values and try again.");
        }
        CliError::MissingParameter(param) => {
            println!("Missing Parameter: {param}");
            println!();
            println!("💡 This operation requires the '{param}' parameter.");
        }
        CliError::UnsupportedOperation(op) => {
            println!("Unsupported Operation: {op}");
            println!();
            println!("💡 This operation is not yet implemented in the CLI example.");
        }
        _ => {
            println!("Error: {error}");
            println!();
            println!("💡 This might be a temporary issue - you can try again.");
        }
    }

    println!();
    println!("Press Enter to continue...");
    let mut input = String::new();
    let _ = io::stdin().read_line(&mut input);
}

/// Ask user if they want to retry after an error
fn should_retry_after_error(error: &CliError) -> Result<bool> {
    match error {
        CliError::Api(_) | CliError::InvalidParameter(_) | CliError::MissingParameter(_) => {
            // These errors are recoverable - user can try different operations
            Ok(true)
        }
        _ => {
            // For other errors, ask the user
            println!("Would you like to try another operation on this device?");

            loop {
                print!("Continue with this device? (y/n): ");
                io::stdout().flush()?;

                let mut input = String::new();
                io::stdin().read_line(&mut input)?;
                let response = input.trim().to_lowercase();

                match response.as_str() {
                    "y" | "yes" => return Ok(true),
                    "n" | "no" => return Ok(false),
                    "" => return Ok(false), // Default to no for empty input
                    _ => {
                        println!("Please enter 'y' for yes or 'n' for no");
                        continue;
                    }
                }
            }
        }
    }
}

/// Display goodbye message with usage summary
fn display_goodbye_message() {
    println!();
    println!("👋 Thank you for using the Sonos API CLI Example!");
    println!("{}", "=".repeat(45));
    println!();
    println!("🎵 What you experienced:");
    println!("   • Device discovery using the sonos-discovery crate");
    println!("   • Interactive operation selection and execution");
    println!("   • Type-safe SOAP operations via the sonos-api crate");
    println!("   • Comprehensive error handling and recovery");
    println!();
    println!("📚 To learn more:");
    println!("   • Check the sonos-api crate documentation");
    println!("   • Explore the source code in sonos-api/examples/cli_example.rs");
    println!("   • Try integrating these operations into your own applications");
    println!();
    println!("Happy coding! 🚀");
}
