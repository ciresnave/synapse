//! TCP Transport implementation conforming to the unified Transport trait

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
};
use tokio::{
    net::{TcpListener, TcpStream},
    io::{AsyncReadExt, AsyncWriteExt},
    sync::Mutex,
};
use tracing::{info, debug, warn, error};
use serde_json;

/// TCP Transport implementation
pub struct TcpTransportImpl {
    /// Local listening port
    listen_port: u16,
    /// TCP listener (if acting as server)
    listener: Option<TcpListener>,
    /// Connection timeout
    connection_timeout: Duration,
    /// Received messages queue
    received_messages: Arc<Mutex<Vec<IncomingMessage>>>,
    /// Current status
    status: Arc<RwLock<TransportStatus>>,
    /// Performance metrics
    metrics: Arc<RwLock<TransportMetrics>>,
    /// Circuit breaker for reliability
    circuit_breaker: Arc<CircuitBreaker>,
}

impl TcpTransportImpl {
    /// Create a new TCP transport instance
    pub async fn new(config: &HashMap<String, String>) -> Result<Self> {
        let listen_port = config.get("listen_port")
            .and_then(|p| p.parse().ok())
            .unwrap_or(0); // 0 means let OS choose port
            
        let connection_timeout = config.get("connection_timeout_ms")
            .and_then(|t| t.parse().ok())
            .map(Duration::from_millis)
            .unwrap_or(Duration::from_secs(10));

        let listener = if listen_port > 0 {
            match TcpListener::bind(format!("0.0.0.0:{}", listen_port)).await {
                Ok(listener) => {
                    info!("TCP transport listening on port {}", listen_port);
                    Some(listener)
                }
                Err(e) => {
                    warn!("Failed to bind TCP port {}: {}", listen_port, e);
                    None
                }
            }
        } else {
            None
        };

        let mut metrics = TransportMetrics::default();
        metrics.transport_type = TransportType::Tcp;

        Ok(Self {
            listen_port,
            listener,
            connection_timeout,
            received_messages: Arc::new(Mutex::new(Vec::new())),
            status: Arc::new(RwLock::new(TransportStatus::Stopped)),
            metrics: Arc::new(RwLock::new(metrics)),
            circuit_breaker: Arc::new(CircuitBreaker::new(CircuitBreakerConfig::default())),
        })
    }

    /// Start the TCP server for incoming connections
    async fn start_server(&self) -> Result<()> {
        if let Some(listener) = &self.listener {
            let received_messages = Arc::clone(&self.received_messages);
            let metrics = Arc::clone(&self.metrics);
            
            // Clone the listener for the task - this requires cloning the socket
            let local_addr = listener.local_addr().map_err(|e| {
                crate::error::EmrpError::Transport(format!("Failed to get local address: {}", e))
            })?;
            
            tokio::spawn(async move {
                // Create a new listener for the server task
                if let Ok(listener) = TcpListener::bind(local_addr).await {
                    info!("TCP server started on {}", local_addr);
                    
                    loop {
                        match listener.accept().await {
                            Ok((stream, addr)) => {
                                debug!("Accepted TCP connection from {}", addr);
                                let messages_clone = Arc::clone(&received_messages);
                                let metrics_clone = Arc::clone(&metrics);
                                
                                tokio::spawn(async move {
                                    Self::handle_connection(stream, addr.to_string(), messages_clone, metrics_clone).await;
                                });
                            }
                            Err(e) => {
                                error!("Failed to accept TCP connection: {}", e);
                                tokio::time::sleep(Duration::from_millis(100)).await;
                            }
                        }
                    }
                } else {
                    error!("Failed to create TCP listener for server task");
                }
            });
            
            Ok(())
        } else {
            debug!("TCP transport running in client-only mode");
            Ok(())
        }
    }

    async fn handle_connection(
        mut stream: TcpStream,
        source_addr: String,
        received_messages: Arc<Mutex<Vec<IncomingMessage>>>,
        metrics: Arc<RwLock<TransportMetrics>>,
    ) {
        let mut buffer = vec![0; 8192];
        
        match tokio::time::timeout(Duration::from_secs(30), stream.read(&mut buffer)).await {
            Ok(Ok(bytes_read)) => {
                debug!("Received {} bytes via TCP from {}", bytes_read, source_addr);
                buffer.truncate(bytes_read);
                
                if let Ok(message_str) = String::from_utf8(buffer) {
                    if let Ok(message) = serde_json::from_str::<SecureMessage>(&message_str) {
                        let incoming = IncomingMessage::new(
                            message,
                            TransportType::Tcp,
                            source_addr,
                        );
                        
                        if let Ok(mut messages) = received_messages.try_lock() {
                            messages.push(incoming);
                            debug!("Queued TCP message, total: {}", messages.len());
                        }
                        
                        // Update metrics
                        if let Ok(mut metrics) = metrics.try_write() {
                            metrics.messages_received += 1;
                            metrics.bytes_received += bytes_read as u64;
                            metrics.touch();
                        }
                    } else {
                        warn!("Failed to parse message from {}", source_addr);
                    }
                } else {
                    warn!("Received invalid UTF-8 data from {}", source_addr);
                }
            }
            Ok(Err(e)) => {
                debug!("Failed to read from TCP connection {}: {}", source_addr, e);
            }
            Err(_) => {
                debug!("TCP connection from {} timed out", source_addr);
            }
        }
    }

    async fn connect_and_send(&self, address: &str, port: u16, message: &SecureMessage) -> Result<DeliveryReceipt> {
        let target_addr = format!("{}:{}", address, port);
        debug!("Connecting to TCP target: {}", target_addr);
        
        let start_time = Instant::now();
        
        // Connect with timeout
        let stream = tokio::time::timeout(
            self.connection_timeout,
            TcpStream::connect(&target_addr)
        ).await
        .map_err(|_| crate::error::EmrpError::Transport("TCP connection timeout".to_string()))?
        .map_err(|e| crate::error::EmrpError::Transport(format!("TCP connection failed: {}", e)))?;
        
        let connect_time = start_time.elapsed();
        debug!("TCP connection established in {:?}", connect_time);
        
        // Serialize message
        let message_json = serde_json::to_string(message)
            .map_err(|e| crate::error::EmrpError::Transport(format!("Failed to serialize message: {}", e)))?;
        
        // Send message
        let mut stream = stream;
        let send_start = Instant::now();
        
        stream.write_all(message_json.as_bytes()).await
            .map_err(|e| crate::error::EmrpError::Transport(format!("Failed to send TCP message: {}", e)))?;
        
        stream.flush().await
            .map_err(|e| crate::error::EmrpError::Transport(format!("Failed to flush TCP stream: {}", e)))?;
        
        let send_time = send_start.elapsed();
        let total_time = start_time.elapsed();
        
        info!("TCP message sent to {} in {:?} (connect: {:?}, send: {:?})", 
              target_addr, total_time, connect_time, send_time);
        
        // Update metrics
        if let Ok(mut metrics) = self.metrics.try_write() {
            metrics.messages_sent += 1;
            metrics.bytes_sent += message_json.len() as u64;
            
            // Update average latency
            let total_messages = metrics.messages_sent;
            let old_avg_ms = metrics.average_latency_ms as f64;
            let new_latency_ms = total_time.as_millis() as f64;
            let new_avg_ms = (old_avg_ms * (total_messages - 1) as f64 + new_latency_ms) / total_messages as f64;
            metrics.average_latency_ms = new_avg_ms as u64;
            
            metrics.touch();
        }
        
        Ok(DeliveryReceipt {
            message_id: message.message_id.to_string(),
            transport_used: TransportType::Tcp,
            delivery_time: total_time,
            target_reached: target_addr,
            confirmation: DeliveryConfirmation::Sent,
            metadata: {
                let mut meta = HashMap::new();
                meta.insert("connect_time_ms".to_string(), connect_time.as_millis().to_string());
                meta.insert("send_time_ms".to_string(), send_time.as_millis().to_string());
                meta
            },
        })
    }

    fn parse_target_address(&self, target: &TransportTarget) -> Result<(String, u16)> {
        if let Some(address) = &target.address {
            // Try to parse as host:port
            if let Some(colon_pos) = address.rfind(':') {
                let host = address[..colon_pos].to_string();
                let port_str = &address[colon_pos + 1..];
                let port = port_str.parse::<u16>()
                    .map_err(|_| crate::error::EmrpError::Transport(
                        format!("Invalid port in address: {}", address)
                    ))?;
                Ok((host, port))
            } else {
                // No port specified, use default
                Ok((address.clone(), 8080))
            }
        } else {
            // Use identifier as hostname with default port
            Ok((target.identifier.clone(), 8080))
        }
    }
}

#[async_trait]
impl Transport for TcpTransportImpl {
    fn transport_type(&self) -> TransportType {
        TransportType::Tcp
    }

    fn capabilities(&self) -> TransportCapabilities {
        TransportCapabilities::tcp()
    }

    async fn can_reach(&self, target: &TransportTarget) -> bool {
        // Try to parse the target address
        if let Ok((host, port)) = self.parse_target_address(target) {
            // Attempt a quick connection test
            match tokio::time::timeout(
                Duration::from_secs(5),
                TcpStream::connect(format!("{}:{}", host, port))
            ).await {
                Ok(Ok(_)) => true,
                _ => false,
            }
        } else {
            false
        }
    }

    async fn estimate_metrics(&self, target: &TransportTarget) -> Result<TransportEstimate> {
        let (host, port) = self.parse_target_address(target)?;
        
        // Perform a quick connection test to estimate metrics
        let start = Instant::now();
        let can_connect = match tokio::time::timeout(
            Duration::from_secs(3),
            TcpStream::connect(format!("{}:{}", host, port))
        ).await {
            Ok(Ok(_)) => true,
            _ => false,
        };
        let rtt = start.elapsed();
        
        Ok(TransportEstimate {
            latency: if can_connect { rtt } else { Duration::from_secs(30) },
            reliability: if can_connect { 0.9 } else { 0.1 },
            bandwidth: 1_000_000, // 1MB/s estimate for TCP
            cost: 1.0, // Relative cost
            available: can_connect,
            confidence: if can_connect { 0.8 } else { 0.3 },
        })
    }

    async fn send_message(&self, target: &TransportTarget, message: &SecureMessage) -> Result<DeliveryReceipt> {
        let (host, port) = self.parse_target_address(target)?;
        self.connect_and_send(&host, port, message).await
    }

    async fn receive_messages(&self) -> Result<Vec<IncomingMessage>> {
        let mut messages = self.received_messages.lock().await;
        let result = messages.drain(..).collect();
        Ok(result)
    }

    async fn test_connectivity(&self, target: &TransportTarget) -> Result<ConnectivityResult> {
        let (host, port) = self.parse_target_address(target)?;
        let target_addr = format!("{}:{}", host, port);
        
        let start = Instant::now();
        match tokio::time::timeout(
            Duration::from_secs(10),
            TcpStream::connect(&target_addr)
        ).await {
            Ok(Ok(_)) => {
                let rtt = start.elapsed();
                Ok(ConnectivityResult {
                    connected: true,
                    rtt: Some(rtt),
                    error: None,
                    quality: 1.0 - (rtt.as_millis() as f64 / 10000.0).min(1.0), // Quality decreases with latency
                    details: {
                        let mut details = HashMap::new();
                        details.insert("target".to_string(), target_addr);
                        details.insert("rtt_ms".to_string(), rtt.as_millis().to_string());
                        details
                    },
                })
            }
            Ok(Err(e)) => {
                Ok(ConnectivityResult {
                    connected: false,
                    rtt: None,
                    error: Some(format!("Connection failed: {}", e)),
                    quality: 0.0,
                    details: {
                        let mut details = HashMap::new();
                        details.insert("target".to_string(), target_addr);
                        details.insert("error".to_string(), e.to_string());
                        details
                    },
                })
            }
            Err(_) => {
                Ok(ConnectivityResult {
                    connected: false,
                    rtt: None,
                    error: Some("Connection timeout".to_string()),
                    quality: 0.0,
                    details: {
                        let mut details = HashMap::new();
                        details.insert("target".to_string(), target_addr);
                        details.insert("error".to_string(), "timeout".to_string());
                        details
                    },
                })
            }
        }
    }

    async fn start(&self) -> Result<()> {
        info!("Starting TCP transport");
        
        {
            let mut status = self.status.write().unwrap();
            *status = TransportStatus::Starting;
        }
        
        // Start server if we have a listener
        self.start_server().await?;
        
        {
            let mut status = self.status.write().unwrap();
            *status = TransportStatus::Running;
        }
        
        info!("TCP transport started successfully");
        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        info!("Stopping TCP transport");
        
        {
            let mut status = self.status.write().unwrap();
            *status = TransportStatus::Stopping;
        }
        
        // TCP transport doesn't need explicit cleanup
        // Connections will be closed when dropped
        
        {
            let mut status = self.status.write().unwrap();
            *status = TransportStatus::Stopped;
        }
        
        info!("TCP transport stopped");
        Ok(())
    }

    async fn status(&self) -> TransportStatus {
        *self.status.read().unwrap()
    }

    async fn metrics(&self) -> TransportMetrics {
        self.metrics.read().unwrap().clone()
    }
}

/// Factory for creating TCP transport instances
pub struct TcpTransportFactory;

#[async_trait]
impl TransportFactory for TcpTransportFactory {
    async fn create_transport(&self, config: &HashMap<String, String>) -> Result<Box<dyn Transport>> {
        let transport = TcpTransportImpl::new(config).await?;
        Ok(Box::new(transport))
    }

    fn transport_type(&self) -> TransportType {
        TransportType::Tcp
    }

    fn default_config(&self) -> HashMap<String, String> {
        let mut config = HashMap::new();
        config.insert("listen_port".to_string(), "8080".to_string());
        config.insert("connection_timeout_ms".to_string(), "10000".to_string());
        config
    }

    fn validate_config(&self, config: &HashMap<String, String>) -> Result<()> {
        if let Some(port_str) = config.get("listen_port") {
            if port_str.parse::<u16>().is_err() {
                return Err(crate::error::EmrpError::Transport(
                    format!("Invalid listen_port: {}", port_str)
                ));
            }
        }
        
        if let Some(timeout_str) = config.get("connection_timeout_ms") {
            if timeout_str.parse::<u64>().is_err() {
                return Err(crate::error::EmrpError::Transport(
                    format!("Invalid connection_timeout_ms: {}", timeout_str)
                ));
            }
        }
        
        Ok(())
    }
}

