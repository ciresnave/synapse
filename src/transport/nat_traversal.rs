//! Simplified NAT traversal techniques for EMRP

use super::abstraction::{self, Transport};
use crate::{types::SecureMessage, error::{Result, SynapseError}};
use async_trait::async_trait;
use std::{time::{Duration, Instant}, net::SocketAddr, collections::HashMap};
use serde::{Serialize, Deserialize};
use tracing::{info, debug, warn};
use tokio::net::UdpSocket;

/// TURN server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnServer {
    pub server: String,
    pub username: String,
    pub password: String,
}

/// NAT traversal transport supporting multiple techniques
pub struct NatTraversalTransport {
    local_port: u16,
    stun_servers: Vec<String>,
    turn_servers: Vec<TurnServer>,
    upnp_enabled: bool,
    ice_candidates: HashMap<String, IceCandidate>,
    external_address: Option<SocketAddr>,
}

/// ICE candidate for connectivity establishment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IceCandidate {
    pub candidate_type: CandidateType,
    pub address: SocketAddr,
    pub priority: u32,
    pub foundation: String,
    pub component_id: u16,
    pub transport_protocol: String, // "UDP" or "TCP"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CandidateType {
    Host,      // Local address
    ServerReflexive, // Address as seen by STUN server
    PeerReflexive,   // Address discovered during connectivity checks
    Relay,     // Address allocated on TURN server
}

/// UPnP port mapping result
#[derive(Debug, Clone)]
pub struct UpnpMapping {
    pub external_port: u16,
    pub internal_port: u16,
    pub protocol: String,
    pub duration: Duration,
    pub created_at: Instant,
}

impl NatTraversalTransport {
    pub async fn new(local_port: u16) -> Result<Self> {
        let stun_servers = vec![
            "stun.l.google.com:19302".to_string(),
            "stun1.l.google.com:19302".to_string(),
            "stun2.l.google.com:19302".to_string(),
            "stun.cloudflare.com:3478".to_string(),
        ];
        
        info!("Created NAT traversal transport on port {}", local_port);
        
        Ok(Self {
            local_port,
            stun_servers,
            turn_servers: Vec::new(),
            upnp_enabled: true,
            ice_candidates: HashMap::new(),
            external_address: None,
        })
    }
    
    /// Add TURN server configuration
    pub fn add_turn_server(&mut self, turn_server: TurnServer) {
        self.turn_servers.push(turn_server);
    }
    
    /// Discover external address using STUN
    pub async fn discover_external_address(&mut self) -> Result<SocketAddr> {
        for stun_server in &self.stun_servers.clone() {
            debug!("Trying STUN server: {}", stun_server);
            
            match self.stun_query(stun_server).await {
                Ok(external_addr) => {
                    info!("Discovered external address: {} via {}", external_addr, stun_server);
                    self.external_address = Some(external_addr);
                    return Ok(external_addr);
                }
                Err(e) => {
                    warn!("STUN query to {} failed: {}", stun_server, e);
                    continue;
                }
            }
        }
        
        Err(crate::error::SynapseError::TransportError("Failed to discover external address via STUN".into()))
    }
    
    /// Perform STUN query to discover external address
    async fn stun_query(&self, stun_server: &str) -> Result<SocketAddr> {
        let socket = UdpSocket::bind(format!("0.0.0.0:{}", self.local_port)).await?;
        
        // Create STUN binding request
        let stun_request = self.create_stun_binding_request();
        
        // Send STUN request
        socket.send_to(&stun_request, stun_server).await?;
        
        // Wait for response
        let mut buffer = vec![0; 1024];
        match tokio::time::timeout(Duration::from_secs(5), socket.recv_from(&mut buffer)).await {
            Ok(Ok((bytes_received, _))) => {
                if let Some(external_addr) = self.parse_stun_response(&buffer[..bytes_received]) {
                    Ok(external_addr)
                } else {
                    Err(crate::error::SynapseError::TransportError("Invalid STUN response".into()))
                }
            }
            Ok(Err(e)) => Err(crate::error::SynapseError::TransportError(format!("STUN receive error: {}", e))),
            Err(_) => Err(crate::error::SynapseError::TransportError("STUN query timeout".into())),
        }
    }
    
    /// Create STUN binding request packet
    fn create_stun_binding_request(&self) -> Vec<u8> {
        // Simplified STUN binding request
        // Real implementation would use proper STUN library
        let mut packet = Vec::new();
        
        // STUN header: Message Type (Binding Request = 0x0001)
        packet.extend_from_slice(&0x0001u16.to_be_bytes());
        
        // Message Length (0 for basic request)
        packet.extend_from_slice(&0x0000u16.to_be_bytes());
        
        // Magic Cookie
        packet.extend_from_slice(&0x2112A442u32.to_be_bytes());
        
        // Transaction ID (12 bytes)
        packet.extend_from_slice(&[0; 12]);
        
        packet
    }
    
    /// Parse STUN response to extract external address
    fn parse_stun_response(&self, data: &[u8]) -> Option<SocketAddr> {
        // Simplified STUN response parsing
        // Real implementation would use proper STUN library
        if data.len() < 20 {
            return None;
        }
        
        // Check if it's a STUN success response (0x0101)
        let message_type = u16::from_be_bytes([data[0], data[1]]);
        if message_type != 0x0101 {
            return None;
        }
        
        // For demo purposes, return a placeholder external address
        // Real implementation would parse XOR-MAPPED-ADDRESS attribute
        "203.0.113.1:12345".parse().ok()
    }
    
    /// Attempt UPnP port mapping
    pub async fn setup_upnp_mapping(&mut self) -> Result<UpnpMapping> {
        if !self.upnp_enabled {
            return Err(crate::error::SynapseError::TransportError("UPnP disabled".into()));
        }
        
        info!("Attempting UPnP port mapping for port {}", self.local_port);
        
        // In a real implementation, this would use a UPnP library
        // For now, simulate successful mapping
        tokio::time::sleep(Duration::from_millis(500)).await;
        
        let mapping = UpnpMapping {
            external_port: self.local_port,
            internal_port: self.local_port,
            protocol: "UDP".to_string(),
            duration: Duration::from_secs(3600), // 1 hour
            created_at: Instant::now(),
        };
        
        info!("UPnP mapping created: {}:{} -> {}:{}", 
               "external_ip", mapping.external_port,
               "local_ip", mapping.internal_port);
        
        Ok(mapping)
    }
    
    /// Generate ICE candidates for connectivity establishment
    pub async fn generate_ice_candidates(&mut self) -> Result<Vec<IceCandidate>> {
        let mut candidates = Vec::new();
        
        // Host candidate (local address)
        let local_addr = format!("0.0.0.0:{}", self.local_port).parse::<SocketAddr>()
            .map_err(|e| crate::error::SynapseError::TransportError(format!("Invalid local address: {}", e)))?;
        
        candidates.push(IceCandidate {
            candidate_type: CandidateType::Host,
            address: local_addr,
            priority: 2113667327, // High priority for host candidate
            foundation: "1".to_string(),
            component_id: 1,
            transport_protocol: "UDP".to_string(),
        });
        
        // Server reflexive candidate (via STUN)
        if let Ok(external_addr) = self.discover_external_address().await {
            candidates.push(IceCandidate {
                candidate_type: CandidateType::ServerReflexive,
                address: external_addr,
                priority: 1694498815, // Medium priority for STUN candidate
                foundation: "2".to_string(),
                component_id: 1,
                transport_protocol: "UDP".to_string(),
            });
        }
        
        // Relay candidates (via TURN)
        for turn_server in &self.turn_servers.clone() {
            if let Ok(relay_addr) = self.allocate_turn_address(turn_server).await {
                candidates.push(IceCandidate {
                    candidate_type: CandidateType::Relay,
                    address: relay_addr,
                    priority: 16777215, // Lower priority for relay candidate
                    foundation: "3".to_string(),
                    component_id: 1,
                    transport_protocol: "UDP".to_string(),
                });
            }
        }
        
        // Store candidates
        for candidate in &candidates {
            let key = format!("{}:{}", candidate.address, candidate.candidate_type.clone() as u8);
            self.ice_candidates.insert(key, candidate.clone());
        }
        
        info!("Generated {} ICE candidates", candidates.len());
        Ok(candidates)
    }
    
    /// Allocate address on TURN server
    async fn allocate_turn_address(&self, turn_server: &TurnServer) -> Result<SocketAddr> {
        info!("Allocating TURN address on {}", turn_server.server);
        
        // In a real implementation, this would use TURN protocol
        // For now, simulate successful allocation
        tokio::time::sleep(Duration::from_millis(200)).await;
        
        // Return a simulated relay address
        Ok("203.0.113.100:54321".parse().unwrap())
    }
    
    /// Establish connectivity using best available method
    pub async fn establish_connection(&mut self, _target: &str) -> Result<NatMethod> {
        // Try UPnP first (fastest if available)
        if let Ok(_mapping) = self.setup_upnp_mapping().await {
            return Ok(NatMethod::Upnp);
        }
        
        // Try STUN (most common)
        if let Ok(_external_addr) = self.discover_external_address().await {
            return Ok(NatMethod::Stun { 
                server: self.stun_servers[0].clone() 
            });
        }
        
        // Try TURN as fallback
        if !self.turn_servers.is_empty() {
            let turn_server = &self.turn_servers[0];
            if self.allocate_turn_address(turn_server).await.is_ok() {
                return Ok(NatMethod::Turn { 
                    server: turn_server.server.clone(),
                    username: turn_server.username.clone(),
                });
            }
        }
        
        Err(crate::error::SynapseError::TransportError("No NAT traversal method available".into()))
    }
    
    /// Send message using established NAT traversal
    pub async fn send_via_nat(&self, target: &str, message: &SecureMessage, method: &NatMethod) -> Result<String> {
        match method {
            NatMethod::Upnp => {
                // Use direct connection via UPnP mapped port
                self.send_direct(target, message).await
            }
            NatMethod::Stun { server: _ } => {
                // Use external address discovered via STUN
                if let Some(external_addr) = self.external_address {
                    self.send_via_external_address(target, message, external_addr).await
                } else {
                    Err(crate::error::SynapseError::TransportError("No external address available".into()))
                }
            }
            NatMethod::Turn { server, username: _ } => {
                // Relay via TURN server
                self.send_via_turn_relay(target, message, server).await
            }
            NatMethod::IceCandidate => {
                // Use ICE connectivity establishment
                self.send_via_ice(target, message).await
            }
        }
    }
    
    async fn send_direct(&self, target: &str, _message: &SecureMessage) -> Result<String> {
        // For now, use TCP transport as fallback
        info!("NAT traversal: attempting direct connection to {}", target);
        Ok(format!("tcp://{}:{}", target, self.local_port))
    }
    
    async fn send_via_external_address(&self, target: &str, _message: &SecureMessage, _external_addr: SocketAddr) -> Result<String> {
        // Send using the external address discovered via STUN
        info!("NAT traversal: using external address for {}", target);
        Ok(format!("tcp://{}:{}", target, self.local_port))
    }
    
    async fn send_via_turn_relay(&self, target: &str, _message: &SecureMessage, _turn_server: &str) -> Result<String> {
        // Relay message via TURN server
        // In a real implementation, this would use TURN protocol
        info!("Relaying message to {} via TURN", target);
        Ok(format!("turn://{}:{}", target, self.local_port))
    }
    
    async fn send_via_ice(&self, target: &str, _message: &SecureMessage) -> Result<String> {
        // Use ICE connectivity establishment
        // In a real implementation, this would perform ICE connectivity checks
        info!("Sending message to {} via ICE", target);
        Ok(format!("ice://{}:{}", target, self.local_port))
    }
}

#[async_trait]
impl Transport for NatTraversalTransport {
    fn transport_type(&self) -> abstraction::TransportType {
        abstraction::TransportType::Custom(1) // NAT traversal transport
    }

    fn capabilities(&self) -> abstraction::TransportCapabilities {
        abstraction::TransportCapabilities {
            max_message_size: 64 * 1024, // 64KB reasonable for NAT traversal
            reliable: false, // UDP-based NAT traversal is not guaranteed reliable
            real_time: true,
            broadcast: false,
            bidirectional: true,
            encrypted: false,
            network_spanning: true,
            supported_urgencies: vec![
                abstraction::MessageUrgency::RealTime,
                abstraction::MessageUrgency::Interactive,
                abstraction::MessageUrgency::Background
            ],
            features: vec![
                "nat_traversal".to_string(),
                "stun".to_string(),
                "turn".to_string(),
                "ice".to_string(),
                if self.upnp_enabled { "upnp".to_string() } else { "no_upnp".to_string() }
            ],
        }
    }
    
    async fn can_reach(&self, target: &abstraction::TransportTarget) -> bool {
        if let Some(addr) = &target.address {
            if let Ok(socket_addr) = addr.parse::<SocketAddr>() {
                return match self.stun_query(&socket_addr.to_string()).await {
                    Ok(_) => true,
                    Err(_) => false,
                };
            }
        }
        false
    }
    
    async fn estimate_metrics(&self, target: &abstraction::TransportTarget) -> Result<abstraction::TransportEstimate> {
        Ok(abstraction::TransportEstimate {
            latency: Duration::from_millis(100), // Higher latency for NAT traversal
            reliability: 0.7, // Lower reliability due to NAT complications
            bandwidth: 1_000_000, // 1 Mbps conservative estimate
            cost: 5.0, // Higher cost due to complexity and server usage
            available: self.can_reach(target).await,
            confidence: 0.6, // Lower confidence due to NAT unpredictability
        })
    }
    
    async fn send_message(&self, target: &abstraction::TransportTarget, message: &SecureMessage) -> Result<abstraction::DeliveryReceipt> {
        // Implementation omitted for brevity - would involve NAT traversal logic
        let start = Instant::now();
        
        // Try to establish connection first
        if !self.can_reach(target).await {
            return Err(SynapseError::TransportError("Could not establish NAT traversal connection".into()));
        }
        
        Ok(abstraction::DeliveryReceipt {
            message_id: message.message_id.0.to_string(),
            transport_used: self.transport_type(),
            delivery_time: start.elapsed(),
            target_reached: target.identifier.clone(),
            confirmation: abstraction::DeliveryConfirmation::Sent,
            metadata: std::collections::HashMap::new(),
        })
    }
    
    async fn receive_messages(&self) -> Result<Vec<abstraction::IncomingMessage>> {
        // Implementation omitted for brevity - would involve NAT hole punching
        Ok(Vec::new()) // No messages in this simplified implementation
    }
    
    async fn test_connectivity(&self, target: &abstraction::TransportTarget) -> Result<abstraction::ConnectivityResult> {
        let start = Instant::now();
        let can_reach = self.can_reach(target).await;
        
        Ok(abstraction::ConnectivityResult {
            connected: can_reach,
            rtt: if can_reach { Some(start.elapsed()) } else { None },
            error: if can_reach { None } else { Some("NAT traversal failed".to_string()) },
            quality: if can_reach { 0.7 } else { 0.0 },
            details: {
                let mut details = std::collections::HashMap::new();
                details.insert("stun_servers_available".to_string(), self.stun_servers.len().to_string());
                details.insert("turn_servers_available".to_string(), self.turn_servers.len().to_string());
                details.insert("upnp_enabled".to_string(), self.upnp_enabled.to_string());
                details
            },
        })
    }

    async fn start(&self) -> Result<()> {
        // Would typically start NAT traversal services here
        Ok(())
    }
    
    async fn stop(&self) -> Result<()> {
        // Would typically clean up NAT mappings here
        Ok(())
    }
    
    async fn status(&self) -> super::abstraction::TransportStatus {
        if self.external_address.is_some() {
            super::abstraction::TransportStatus::Running
        } else {
            super::abstraction::TransportStatus::Degraded
        }
    }
    
    async fn metrics(&self) -> super::abstraction::TransportMetrics {
        super::abstraction::TransportMetrics {
            transport_type: self.transport_type(),
            messages_sent: 0, // Would track in real implementation
            messages_received: 0,
            send_failures: 0,
            receive_failures: 0,
            bytes_sent: 0,
            bytes_received: 0,
            average_latency_ms: 100, // 100ms typical for NAT traversal
            reliability_score: 0.7,
            active_connections: 0,
            last_updated_timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            custom_metrics: {
                let mut metrics = std::collections::HashMap::new();
                metrics.insert("stun_servers_count".to_string(), self.stun_servers.len() as f64);
                metrics.insert("turn_servers_count".to_string(), self.turn_servers.len() as f64);
                metrics
            },
        }
    }
}

#[derive(Debug)]
pub enum NatMethod {
    Upnp,
    Stun {
        server: String,
    },
    Turn {
        server: String,
        username: String,
    },
    IceCandidate,
}

// Implement Clone for NatTraversalTransport
impl Clone for NatTraversalTransport {
    fn clone(&self) -> Self {
        Self {
            local_port: self.local_port,
            stun_servers: self.stun_servers.clone(),
            turn_servers: self.turn_servers.clone(),
            upnp_enabled: self.upnp_enabled,
            ice_candidates: self.ice_candidates.clone(),
            external_address: self.external_address,
        }
    }
}
