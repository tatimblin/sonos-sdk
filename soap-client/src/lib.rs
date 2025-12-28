//! Private SOAP client for UPnP device communication
//! 
//! This crate provides a minimal SOAP client specifically designed for
//! communicating with UPnP devices like Sonos speakers. It also supports
//! UPnP event subscriptions using SUBSCRIBE/UNSUBSCRIBE methods.

mod error;

pub use error::SoapError;

use std::time::Duration;
use xmltree::Element;

/// Response from a UPnP subscription request
#[derive(Debug, Clone)]
pub struct SubscriptionResponse {
    /// Subscription ID returned by the device
    pub sid: String,
    /// Actual timeout granted by the device (in seconds)
    pub timeout_seconds: u32,
}

/// A minimal SOAP client for UPnP device communication
#[derive(Debug, Clone)]
pub struct SoapClient {
    agent: ureq::Agent,
}

impl SoapClient {
    /// Create a new SOAP client with default configuration
    pub fn new() -> Self {
        Self {
            agent: ureq::AgentBuilder::new()
                .timeout_connect(Duration::from_secs(5))
                .timeout_read(Duration::from_secs(10))
                .build(),
        }
    }

    /// Send a SOAP request and return the parsed response element
    pub fn call(
        &self,
        ip: &str,
        endpoint: &str,
        service_uri: &str,
        action: &str,
        payload: &str,
    ) -> Result<Element, SoapError> {
        // Inline SOAP envelope construction - no separate module needed
        let body = format!(
            r#"<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
                <s:Body>
                    <u:{action} xmlns:u="{service_uri}">
                        {payload}
                    </u:{action}>
                </s:Body>
            </s:Envelope>"#,
            action = action,
            service_uri = service_uri,
            payload = payload
        );
        
        let url = format!("http://{}:1400/{}", ip, endpoint);
        let soap_action = format!("\"{}#{}\"", service_uri, action);
        
        let response = self.agent
            .post(&url)
            .set("Content-Type", "text/xml; charset=\"utf-8\"")
            .set("SOAPACTION", &soap_action)
            .send_string(&body)
            .map_err(|e| SoapError::Network(e.to_string()))?;
            
        let xml_text = response.into_string()
            .map_err(|e| SoapError::Network(e.to_string()))?;
            
        let xml = Element::parse(xml_text.as_bytes())
            .map_err(|e| SoapError::Parse(e.to_string()))?;
            
        // Extract response or handle SOAP fault
        self.extract_response(&xml, action)
    }

    /// Subscribe to UPnP events for a specific service endpoint
    /// 
    /// # Arguments
    /// * `ip` - Device IP address
    /// * `port` - Device port (typically 1400)
    /// * `event_endpoint` - Event endpoint path (e.g., "MediaRenderer/AVTransport/Event")
    /// * `callback_url` - URL where events should be sent
    /// * `timeout_seconds` - Requested subscription timeout in seconds
    /// 
    /// # Returns
    /// A `SubscriptionResponse` containing the SID and actual timeout
    pub fn subscribe(
        &self,
        ip: &str,
        port: u16,
        event_endpoint: &str,
        callback_url: &str,
        timeout_seconds: u32,
    ) -> Result<SubscriptionResponse, SoapError> {
        let url = format!("http://{}:{}/{}", ip, port, event_endpoint);
        let host = format!("{}:{}", ip, port);
        
        let response = self.agent
            .request("SUBSCRIBE", &url)
            .set("HOST", &host)
            .set("CALLBACK", &format!("<{}>", callback_url))
            .set("NT", "upnp:event")
            .set("TIMEOUT", &format!("Second-{}", timeout_seconds))
            .call()
            .map_err(|e| SoapError::Network(e.to_string()))?;

        if response.status() != 200 {
            return Err(SoapError::Network(format!(
                "SUBSCRIBE failed: HTTP {}",
                response.status()
            )));
        }

        // Extract SID from response headers
        let sid = response
            .header("SID")
            .ok_or_else(|| SoapError::Parse("Missing SID header in SUBSCRIBE response".to_string()))?
            .to_string();

        // Extract timeout from response headers (optional, fallback to requested timeout)
        let actual_timeout_seconds = response
            .header("TIMEOUT")
            .and_then(|s| {
                // Parse "Second-1800" format
                if s.starts_with("Second-") {
                    s.strip_prefix("Second-")?.parse::<u32>().ok()
                } else {
                    None
                }
            })
            .unwrap_or(timeout_seconds);

        Ok(SubscriptionResponse {
            sid,
            timeout_seconds: actual_timeout_seconds,
        })
    }

    /// Renew an existing UPnP subscription
    /// 
    /// # Arguments
    /// * `ip` - Device IP address
    /// * `port` - Device port (typically 1400)
    /// * `event_endpoint` - Event endpoint path
    /// * `sid` - Subscription ID to renew
    /// * `timeout_seconds` - Requested renewal timeout in seconds
    /// 
    /// # Returns
    /// The actual timeout granted by the device
    pub fn renew_subscription(
        &self,
        ip: &str,
        port: u16,
        event_endpoint: &str,
        sid: &str,
        timeout_seconds: u32,
    ) -> Result<u32, SoapError> {
        let url = format!("http://{}:{}/{}", ip, port, event_endpoint);
        let host = format!("{}:{}", ip, port);
        
        let response = self.agent
            .request("SUBSCRIBE", &url)
            .set("HOST", &host)
            .set("SID", sid)
            .set("TIMEOUT", &format!("Second-{}", timeout_seconds))
            .call()
            .map_err(|e| SoapError::Network(e.to_string()))?;

        if response.status() != 200 {
            return Err(SoapError::Network(format!(
                "SUBSCRIBE renewal failed: HTTP {}",
                response.status()
            )));
        }

        // Extract timeout from response headers
        let actual_timeout_seconds = response
            .header("TIMEOUT")
            .and_then(|s| {
                if s.starts_with("Second-") {
                    s.strip_prefix("Second-")?.parse::<u32>().ok()
                } else {
                    None
                }
            })
            .unwrap_or(timeout_seconds);

        Ok(actual_timeout_seconds)
    }

    /// Unsubscribe from UPnP events
    /// 
    /// # Arguments
    /// * `ip` - Device IP address
    /// * `port` - Device port (typically 1400)
    /// * `event_endpoint` - Event endpoint path
    /// * `sid` - Subscription ID to cancel
    pub fn unsubscribe(
        &self,
        ip: &str,
        port: u16,
        event_endpoint: &str,
        sid: &str,
    ) -> Result<(), SoapError> {
        let url = format!("http://{}:{}/{}", ip, port, event_endpoint);
        let host = format!("{}:{}", ip, port);
        
        let response = self.agent
            .request("UNSUBSCRIBE", &url)
            .set("HOST", &host)
            .set("SID", sid)
            .call()
            .map_err(|e| SoapError::Network(e.to_string()))?;

        if response.status() != 200 {
            return Err(SoapError::Network(format!(
                "UNSUBSCRIBE failed: HTTP {}",
                response.status()
            )));
        }

        Ok(())
    }

    fn extract_response(&self, xml: &Element, action: &str) -> Result<Element, SoapError> {
        let body = xml.get_child("Body")
            .ok_or_else(|| SoapError::Parse("Missing SOAP Body".to_string()))?;
            
        // Check for SOAP fault first
        if let Some(fault) = body.get_child("Fault") {
            let error_code = fault
                .get_child("detail")
                .and_then(|d| d.get_child("UpnPError"))
                .and_then(|e| e.get_child("errorCode"))
                .and_then(|c| c.get_text())
                .and_then(|t| t.parse::<u16>().ok())
                .unwrap_or(500);
            return Err(SoapError::Fault(error_code));
        }
        
        // Extract the action response
        let response_name = format!("{}Response", action);
        body.get_child(response_name.as_str())
            .cloned()
            .ok_or_else(|| SoapError::Parse(format!("Missing {} element", response_name)))
    }
}

impl Default for SoapClient {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_soap_client_creation() {
        let _client = SoapClient::new();
        
        // Test that the client can be created without panicking
        // and that it has the expected timeout configuration
        // We can't easily test the timeout values directly, but we can verify
        // the client was created successfully
        let _default_client = SoapClient::default();
    }

    #[test]
    fn test_extract_response_with_valid_response() {
        let client = SoapClient::new();
        
        let xml_str = r#"
            <s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">
                <s:Body>
                    <u:PlayResponse xmlns:u="urn:schemas-upnp-org:service:AVTransport:1">
                    </u:PlayResponse>
                </s:Body>
            </s:Envelope>
        "#;
        
        let xml = Element::parse(xml_str.as_bytes()).unwrap();
        let result = client.extract_response(&xml, "Play");
        
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.name, "PlayResponse");
    }

    #[test]
    fn test_extract_response_with_soap_fault() {
        let client = SoapClient::new();
        
        let xml_str = r#"
            <s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">
                <s:Body>
                    <s:Fault>
                        <faultcode>s:Client</faultcode>
                        <faultstring>UPnPError</faultstring>
                        <detail>
                            <UpnPError xmlns="urn:schemas-upnp-org:control-1-0">
                                <errorCode>401</errorCode>
                                <errorDescription>Invalid Action</errorDescription>
                            </UpnPError>
                        </detail>
                    </s:Fault>
                </s:Body>
            </s:Envelope>
        "#;
        
        let xml = Element::parse(xml_str.as_bytes()).unwrap();
        let result = client.extract_response(&xml, "Play");
        
        assert!(result.is_err());
        match result.unwrap_err() {
            SoapError::Fault(code) => assert_eq!(code, 401),
            _ => panic!("Expected SoapError::Fault"),
        }
    }

    #[test]
    fn test_extract_response_missing_body() {
        let client = SoapClient::new();
        
        let xml_str = r#"
            <s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">
            </s:Envelope>
        "#;
        
        let xml = Element::parse(xml_str.as_bytes()).unwrap();
        let result = client.extract_response(&xml, "Play");
        
        assert!(result.is_err());
        match result.unwrap_err() {
            SoapError::Parse(msg) => assert!(msg.contains("Missing SOAP Body")),
            _ => panic!("Expected SoapError::Parse"),
        }
    }

    #[test]
    fn test_extract_response_missing_action_response() {
        let client = SoapClient::new();
        
        let xml_str = r#"
            <s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">
                <s:Body>
                </s:Body>
            </s:Envelope>
        "#;
        
        let xml = Element::parse(xml_str.as_bytes()).unwrap();
        let result = client.extract_response(&xml, "Play");
        
        assert!(result.is_err());
        match result.unwrap_err() {
            SoapError::Parse(msg) => assert!(msg.contains("Missing PlayResponse element")),
            _ => panic!("Expected SoapError::Parse"),
        }
    }

    #[test]
    fn test_soap_fault_with_default_error_code() {
        let client = SoapClient::new();
        
        let xml_str = r#"
            <s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">
                <s:Body>
                    <s:Fault>
                        <faultcode>s:Server</faultcode>
                        <faultstring>Internal Error</faultstring>
                    </s:Fault>
                </s:Body>
            </s:Envelope>
        "#;
        
        let xml = Element::parse(xml_str.as_bytes()).unwrap();
        let result = client.extract_response(&xml, "Play");
        
        assert!(result.is_err());
        match result.unwrap_err() {
            SoapError::Fault(code) => assert_eq!(code, 500), // Default error code
            _ => panic!("Expected SoapError::Fault"),
        }
    }
}