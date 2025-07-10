//! WebSocket Transport implementation conforming to the unified Transport trait

use crate::{
    types::SecureMessage,
    error::Result,
    circuit_breaker::{CircuitBreaker, CircuitBreakerConfig},
};
use super::abstraction::*;
use async_trait::async_trait;
use std::{
    time::{Duration, Instant},
    sync::{Arc, RwLock},
    collections::HashMap,
    net::SocketAddr,
};
use tokio::{
    sync::Mutex,
    time::timeout,
    net::{TcpListener, TcpStream},
};
use tracing::{info, debug, warn, error};
use serde_json;
use url::Url;

/// WebSocket Transport implementation for unified abstraction
pub struct WebSocketTransportImpl {
    /// Local port for WebSocket server
    local_port: u16,
    /// TCP listener for WebSocket server
    listener: Option<TcpListener>,
    /// Connection timeout
    connection_timeout: Duration,
    /// Active connections
    connections: Arc<Mutex<HashMap<String, WebSocketConnection>>>,
    /// Received messages queue
    received_messages: Arc<Mutex<Vec<IncomingMessage>>>,
    /// Current status
    status: Arc<RwLock<TransportStatus>>,
    /// Performance metrics
    metrics: Arc<RwLock<TransportMetrics>>,
    /// Circuit breaker for reliability
    circuit_breaker: Arc<CircuitBreaker>,
    /// Maximum message size
    max_message_size: usize,
}

/// Represents a WebSocket connection
#[derive(Debug, Clone)]
struct WebSocketConnection {
    id: String,
    remote_addr: SocketAddr,
    connected_at: Instant,
    last_activity: Instant,
    is_server: bool, // True if we accepted the connection, false if we initiated it
}

impl WebSocketTransportImpl {
    /// Create a new WebSocket transport instance
    pub async fn new(config: &HashMap<String, String>) -> Result<Self> {
        let local_port = config.get("local_port")
            .and_then(|p| p.parse().ok())
            .unwrap_or(0); // 0 means let OS choose port
            
        let connection_timeout = config.get("connection_timeout_ms")
            .and_then(|t| t.parse().ok())
            .map(Duration::from_millis)
            .unwrap_or(Duration::from_secs(30));
            
        let max_message_size = config.get("max_message_size")
            .and_then(|s| s.parse().ok())
            .unwrap_or(16 * 1024 * 1024); // 16MB default for WebSocket
        
        let circuit_breaker_config = CircuitBreakerConfig {
            failure_threshold: 5,
            recovery_timeout: Duration::from_secs(30),
            success_threshold: 3,
            timeout: Duration::from_secs(10),
            max_half_open_requests: 2,
        };
        
        let mut metrics = TransportMetrics::default();
        metrics.transport_type = TransportType::WebSocket;
        
        Ok(Self {
            local_port,
            listener: None,
            connection_timeout,
            connections: Arc::new(Mutex::new(HashMap::new())),
            received_messages: Arc::new(Mutex::new(Vec::new())),
            status: Arc::new(RwLock::new(TransportStatus::Stopped)),
            metrics: Arc::new(RwLock::new(metrics)),
            circuit_breaker: Arc::new(CircuitBreaker::new(circuit_breaker_config)),
            max_message_size,
        })
    }
    
    /// Connect to a WebSocket server
    async fn connect_to_server(&self, url: &str) -> Result<WebSocketConnection> {
        let start_time = Instant::now();
        
        debug!("Connecting to WebSocket server: {}", url);
        
        // Parse URL
        let parsed_url = Url::parse(url)
            .map_err(|e| crate::error::EmrpError::Generic("Validation error".to_string())", e),
            })?;
        
        // Extract host and port
        let host = parsed_url.host_str()
            .ok_or_else(|| crate::error::EmrpError::Generic("Validation error".to_string()))?;
        
        let port = parsed_url.port_or_known_default()
            .ok_or_else(|| crate::error::EmrpError::Generic("Validation error".to_string()))?;
        
        // Establish TCP connection
        let tcp_stream = TcpStream::connect((host, port)).await
            .map_err(|e| crate::error::EmrpError::Network("Network error".to_string()))?;
        
        let remote_addr = tcp_stream.peer_addr()
            .map_err(|e| crate::error::EmrpError::Network("Network error".to_string()))?;
        
        // In a real implementation, this would perform WebSocket handshake
        // For now, we'll simulate a successful connection
        
        let connection_id = format!("client_{}", remote_addr);
        let connection = WebSocketConnection {
            id: connection_id.clone(),
            remote_addr,
            connected_at: start_time,
            last_activity: Instant::now(),
            is_server: false,
        };
        
        // Store connection
        {
            let mut connections = self.connections.lock().await;
            connections.insert(connection_id, connection.clone());
        }
        
        debug!("WebSocket connection established to {}", remote_addr);
        Ok(connection)
    }
    
    /// Send message via WebSocket
    async fn send_websocket_message(&self, connection_id: &str, message: &SecureMessage) -> Result<Duration> {
        let start_time = Instant::now();
        
        // Check if connection exists
        let connection = {
            let connections = self.connections.lock().await;
            connections.get(connection_id).cloned()
        };
        
        if connection.is_none() {
            return Err(crate::error::EmrpError::Network("Network error".to_string()));
        }
        
        // Serialize message
        let message_json = serde_json::to_string(message)
            .map_err(|e| crate::error::EmrpError::Serialization("Serialization error".to_string()))?;
        
        // Check message size
        if message_json.len() > self.max_message_size {
            return Err(crate::error::EmrpError::Generic("Validation error".to_string()) bytes", message_json.len()),
            });
        }
        
        // In real implementation, send via WebSocket frame
        // For simulation, just log and simulate network delay
        debug!("Sending WebSocket message to connection {}: {} bytes", connection_id, message_json.len());
        tokio::time::sleep(Duration::from_millis(10)).await;
        
        // Update connection activity
        {
            let mut connections = self.connections.lock().await;
            if let Some(conn) = connections.get_mut(connection_id) {
                conn.last_activity = Instant::now();
            }
        }
        
        let duration = start_time.elapsed();
        debug!("WebSocket message sent in {:?}", duration);
        Ok(duration)
    }
    
    /// Receive messages from WebSocket connections
    async fn receive_websocket_messages(&self) -> Result<Vec<IncomingMessage>> {
        // In real implementation, this would:
        // 1. Poll all active WebSocket connections for incoming frames
        // 2. Parse WebSocket frames to extract message data
        // 3. Deserialize SecureMessage from frame payload
        // 4. Return as IncomingMessage instances
        
        // For simulation, return empty list
        Ok(Vec::new())
    }
    
    /// Accept incoming WebSocket connections
    async fn accept_connections(&self) -> Result<()> {
        if let Some(listener) = &self.listener {
            // In real implementation, this would run in a background task
            // accepting incoming TCP connections and performing WebSocket handshake
            debug!("WebSocket server listening for connections");
        }
        Ok(())
    }
    
    /// Update transport metrics
    async fn update_metrics(&self, operation: &str, duration: Duration, success: bool) {
        let mut metrics = self.metrics.write().await;
        
        if success {
            if operation == "send" {
                metrics.messages_sent += 1;
                metrics.bytes_sent += 1024; // Estimate
            } else if operation == "receive" {
                metrics.messages_received += 1;
                metrics.bytes_received += 1024; // Estimate
            }
            
            // Update average latency
            let current_avg = metrics.average_latency_ms;
            let new_latency = duration.as_millis() as u64;
            metrics.average_latency_ms = if current_avg == 0 {
                new_latency
            } else {
                (current_avg + new_latency) / 2
            };
            
            // Update reliability (moving average)
            metrics.reliability_score = (metrics.reliability_score * 0.9) + 0.1;
        } else {
            if operation == "send" {
                metrics.send_failures += 1;
            } else if operation == "receive" {
                metrics.receive_failures += 1;
            }
            
            // Decrease reliability
            metrics.reliability_score = (metrics.reliability_score * 0.9);
        }
        
        // Update active connections count
        let connections = self.connections.lock().await;
        metrics.active_connections = connections.len() as u32;
        
        metrics.last_updated_timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
    }
}

#[async_trait]
impl Transport for WebSocketTransportImpl {
    fn transport_type(&self) -> TransportType {
        TransportType::WebSocket
    }
    
    fn capabilities(&self) -> TransportCapabilities {
        TransportCapabilities {
            max_message_size: self.max_message_size,
            reliable: true, // TCP-based, reliable
            real_time: true, // Good for real-time communication
            broadcast: false, // WebSocket is point-to-point
            bidirectional: true, // Full-duplex communication
            encrypted: true, // Can use WSS (WebSocket Secure)
            network_spanning: true, // Works across networks/internet
            supported_urgencies: vec![
                MessageUrgency::Critical,
                MessageUrgency::RealTime,
                MessageUrgency::Interactive,
                MessageUrgency::Background,
            ],
            features: vec![
                "full_duplex".to_string(),
                "low_latency".to_string(),
                "persistent_connection".to_string(),
                "binary_support".to_string(),
                "compression".to_string(),
                "wss_encryption".to_string(),
            ],
        }
    }
    
    async fn can_reach(&self, target: &TransportTarget) -> bool {
        // Check if target has WebSocket URL or looks like a WebSocket endpoint
        if let Some(address) = &target.address {
            // Check for WebSocket URL schemes
            if address.starts_with("ws://") || address.starts_with("wss://") {
                return true;
            }
            
            // Check if it's a valid HTTP URL that could be upgraded to WebSocket
            if address.starts_with("http://") || address.starts_with("https://") {
                return true;
            }
            
            // Check if it's a socket address that we could connect to
            if address.parse::<SocketAddr>().is_ok() {
                return true;
            }
        }
        
        // Check if we have an existing connection to this target
        let connections = self.connections.lock().await;
        connections.contains_key(&target.identifier)
    }
    
    async fn estimate_metrics(&self, target: &TransportTarget) -> Result<TransportEstimate> {
        let can_reach = self.can_reach(target).await;
        
        if can_reach {
            Ok(TransportEstimate {
                latency: Duration::from_millis(50), // Low latency for WebSocket
                reliability: 0.95, // High reliability due to TCP
                bandwidth: 10 * 1024 * 1024, // 10Mbps typical
                cost: 2.0, // Medium cost
                available: true,
                confidence: 0.85, // Good confidence
            })
        } else {
            Ok(TransportEstimate {
                latency: Duration::from_secs(5),
                reliability: 0.0,
                bandwidth: 0,
                cost: 1000.0,
                available: false,
                confidence: 0.95,
            })
        }
    }
    
    async fn send_message(&self, target: &TransportTarget, message: &SecureMessage) -> Result<DeliveryReceipt> {
        // Try to find existing connection or establish new one
        let connection_id = {
            let connections = self.connections.lock().await;
            if connections.contains_key(&target.identifier) {
                target.identifier.clone()
            } else {
                drop(connections); // Release lock before async operation
                
                // Try to establish new connection
                if let Some(address) = &target.address {
                    let connection = self.connect_to_server(address).await?;
                    connection.id
                } else {
                    return Err(crate::error::EmrpError::Generic("Validation error".to_string()));
                }
            }
        };
        
        // Use circuit breaker to protect against failures
        let send_result = self.circuit_breaker.call(async {
            timeout(self.connection_timeout, self.send_websocket_message(&connection_id, message)).await
                .map_err(|_| crate::error::EmrpError::Network("Network error".to_string()))?
        }).await;
        
        match send_result {
            Ok(duration) => {
                self.update_metrics("send", duration, true).await;
                
                Ok(DeliveryReceipt {
                    message_id: message.id.clone(),
                    transport_used: TransportType::WebSocket,
                    delivery_time: duration,
                    target_reached: connection_id.clone(),
                    confirmation: DeliveryConfirmation::Delivered,
                    metadata: {
                        let mut map = HashMap::new();
                        map.insert("connection_id".to_string(), connection_id);
                        if let Some(addr) = &target.address {
                            map.insert("target_url".to_string(), addr.clone());
                        }
                        map
                    },
                })
            }
            Err(e) => {
                self.update_metrics("send", Duration::from_secs(0), false).await;
                Err(e)
            }
        }
    }
    
    async fn receive_messages(&self) -> Result<Vec<IncomingMessage>> {
        let receive_result = self.circuit_breaker.call(async {
            self.receive_websocket_messages().await
        }).await;
        
        match receive_result {
            Ok(messages) => {
                if !messages.is_empty() {
                    self.update_metrics("receive", Duration::from_millis(1), true).await;
                    
                    // Add messages to internal queue
                    let mut received = self.received_messages.lock().await;
                    received.extend(messages.clone());
                }
                
                Ok(messages)
            }
            Err(e) => {
                self.update_metrics("receive", Duration::from_secs(0), false).await;
                Err(e)
            }
        }
    }
    
    async fn test_connectivity(&self, target: &TransportTarget) -> Result<ConnectivityResult> {
        let start_time = Instant::now();
        
        if let Some(address) = &target.address {
            // Try to establish a test connection
            match self.connect_to_server(address).await {
                Ok(connection) => {
                    let rtt = start_time.elapsed();
                    
                    // Clean up test connection
                    {
                        let mut connections = self.connections.lock().await;
                        connections.remove(&connection.id);
                    }
                    
                    Ok(ConnectivityResult {
                        connected: true,
                        rtt: Some(rtt),
                        error: None,
                        quality: 0.9,
                        details: {
                            let mut map = HashMap::new();
                            map.insert("target_url".to_string(), address.clone());
                            map.insert("connection_time_ms".to_string(), rtt.as_millis().to_string());
                            map
                        },
                    })
                }
                Err(e) => {
                    let rtt = start_time.elapsed();
                    Ok(ConnectivityResult {
                        connected: false,
                        rtt: Some(rtt),
                        error: Some(format!("Connection failed: {}", e)),
                        quality: 0.0,
                        details: HashMap::new(),
                    })
                }
            }
        } else {
            Ok(ConnectivityResult {
                connected: false,
                rtt: None,
                error: Some("No target address provided".to_string()),
                quality: 0.0,
                details: HashMap::new(),
            })
        }
    }
    
    async fn start(&self) -> Result<()> {
        info!("Starting WebSocket transport");
        
        {
            let mut status = self.status.write().await;
            *status = TransportStatus::Starting;
        }
        
        // Bind TCP listener for WebSocket server
        let listener = TcpListener::bind(format!("0.0.0.0:{}", self.local_port)).await
            .map_err(|e| crate::error::EmrpError::Network("Network error".to_string()))?;
        
        let actual_port = listener.local_addr()
            .map_err(|e| crate::error::EmrpError::Network("Network error".to_string()))?.port();
        
        // Store listener
        self.listener.replace(listener);
        
        // Start accepting connections (in real implementation, this would be a background task)
        self.accept_connections().await?;
        
        {
            let mut status = self.status.write().await;
            *status = TransportStatus::Running;
        }
        
        info!("WebSocket transport started on port {} (requested: {})", actual_port, self.local_port);
        Ok(())
    }
    
    async fn stop(&self) -> Result<()> {
        info!("Stopping WebSocket transport");
        
        {
            let mut status = self.status.write().await;
            *status = TransportStatus::Stopping;
        }
        
        // Close all connections
        {
            let mut connections = self.connections.lock().await;
            connections.clear();
        }
        
        // Close listener
        self.listener.take();
        
        {
            let mut status = self.status.write().await;
            *status = TransportStatus::Stopped;
        }
        
        info!("WebSocket transport stopped");
        Ok(())
    }
    
    async fn status(&self) -> TransportStatus {
        *self.status.read().await
    }
    
    async fn metrics(&self) -> TransportMetrics {
        self.metrics.read().await.clone()
    }
}

