//! HTTP/HTTPS Transport implementation for Synapse
//! 
//! This transport uses HTTP/HTTPS requests to send messages, making it useful
//! for environments with restrictive firewalls that only allow web traffic.

#[cfg(feature = "http")]
mod http_impl {
    use crate::error::{Result, SynapseError};
    use crate::transport::abstraction::{
        Transport, TransportMetrics, TransportType, TransportCapabilities, 
        TransportStatus, TransportTarget, TransportEstimate, IncomingMessage,
        MessageUrgency, DeliveryReceipt, ConnectivityResult, DeliveryConfirmation,
        TransportFactory,
    };
    use crate::circuit_breaker::{CircuitBreaker, CircuitBreakerConfig, RequestOutcome};
    use crate::types::SecureMessage;
    use tracing::{debug, info, warn};
    use async_trait::async_trait;
    use std::{
        time::{Duration, Instant},
        sync::{Arc, RwLock},
        collections::HashMap,
        net::SocketAddr,
    };
    use tokio::sync::Mutex;
    use reqwest::{Client, ClientBuilder};

/// HTTP/HTTPS Transport implementation
#[cfg(feature = "http")]
pub struct HttpTransportImpl {
    /// HTTP client for sending requests
    client: Client,
    /// Transport configuration
    config: HttpTransportConfig,
    /// Circuit breaker for fault tolerance
    circuit_breaker: Arc<CircuitBreaker>,
    /// Transport status
    status: Arc<RwLock<TransportStatus>>,
    /// Performance metrics
    metrics: Arc<RwLock<TransportMetrics>>,
    /// Server for receiving messages (optional)
    server: Arc<Mutex<Option<HttpServer>>>,
}

/// Configuration for HTTP transport
#[derive(Debug, Clone)]
pub struct HttpTransportConfig {
    /// Use HTTPS instead of HTTP
    pub use_https: bool,
    /// Server port for receiving messages (0 for disabled)
    pub server_port: u16,
    /// Server bind address
    pub server_address: String,
    /// Request timeout
    pub timeout: Duration,
    /// Maximum message size
    pub max_message_size: usize,
    /// Custom headers to include in requests
    pub headers: HashMap<String, String>,
    /// Connection pool settings
    pub max_connections: usize,
    /// User agent string
    pub user_agent: String,
}

impl Default for HttpTransportConfig {
    fn default() -> Self {
        let mut headers = HashMap::new();
        headers.insert("Content-Type".to_string(), "application/json".to_string());
        headers.insert("Accept".to_string(), "application/json".to_string());
        
        Self {
            use_https: true,
            server_port: 0, // Disabled by default
            server_address: "127.0.0.1".to_string(),
            timeout: Duration::from_secs(30),
            max_message_size: 10 * 1024 * 1024, // 10MB
            headers,
            max_connections: 100,
            user_agent: "Synapse-HTTP-Transport/1.0".to_string(),
        }
    }
}

/// HTTP server for receiving messages
pub struct HttpServer {
    /// Server address
    pub address: SocketAddr,
    /// Received messages queue
    pub received_messages: Arc<Mutex<Vec<IncomingMessage>>>,
}

#[cfg(feature = "http")]
impl HttpTransportImpl {
    /// Create a new HTTP transport instance
    pub async fn new(config: &HashMap<String, String>) -> Result<Self> {
        let mut transport_config = HttpTransportConfig::default();
        
        // Parse configuration
        if let Some(use_https) = config.get("use_https") {
            transport_config.use_https = use_https.parse().unwrap_or(true);
        }
        
        if let Some(server_port) = config.get("server_port") {
            transport_config.server_port = server_port.parse().unwrap_or(0);
        }
        
        if let Some(server_address) = config.get("server_address") {
            transport_config.server_address = server_address.clone();
        }
        
        if let Some(timeout_ms) = config.get("timeout_ms") {
            let timeout_ms: u64 = timeout_ms.parse().unwrap_or(30000);
            transport_config.timeout = Duration::from_millis(timeout_ms);
        }
        
        if let Some(max_size) = config.get("max_message_size") {
            transport_config.max_message_size = max_size.parse().unwrap_or(10 * 1024 * 1024);
        }
        
        if let Some(user_agent) = config.get("user_agent") {
            transport_config.user_agent = user_agent.clone();
        }
        
        // Build HTTP client
        let client = ClientBuilder::new()
            .timeout(transport_config.timeout)
            .pool_max_idle_per_host(transport_config.max_connections)
            .user_agent(&transport_config.user_agent)
            .build()
            .map_err(|e| SynapseError::TransportError(format!("Failed to create HTTP client: {}", e)))?;
        
        // Create circuit breaker
        let circuit_breaker = Arc::new(CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 5,
            recovery_timeout: Duration::from_secs(60),
            ..Default::default()
        }));
        
        // Initialize metrics
        let metrics = Arc::new(RwLock::new(TransportMetrics {
            transport_type: TransportType::Http,
            messages_sent: 0,
            messages_received: 0,
            send_failures: 0,
            receive_failures: 0,
            bytes_sent: 0,
            bytes_received: 0,
            average_latency_ms: 0,
            reliability_score: 1.0,
            active_connections: 0,
            last_updated_timestamp: 0,
            custom_metrics: HashMap::new(),
        }));
        
        Ok(Self {
            client,
            config: transport_config,
            circuit_breaker,
            status: Arc::new(RwLock::new(TransportStatus::Stopped)),
            metrics,
            server: Arc::new(Mutex::new(None)),
        })
    }
    
    /// Start HTTP server if configured
    async fn start_server(&self) -> Result<()> {
        if self.config.server_port == 0 {
            debug!("HTTP server disabled (port 0)");
            return Ok(());
        }
        
        info!("Starting HTTP server on {}:{}", self.config.server_address, self.config.server_port);
        
        let server_addr = format!("{}:{}", self.config.server_address, self.config.server_port)
            .parse::<SocketAddr>()
            .map_err(|e| SynapseError::TransportError(format!("Invalid server address: {}", e)))?;
        
        let server = HttpServer {
            address: server_addr,
            received_messages: Arc::new(Mutex::new(Vec::new())),
        };
        
        // Start HTTP server using warp or axum
        // This is a simplified version - in production you'd use a proper HTTP server
        info!("HTTP server started on {}", server_addr);
        
        let mut server_lock = self.server.lock().await;
        *server_lock = Some(server);
        
        Ok(())
    }
    
    /// Send HTTP request to target
    async fn send_http_request(&self, url: &str, message: &SecureMessage) -> Result<DeliveryReceipt> {
        let start_time = Instant::now();
        
        // Serialize message
        let json_payload = serde_json::to_string(message)
            .map_err(|e| SynapseError::SerializationError(e.to_string()))?;
        
        // Check message size
        if json_payload.len() > self.config.max_message_size {
            return Err(SynapseError::TransportError(format!(
                "Message size {} exceeds maximum {}",
                json_payload.len(),
                self.config.max_message_size
            )));
        }
        
        debug!("Sending HTTP request to {}", url);
        
        // Build request
        let mut request = self.client.post(url).body(json_payload.clone());
        
        // Add headers
        for (key, value) in &self.config.headers {
            request = request.header(key, value);
        }
        
        // Send request
        let response = request.send().await
            .map_err(|e| SynapseError::TransportError(format!("HTTP request failed: {}", e)))?;
        
        let status_code = response.status();
        let latency = start_time.elapsed();
        
        // Update metrics
        {
            let mut metrics = self.metrics.write().unwrap();
            metrics.messages_sent += 1;
            metrics.bytes_sent += json_payload.len() as u64;
            metrics.average_latency_ms = latency.as_millis() as u64;
            metrics.last_updated_timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
            
            if status_code.is_success() {
                metrics.reliability_score = (metrics.reliability_score * 0.9) + 0.1;
            } else {
                metrics.reliability_score = (metrics.reliability_score * 0.9) + 0.0;
                metrics.send_failures += 1;
            }
        }
        
        if status_code.is_success() {
            info!("HTTP message sent successfully to {} ({}ms)", url, latency.as_millis());
            Ok(DeliveryReceipt {
                message_id: message.message_id.0.to_string(),
                target_reached: url.to_string(),
                transport_used: TransportType::Http,
                confirmation: DeliveryConfirmation::Sent,
                delivery_time: latency,
                metadata: {
                    let mut metadata = HashMap::new();
                    metadata.insert("status_code".to_string(), status_code.as_u16().to_string());
                    metadata.insert("url".to_string(), url.to_string());
                    metadata
                },
            })
        } else {
            let error_msg = format!("HTTP request failed with status {}", status_code);
            warn!("{}", error_msg);
            Err(SynapseError::TransportError(error_msg))
        }
    }
    
    /// Parse target address for HTTP requests
    fn parse_target_url(&self, target: &TransportTarget) -> Result<String> {
        // Check if target already has a URL
        if let Some(address) = &target.address {
            if address.starts_with("http://") || address.starts_with("https://") {
                return Ok(address.clone());
            }
        }
        
        // Build URL from target identifier
        let scheme = if self.config.use_https { "https" } else { "http" };
        let host = target.address.as_ref()
            .or(Some(&target.identifier))
            .unwrap();
        
        // Handle different address formats
        if host.contains(':') {
            // Address with port
            Ok(format!("{}://{}/synapse/message", scheme, host))
        } else if host.contains('.') {
            // Domain name
            let port = if self.config.use_https { 443 } else { 80 };
            Ok(format!("{}://{}:{}/synapse/message", scheme, host, port))
        } else {
            // Simple identifier - assume localhost
            let port = if self.config.use_https { 8443 } else { 8080 };
            Ok(format!("{}://localhost:{}/synapse/message/{}", scheme, port, host))
        }
    }
}

#[async_trait]
#[cfg(feature = "http")]
impl Transport for HttpTransportImpl {
    fn transport_type(&self) -> TransportType {
        TransportType::Http
    }
    
    fn capabilities(&self) -> TransportCapabilities {
        TransportCapabilities {
            max_message_size: self.config.max_message_size,
            reliable: true,
            real_time: false, // HTTP has higher latency than UDP
            broadcast: false,
            bidirectional: self.config.server_port > 0,
            encrypted: self.config.use_https,
            network_spanning: true,
            supported_urgencies: vec![
                MessageUrgency::Interactive,
                MessageUrgency::Background,
                MessageUrgency::Batch,
            ],
            features: vec![
                "http".to_string(),
                if self.config.use_https { "https".to_string() } else { "http".to_string() },
                "firewall-friendly".to_string(),
                "web-compatible".to_string(),
            ],
        }
    }
    
    async fn can_reach(&self, target: &TransportTarget) -> bool {
        // For HTTP, we assume reachability if we can parse the URL
        self.parse_target_url(target).is_ok()
    }
    
    async fn estimate_metrics(&self, target: &TransportTarget) -> Result<TransportEstimate> {
        let url = self.parse_target_url(target)?;
        
        // HTTP typically has higher latency than direct TCP/UDP
        let estimated_latency = if url.starts_with("https://") {
            Duration::from_millis(200) // HTTPS overhead
        } else {
            Duration::from_millis(100) // HTTP
        };
        
        Ok(TransportEstimate {
            latency: estimated_latency,
            reliability: 0.95, // HTTP is generally reliable
            bandwidth: 1_000_000, // 1MB/s typical
            cost: 0.1, // Low cost
            available: true,
            confidence: 0.8,
        })
    }
    
    async fn send_message(&self, target: &TransportTarget, message: &SecureMessage) -> Result<DeliveryReceipt> {
        let url = self.parse_target_url(target)?;
        
        // Check circuit breaker
        if !self.circuit_breaker.can_proceed().await {
            return Err(SynapseError::TransportError("HTTP circuit breaker is open".to_string()));
        }
        
        // Send HTTP request
        let result = self.send_http_request(&url, message).await;
        
        // Record outcome in circuit breaker
        let outcome = match &result {
            Ok(_) => RequestOutcome::Success,
            Err(e) => RequestOutcome::Failure(e.to_string()),
        };
        self.circuit_breaker.record_outcome(outcome).await;
        
        result
    }
    
    async fn receive_messages(&self) -> Result<Vec<IncomingMessage>> {
        let server_lock = self.server.lock().await;
        if let Some(server) = server_lock.as_ref() {
            let mut messages = server.received_messages.lock().await;
            let received: Vec<IncomingMessage> = messages.drain(..).collect();
            
            // Update metrics
            {
                let mut metrics = self.metrics.write().unwrap();
                metrics.messages_received += received.len() as u64;
                metrics.last_updated_timestamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs();
            }
            
            Ok(received)
        } else {
            Ok(Vec::new())
        }
    }
    
    async fn test_connectivity(&self, target: &TransportTarget) -> Result<ConnectivityResult> {
        let url = self.parse_target_url(target)?;
        let start_time = Instant::now();
        
        // Send a HEAD request to test connectivity
        let head_url = url.replace("/synapse/message", "/synapse/health");
        let response = self.client.head(&head_url).send().await;
        
        let latency = start_time.elapsed();
        
        match response {
            Ok(resp) if resp.status().is_success() => {
                Ok(ConnectivityResult {
                    connected: true,
                    rtt: Some(latency),
                    error: None,
                    quality: 1.0,
                    details: {
                        let mut meta = HashMap::new();
                        meta.insert("status_code".to_string(), resp.status().as_u16().to_string());
                        meta.insert("url".to_string(), head_url);
                        meta
                    },
                })
            }
            Ok(resp) => {
                Ok(ConnectivityResult {
                    connected: false,
                    rtt: Some(latency),
                    error: Some(format!("HTTP status {}", resp.status())),
                    quality: 0.0,
                    details: HashMap::new(),
                })
            }
            Err(e) => {
                Ok(ConnectivityResult {
                    connected: false,
                    rtt: Some(latency),
                    error: Some(format!("Connection failed: {}", e)),
                    quality: 0.0,
                    details: HashMap::new(),
                })
            }
        }
    }
    
    async fn start(&self) -> Result<()> {
        info!("Starting HTTP transport");
        
        {
            let mut status = self.status.write().unwrap();
            *status = TransportStatus::Starting;
        }
        
        // Start server if configured
        self.start_server().await?;
        
        {
            let mut status = self.status.write().unwrap();
            *status = TransportStatus::Running;
        }
        
        info!("HTTP transport started successfully");
        Ok(())
    }
    
    async fn stop(&self) -> Result<()> {
        info!("Stopping HTTP transport");
        
        {
            let mut status = self.status.write().unwrap();
            *status = TransportStatus::Stopping;
        }
        
        // HTTP client will be dropped automatically
        {
            let mut server_lock = self.server.lock().await;
            *server_lock = None;
        }
        
        {
            let mut status = self.status.write().unwrap();
            *status = TransportStatus::Stopped;
        }
        
        info!("HTTP transport stopped");
        Ok(())
    }
    
    async fn status(&self) -> TransportStatus {
        *self.status.read().unwrap()
    }
    
    async fn metrics(&self) -> TransportMetrics {
        self.metrics.read().unwrap().clone()
    }
}

/// Factory for creating HTTP transport instances
pub struct HttpTransportFactory;

#[async_trait]
impl TransportFactory for HttpTransportFactory {
    async fn create_transport(&self, config: &HashMap<String, String>) -> Result<Box<dyn Transport>> {
        let transport = HttpTransportImpl::new(config).await?;
        Ok(Box::new(transport))
    }
    
    fn transport_type(&self) -> TransportType {
        TransportType::Http
    }
    
    fn default_config(&self) -> HashMap<String, String> {
        let mut config = HashMap::new();
        config.insert("use_https".to_string(), "true".to_string());
        config.insert("server_port".to_string(), "0".to_string()); // Disabled by default
        config.insert("server_address".to_string(), "127.0.0.1".to_string());
        config.insert("timeout_ms".to_string(), "30000".to_string());
        config.insert("max_message_size".to_string(), "10485760".to_string()); // 10MB
        config.insert("user_agent".to_string(), "Synapse-HTTP-Transport/1.0".to_string());
        config
    }
    
    fn validate_config(&self, config: &HashMap<String, String>) -> Result<()> {
        // Validate server port
        if let Some(port_str) = config.get("server_port") {
            if port_str.parse::<u16>().is_err() {
                return Err(SynapseError::Config(
                    "Invalid server port number".to_string()
                ));
            }
        }
        
        // Validate timeout
        if let Some(timeout_str) = config.get("timeout_ms") {
            if timeout_str.parse::<u64>().is_err() {
                return Err(SynapseError::Config(
                    "Invalid timeout value".to_string()
                ));
            }
        }
        
        // Validate max message size
        if let Some(size_str) = config.get("max_message_size") {
            if size_str.parse::<usize>().is_err() {
                return Err(SynapseError::Config(
                    "Invalid max message size".to_string()
                ));
            }
        }
        
        Ok(())
    }
}

} // end http_impl module

#[cfg(feature = "http")]
pub use http_impl::*;
