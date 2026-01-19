//! Test arbitrary UPnP operations against a Sonos speaker
//!
//! This example sends raw SOAP requests to test operations during development.
//!
//! Usage:
//!   cargo run -p sonos-api --example test_operation -- <ip> <service> <action> [param=value...]
//!
//! Examples:
//!   cargo run -p sonos-api --example test_operation -- 192.168.1.100 AVTransport GetTransportInfo
//!   cargo run -p sonos-api --example test_operation -- 192.168.1.100 RenderingControl GetVolume Channel=Master
//!   cargo run -p sonos-api --example test_operation -- 192.168.1.100 AVTransport Play Speed=1

use std::collections::HashMap;
use std::env;

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 4 {
        eprintln!("Usage: {} <ip> <service> <action> [param=value...]", args[0]);
        eprintln!();
        eprintln!("Services: AVTransport, RenderingControl, ZoneGroupTopology, GroupRenderingControl");
        eprintln!();
        eprintln!("Examples:");
        eprintln!("  {} 192.168.1.100 AVTransport GetTransportInfo", args[0]);
        eprintln!("  {} 192.168.1.100 RenderingControl GetVolume Channel=Master", args[0]);
        eprintln!("  {} 192.168.1.100 AVTransport Play Speed=1", args[0]);
        std::process::exit(1);
    }

    let ip = &args[1];
    let service = &args[2];
    let action = &args[3];

    // Parse parameters
    let mut params: HashMap<String, String> = HashMap::new();
    params.insert("InstanceID".to_string(), "0".to_string()); // Default InstanceID

    for arg in args.iter().skip(4) {
        if let Some((key, value)) = arg.split_once('=') {
            params.insert(key.to_string(), value.to_string());
        } else {
            eprintln!("Warning: ignoring malformed parameter '{}'", arg);
        }
    }

    // Build the SOAP body
    let body_content: String = params
        .iter()
        .map(|(k, v)| format!("<{}>{}</{}>", k, v, k))
        .collect::<Vec<_>>()
        .join("");

    let (endpoint, service_uri) = match service.to_lowercase().as_str() {
        "avtransport" => (
            "MediaRenderer/AVTransport/Control",
            "urn:schemas-upnp-org:service:AVTransport:1",
        ),
        "renderingcontrol" => (
            "MediaRenderer/RenderingControl/Control",
            "urn:schemas-upnp-org:service:RenderingControl:1",
        ),
        "zonegrouptopology" => (
            "ZoneGroupTopology/Control",
            "urn:schemas-upnp-org:service:ZoneGroupTopology:1",
        ),
        "grouprenderingcontrol" => (
            "MediaRenderer/GroupRenderingControl/Control",
            "urn:schemas-upnp-org:service:GroupRenderingControl:1",
        ),
        _ => {
            eprintln!("Unknown service: {}. Use: AVTransport, RenderingControl, ZoneGroupTopology, GroupRenderingControl", service);
            std::process::exit(1);
        }
    };

    let url = format!("http://{}:1400/{}", ip, endpoint);

    let soap_body = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
  <s:Body>
    <u:{action} xmlns:u="{service_uri}">
      {body_content}
    </u:{action}>
  </s:Body>
</s:Envelope>"#,
        action = action,
        service_uri = service_uri,
        body_content = body_content
    );

    let soap_action = format!("{}#{}", service_uri, action);

    println!("=== Request ===");
    println!("URL: {}", url);
    println!("SOAPAction: {}", soap_action);
    println!("Body:\n{}\n", soap_body);

    // Send the request
    let client = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(10))
        .build();

    match client
        .post(&url)
        .set("Content-Type", "text/xml; charset=utf-8")
        .set("SOAPAction", &soap_action)
        .send_string(&soap_body)
    {
        Ok(response) => {
            let status = response.status();
            let body = response.into_string().unwrap_or_else(|e| format!("Error reading body: {}", e));

            println!("=== Response ===");
            println!("Status: {}", status);
            println!("Body:\n{}", body);

            if status >= 200 && status < 300 {
                println!("\n=== Success ===");
            } else {
                println!("\n=== Error (Status {}) ===", status);
            }
        }
        Err(e) => {
            eprintln!("=== Error ===");
            eprintln!("Request failed: {}", e);
            std::process::exit(1);
        }
    }
}
