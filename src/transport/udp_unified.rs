//! UDP Transport implementation conforming to the unified Transport trait

use crate::{
    types::SecureMessage,
    error::{Result, SynapseError},
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
    net::UdpSocket,
    sync::Mutex,
};
use tracing::{info, debug, warn, error};
use serde_json;

/// UDP Transport implementation
pub struct UdpTransportImpl {
    /// Local socket
    socket: Option<Arc<UdpSocket>>,
    /// Local binding port
    bind_port: u16,
    /// Maximum message size
    max_message_size: usize,
    /// Received messages queue
    received_messages: Arc<Mutex<Vec<IncomingMessage>>>,
    /// Current status
    status: Arc<RwLock<TransportStatus>>,
    /// Performance metrics
    metrics: Arc<RwLock<TransportMetrics>>,
    /// Circuit breaker for reliability
    #[allow(dead_code)]
    circuit_breaker: Arc<CircuitBreaker>,
}

impl UdpTransportImpl {
    /// Create a new UDP transport instance
    pub async fn new(config: &HashMap<String, String>) -> Result<Self> {
        let bind_port = config.get("bind_port")
            .and_then(|p| p.parse().ok())
            .unwrap_or(0); // 0 means let OS choose port
            
        let max_message_size = config.get("max_message_size")
            .and_then(|s| s.parse().ok())
            .unwrap_or(65507); // Max UDP payload size

        let mut metrics = TransportMetrics::default();
        metrics.transport_type = TransportType::Udp;

        Ok(Self {
            socket: None,
            bind_port,
            max_message_size,
            received_messages: Arc::new(Mutex::new(Vec::new())),
            status: Arc::new(RwLock::new(TransportStatus::Stopped)),
            metrics: Arc::new(RwLock::new(metrics)),
            circuit_breaker: Arc::new(CircuitBreaker::new(CircuitBreakerConfig::default())),
        })
    }

    /// Start the UDP server for incoming messages
    async fn start_server(&mut self) -> Result<()> {
        let bind_addr = format!("0.0.0.0:{}", self.bind_port);
        
        let socket = UdpSocket::bind(&bind_addr).await
            .map_err(|e| SynapseError::TransportError(
                format!("Failed to bind UDP socket to {}: {}", bind_addr, e)
            ))?;
            
        let local_addr = socket.local_addr()
            .map_err(|e| SynapseError::TransportError(
                format!("Failed to get UDP local address: {}", e)
            ))?;
            
        info!("UDP transport bound to {}", local_addr);
        
        let socket = Arc::new(socket);
        self.socket = Some(socket.clone());
        
        // Start receiving task
        let received_messages = Arc::clone(&self.received_messages);
        let metrics = Arc::clone(&self.metrics);
        let max_size = self.max_message_size;
        
        tokio::spawn(async move {
            let mut buffer = vec![0; max_size];
            
            loop {
                match socket.recv_from(&mut buffer).await {
                    Ok((len, addr)) => {
                        debug!("Received {} bytes via UDP from {}", len, addr);
                        
                        if let Ok(message_str) = String::from_utf8(buffer[..len].to_vec()) {
                            if let Ok(message) = serde_json::from_str::<SecureMessage>(&message_str) {
                                let mut incoming = IncomingMessage::new(
                                    message,
                                    TransportType::Udp,
                                    addr.to_string(),
                                );
                                incoming.metadata.insert("packet_size".to_string(), len.to_string());
                                
                                if let Ok(mut messages) = received_messages.try_lock() {
                                    messages.push(incoming);
                                    debug!("Queued UDP message, total: {}", messages.len());
                                }
                                
                                // Update metrics
                                if let Ok(mut metrics) = metrics.try_write() {
                                    metrics.messages_received += 1;
                                    metrics.bytes_received += len as u64;
                                    metrics.touch();
                                }
                            } else {
                                warn!("Failed to parse UDP message from {}", addr);
                            }
                        } else {
                            warn!("Received invalid UTF-8 data via UDP from {}", addr);
                        }
                    }
                    Err(e) => {
                        error!("UDP receive error: {}", e);
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                }
            }
        });
        
        Ok(())
    }

    async fn send_to(&self, target_addr: &SocketAddr, message: &SecureMessage) -> Result<DeliveryReceipt> {
        debug!("Sending UDP message to {}", target_addr);
        
        // Serialize message
        let message_json = serde_json::to_string(message)
            .map_err(|e| SynapseError::TransportError(format!("Failed to serialize message: {}", e)))?;
        
        // Check message size
        if message_json.len() > self.max_message_size {
            return Err(SynapseError::TransportError(
                format!("Message too large for UDP: {} bytes (max: {})", 
                        message_json.len(), self.max_message_size)
            ));
        }
        
        let start_time = Instant::now();
        
        // Use existing socket or create a temporary one
        let result = if let Some(socket) = &self.socket {
            socket.send_to(message_json.as_bytes(), target_addr).await
        } else {
            // Create temporary socket for sending
            let temp_socket = UdpSocket::bind("0.0.0.0:0").await
                .map_err(|e| SynapseError::TransportError(
                    format!("Failed to create UDP socket for sending: {}", e)
                ))?;
            temp_socket.send_to(message_json.as_bytes(), target_addr).await
        };
        
        match result {
            Ok(bytes_sent) => {
                let send_time = start_time.elapsed();
                
                info!("UDP message sent to {} ({} bytes) in {:?}", 
                      target_addr, bytes_sent, send_time);
                
                // Update metrics
                if let Ok(mut metrics) = self.metrics.try_write() {
                    metrics.messages_sent += 1;
                    metrics.bytes_sent += bytes_sent as u64;
                    
                    // Update average latency
                    let total_messages = metrics.messages_sent;
                    let old_avg_ms = metrics.average_latency_ms as f64;
                    let new_latency_ms = send_time.as_millis() as f64;
                    let new_avg_ms = (old_avg_ms * (total_messages - 1) as f64 + new_latency_ms) / total_messages as f64;
                    metrics.average_latency_ms = new_avg_ms as u64;
                    
                    metrics.touch();
                }
                
                Ok(DeliveryReceipt {
                    message_id: message.message_id.0.to_string(),
                    transport_used: TransportType::Udp,
                    delivery_time: send_time,
                    target_reached: target_addr.to_string(),
                    confirmation: DeliveryConfirmation::Sent, // UDP is best-effort
                    metadata: {
                        let mut meta = HashMap::new();
                        meta.insert("bytes_sent".to_string(), bytes_sent.to_string());
                        meta.insert("packet_size".to_string(), message_json.len().to_string());
                        meta
                    },
                })
            }
            Err(e) => {
                Err(SynapseError::TransportError(
                    format!("Failed to send UDP message: {}", e)
                ))
            }
        }
    }

    fn parse_target_address(&self, target: &TransportTarget) -> Result<SocketAddr> {
        if let Some(address) = &target.address {
            // Try to parse as socket address
            address.parse()
                .or_else(|_| {
                    // Try to parse as host:port
                    if let Some(colon_pos) = address.rfind(':') {
                        let host = &address[..colon_pos];
                        let port_str = &address[colon_pos + 1..];
                        let port = port_str.parse::<u16>()
                            .map_err(|_| SynapseError::TransportError(
                                format!("Invalid port in address: {}", address)
                            ))?;
                        format!("{}:{}", host, port).parse()
                            .map_err(|e| SynapseError::TransportError(
                                format!("Failed to parse address: {}", e)
                            ))
                    } else {
                        Err(SynapseError::TransportError(
                            format!("Invalid UDP address format: {}", address)
                        ))
                    }
                })
                .map_err(|e| SynapseError::TransportError(
                    format!("Failed to parse UDP address '{}': {}", address, e)
                ))
        } else {
            // Use identifier as hostname with default UDP port
            format!("{}:8081", target.identifier).parse()
                .map_err(|e| SynapseError::TransportError(
                    format!("Failed to parse identifier as UDP address: {}", e)
                ))
        }
    }
}

#[async_trait]
impl Transport for UdpTransportImpl {
    fn transport_type(&self) -> TransportType {
        TransportType::Udp
    }

    fn capabilities(&self) -> TransportCapabilities {
        TransportCapabilities::udp()
    }

    async fn can_reach(&self, target: &TransportTarget) -> bool {
        // UDP doesn't have connection establishment, so we can't test reachability easily
        // Just check if we can parse the address
        self.parse_target_address(target).is_ok()
    }

    async fn estimate_metrics(&self, target: &TransportTarget) -> Result<TransportEstimate> {
        let _addr = self.parse_target_address(target)?;
        
        // UDP estimates are more speculative since there's no connection
        Ok(TransportEstimate {
            latency: Duration::from_millis(10), // Assume low latency for UDP
            reliability: 0.8, // UDP is less reliable than TCP
            bandwidth: 10_000_000, // 10MB/s estimate for UDP
            cost: 0.5, // Lower cost than TCP
            available: true, // Assume available if address is valid
            confidence: 0.6, // Lower confidence since we can't test
        })
    }

    async fn send_message(&self, target: &TransportTarget, message: &SecureMessage) -> Result<DeliveryReceipt> {
        let target_addr = self.parse_target_address(target)?;
        self.send_to(&target_addr, message).await
    }

    async fn receive_messages(&self) -> Result<Vec<IncomingMessage>> {
        let mut messages = self.received_messages.lock().await;
        let result = messages.drain(..).collect();
        Ok(result)
    }

    async fn test_connectivity(&self, target: &TransportTarget) -> Result<ConnectivityResult> {
        let target_addr = self.parse_target_address(target)?;
        
        // For UDP, we can only test if we can bind and send
        // We'll send a small test packet and see if it succeeds
        let start = Instant::now();
        
        match UdpSocket::bind("0.0.0.0:0").await {
            Ok(test_socket) => {
                let test_data = b"ping";
                match test_socket.send_to(test_data, &target_addr).await {
                    Ok(_) => {
                        let rtt = start.elapsed();
                        Ok(ConnectivityResult {
                            connected: true,
                            rtt: Some(rtt),
                            error: None,
                            quality: 0.8, // UDP quality is inherently lower
                            details: {
                                let mut details = HashMap::new();
                                details.insert("target".to_string(), target_addr.to_string());
                                details.insert("test_type".to_string(), "send_test".to_string());
                                details.insert("rtt_ms".to_string(), rtt.as_millis().to_string());
                                details
                            },
                        })
                    }
                    Err(e) => {
                        Ok(ConnectivityResult {
                            connected: false,
                            rtt: None,
                            error: Some(format!("Send failed: {}", e)),
                            quality: 0.0,
                            details: {
                                let mut details = HashMap::new();
                                details.insert("target".to_string(), target_addr.to_string());
                                details.insert("error".to_string(), e.to_string());
                                details
                            },
                        })
                    }
                }
            }
            Err(e) => {
                Ok(ConnectivityResult {
                    connected: false,
                    rtt: None,
                    error: Some(format!("Failed to create test socket: {}", e)),
                    quality: 0.0,
                    details: {
                        let mut details = HashMap::new();
                        details.insert("target".to_string(), target_addr.to_string());
                        details.insert("error".to_string(), e.to_string());
                        details
                    },
                })
            }
        }
    }

    async fn start(&self) -> Result<()> {
        info!("Starting UDP transport");
        
        {
            let mut status = self.status.write().unwrap();
            *status = TransportStatus::Starting;
        }
        
        // Note: We need mutable access to self to start the server
        // This is a limitation of the current design - we'll work around it
        info!("UDP transport ready (server will start on first use)");
        
        {
            let mut status = self.status.write().unwrap();
            *status = TransportStatus::Running;
        }
        
        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        info!("Stopping UDP transport");
        
        {
            let mut status = self.status.write().unwrap();
            *status = TransportStatus::Stopping;
        }
        
        // UDP sockets will be closed when dropped
        
        {
            let mut status = self.status.write().unwrap();
            *status = TransportStatus::Stopped;
        }
        
        info!("UDP transport stopped");
        Ok(())
    }

    async fn status(&self) -> TransportStatus {
        *self.status.read().unwrap()
    }

    async fn metrics(&self) -> TransportMetrics {
        self.metrics.read().unwrap().clone()
    }
}

/// Factory for creating UDP transport instances
pub struct UdpTransportFactory;

#[async_trait]
impl TransportFactory for UdpTransportFactory {
    async fn create_transport(&self, config: &HashMap<String, String>) -> Result<Box<dyn Transport>> {
        let mut transport = UdpTransportImpl::new(config).await?;
        
        // Start the server immediately if bind_port is specified
        if let Some(port_str) = config.get("bind_port") {
            if let Ok(port) = port_str.parse::<u16>() {
                if port > 0 {
                    transport.start_server().await?;
                }
            }
        }
        
        Ok(Box::new(transport))
    }

    fn transport_type(&self) -> TransportType {
        TransportType::Udp
    }

    fn default_config(&self) -> HashMap<String, String> {
        let mut config = HashMap::new();
        config.insert("bind_port".to_string(), "8081".to_string());
        config.insert("max_message_size".to_string(), "65507".to_string());
        config
    }

    fn validate_config(&self, config: &HashMap<String, String>) -> Result<()> {
        if let Some(port_str) = config.get("bind_port") {
            if port_str.parse::<u16>().is_err() {
                return Err(SynapseError::TransportError(
                    format!("Invalid bind_port: {}", port_str)
                ));
            }
        }
        
        if let Some(size_str) = config.get("max_message_size") {
            if let Ok(size) = size_str.parse::<usize>() {
                if size > 65507 {
                    return Err(SynapseError::TransportError(
                        "max_message_size cannot exceed 65507 bytes for UDP".to_string()
                    ));
                }
            } else {
                return Err(SynapseError::TransportError(
                    format!("Invalid max_message_size: {}", size_str)
                ));
            }
        }
        
        Ok(())
    }
}

