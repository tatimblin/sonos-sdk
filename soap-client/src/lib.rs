//! Private SOAP client for UPnP device communication
//! 
//! This crate provides a minimal SOAP client specifically designed for
//! communicating with UPnP devices like Sonos speakers.

mod error;

pub use error::SoapError;

use std::time::Duration;
use xmltree::Element;

/// A minimal SOAP client for UPnP device communication
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