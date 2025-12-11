//! GetTransportInfo operation for AVTransport service

use serde::{Deserialize, Serialize};
use xmltree::Element;
use crate::{ApiError, Service, SonosOperation};

/// GetTransportInfo operation
pub struct GetTransportInfoOperation;

/// Request for GetTransportInfo operation
#[derive(Serialize)]
pub struct GetTransportInfoRequest {
    pub instance_id: u32,
}

/// Transport state enum
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PlayState {
    Playing,
    #[serde(rename = "PAUSED_PLAYBACK")]
    Paused,
    Stopped,
    Transitioning,
}

/// Response for GetTransportInfo operation
#[derive(Debug, Clone, Deserialize)]
pub struct GetTransportInfoResponse {
    #[serde(rename = "CurrentTransportState")]
    pub current_transport_state: PlayState,
    #[serde(rename = "CurrentTransportStatus")]
    pub current_transport_status: String,
    #[serde(rename = "CurrentSpeed")]
    pub current_speed: String,
}

impl SonosOperation for GetTransportInfoOperation {
    type Request = GetTransportInfoRequest;
    type Response = GetTransportInfoResponse;
    
    const SERVICE: Service = Service::AVTransport;
    const ACTION: &'static str = "GetTransportInfo";
    
    fn build_payload(request: &Self::Request) -> String {
        format!("<InstanceID>{}</InstanceID>", request.instance_id)
    }
    
    fn parse_response(xml: &Element) -> Result<Self::Response, ApiError> {
        let current_transport_state = xml
            .get_child("CurrentTransportState")
            .and_then(|e| e.get_text())
            .ok_or_else(|| ApiError::ParseError("Missing CurrentTransportState element".to_string()))?;
            
        let current_transport_status = xml
            .get_child("CurrentTransportStatus")
            .and_then(|e| e.get_text())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "OK".to_string());
            
        let current_speed = xml
            .get_child("CurrentSpeed")
            .and_then(|e| e.get_text())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "1".to_string());
            
        let play_state = match current_transport_state.as_ref() {
            "PLAYING" => PlayState::Playing,
            "PAUSED_PLAYBACK" => PlayState::Paused,
            "STOPPED" => PlayState::Stopped,
            "TRANSITIONING" => PlayState::Transitioning,
            _ => return Err(ApiError::ParseError(format!("Unknown transport state: {}", current_transport_state))),
        };
        
        Ok(GetTransportInfoResponse {
            current_transport_state: play_state,
            current_transport_status,
            current_speed,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_transport_info_payload_construction() {
        let request = GetTransportInfoRequest {
            instance_id: 0,
        };
        
        let payload = GetTransportInfoOperation::build_payload(&request);
        assert_eq!(payload, "<InstanceID>0</InstanceID>");
    }

    #[test]
    fn test_get_transport_info_response_parsing() {
        let xml_str = r#"
            <GetTransportInfoResponse>
                <CurrentTransportState>PLAYING</CurrentTransportState>
                <CurrentTransportStatus>OK</CurrentTransportStatus>
                <CurrentSpeed>1</CurrentSpeed>
            </GetTransportInfoResponse>
        "#;
        let xml = Element::parse(xml_str.as_bytes()).unwrap();
        
        let result = GetTransportInfoOperation::parse_response(&xml).unwrap();
        assert_eq!(result.current_transport_state, PlayState::Playing);
        assert_eq!(result.current_transport_status, "OK");
        assert_eq!(result.current_speed, "1");
    }

    #[test]
    fn test_get_transport_info_response_parsing_paused() {
        let xml_str = r#"
            <GetTransportInfoResponse>
                <CurrentTransportState>PAUSED_PLAYBACK</CurrentTransportState>
                <CurrentTransportStatus>OK</CurrentTransportStatus>
                <CurrentSpeed>1</CurrentSpeed>
            </GetTransportInfoResponse>
        "#;
        let xml = Element::parse(xml_str.as_bytes()).unwrap();
        
        let result = GetTransportInfoOperation::parse_response(&xml).unwrap();
        assert_eq!(result.current_transport_state, PlayState::Paused);
    }

    #[test]
    fn test_get_transport_info_response_parsing_stopped() {
        let xml_str = r#"
            <GetTransportInfoResponse>
                <CurrentTransportState>STOPPED</CurrentTransportState>
                <CurrentTransportStatus>OK</CurrentTransportStatus>
                <CurrentSpeed>1</CurrentSpeed>
            </GetTransportInfoResponse>
        "#;
        let xml = Element::parse(xml_str.as_bytes()).unwrap();
        
        let result = GetTransportInfoOperation::parse_response(&xml).unwrap();
        assert_eq!(result.current_transport_state, PlayState::Stopped);
    }

    #[test]
    fn test_get_transport_info_response_parsing_missing_state() {
        let xml_str = r#"
            <GetTransportInfoResponse>
                <CurrentTransportStatus>OK</CurrentTransportStatus>
                <CurrentSpeed>1</CurrentSpeed>
            </GetTransportInfoResponse>
        "#;
        let xml = Element::parse(xml_str.as_bytes()).unwrap();
        
        let result = GetTransportInfoOperation::parse_response(&xml);
        assert!(result.is_err());
        if let Err(ApiError::ParseError(msg)) = result {
            assert!(msg.contains("Missing CurrentTransportState element"));
        } else {
            panic!("Expected ParseError");
        }
    }

    #[test]
    fn test_get_transport_info_response_parsing_unknown_state() {
        let xml_str = r#"
            <GetTransportInfoResponse>
                <CurrentTransportState>UNKNOWN_STATE</CurrentTransportState>
                <CurrentTransportStatus>OK</CurrentTransportStatus>
                <CurrentSpeed>1</CurrentSpeed>
            </GetTransportInfoResponse>
        "#;
        let xml = Element::parse(xml_str.as_bytes()).unwrap();
        
        let result = GetTransportInfoOperation::parse_response(&xml);
        assert!(result.is_err());
        if let Err(ApiError::ParseError(msg)) = result {
            assert!(msg.contains("Unknown transport state: UNKNOWN_STATE"));
        } else {
            panic!("Expected ParseError");
        }
    }
}