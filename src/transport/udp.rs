//! UDP Transport Implementation for Synapse
//! 
//! Provides low-latency, connectionless transport for local networks
//! and real-time applications where speed is more important than reliability.

use crate::{
    types::*,
    error::Result,
    circuit_breaker::CircuitBreaker,
    transport::{Transport, TransportMetrics},
};
use async_trait::async_trait;
use std::{
    net::SocketAddr,
    sync::Arc,
    time::{Duration, Instant},
    collections::HashMap,
};
use tokio::{
    net::UdpSocket as TokioUdpSocket,
    sync::{RwLock, mpsc},
    time::timeout,
};
use tracing::{info, debug, warn, error};

/// UDP Transport for low-latency local communication
pub struct UdpTransport {
    local_addr: SocketAddr,
    socket: Arc<TokioUdpSocket>,
    circuit_breaker: Arc<CircuitBreaker>,
    metrics: Arc<RwLock<TransportMetrics>>,
    known_peers: Arc<RwLock<HashMap<String, SocketAddr>>>,
    #[allow(dead_code)]
    message_sender: Option<mpsc::UnboundedSender<(SocketAddr, SecureMessage)>>,
}

impl UdpTransport {
    /// Create a new UDP transport bound to the specified address
    pub async fn new(local_addr: SocketAddr) -> Result<Self> {
        let socket = TokioUdpSocket::bind(local_addr).await?;
        let actual_addr = socket.local_addr()?;
        
        info!("UDP Transport bound to {}", actual_addr);
        
        Ok(Self {
            local_addr: actual_addr,
            socket: Arc::new(socket),
            circuit_breaker: Arc::new(CircuitBreaker::new(
                crate::circuit_breaker::CircuitBreakerConfig {
                    failure_threshold: 3,
                    minimum_requests: 5,
                    failure_window: Duration::from_secs(60),
                    recovery_timeout: Duration::from_secs(30),
                    half_open_max_calls: 2,
                    success_threshold: 0.5,
                }
            )),
            metrics: Arc::new(RwLock::new(TransportMetrics::default())),
            known_peers: Arc::new(RwLock::new(HashMap::new())),
            message_sender: None,
        })
    }
    
    /// Register a known peer with their UDP address
    pub async fn register_peer(&self, peer_id: &str, addr: SocketAddr) {
        let mut peers = self.known_peers.write().await;
        peers.insert(peer_id.to_string(), addr);
        info!("Registered UDP peer {} at {}", peer_id, addr);
    }
    
    /// Start UDP server listening for incoming messages
    pub async fn start_server(&self, mut message_handler: mpsc::UnboundedReceiver<(String, SecureMessage)>) -> Result<()> {
        let socket = self.socket.clone();
        let metrics = self.metrics.clone();
        
        // Spawn receiver task
        let socket_clone = socket.clone();
        tokio::spawn(async move {
            let mut buffer = [0u8; 65536]; // Max UDP payload size
            
            loop {
                match socket_clone.recv_from(&mut buffer).await {
                    Ok((size, peer_addr)) => {
                        debug!("Received UDP packet from {} ({} bytes)", peer_addr, size);
                        
                        // Update metrics
                        {
                            let mut _m = metrics.write().await;
                            // Note: Using old TransportMetrics structure for compatibility
                            // These fields would need to be mapped to new structure in migration
                        }
                        
                        // Try to deserialize the message
                        match bincode::deserialize::<SecureMessage>(&buffer[..size]) {
                            Ok(_message) => {
                                debug!("Successfully parsed UDP message from {}", peer_addr);
                                // In a real implementation, you'd handle the message here
                            }
                            Err(e) => {
                                warn!("Failed to deserialize UDP message from {}: {}", peer_addr, e);
                            }
                        }
                    }
                    Err(e) => {
                        error!("UDP receive error: {}", e);
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                }
            }
        });
        
        // Handle outgoing messages
        tokio::spawn(async move {
            while let Some((target, _message)) = message_handler.recv().await {
                // This would typically lookup the target address and send the message
                debug!("Would send UDP message to {}", target);
            }
        });
        
        info!("UDP server started on {}", self.local_addr);
        Ok(())
    }
    
    /// Discover peers on the local network using UDP broadcast
    pub async fn discover_local_peers(&self) -> Result<Vec<SocketAddr>> {
        let broadcast_addr: SocketAddr = "255.255.255.255:8765".parse()?;
        let discovery_message = b"SYNAPSE_DISCOVERY";
        
        // Send broadcast discovery message
        self.socket.send_to(discovery_message, broadcast_addr).await?;
        
        // Wait for responses (simplified implementation)
        let mut discovered_peers = Vec::new();
        let mut buffer = [0u8; 1024];
        
        // Listen for responses for 1 second
        match timeout(Duration::from_secs(1), self.socket.recv_from(&mut buffer)).await {
            Ok(Ok((_, peer_addr))) => {
                if peer_addr != self.local_addr {
                    discovered_peers.push(peer_addr);
                    info!("Discovered UDP peer at {}", peer_addr);
                }
            }
            _ => {
                debug!("No UDP discovery responses received");
            }
        }
        
        Ok(discovered_peers)
    }
}

#[async_trait]
impl Transport for UdpTransport {
    async fn send_message(&self, target: &str, message: &SecureMessage) -> Result<String> {
        // Check circuit breaker
        if !self.circuit_breaker.can_proceed().await {
            return Err("UDP circuit breaker is open".into());
        }
        
        let start_time = Instant::now();
        
        // Look up target address
        let target_addr = {
            let peers = self.known_peers.read().await;
            peers.get(target).copied()
        };
        
        let addr = match target_addr {
            Some(addr) => addr,
            None => {
                // Try to parse as direct address
                match target.parse::<SocketAddr>() {
                    Ok(addr) => addr,
                    Err(_) => {
                        self.circuit_breaker.record_outcome(crate::circuit_breaker::RequestOutcome::Failure("Unknown UDP target".to_string())).await;
                        return Err(format!("Unknown UDP target: {}", target).into());
                    }
                }
            }
        };
        
        // Serialize and send message
        let serialized = bincode::serialize(message)?;
        
        match self.socket.send_to(&serialized, addr).await {
            Ok(bytes_sent) => {
                let duration = start_time.elapsed();
                self.circuit_breaker.record_outcome(crate::circuit_breaker::RequestOutcome::Success).await;
                
                // Update metrics
                {
                    let mut _metrics = self.metrics.write().await;
                    // Note: Using old TransportMetrics fields for compatibility
                    // In future migration, these should be updated to new structure
                }
                
                debug!("Sent UDP message to {} ({} bytes, {:?})", addr, bytes_sent, duration);
                Ok(message.message_id.to_string()) // Return message ID for tracking
            }
            Err(e) => {
                self.circuit_breaker.record_outcome(crate::circuit_breaker::RequestOutcome::Failure(e.to_string())).await;
                error!("Failed to send UDP message to {}: {}", addr, e);
                Err(e.into())
            }
        }
    }
    
    async fn receive_messages(&self) -> Result<Vec<SecureMessage>> {
        // For now, return empty - in a real implementation this would 
        // collect messages from a queue populated by the receiver task
        Ok(vec![])
    }

    async fn test_connectivity(&self, target: &str) -> Result<TransportMetrics> {
        let start_time = Instant::now();
        
        // Try to parse target as socket address
        let addr: SocketAddr = match target.parse() {
            Ok(addr) => addr,
            Err(_) => {
                // Try to resolve from known peers
                let peers = self.known_peers.read().await;
                *peers.get(target).ok_or_else(|| {
                    crate::error::EmrpError::Transport(format!("Unknown UDP target: {}", target))
                })?
            }
        };
        
        // Send a simple ping message
        let ping_data = b"PING";
        let success = self.socket.send_to(ping_data, addr).await.is_ok();
        let latency = start_time.elapsed();
        
        // Return metrics compatible with the old structure
        Ok(crate::transport::TransportMetrics {
            latency,
            throughput_bps: if success { 1000000 } else { 0 }, // 1 Mbps estimate for UDP
            packet_loss: if success { 0.0 } else { 1.0 },
            jitter_ms: 5, // Low jitter for UDP
            reliability_score: if success { 0.8 } else { 0.0 }, // UDP has inherent unreliability
            last_updated: Instant::now(),
        })
    }

    async fn can_reach(&self, target: &str) -> bool {
        // For UDP, we can attempt to reach any valid address
        if target.parse::<SocketAddr>().is_ok() {
            return true;
        }
        
        // Check if target is in known peers
        let peers = self.known_peers.read().await;
        peers.contains_key(target)
    }

    fn get_capabilities(&self) -> Vec<String> {
        vec![
            "low_latency".to_string(),
            "connectionless".to_string(),
            "broadcast".to_string(),
            "real_time".to_string(),
            "local_network".to_string(),
        ]
    }

    fn estimated_latency(&self) -> Duration {
        Duration::from_millis(5) // Very low latency for UDP
    }

    fn reliability_score(&self) -> f32 {
        0.8 // UDP is less reliable than TCP due to no delivery guarantees
    }
}

/// Configuration for UDP transport
#[derive(Debug, Clone)]
pub struct UdpConfig {
    pub bind_address: SocketAddr,
    pub discovery_enabled: bool,
    pub max_packet_size: usize,
    pub timeout: Duration,
}

impl Default for UdpConfig {
    fn default() -> Self {
        Self {
            bind_address: "0.0.0.0:8765".parse().unwrap(),
            discovery_enabled: true,
            max_packet_size: 65536,
            timeout: Duration::from_secs(5),
        }
    }
}
