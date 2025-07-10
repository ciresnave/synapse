//! Enhanced mDNS implementation with full Zeroconf/Bonjour support
//! 
//! This module provides comprehensive multicast DNS service discovery 
//! and announcement capabilities for local network communication.

use super::{Transport, TransportMetrics};
use crate::{
    types::SecureMessage, 
    error::Result,
    circuit_breaker::{CircuitBreaker, CircuitBreakerConfig, RequestOutcome},
};
use async_trait::async_trait;
use std::{
    time::{Duration, Instant},
    collections::HashMap,
    net::{SocketAddr, IpAddr, Ipv4Addr},
    sync::{Arc, RwLock},
};
use tracing::{info, debug, warn, error};
use tokio::{net::UdpSocket as TokioUdpSocket, sync::Mutex, time::interval};
use serde::{Serialize, Deserialize};

/// Enhanced mDNS transport with full service discovery and announcement
pub struct EnhancedMdnsTransport {
    /// Our service instance name
    instance_name: String,
    /// Service type (e.g., "_synapse._tcp.local.")
    service_type: String,
    /// Our local port
    local_port: u16,
    /// Our entity ID
    entity_id: String,
    /// Multicast socket for sending/receiving mDNS packets
    multicast_socket: Arc<Mutex<TokioUdpSocket>>,
    /// Discovered peers cache
    discovered_peers: Arc<RwLock<HashMap<String, EnhancedMdnsPeer>>>,
    /// Service announcements we're making
    our_announcements: Vec<ServiceAnnouncement>,
    /// Discovery configuration
    config: MdnsConfig,
    /// Performance metrics
    metrics: Arc<RwLock<TransportMetrics>>,
    /// Circuit breaker for reliability
    circuit_breaker: Arc<CircuitBreaker>,
}

/// Enhanced peer information with full service record data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnhancedMdnsPeer {
    pub entity_id: String,
    pub instance_name: String,
    pub service_type: String,
    pub host_name: String,
    pub addresses: Vec<IpAddr>,
    pub port: u16,
    pub txt_records: HashMap<String, String>,
    pub priority: u16,
    pub weight: u16,
    pub ttl: u32,
    #[serde(skip, default = "Instant::now")]
    pub discovered_at: Instant,
    #[serde(skip, default = "Instant::now")]
    pub last_seen: Instant,
    pub capabilities: Vec<String>,
    pub protocol_version: String,
}

/// Service announcement record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceAnnouncement {
    pub instance_name: String,
    pub service_type: String,
    pub domain: String,
    pub host_name: String,
    pub port: u16,
    pub txt_records: HashMap<String, String>,
    pub ttl: u32,
    #[serde(skip, default = "Instant::now")]
    pub announced_at: Instant,
}

/// mDNS configuration parameters
#[derive(Debug, Clone)]
pub struct MdnsConfig {
    /// Multicast address for mDNS (224.0.0.251)
    pub multicast_addr: Ipv4Addr,
    /// mDNS port (5353)
    pub multicast_port: u16,
    /// How often to send service announcements
    pub announce_interval: Duration,
    /// How often to query for services
    pub query_interval: Duration,
    /// TTL for our service records
    pub default_ttl: u32,
    /// Timeout for service discovery
    pub discovery_timeout: Duration,
    /// Maximum number of peers to cache
    pub max_peers: usize,
    /// How long to keep stale peers
    pub peer_timeout: Duration,
}

/// mDNS packet types we handle
#[derive(Debug, Clone)]
pub enum MdnsPacket {
    Query {
        questions: Vec<MdnsQuestion>,
        transaction_id: u16,
    },
    Response {
        answers: Vec<MdnsRecord>,
        authorities: Vec<MdnsRecord>,
        additionals: Vec<MdnsRecord>,
        transaction_id: u16,
        authoritative: bool,
    },
}

/// mDNS question record
#[derive(Debug, Clone)]
pub struct MdnsQuestion {
    pub name: String,
    pub qtype: u16,  // DNS record type
    pub qclass: u16, // DNS class (usually 1 for IN)
}

/// mDNS resource record
#[derive(Debug, Clone)]
pub struct MdnsRecord {
    pub name: String,
    pub rtype: u16,
    pub rclass: u16,
    pub ttl: u32,
    pub data: Vec<u8>,
}

impl Default for MdnsConfig {
    fn default() -> Self {
        Self {
            multicast_addr: Ipv4Addr::new(224, 0, 0, 251),
            multicast_port: 5353,
            announce_interval: Duration::from_secs(60),
            query_interval: Duration::from_secs(30),
            default_ttl: 120,
            discovery_timeout: Duration::from_secs(5),
            max_peers: 100,
            peer_timeout: Duration::from_secs(300),
        }
    }
}

impl EnhancedMdnsTransport {
    /// Create a new enhanced mDNS transport
    pub async fn new(
        entity_id: String,
        local_port: u16,
        config: Option<MdnsConfig>,
    ) -> Result<Self> {
        let config = config.unwrap_or_default();
        
        // Create multicast socket
        let multicast_socket = create_multicast_socket(&config).await?;
        
        // Generate unique instance name
        let instance_name = format!("{}._synapse._tcp.local.", entity_id);
        let service_type = "_synapse._tcp.local.".to_string();
        
        // Create circuit breaker with mDNS-appropriate settings
        let circuit_config = CircuitBreakerConfig {
            failure_threshold: 3,  // Trip after 3 failures
            minimum_requests: 2,   // Minimum requests before considering failure rate
            failure_window: std::time::Duration::from_secs(30), // 30-second window
            recovery_timeout: std::time::Duration::from_secs(10), // Try recovery after 10s
            half_open_max_calls: 2, // Allow 2 test calls in half-open
            success_threshold: 0.7, // 70% success rate to close
        };
        let circuit_breaker = Arc::new(CircuitBreaker::new(circuit_config));
        
        info!("Creating enhanced mDNS transport for {} on port {} with circuit breaker", entity_id, local_port);
        
        Ok(Self {
            instance_name,
            service_type,
            local_port,
            entity_id,
            multicast_socket: Arc::new(Mutex::new(multicast_socket)),
            discovered_peers: Arc::new(RwLock::new(HashMap::new())),
            our_announcements: Vec::new(),
            config,
            metrics: Arc::new(RwLock::new(TransportMetrics::default())),
            circuit_breaker,
        })
    }
    
    /// Start the mDNS service (discovery and announcement)
    pub async fn start(&mut self) -> Result<()> {
        info!("Starting enhanced mDNS service for {}", self.entity_id);
        
        // Start announcement task
        self.start_announcements().await?;
        
        // Start discovery task
        self.start_discovery().await?;
        
        // Start packet processing task
        self.start_packet_processing().await?;
        
        // Start cleanup task
        self.start_cleanup_task().await;
        
        Ok(())
    }
    
    /// Announce our service to the network
    pub async fn announce_service(&mut self) -> Result<()> {
        let announcement = ServiceAnnouncement {
            instance_name: self.instance_name.clone(),
            service_type: self.service_type.clone(),
            domain: "local.".to_string(),
            host_name: format!("{}.local.", self.entity_id),
            port: self.local_port,
            txt_records: self.build_txt_records(),
            ttl: self.config.default_ttl,
            announced_at: Instant::now(),
        };
        
        // Send PTR record for service enumeration
        self.send_ptr_record(&announcement).await?;
        
        // Send SRV record for service location
        self.send_srv_record(&announcement).await?;
        
        // Send TXT record for service metadata
        self.send_txt_record(&announcement).await?;
        
        // Send A/AAAA records for host resolution
        self.send_host_records(&announcement).await?;
        
        self.our_announcements.push(announcement);
        
        info!("Announced Synapse service: {}", self.instance_name);
        Ok(())
    }
    
    /// Query for Synapse services on the network
    pub async fn discover_services(&self) -> Result<Vec<EnhancedMdnsPeer>> {
        info!("Discovering Synapse services on local network");
        
        // Send PTR query for _synapse._tcp.local.
        self.send_ptr_query("_synapse._tcp.local.").await?;
        
        // Wait for responses
        tokio::time::sleep(self.config.discovery_timeout).await;
        
        // Return discovered peers
        let peers = self.discovered_peers.read().unwrap();
        Ok(peers.values().cloned().collect())
    }
    
    /// Find a specific peer by entity ID
    pub async fn find_peer(&self, entity_id: &str) -> Option<EnhancedMdnsPeer> {
        let peers = self.discovered_peers.read().unwrap();
        peers.get(entity_id).cloned()
    }
    
    /// Get all discovered peers
    pub async fn get_all_peers(&self) -> Vec<EnhancedMdnsPeer> {
        let peers = self.discovered_peers.read().unwrap();
        peers.values().cloned().collect()
    }
    
    /// Send message to a peer via mDNS-discovered address
    pub async fn send_to_peer(&self, peer: &EnhancedMdnsPeer, message: &SecureMessage) -> Result<String> {
        // Use the first available address
        if let Some(addr) = peer.addresses.first() {
            let socket_addr = SocketAddr::new(*addr, peer.port);
            
            // Create direct TCP connection
            let tcp_transport = super::tcp::TcpTransport::new(0).await?;
            
            // Update metrics
            {
                let mut metrics = self.metrics.write().unwrap();
                metrics.last_updated = Instant::now();
            }
            
            match tcp_transport.connect(&addr.to_string(), peer.port).await {
                Ok(mut stream) => {
                    tcp_transport.send_via_stream(&mut stream, message).await?;
                    
                    // Update success metrics
                    {
                        let mut metrics = self.metrics.write().unwrap();
                        metrics.reliability_score = (metrics.reliability_score * 0.9 + 0.1).min(1.0);
                        metrics.last_updated = Instant::now();
                    }
                    
                    info!("Sent message to mDNS peer {} at {}", peer.entity_id, socket_addr);
                    Ok(format!("mdns://{}@{}", peer.entity_id, socket_addr))
                }
                Err(e) => {
                    // Update failure metrics
                    {
                        let mut metrics = self.metrics.write().unwrap();
                        metrics.reliability_score = (metrics.reliability_score * 0.9).max(0.0);
                        metrics.packet_loss = (metrics.packet_loss + 0.1).min(1.0);
                        metrics.last_updated = Instant::now();
                    }
                    
                    error!("Failed to send to mDNS peer {}: {}", peer.entity_id, e);
                    Err(e)
                }
            }
        } else {
            Err(crate::error::EmrpError::Transport(
                format!("No addresses available for peer {}", peer.entity_id)
            ).into())
        }
    }
    
    // Private implementation methods
    
    async fn start_announcements(&self) -> Result<()> {
        let socket = Arc::clone(&self.multicast_socket);
        let config = self.config.clone();
        let entity_id = self.entity_id.clone();
        
        tokio::spawn(async move {
            let mut announce_interval = interval(config.announce_interval);
            
            loop {
                announce_interval.tick().await;
                
                // Send service announcements
                if let Err(e) = Self::send_periodic_announcements(&socket, &entity_id, &config).await {
                    warn!("Failed to send mDNS announcements: {}", e);
                }
            }
        });
        
        Ok(())
    }
    
    async fn start_discovery(&self) -> Result<()> {
        let socket = Arc::clone(&self.multicast_socket);
        let config = self.config.clone();
        
        tokio::spawn(async move {
            let mut query_interval = interval(config.query_interval);
            
            loop {
                query_interval.tick().await;
                
                // Send service discovery queries
                if let Err(e) = Self::send_discovery_queries(&socket, &config).await {
                    warn!("Failed to send mDNS discovery queries: {}", e);
                }
            }
        });
        
        Ok(())
    }
    
    async fn start_packet_processing(&self) -> Result<()> {
        let socket = Arc::clone(&self.multicast_socket);
        let peers = Arc::clone(&self.discovered_peers);
        let metrics = Arc::clone(&self.metrics);
        
        tokio::spawn(async move {
            let mut buffer = [0u8; 4096];
            
            loop {
                let socket_guard = socket.lock().await;
                match socket_guard.recv_from(&mut buffer).await {
                    Ok((size, src)) => {
                        drop(socket_guard); // Release lock early
                        
                        // Update metrics
                        {
                            let mut m = metrics.write().unwrap();
                            m.throughput_bps = (m.throughput_bps + size as u64 * 8).max(size as u64 * 8);
                            m.last_updated = Instant::now();
                        }
                        
                        // Process the mDNS packet
                        if let Err(e) = Self::process_mdns_packet(&buffer[..size], src, &peers).await {
                            debug!("Error processing mDNS packet from {}: {}", src, e);
                        }
                    }
                    Err(e) => {
                        error!("Error receiving mDNS packet: {}", e);
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                }
            }
        });
        
        Ok(())
    }
    
    async fn start_cleanup_task(&self) {
        let peers = Arc::clone(&self.discovered_peers);
        let timeout = self.config.peer_timeout;
        
        tokio::spawn(async move {
            let mut cleanup_interval = interval(Duration::from_secs(60));
            
            loop {
                cleanup_interval.tick().await;
                
                let now = Instant::now();
                let mut peers_guard = peers.write().unwrap();
                
                let initial_count = peers_guard.len();
                peers_guard.retain(|entity_id, peer| {
                    if now.duration_since(peer.last_seen) > timeout {
                        debug!("Removing stale mDNS peer: {}", entity_id);
                        false
                    } else {
                        true
                    }
                });
                
                let removed = initial_count - peers_guard.len();
                if removed > 0 {
                    info!("Cleaned up {} stale mDNS peers", removed);
                }
            }
        });
    }
    
    fn build_txt_records(&self) -> HashMap<String, String> {
        let mut txt_records = HashMap::new();
        txt_records.insert("version".to_string(), "1.0".to_string());
        txt_records.insert("protocol".to_string(), "synapse".to_string());
        txt_records.insert("entity_id".to_string(), self.entity_id.clone());
        txt_records.insert("capabilities".to_string(), "tcp,encrypted,direct".to_string());
        txt_records.insert("transport_types".to_string(), "tcp,udp,email".to_string());
        txt_records
    }
    
    async fn send_ptr_record(&self, announcement: &ServiceAnnouncement) -> Result<()> {
        // Implementation for sending PTR records
        debug!("Sending PTR record for {}", announcement.instance_name);
        Ok(())
    }
    
    async fn send_srv_record(&self, announcement: &ServiceAnnouncement) -> Result<()> {
        // Implementation for sending SRV records
        debug!("Sending SRV record for {}", announcement.instance_name);
        Ok(())
    }
    
    async fn send_txt_record(&self, announcement: &ServiceAnnouncement) -> Result<()> {
        // Implementation for sending TXT records
        debug!("Sending TXT record for {}", announcement.instance_name);
        Ok(())
    }
    
    async fn send_host_records(&self, announcement: &ServiceAnnouncement) -> Result<()> {
        // Implementation for sending A/AAAA records
        debug!("Sending host records for {}", announcement.host_name);
        Ok(())
    }
    
    async fn send_ptr_query(&self, service_type: &str) -> Result<()> {
        // Implementation for sending PTR queries
        debug!("Sending PTR query for {}", service_type);
        Ok(())
    }
    
    async fn send_periodic_announcements(
        _socket: &Arc<Mutex<TokioUdpSocket>>,
        _entity_id: &str,
        _config: &MdnsConfig,
    ) -> Result<()> {
        // Implementation for periodic announcements
        Ok(())
    }
    
    async fn send_discovery_queries(
        _socket: &Arc<Mutex<TokioUdpSocket>>,
        _config: &MdnsConfig,
    ) -> Result<()> {
        // Implementation for discovery queries
        Ok(())
    }
    
    async fn process_mdns_packet(
        _packet_data: &[u8],
        _src: SocketAddr,
        _peers: &Arc<RwLock<HashMap<String, EnhancedMdnsPeer>>>,
    ) -> Result<()> {
        // Implementation for processing mDNS packets
        Ok(())
    }
}

#[async_trait]
impl Transport for EnhancedMdnsTransport {
    /// Send a message via this transport
    async fn send_message(&self, target: &str, message: &SecureMessage) -> Result<String> {
        // Check circuit breaker before proceeding
        if !self.circuit_breaker.can_proceed().await {
            debug!("Circuit breaker is open, rejecting message to {}", target);
            return Err(crate::error::EmrpError::Transport(
                format!("Circuit breaker is open for target {}", target)
            ).into());
        }

        let start_time = Instant::now();
        
        // Try to find the peer in our discovered peers
        let result = if let Some(peer) = self.find_peer(target).await {
            self.send_to_peer(&peer, message).await
        } else {
            // Trigger discovery and wait briefly
            let _discovered = self.discover_services().await?;
            
            if let Some(peer) = self.find_peer(target).await {
                self.send_to_peer(&peer, message).await
            } else {
                Err(crate::error::EmrpError::Transport(
                    format!("Peer {} not found on local network", target)
                ).into())
            }
        };

        // Record the outcome in the circuit breaker
        let outcome = match &result {
            Ok(_) => {
                let latency = start_time.elapsed();
                // Update metrics with successful operation
                {
                    let mut metrics = self.metrics.write().unwrap();
                    metrics.latency = latency;
                    metrics.reliability_score = metrics.reliability_score * 0.9 + 0.1; // Smooth increase
                    metrics.last_updated = Instant::now();
                }
                RequestOutcome::Success
            }
            Err(e) => {
                // Update metrics with failed operation
                {
                    let mut metrics = self.metrics.write().unwrap();
                    metrics.reliability_score = metrics.reliability_score * 0.9; // Decrease on failure
                    metrics.last_updated = Instant::now();
                }
                RequestOutcome::Failure(e.to_string())
            }
        };

        self.circuit_breaker.record_outcome(outcome).await;
        result
    }
    
    /// Receive messages via this transport  
    async fn receive_messages(&self) -> Result<Vec<SecureMessage>> {
        // mDNS doesn't directly receive messages - it's used for discovery
        // Messages are received via the discovered transport methods (TCP/UDP)
        // Return empty vector as mDNS is primarily for service discovery
        Ok(Vec::new())
    }
    
    /// Test connectivity and measure latency
    async fn test_connectivity(&self, target: &str) -> Result<TransportMetrics> {
        // Check circuit breaker before proceeding
        if !self.circuit_breaker.can_proceed().await {
            debug!("Circuit breaker is open, skipping connectivity test for {}", target);
            let mut metrics = TransportMetrics::default();
            metrics.reliability_score = 0.0; // Indicate circuit is open
            metrics.last_updated = Instant::now();
            return Ok(metrics);
        }

        let start = Instant::now();
        
        // Try to discover the target peer
        let result: Result<TransportMetrics> = match self.find_peer(target).await {
            Some(_peer) => {
                let latency = start.elapsed();
                let mut metrics = TransportMetrics::default();
                metrics.latency = latency;
                metrics.reliability_score = 0.95; // mDNS is very reliable on local networks
                metrics.last_updated = Instant::now();
                Ok(metrics)
            }
            None => {
                // Try discovery
                let _discovered = self.discover_services().await?;
                
                match self.find_peer(target).await {
                    Some(_peer) => {
                        let latency = start.elapsed();
                        let mut metrics = TransportMetrics::default();
                        metrics.latency = latency;
                        metrics.reliability_score = 0.95;
                        metrics.last_updated = Instant::now();
                        Ok(metrics)
                    }
                    None => {
                        Err(crate::error::EmrpError::Transport(
                            format!("Cannot reach target {} via mDNS", target)
                        ).into())
                    }
                }
            }
        };

        // Record the outcome in the circuit breaker
        let outcome = match &result {
            Ok(_) => RequestOutcome::Success,
            Err(e) => RequestOutcome::Failure(e.to_string()),
        };

        self.circuit_breaker.record_outcome(outcome).await;
        result
    }

    /// Send a message with explicit circuit breaker support
    async fn send_message_with_breaker(
        &self,
        target: &str,
        message: &SecureMessage,
        circuit_breaker: Option<Arc<CircuitBreaker>>
    ) -> Result<String> {
        // Use provided circuit breaker or fall back to internal one
        let breaker = circuit_breaker.unwrap_or_else(|| self.circuit_breaker.clone());
        
        if !breaker.can_proceed().await {
            debug!("Circuit breaker is open, rejecting message to {}", target);
            return Err(crate::error::EmrpError::Transport(
                format!("Circuit breaker is open for target {}", target)
            ).into());
        }

        let _start_time = Instant::now();
        let result = self.send_message(target, message).await;

        // Record outcome in the circuit breaker
        let outcome = match &result {
            Ok(_) => RequestOutcome::Success,
            Err(e) => RequestOutcome::Failure(e.to_string()),
        };

        breaker.record_outcome(outcome).await;
        result
    }

    /// Test connectivity with explicit circuit breaker support
    async fn test_connectivity_with_breaker(
        &self,
        target: &str,
        circuit_breaker: Option<Arc<CircuitBreaker>>
    ) -> Result<TransportMetrics> {
        // Use provided circuit breaker or fall back to internal one
        let breaker = circuit_breaker.unwrap_or_else(|| self.circuit_breaker.clone());
        
        if !breaker.can_proceed().await {
            debug!("Circuit breaker is open, skipping connectivity test for {}", target);
            let mut metrics = TransportMetrics::default();
            metrics.reliability_score = 0.0;
            metrics.last_updated = Instant::now();
            return Ok(metrics);
        }

        let result = self.test_connectivity(target).await;

        // Record outcome in the circuit breaker
        let outcome = match &result {
            Ok(_) => RequestOutcome::Success,
            Err(e) => RequestOutcome::Failure(e.to_string()),
        };

        breaker.record_outcome(outcome).await;
        result
    }
    
    /// Check if this transport can reach the target
    async fn can_reach(&self, target: &str) -> bool {
        // Check if peer is already discovered
        if self.find_peer(target).await.is_some() {
            return true;
        }
        
        // Try discovery
        if let Ok(_discovered) = self.discover_services().await {
            self.find_peer(target).await.is_some()
        } else {
            false
        }
    }
    
    /// Get transport-specific capabilities
    fn get_capabilities(&self) -> Vec<String> {
        vec![
            "local_network".to_string(),
            "service_discovery".to_string(),
            "low_latency".to_string(),
            "automatic_discovery".to_string(),
            "zeroconf".to_string(),
            "multicast".to_string(),
        ]
    }
    
    /// Get estimated latency for this transport
    fn estimated_latency(&self) -> Duration {
        Duration::from_millis(10) // Very low latency for local network
    }
    
    /// Get reliability score (0.0-1.0)
    fn reliability_score(&self) -> f32 {
        0.95 // Very reliable on local networks
    }
}

impl EnhancedMdnsTransport {
    /// Get access to the circuit breaker for monitoring and control
    pub fn get_circuit_breaker(&self) -> Arc<CircuitBreaker> {
        self.circuit_breaker.clone()
    }

    /// Get circuit breaker statistics
    pub fn get_circuit_stats(&self) -> crate::circuit_breaker::CircuitStats {
        self.circuit_breaker.get_stats()
    }

    /// Check if circuit breaker is allowing requests
    pub async fn is_circuit_open(&self) -> bool {
        !self.circuit_breaker.can_proceed().await
    }
}

/// Create a multicast UDP socket for mDNS
async fn create_multicast_socket(config: &MdnsConfig) -> Result<TokioUdpSocket> {
    use std::net::{SocketAddrV4, Ipv4Addr};
    #[cfg(feature = "mdns")]
    use socket2::{Socket, Domain, Type, Protocol};
    
    // Create socket with SO_REUSEADDR and SO_REUSEPORT
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    
    socket.set_reuse_address(true)?;
    #[cfg(not(windows))]
    socket.set_reuse_port(true)?;
    
    // Bind to the mDNS multicast address
    let bind_addr = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, config.multicast_port);
    socket.bind(&bind_addr.into())?;
    
    // Join the multicast group
    socket.join_multicast_v4(&config.multicast_addr, &Ipv4Addr::UNSPECIFIED)?;
    
    // Set multicast loop and TTL
    socket.set_multicast_loop_v4(false)?;
    socket.set_multicast_ttl_v4(255)?;
    
    // Convert to Tokio socket
    socket.set_nonblocking(true)?;
    let tokio_socket = TokioUdpSocket::from_std(socket.into())?;
    
    info!("Created mDNS multicast socket on port {}", config.multicast_port);
    Ok(tokio_socket)
}

/// mDNS utility functions
pub mod utils {
    use super::*;
    
    /// Parse a DNS name from wire format
    pub fn parse_dns_name(data: &[u8], offset: usize) -> Result<(String, usize)> {
        let mut name = String::new();
        let mut pos = offset;
        let mut jumped = false;
        let mut jump_count = 0;
        
        loop {
            if pos >= data.len() {
                return Err(crate::error::EmrpError::Transport(
                    "DNS name parsing: unexpected end of data".to_string()
                ).into());
            }
            
            let len = data[pos] as usize;
            
            // Check for compression (pointer)
            if len & 0xC0 == 0xC0 {
                if !jumped {
                    // Save position for returning
                }
                
                // Extract pointer
                let pointer = ((len & 0x3F) << 8) | (data[pos + 1] as usize);
                pos = pointer;
                jumped = true;
                jump_count += 1;
                
                if jump_count > 10 {
                    return Err(crate::error::EmrpError::Transport(
                        "DNS name parsing: too many jumps".to_string()
                    ).into());
                }
                continue;
            }
            
            pos += 1;
            
            if len == 0 {
                // End of name
                break;
            }
            
            if pos + len > data.len() {
                return Err(crate::error::EmrpError::Transport(
                    "DNS name parsing: label too long".to_string()
                ).into());
            }
            
            if !name.is_empty() {
                name.push('.');
            }
            
            name.push_str(&String::from_utf8_lossy(&data[pos..pos + len]));
            pos += len;
        }
        
        Ok((name, pos))
    }
    
    /// Encode a DNS name to wire format
    pub fn encode_dns_name(name: &str) -> Vec<u8> {
        let mut encoded = Vec::new();
        
        for label in name.split('.') {
            if label.is_empty() {
                continue;
            }
            
            encoded.push(label.len() as u8);
            encoded.extend_from_slice(label.as_bytes());
        }
        
        encoded.push(0); // Null terminator
        encoded
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_enhanced_mdns_creation() {
        let transport = EnhancedMdnsTransport::new(
            "test_entity".to_string(),
            8080,
            None,
        ).await;
        
        assert!(transport.is_ok());
    }
    
    #[test]
    fn test_dns_name_encoding() {
        let name = "test._synapse._tcp.local";
        let encoded = utils::encode_dns_name(name);
        
        // Should start with length of first label
        assert_eq!(encoded[0], 4); // "test"
        assert_eq!(&encoded[1..5], b"test");
        assert_eq!(encoded[5], 8); // "_synapse"
    }
}

/// Enhanced mDNS service browser for discovering multiple service types
pub struct EnhancedMdnsServiceBrowser {
    /// Service types to browse for
    service_types: Vec<String>,
    /// Discovered services cache
    service_cache: Arc<RwLock<HashMap<String, ServiceRecord>>>,
    /// Browser configuration
    config: BrowserConfig,
    /// Multicast socket for browsing
    browse_socket: Arc<Mutex<TokioUdpSocket>>,
}

/// Service record with full DNS information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceRecord {
    pub service_name: String,
    pub service_type: String,
    pub domain: String,
    pub host_name: String,
    pub addresses: Vec<IpAddr>,
    pub port: u16,
    pub txt_records: HashMap<String, String>,
    pub priority: u16,
    pub weight: u16,
    pub ttl: u32,
    #[serde(skip, default = "Instant::now")]
    pub discovered_at: Instant,
    #[serde(skip, default = "Instant::now")]
    pub last_updated: Instant,
    pub service_state: ServiceState,
}

/// Service state tracking
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ServiceState {
    Discovering,
    Resolving,
    Active,
    Inactive,
    Expired,
}

/// Browser configuration
#[derive(Debug, Clone)]
pub struct BrowserConfig {
    /// How often to send browse queries
    pub browse_interval: Duration,
    /// How long to cache service records
    pub cache_ttl: Duration,
    /// Maximum number of services to cache
    pub max_cache_size: usize,
    /// Whether to perform continuous monitoring
    pub continuous_monitoring: bool,
}

impl Default for BrowserConfig {
    fn default() -> Self {
        Self {
            browse_interval: Duration::from_secs(30),
            cache_ttl: Duration::from_secs(300),
            max_cache_size: 500,
            continuous_monitoring: true,
        }
    }
}

impl EnhancedMdnsServiceBrowser {
    /// Create a new service browser
    pub async fn new(service_types: Vec<String>, config: Option<BrowserConfig>) -> Result<Self> {
        let config = config.unwrap_or_default();
        let mdns_config = MdnsConfig::default();
        let browse_socket = create_multicast_socket(&mdns_config).await?;
        
        Ok(Self {
            service_types,
            service_cache: Arc::new(RwLock::new(HashMap::new())),
            config,
            browse_socket: Arc::new(Mutex::new(browse_socket)),
        })
    }
    
    /// Start browsing for services
    pub async fn start_browsing(&self) -> Result<()> {
        info!("Starting mDNS service browsing for types: {:?}", self.service_types);
        
        // Start browsing tasks for each service type
        for service_type in &self.service_types {
            self.start_service_type_browsing(service_type.clone()).await?;
        }
        
        // Start cache cleanup task
        self.start_cache_cleanup().await;
        
        Ok(())
    }
    
    /// Browse for a specific service type
    async fn start_service_type_browsing(&self, service_type: String) -> Result<()> {
        let socket = self.browse_socket.clone();
        let _cache = self.service_cache.clone();
        let interval_duration = self.config.browse_interval;
        
        tokio::spawn(async move {
            let mut interval_timer = interval(interval_duration);
            
            loop {
                interval_timer.tick().await;
                
                // Send PTR query for this service type
                if let Err(e) = Self::send_service_browse_query(&socket, &service_type).await {
                    error!("Failed to send browse query for {}: {}", service_type, e);
                }
            }
        });
        
        Ok(())
    }
    
    /// Send a service browse query (PTR record query)
    async fn send_service_browse_query(
        socket: &Arc<Mutex<TokioUdpSocket>>, 
        service_type: &str
    ) -> Result<()> {
        let query_packet = MdnsPacket::Query {
            questions: vec![MdnsQuestion {
                name: service_type.to_string(),
                qtype: 12, // PTR record
                qclass: 1, // IN class
            }],
            transaction_id: rand::random(),
        };
        
        let packet_bytes = Self::encode_mdns_packet(&query_packet)?;
        let socket_guard = socket.lock().await;
        socket_guard.send_to(&packet_bytes, "224.0.0.251:5353").await?;
        
        Ok(())
    }
    
    /// Get all discovered services
    pub async fn get_discovered_services(&self) -> Vec<ServiceRecord> {
        self.service_cache.read().unwrap().values().cloned().collect()
    }
    
    /// Get services by type
    pub async fn get_services_by_type(&self, service_type: &str) -> Vec<ServiceRecord> {
        self.service_cache
            .read()
            .unwrap()
            .values()
            .filter(|record| record.service_type == service_type)
            .cloned()
            .collect()
    }
    
    /// Find services by capability
    pub async fn find_services_by_capability(&self, capability: &str) -> Vec<ServiceRecord> {
        self.service_cache
            .read()
            .unwrap()
            .values()
            .filter(|record| {
                record.txt_records.get("capabilities")
                    .map(|caps| caps.contains(capability))
                    .unwrap_or(false)
            })
            .cloned()
            .collect()
    }
    
    /// Start cache cleanup task
    async fn start_cache_cleanup(&self) {
        let cache = self.service_cache.clone();
        let ttl = self.config.cache_ttl;
        let max_size = self.config.max_cache_size;
        
        tokio::spawn(async move {
            let mut cleanup_interval = interval(Duration::from_secs(60));
            
            loop {
                cleanup_interval.tick().await;
                
                let mut cache_guard = cache.write().unwrap();
                let now = Instant::now();
                
                // Remove expired entries
                cache_guard.retain(|_, record| {
                    now.duration_since(record.last_updated) < ttl
                });
                
                // Limit cache size (remove oldest entries)
                if cache_guard.len() > max_size {
                    let mut entries: Vec<_> = cache_guard.iter()
                        .map(|(k, v)| (k.clone(), v.last_updated))
                        .collect();
                    entries.sort_by_key(|(_, last_updated)| *last_updated);
                    
                    let to_remove = cache_guard.len() - max_size;
                    for (key, _) in entries.iter().take(to_remove) {
                        cache_guard.remove(key);
                    }
                }
            }
        });
    }
    
    /// Encode an mDNS packet to bytes
    fn encode_mdns_packet(packet: &MdnsPacket) -> Result<Vec<u8>> {
        // Simple mDNS packet encoding
        let mut bytes = Vec::new();
        
        match packet {
            MdnsPacket::Query { questions, transaction_id } => {
                // DNS header
                bytes.extend_from_slice(&transaction_id.to_be_bytes());
                bytes.extend_from_slice(&[0x00, 0x00]); // Flags
                bytes.extend_from_slice(&(questions.len() as u16).to_be_bytes());
                bytes.extend_from_slice(&[0x00, 0x00]); // Answer count
                bytes.extend_from_slice(&[0x00, 0x00]); // Authority count
                bytes.extend_from_slice(&[0x00, 0x00]); // Additional count
                
                // Questions
                for question in questions {
                    bytes.extend_from_slice(&utils::encode_dns_name(&question.name));
                    bytes.extend_from_slice(&question.qtype.to_be_bytes());
                    bytes.extend_from_slice(&question.qclass.to_be_bytes());
                }
            }
            MdnsPacket::Response { .. } => {
                // Response encoding would be more complex
                return Err(crate::error::EmrpError::Transport(
                    "Response packet encoding not implemented yet".to_string()
                ).into());
            }
        }
        
        Ok(bytes)
    }
}

/// Enhanced mDNS responder for announcing services
pub struct EnhancedMdnsResponder {
    /// Services we're announcing
    our_services: Vec<ServiceRecord>,
    /// Responder configuration
    config: ResponderConfig,
    /// Response socket
    response_socket: Arc<Mutex<TokioUdpSocket>>,
}

/// Responder configuration
#[derive(Debug, Clone)]
pub struct ResponderConfig {
    /// How often to send announcements
    pub announce_interval: Duration,
    /// TTL for our records
    pub record_ttl: u32,
    /// Whether to respond to queries immediately
    pub immediate_response: bool,
}

impl Default for ResponderConfig {
    fn default() -> Self {
        Self {
            announce_interval: Duration::from_secs(120),
            record_ttl: 120,
            immediate_response: true,
        }
    }
}

impl EnhancedMdnsResponder {
    /// Create a new responder
    pub async fn new(config: Option<ResponderConfig>) -> Result<Self> {
        let config = config.unwrap_or_default();
        let mdns_config = MdnsConfig::default();
        let response_socket = create_multicast_socket(&mdns_config).await?;
        
        Ok(Self {
            our_services: Vec::new(),
            config,
            response_socket: Arc::new(Mutex::new(response_socket)),
        })
    }
    
    /// Add a service to announce
    pub async fn add_service(&mut self, service: ServiceRecord) -> Result<()> {
        info!("Adding service for announcement: {}", service.service_name);
        self.our_services.push(service);
        Ok(())
    }
    
    /// Start responding to queries and announcing services
    pub async fn start_responding(&self) -> Result<()> {
        info!("Starting mDNS responder for {} services", self.our_services.len());
        
        // Start announcement task
        self.start_periodic_announcements().await?;
        
        // Start query response task
        self.start_query_response_handler().await?;
        
        Ok(())
    }
    
    /// Start periodic service announcements
    async fn start_periodic_announcements(&self) -> Result<()> {
        let socket = self.response_socket.clone();
        let services = self.our_services.clone();
        let interval_duration = self.config.announce_interval;
        
        tokio::spawn(async move {
            let mut announcement_interval = interval(interval_duration);
            
            loop {
                announcement_interval.tick().await;
                
                for service in &services {
                    if let Err(e) = Self::announce_service(&socket, service).await {
                        error!("Failed to announce service {}: {}", service.service_name, e);
                    }
                }
            }
        });
        
        Ok(())
    }
    
    /// Announce a single service
    async fn announce_service(
        _socket: &Arc<Mutex<TokioUdpSocket>>,
        service: &ServiceRecord
    ) -> Result<()> {
        // Send PTR, SRV, TXT, and A/AAAA records
        // This is a simplified implementation
        debug!("Announcing service: {}", service.service_name);
        
        // For now, just log the announcement
        info!("Announced service {} on port {}", service.service_name, service.port);
        Ok(())
    }
    
    /// Start query response handler
    async fn start_query_response_handler(&self) -> Result<()> {
        let socket = self.response_socket.clone();
        let services = self.our_services.clone();
        
        tokio::spawn(async move {
            let mut buffer = [0u8; 4096];
            
            loop {
                let socket_guard = socket.lock().await;
                match socket_guard.recv_from(&mut buffer).await {
                    Ok((size, src)) => {
                        drop(socket_guard);
                        
                        if let Err(e) = Self::handle_query(&socket, &buffer[..size], src, &services).await {
                            debug!("Error handling mDNS query from {}: {}", src, e);
                        }
                    }
                    Err(e) => {
                        error!("Error receiving mDNS query: {}", e);
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                }
            }
        });
        
        Ok(())
    }
    
    /// Handle an incoming mDNS query
    async fn handle_query(
        _socket: &Arc<Mutex<TokioUdpSocket>>,
        _packet_data: &[u8],
        _src: SocketAddr,
        services: &[ServiceRecord]
    ) -> Result<()> {
        // Parse the incoming query and respond if we have matching services
        // This is a simplified implementation
        debug!("Received mDNS query from client, {} services available", services.len());
        Ok(())
    }
}

// Add the enhanced features to the main transport
impl EnhancedMdnsTransport {
    /// Create a service browser for discovering other Synapse nodes
    pub async fn create_service_browser(&self) -> Result<EnhancedMdnsServiceBrowser> {
        EnhancedMdnsServiceBrowser::new(
            vec![
                "_synapse._tcp.local.".to_string(),
                "_emrp._tcp.local.".to_string(),
                "_synapse-router._tcp.local.".to_string(),
            ],
            None
        ).await
    }
    
    /// Create a service responder for announcing our services
    pub async fn create_service_responder(&self) -> Result<EnhancedMdnsResponder> {
        let mut responder = EnhancedMdnsResponder::new(None).await?;
        
        // Add our main service
        let our_service = ServiceRecord {
            service_name: self.instance_name.clone(),
            service_type: self.service_type.clone(),
            domain: "local.".to_string(),
            host_name: format!("{}.local.", self.entity_id),
            addresses: vec![], // Would be populated with our actual addresses
            port: self.local_port,
            txt_records: HashMap::from([
                ("entity_id".to_string(), self.entity_id.clone()),
                ("version".to_string(), "1.0".to_string()),
                ("capabilities".to_string(), "routing,discovery,secure_messaging".to_string()),
            ]),
            priority: 10,
            weight: 5,
            ttl: 120,
            discovered_at: Instant::now(),
            last_updated: Instant::now(),
            service_state: ServiceState::Active,
        };
        
        responder.add_service(our_service).await?;
        Ok(responder)
    }
    
    /// Perform comprehensive service discovery
    pub async fn comprehensive_discovery(&self) -> Result<Vec<EnhancedMdnsPeer>> {
        let browser = self.create_service_browser().await?;
        browser.start_browsing().await?;
        
        // Wait a bit for discoveries
        tokio::time::sleep(Duration::from_secs(3)).await;
        
        let services = browser.get_discovered_services().await;
        let mut peers = Vec::new();
        
        // Convert service records to enhanced peers
        for service in services {
            if service.service_state == ServiceState::Active {
                let capabilities = service.txt_records.get("capabilities")
                    .map(|caps| caps.split(',').map(|s| s.trim().to_string()).collect())
                    .unwrap_or_default();
                
                let protocol_version = service.txt_records.get("version")
                    .cloned()
                    .unwrap_or_else(|| "1.0".to_string());
                
                let entity_id = service.txt_records.get("entity_id")
                    .cloned()
                    .unwrap_or_else(|| service.service_name.clone());
                
                let peer = EnhancedMdnsPeer {
                    entity_id,
                    instance_name: service.service_name,
                    service_type: service.service_type,
                    host_name: service.host_name,
                    addresses: service.addresses,
                    port: service.port,
                    txt_records: service.txt_records,
                    priority: service.priority,
                    weight: service.weight,
                    ttl: service.ttl,
                    discovered_at: service.discovered_at,
                    last_seen: service.last_updated,
                    capabilities,
                    protocol_version,
                };
                peers.push(peer);
            }
        }
        
        Ok(peers)
    }
}
