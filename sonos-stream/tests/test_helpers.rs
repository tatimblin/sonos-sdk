//! Test helpers and mock servers for integration testing.
//!
//! This module provides utilities for testing the sonos-stream crate including:
//! - Mock UPnP servers for simulating device endpoints
//! - Custom test strategies for different scenarios
//! - Helper functions for creating test data

use sonos_stream::{
    BaseStrategy, ServiceType, SpeakerId, Speaker, SubscriptionConfig, SubscriptionScope,
    StrategyError, TypedEvent,
};
use std::collections::HashMap;
use std::net::IpAddr;

/// Helper function to create a test speaker.
pub fn create_test_speaker(id: &str, ip: &str, name: &str, room: &str) -> Speaker {
    Speaker::new(
        SpeakerId::new(id),
        ip.parse::<IpAddr>().unwrap(),
        name.to_string(),
        room.to_string(),
    )
}

/// Helper function to check if a port is available.
pub fn is_port_available(port: u16) -> bool {
    std::net::TcpListener::bind(format!("127.0.0.1:{}", port)).is_ok()
}

/// Simple UPnP mock server for testing
pub struct UPnPMockServer {
    server: tokio::net::TcpListener,
    port: u16,
    expected_subscriptions: std::sync::Arc<tokio::sync::Mutex<Vec<String>>>, // subscription IDs to return
    received_requests: std::sync::Arc<tokio::sync::Mutex<Vec<String>>>, // track received requests
}

impl UPnPMockServer {
    pub async fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let server = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let port = server.local_addr()?.port();
        
        Ok(Self {
            server,
            port,
            expected_subscriptions: std::sync::Arc::new(tokio::sync::Mutex::new(Vec::new())),
            received_requests: std::sync::Arc::new(tokio::sync::Mutex::new(Vec::new())),
        })
    }
    
    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
    
    pub async fn add_expected_subscription(&self, subscription_id: String) {
        self.expected_subscriptions.lock().await.push(subscription_id);
    }
    
    #[allow(dead_code)]
    pub async fn get_received_requests(&self) -> Vec<String> {
        self.received_requests.lock().await.clone()
    }
    
    pub async fn start(self) -> tokio::task::JoinHandle<()> {
        let expected_subscriptions = self.expected_subscriptions.clone();
        let received_requests = self.received_requests.clone();
        
        tokio::spawn(async move {
            while let Ok((stream, _)) = self.server.accept().await {
                let expected_subscriptions = expected_subscriptions.clone();
                let received_requests = received_requests.clone();
                
                tokio::spawn(async move {
                    if let Err(e) = Self::handle_connection(stream, expected_subscriptions, received_requests).await {
                        eprintln!("Error handling connection: {}", e);
                    }
                });
            }
        })
    }
    
    async fn handle_connection(
        stream: tokio::net::TcpStream,
        expected_subscriptions: std::sync::Arc<tokio::sync::Mutex<Vec<String>>>,
        received_requests: std::sync::Arc<tokio::sync::Mutex<Vec<String>>>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        
        let mut stream = stream;
        let mut buffer = [0; 4096];
        let n = stream.read(&mut buffer).await?;
        let request = String::from_utf8_lossy(&buffer[..n]);
        
        // Log the request
        received_requests.lock().await.push(request.to_string());
        
        // Parse the request
        let lines: Vec<&str> = request.lines().collect();
        if lines.is_empty() {
            return Ok(());
        }
        
        let request_line = lines[0];
        let parts: Vec<&str> = request_line.split_whitespace().collect();
        if parts.len() < 2 {
            return Ok(());
        }
        
        let method = parts[0];
        let _path = parts[1];
        
        match method {
            "SUBSCRIBE" => {
                // Check if this is a new subscription or renewal
                let has_sid = request.contains("SID:");
                
                if !has_sid {
                    // New subscription - return a subscription ID
                    let mut subs = expected_subscriptions.lock().await;
                    let subscription_id = if !subs.is_empty() {
                        subs.remove(0)
                    } else {
                        "uuid:default-subscription".to_string()
                    };
                    
                    let response = format!(
                        "HTTP/1.1 200 OK\r\n\
                         SID: {}\r\n\
                         TIMEOUT: Second-1800\r\n\
                         \r\n",
                        subscription_id
                    );
                    stream.write_all(response.as_bytes()).await?;
                } else {
                    // Renewal - just return OK
                    let response = "HTTP/1.1 200 OK\r\n\
                                   TIMEOUT: Second-1800\r\n\
                                   \r\n";
                    stream.write_all(response.as_bytes()).await?;
                }
            }
            "UNSUBSCRIBE" => {
                // Always return OK for unsubscribe
                let response = "HTTP/1.1 200 OK\r\n\r\n";
                stream.write_all(response.as_bytes()).await?;
            }
            _ => {
                // Unknown method
                let response = "HTTP/1.1 405 Method Not Allowed\r\n\r\n";
                stream.write_all(response.as_bytes()).await?;
            }
        }
        
        Ok(())
    }
}

/// Custom AVTransport strategy for testing that uses mock server URLs
pub struct TestAVTransportStrategy {
    mock_server_url: String,
}

impl TestAVTransportStrategy {
    pub fn new(mock_server_url: String) -> Self {
        Self { mock_server_url }
    }
}

#[async_trait::async_trait]
impl BaseStrategy for TestAVTransportStrategy {
    fn service_type(&self) -> ServiceType {
        ServiceType::AVTransport
    }

    fn subscription_scope(&self) -> SubscriptionScope {
        SubscriptionScope::PerSpeaker
    }

    fn service_endpoint_path(&self) -> &'static str {
        "/MediaRenderer/AVTransport/Event"
    }

    async fn create_subscription(
        &self,
        speaker: &Speaker,
        callback_url: String,
        config: &SubscriptionConfig,
    ) -> Result<Box<dyn sonos_stream::Subscription>, StrategyError> {
        use sonos_stream::UPnPSubscription;
        
        // Override endpoint URL to use mock server instead of speaker IP:1400
        let endpoint_url = format!("{}{}", self.mock_server_url, self.service_endpoint_path());
        let service_type = self.service_type();
        
        let subscription = UPnPSubscription::create_subscription(
            speaker.id.clone(),
            service_type,
            endpoint_url,
            callback_url,
            config.timeout_seconds,
        )
        .await
        .map_err(|e| match e {
            sonos_stream::SubscriptionError::NetworkError(msg) => StrategyError::NetworkError(msg),
            sonos_stream::SubscriptionError::UnsubscribeFailed(msg) => StrategyError::SubscriptionCreationFailed(msg),
            sonos_stream::SubscriptionError::RenewalFailed(msg) => StrategyError::SubscriptionCreationFailed(msg),
            sonos_stream::SubscriptionError::Expired => StrategyError::SubscriptionCreationFailed("Subscription expired during creation".to_string()),
        })?;

        Ok(Box::new(subscription))
    }

    fn parse_event(
        &self,
        _speaker_id: &SpeakerId,
        event_xml: &str,
    ) -> Result<TypedEvent, StrategyError> {
        // Use the same parsing logic as AVTransportStrategy
        use sonos_parser::services::av_transport::AVTransportParser;
        use sonos_stream::{AVTransportEvent, TypedEvent};

        let parsed = AVTransportParser::from_xml(event_xml)
            .map_err(|e| StrategyError::EventParseFailed(format!("Failed to parse AVTransport event: {}", e)))?;
        
        let av_event = AVTransportEvent::from_parser(parsed);
        Ok(TypedEvent::new(Box::new(av_event)))
    }
}

/// Custom strategy for multiple server testing
pub struct MultiTestAVTransportStrategy {
    server_urls: HashMap<String, String>, // speaker_id -> server_url
}

impl MultiTestAVTransportStrategy {
    pub fn new() -> Self {
        Self {
            server_urls: HashMap::new(),
        }
    }
    
    pub fn add_server(&mut self, speaker_id: String, server_url: String) {
        self.server_urls.insert(speaker_id, server_url);
    }
}

#[async_trait::async_trait]
impl BaseStrategy for MultiTestAVTransportStrategy {
    fn service_type(&self) -> ServiceType {
        ServiceType::AVTransport
    }

    fn subscription_scope(&self) -> SubscriptionScope {
        SubscriptionScope::PerSpeaker
    }

    fn service_endpoint_path(&self) -> &'static str {
        "/MediaRenderer/AVTransport/Event"
    }

    async fn create_subscription(
        &self,
        speaker: &Speaker,
        callback_url: String,
        config: &SubscriptionConfig,
    ) -> Result<Box<dyn sonos_stream::Subscription>, StrategyError> {
        use sonos_stream::UPnPSubscription;
        
        // Get the mock server URL for this speaker
        let server_url = self.server_urls.get(speaker.id.as_str())
            .ok_or_else(|| StrategyError::SubscriptionCreationFailed(
                format!("No mock server configured for speaker {}", speaker.id.as_str())
            ))?;
        
        let endpoint_url = format!("{}{}", server_url, self.service_endpoint_path());
        let service_type = self.service_type();
        
        let subscription = UPnPSubscription::create_subscription(
            speaker.id.clone(),
            service_type,
            endpoint_url,
            callback_url,
            config.timeout_seconds,
        )
        .await
        .map_err(|e| match e {
            sonos_stream::SubscriptionError::NetworkError(msg) => StrategyError::NetworkError(msg),
            sonos_stream::SubscriptionError::UnsubscribeFailed(msg) => StrategyError::SubscriptionCreationFailed(msg),
            sonos_stream::SubscriptionError::RenewalFailed(msg) => StrategyError::SubscriptionCreationFailed(msg),
            sonos_stream::SubscriptionError::Expired => StrategyError::SubscriptionCreationFailed("Subscription expired during creation".to_string()),
        })?;

        Ok(Box::new(subscription))
    }

    fn parse_event(
        &self,
        _speaker_id: &SpeakerId,
        event_xml: &str,
    ) -> Result<TypedEvent, StrategyError> {
        use sonos_parser::services::av_transport::AVTransportParser;
        use sonos_stream::{AVTransportEvent, TypedEvent};

        let parsed = AVTransportParser::from_xml(event_xml)
            .map_err(|e| StrategyError::EventParseFailed(format!("Failed to parse AVTransport event: {}", e)))?;
        
        let av_event = AVTransportEvent::from_parser(parsed);
        Ok(TypedEvent::new(Box::new(av_event)))
    }
}

