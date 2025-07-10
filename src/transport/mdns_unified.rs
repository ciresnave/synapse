//! mDNS Transport implementation conforming to the unified Transport trait

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
    net::{SocketAddr, IpAddr},
};
use tokio::{
    sync::Mutex,
    time::timeout,
    net::UdpSocket,
};
use tracing::{info, debug, warn, error};
use serde_json;

/// mDNS Transport implementation for unified abstraction
pub struct MdnsTransportImpl {
    /// Service name for mDNS discovery
    service_name: String,
    /// Local port for UDP communication
    local_port: u16,
    /// UDP socket for mDNS communication
    socket: Option<Arc<UdpSocket>>,
    /// Discovery timeout
    discovery_timeout: Duration,
    /// Discovered peers
    discovered_peers: Arc<Mutex<HashMap<String, MdnsPeer>>>,
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

/// Represents a discovered mDNS peer
#[derive(Debug, Clone)]
struct MdnsPeer {
    service_name: String,
    address: SocketAddr,
    last_seen: Instant,
    capabilities: Vec<String>,
}

impl MdnsTransportImpl {
    /// Create a new mDNS transport instance
    pub async fn new(config: &HashMap<String, String>) -> Result<Self> {
        let service_name = config.get("service_name")
            .cloned()
            .unwrap_or_else(|| "_synapse._tcp.local".to_string());
        
        let local_port = config.get("local_port")
            .and_then(|p| p.parse().ok())
            .unwrap_or(0); // 0 means let OS choose port
            
        let discovery_timeout = config.get("discovery_timeout_ms")
            .and_then(|t| t.parse().ok())
            .map(Duration::from_millis)
            .unwrap_or(Duration::from_secs(5));
            
        let max_message_size = config.get("max_message_size")
            .and_then(|s| s.parse().ok())
            .unwrap_or(65507); // Max UDP payload size
        
        let circuit_breaker_config = CircuitBreakerConfig {
            failure_threshold: 3,
            recovery_timeout: Duration::from_secs(30),
            success_threshold: 2,
            timeout: Duration::from_secs(5),
            max_half_open_requests: 1,
        };
        
        let mut metrics = TransportMetrics::default();
        metrics.transport_type = TransportType::Mdns;
        
        Ok(Self {
            service_name,
            local_port,
            socket: None,
            discovery_timeout,
            discovered_peers: Arc::new(Mutex::new(HashMap::new())),
            received_messages: Arc::new(Mutex::new(Vec::new())),
            status: Arc::new(RwLock::new(TransportStatus::Stopped)),
            metrics: Arc::new(RwLock::new(metrics)),
            circuit_breaker: Arc::new(CircuitBreaker::new(circuit_breaker_config)),
            max_message_size,
        })
    }
    
    /// Discover peers using mDNS
    async fn discover_peers(&self) -> Result<Vec<MdnsPeer>> {
        debug!("Starting mDNS peer discovery for service: {}", self.service_name);
        
        // In real implementation, this would:
        // 1. Send mDNS queries for the service type
        // 2. Listen for mDNS responses
        // 3. Parse service records to extract peer information
        // 4. Return list of discovered peers
        
        // For simulation, return some mock peers
        let mock_peers = vec![
            MdnsPeer {
                service_name: format!("peer1.{}", self.service_name),
                address: "192.168.1.100:8080".parse().unwrap(),
                last_seen: Instant::now(),
                capabilities: vec!["messaging".to_string(), "discovery".to_string()],
            },
            MdnsPeer {
                service_name: format!("peer2.{}", self.service_name),
                address: "192.168.1.101:8080".parse().unwrap(),
                last_seen: Instant::now(),
                capabilities: vec!["messaging".to_string()],
            },
        ];
        
        // Update discovered peers cache
        {
            let mut peers = self.discovered_peers.lock().await;
            for peer in &mock_peers {
                peers.insert(peer.service_name.clone(), peer.clone());
            }
        }
        
        debug!("Discovered {} mDNS peers", mock_peers.len());
        Ok(mock_peers)
    }
    
    /// Send message to a specific peer via UDP
    async fn send_udp_message(&self, target: &SocketAddr, message: &SecureMessage) -> Result<Duration> {
        let start_time = Instant::now();
        
        if let Some(socket) = &self.socket {
            // Serialize message
            let message_json = serde_json::to_string(message)
                .map_err(|e| crate::error::EmrpError::Serialization("Serialization error".to_string()))?;
            
            // Check message size
            let message_bytes = message_json.as_bytes();
            if message_bytes.len() > self.max_message_size {
                return Err(crate::error::EmrpError::Generic(
                    format!("Message too large: {} bytes", message_bytes.len())
                ));
            }
            
            // Send via UDP
            socket.send_to(message_bytes, target).await
                .map_err(|e| crate::error::EmrpError::Network(format!("Network error: {}", e)))?;
            
            let duration = start_time.elapsed();
            debug!("mDNS message sent to {} in {:?}", target, duration);
            Ok(duration)
        } else {
            Err(crate::error::EmrpError::Network("Socket not available".to_string()))
        }
    }
    
    /// Receive messages from UDP socket
    async fn receive_udp_messages(&self) -> Result<Vec<IncomingMessage>> {
        if let Some(socket) = &self.socket {
            let mut messages = Vec::new();
            let mut buf = vec![0u8; self.max_message_size];
            
            // Try to receive a message (non-blocking)
            match socket.try_recv_from(&mut buf) {
                Ok((len, addr)) => {
                    let message_data = &buf[..len];
                    
                    // Deserialize message
                    match serde_json::from_slice::<SecureMessage>(message_data) {
                        Ok(secure_message) => {
                            let incoming = IncomingMessage::new(
                                secure_message,
                                TransportType::Mdns,
                                addr.to_string(),
                            );
                            messages.push(incoming);
                            debug!("Received mDNS message from {}", addr);
                        }
                        Err(e) => {
                            warn!("Failed to deserialize mDNS message from {}: {}", addr, e);
                        }
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    // No messages available, this is normal
                }
                Err(e) => {
                    warn!("Error receiving mDNS message: {}", e);
                }
            }
            
            Ok(messages)
        } else {
            Ok(Vec::new())
        }
    }
    
    /// Find peer by identifier
    async fn find_peer(&self, identifier: &str) -> Option<MdnsPeer> {
        // First try to find in discovered peers
        {
            let peers = self.discovered_peers.lock().await;
            if let Some(peer) = peers.get(identifier) {
                return Some(peer.clone());
            }
        }
        
        // If not found, try discovery
        if let Ok(_peers) = self.discover_peers().await {
            let peers = self.discovered_peers.lock().await;
            peers.get(identifier).cloned()
        } else {
            None
        }
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
        
        // Update active connections (peer count)
        let peers = self.discovered_peers.lock().await;
        metrics.active_connections = peers.len() as u32;
        
        metrics.last_updated_timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
    }
}

#[async_trait]
impl Transport for MdnsTransportImpl {
    fn transport_type(&self) -> TransportType {
        TransportType::Mdns
    }
    
    fn capabilities(&self) -> TransportCapabilities {
        TransportCapabilities {
            max_message_size: self.max_message_size,
            reliable: false, // UDP-based, not reliable by default
            real_time: true, // Good for real-time local communication
            broadcast: true, // mDNS supports multicast/broadcast
            bidirectional: true, // Can send and receive
            encrypted: false, // Basic mDNS is not encrypted (but can layer encryption)
            network_spanning: false, // Limited to local network
            supported_urgencies: vec![
                MessageUrgency::Critical,
                MessageUrgency::RealTime,
                MessageUrgency::Interactive,
                MessageUrgency::Background,
            ],
            features: vec![
                "service_discovery".to_string(),
                "multicast".to_string(),
                "local_network".to_string(),
                "zero_config".to_string(),
                "peer_discovery".to_string(),
            ],
        }
    }
    
    async fn can_reach(&self, target: &TransportTarget) -> bool {
        // Check if we have a discovered peer or can parse as local address
        if let Some(_peer) = self.find_peer(&target.identifier).await {
            return true;
        }
        
        // Check if target looks like a local network address
        if let Some(address) = &target.address {
            if let Ok(addr) = address.parse::<SocketAddr>() {
                // Check if it's a local network address
                match addr.ip() {
                    IpAddr::V4(ipv4) => {
                        ipv4.is_private() || ipv4.is_loopback()
                    }
                    IpAddr::V6(ipv6) => {
                        ipv6.is_loopback() || ipv6.to_string().starts_with("fe80:")
                    }
                }
            } else {
                false
            }
        } else {
            false
        }
    }
    
    async fn estimate_metrics(&self, target: &TransportTarget) -> Result<TransportEstimate> {
        let can_reach = self.can_reach(target).await;
        
        if can_reach {
            Ok(TransportEstimate {
                latency: Duration::from_millis(10), // Very low latency on local network
                reliability: 0.85, // UDP reliability
                bandwidth: 100 * 1024 * 1024, // 100Mbps typical local network
                cost: 0.1, // Very low cost for local communication
                available: true,
                confidence: 0.9, // High confidence for local network
            })
        } else {
            Ok(TransportEstimate {
                latency: Duration::from_secs(1),
                reliability: 0.0,
                bandwidth: 0,
                cost: 100.0, // High cost if not reachable
                available: false,
                confidence: 0.95,
            })
        }
    }
    
    async fn send_message(&self, target: &TransportTarget, message: &SecureMessage) -> Result<DeliveryReceipt> {
        // Try to find peer or parse address
        let target_addr = if let Some(peer) = self.find_peer(&target.identifier).await {
            peer.address
        } else if let Some(address) = &target.address {
            address.parse::<SocketAddr>()
                .map_err(|e| crate::error::EmrpError::Generic(format!("Invalid address: {}", e)))?
        } else {
            return Err(crate::error::EmrpError::Generic("No valid target address".to_string()));
        };
        
        // Use circuit breaker to protect against failures
        let send_result = self.circuit_breaker.call(async {
            timeout(self.discovery_timeout, self.send_udp_message(&target_addr, message)).await
                .map_err(|_| crate::error::EmrpError::Network("Network error".to_string()))?
        }).await;
        
        match send_result {
            Ok(duration) => {
                self.update_metrics("send", duration, true).await;
                
                Ok(DeliveryReceipt {
                    message_id: message.id.clone(),
                    transport_used: TransportType::Mdns,
                    delivery_time: duration,
                    target_reached: target_addr.to_string(),
                    confirmation: DeliveryConfirmation::Sent,
                    metadata: {
                        let mut map = HashMap::new();
                        map.insert("target_address".to_string(), target_addr.to_string());
                        map.insert("service_name".to_string(), self.service_name.clone());
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
            self.receive_udp_messages().await
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
        
        let can_reach = self.can_reach(target).await;
        let rtt = start_time.elapsed();
        
        if can_reach {
            Ok(ConnectivityResult {
                connected: true,
                rtt: Some(rtt),
                error: None,
                quality: 0.9, // High quality for local network
                details: {
                    let mut map = HashMap::new();
                    map.insert("service_name".to_string(), self.service_name.clone());
                    map.insert("discovery_timeout_ms".to_string(), 
                              self.discovery_timeout.as_millis().to_string());
                    if let Some(addr) = &target.address {
                        map.insert("target_address".to_string(), addr.clone());
                    }
                    map
                },
            })
        } else {
            Ok(ConnectivityResult {
                connected: false,
                rtt: Some(rtt),
                error: Some("Target not reachable via mDNS".to_string()),
                quality: 0.0,
                details: HashMap::new(),
            })
        }
    }
    
    async fn start(&self) -> Result<()> {
        info!("Starting mDNS transport");
        
        {
            let mut status = self.status.write().await;
            *status = TransportStatus::Starting;
        }
        
        // Bind UDP socket
        let socket = UdpSocket::bind(format!("0.0.0.0:{}", self.local_port)).await
            .map_err(|e| crate::error::EmrpError::Network("Network error".to_string()))?;
        
        let actual_port = socket.local_addr()
            .map_err(|e| crate::error::EmrpError::Network("Network error".to_string()))?.port();
        
        // Make socket non-blocking for try_recv_from
        socket.set_nonblocking(true)
            .map_err(|e| crate::error::EmrpError::Network("Network error".to_string()))?;
        
        // Store socket
        self.socket.replace(Arc::new(socket));
        
        // Start peer discovery
        self.discover_peers().await?;
        
        {
            let mut status = self.status.write().await;
            *status = TransportStatus::Running;
        }
        
        info!("mDNS transport started on port {} (requested: {})", actual_port, self.local_port);
        Ok(())
    }
    
    async fn stop(&self) -> Result<()> {
        info!("Stopping mDNS transport");
        
        {
            let mut status = self.status.write().await;
            *status = TransportStatus::Stopping;
        }
        
        // Close socket
        self.socket.take();
        
        // Clear discovered peers
        {
            let mut peers = self.discovered_peers.lock().await;
            peers.clear();
        }
        
        {
            let mut status = self.status.write().await;
            *status = TransportStatus::Stopped;
        }
        
        info!("mDNS transport stopped");
        Ok(())
    }
    
    async fn status(&self) -> TransportStatus {
        *self.status.read().await
    }
    
    async fn metrics(&self) -> TransportMetrics {
        self.metrics.read().await.clone()
    }
}

