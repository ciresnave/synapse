//! Multi-transport layer for EMRP
//! 
//! This module provides the core transport abstraction and implementations
//! for different communication methods: TCP/UDP, auto-discovery, NAT traversal, and email.

// Core abstraction layer
pub mod abstraction;
pub mod manager;

// Dependency injection providers for testability
pub mod providers;

#[cfg(test)]
mod providers_test;

// Transport implementations
#[cfg(not(target_arch = "wasm32"))]
pub mod discovery; // New auto-discovery based implementation

#[cfg(not(target_arch = "wasm32"))]
pub mod udp_unified;

// Re-export key types
pub use abstraction::{TransportType, TransportCapabilities};
pub use discovery::DiscoveryTransport;
pub use auto_discovery::config::DiscoveryConfig;

// Simple, working transport implementation (template)
#[cfg(not(target_arch = "wasm32"))]
pub mod tcp_simple;

// Platform-specific transport modules (not available in WASM)
#[cfg(not(target_arch = "wasm32"))]
pub mod tcp;
#[cfg(not(target_arch = "wasm32"))]
pub mod tcp_enhanced;
#[cfg(not(target_arch = "wasm32"))]
pub mod udp;
#[cfg(not(target_arch = "wasm32"))]
pub mod http_unified;
// #[cfg(not(target_arch = "wasm32"))]
// pub mod mdns;
#[cfg(not(target_arch = "wasm32"))]
pub mod mdns_enhanced;
#[cfg(not(target_arch = "wasm32"))]
pub mod nat_traversal;
#[cfg(not(target_arch = "wasm32"))]
pub mod email_enhanced;
// Legacy implementations - temporarily disabled
// #[cfg(not(target_arch = "wasm32"))]
// pub mod websocket;
// #[cfg(not(target_arch = "wasm32"))]
// pub mod quic;
#[cfg(not(target_arch = "wasm32"))]
pub mod router;
#[cfg(not(target_arch = "wasm32"))]
pub mod llm_discovery;

use crate::{types::*, error::Result, circuit_breaker::{CircuitBreaker, RequestOutcome}};
use async_trait::async_trait;
use std::{time::{Duration, Instant}, sync::Arc};
use serde::{Serialize, Deserialize};

// Platform-specific imports
#[cfg(not(target_arch = "wasm32"))]
use tokio::time::timeout;

/// Available transport routes with performance characteristics
#[derive(Debug, Clone)]
pub enum TransportRoute {
    // Platform-specific transport routes (not available in WASM)
    #[cfg(not(target_arch = "wasm32"))]
    DirectTcp { 
        address: String,
        port: u16, 
        latency_ms: u32,
        established_at: Instant,
    },
    #[cfg(not(target_arch = "wasm32"))]
    DirectUdp {
        address: String, 
        port: u16,
        latency_ms: u32,
        established_at: Instant,
    },
    #[cfg(not(target_arch = "wasm32"))]
    Udp {
        address: String,
        latency: Duration,
        reliability: f64,
    },
    #[cfg(not(target_arch = "wasm32"))]
    WebSocket {
        url: String,
        latency: Duration,
        reliability: f64,
    },
    #[cfg(not(target_arch = "wasm32"))]
    Quic {
        address: String,
        latency: Duration,
        reliability: f64,
        multiplexed: bool,
    },
    #[cfg(not(target_arch = "wasm32"))]
    LocalMdns { 
        service_name: String,
        address: String,
        port: u16,
        latency_ms: u32,
        discovered_at: Instant,
    },
    #[cfg(not(target_arch = "wasm32"))]
    NatTraversal { 
        method: NatMethod,
        external_address: String,
        external_port: u16,
        latency_ms: u32,
        established_at: Instant,
    },
    #[cfg(not(target_arch = "wasm32"))]
    FastEmailRelay { 
        relay_server: String,
        estimated_latency_ms: u32,
    },
    #[cfg(not(target_arch = "wasm32"))]
    StandardEmail { 
        estimated_latency_min: u32,
    },
    #[cfg(not(target_arch = "wasm32"))]
    EmailDiscovery { 
        target_transport: Box<TransportRoute>,
    },
    
    // WASM-compatible transport routes
    #[cfg(target_arch = "wasm32")]
    WebSocket {
        url: String,
        latency_ms: u32,
        established_at: Instant,
    },
    #[cfg(target_arch = "wasm32")]
    WebRtc {
        peer_connection: String,
        latency_ms: u32,
        established_at: Instant,
    },
    #[cfg(target_arch = "wasm32")]
    WebAssembly {
        estimated_latency_ms: u32,
    },
}

/// NAT traversal methods (not available in WASM)
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NatMethod {
    Upnp,
    Stun { server: String },
    Turn { server: String, username: String },
    IceCandidate,
}

/// Connection offer for transport negotiation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionOffer {
    pub entity_id: String,
    // Platform-specific endpoints (not available in WASM)
    #[cfg(not(target_arch = "wasm32"))]
    pub tcp_endpoints: Vec<String>,
    #[cfg(not(target_arch = "wasm32"))]
    pub udp_endpoints: Vec<String>, 
    #[cfg(not(target_arch = "wasm32"))]
    pub stun_servers: Vec<String>,
    #[cfg(not(target_arch = "wasm32"))]
    pub turn_servers: Vec<TurnServer>,
    
    // Common fields
    pub capabilities: Vec<String>,
    pub public_key: String,
    pub expires_at: chrono::DateTime<chrono::Utc>,
    pub priority: u8, // 0-255, higher = more preferred
    
    // WASM-specific endpoints
    #[cfg(target_arch = "wasm32")]
    pub websocket_endpoints: Vec<String>,
    #[cfg(target_arch = "wasm32")]
    pub webrtc_endpoints: Vec<String>,
}

/// TURN server configuration (not available in WASM)
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnServer {
    pub url: String,
    pub username: String, 
    pub credential: String,
}

/// Transport performance metrics
#[derive(Debug, Clone)]
pub struct TransportMetrics {
    pub latency: Duration,
    pub throughput_bps: u64,
    pub packet_loss: f32,
    pub jitter_ms: u32,
    pub reliability_score: f32, // 0.0-1.0
    pub last_updated: Instant,
}

impl Default for TransportMetrics {
    fn default() -> Self {
        Self {
            latency: Duration::from_millis(50),
            throughput_bps: 1_000_000, // 1 Mbps
            packet_loss: 0.0,
            jitter_ms: 10,
            reliability_score: 0.95,
            last_updated: Instant::now(),
        }
    }
}

/// Hybrid connection combining multiple transports
#[derive(Debug)]
pub struct HybridConnection {
    pub primary: TransportRoute,
    pub fallback: TransportRoute,
    pub discovery_latency: Duration,
    pub connection_latency: Duration,
    pub total_setup_time: Duration,
    pub metrics: TransportMetrics,
}

/// Abstract transport trait for all communication methods
#[async_trait]
pub trait Transport: Send + Sync {
    /// Send a message via this transport
    async fn send_message(&self, target: &str, message: &SecureMessage) -> Result<String>;
    
    /// Send a message with circuit breaker protection
    async fn send_message_with_breaker(
        &self, 
        target: &str, 
        message: &SecureMessage,
        circuit_breaker: Option<Arc<CircuitBreaker>>
    ) -> Result<String> {
        if let Some(breaker) = circuit_breaker {
            // Check if circuit allows request
            if !breaker.can_proceed().await {
                return Err(crate::error::SynapseError::TransportError(
                    "Circuit breaker open - request rejected".to_string()
                ).into());
            }
            
            // Attempt the request
            match self.send_message(target, message).await {
                Ok(result) => {
                    breaker.record_outcome(RequestOutcome::Success).await;
                    Ok(result)
                }
                Err(e) => {
                    breaker.record_outcome(RequestOutcome::Failure(e.to_string())).await;
                    Err(e)
                }
            }
        } else {
            // No circuit breaker - direct call
            self.send_message(target, message).await
        }
    }
    
    /// Receive messages via this transport  
    async fn receive_messages(&self) -> Result<Vec<SecureMessage>>;
    
    /// Test connectivity and measure latency
    async fn test_connectivity(&self, target: &str) -> Result<TransportMetrics>;
    
    /// Test connectivity with circuit breaker integration
    async fn test_connectivity_with_breaker(
        &self,
        target: &str,
        circuit_breaker: Option<Arc<CircuitBreaker>>
    ) -> Result<TransportMetrics> {
        if let Some(breaker) = circuit_breaker {
            if !breaker.can_proceed().await {
                return Err(crate::error::SynapseError::TransportError(
                    "Circuit breaker open - connectivity test rejected".to_string()
                ).into());
            }
            
            let start_time = Instant::now();
            match self.test_connectivity(target).await {
                Ok(metrics) => {
                    breaker.record_outcome(RequestOutcome::Success).await;
                    
                    // Update circuit breaker with latest metrics
                    breaker.check_external_triggers(&metrics).await;
                    
                    Ok(metrics)
                }
                Err(e) => {
                    let elapsed = start_time.elapsed();
                    if elapsed > Duration::from_secs(10) {
                        breaker.record_outcome(RequestOutcome::Timeout).await;
                    } else {
                        breaker.record_outcome(RequestOutcome::Failure(e.to_string())).await;
                    }
                    Err(e)
                }
            }
        } else {
            self.test_connectivity(target).await
        }
    }
    
    /// Check if this transport can reach the target
    async fn can_reach(&self, target: &str) -> bool;
    
    /// Get transport-specific capabilities
    fn get_capabilities(&self) -> Vec<String>;
    
    /// Get estimated latency for this transport
    fn estimated_latency(&self) -> Duration;
    
    /// Get reliability score (0.0-1.0)
    fn reliability_score(&self) -> f32;
}

/// Transport discovery and testing utilities
pub struct TransportDiscovery {
    discovery_timeout: Duration,
    #[allow(dead_code)]
    connectivity_cache: dashmap::DashMap<String, (TransportMetrics, Instant)>,
}

impl TransportDiscovery {
    pub fn new() -> Self {
        Self {
            discovery_timeout: Duration::from_secs(10),
            connectivity_cache: dashmap::DashMap::new(),
        }
    }
    
    /// Discover all available transports to a target (non-WASM platforms)
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn discover_transports(&mut self, target: &str) -> Result<Vec<TransportRoute>> {
        let mut routes = Vec::new();
        
        // Try direct TCP connection
        if let Ok(tcp_route) = self.test_direct_tcp(target).await {
            routes.push(tcp_route);
        }
        
        // Try direct UDP connection
        if let Ok(udp_route) = self.test_direct_udp(target).await {
            routes.push(udp_route);
        }
        
        // Try mDNS discovery on local network
        if let Ok(mdns_route) = self.discover_mdns_peer(target).await {
            routes.push(mdns_route);
        }
        
        // Try NAT traversal techniques
        if let Ok(nat_routes) = self.establish_nat_traversal(target).await {
            routes.extend(nat_routes);
        }
        
        // Always include email fallback
        routes.push(TransportRoute::StandardEmail { 
            estimated_latency_min: 1 // 1+ minute typical email latency
        });
        
        Ok(routes)
    }
    
    /// WASM stub for transport discovery
    #[cfg(target_arch = "wasm32")]
    pub async fn discover_transports(&mut self, _target: &str) -> Result<Vec<TransportRoute>> {
        // WASM currently only supports basic message passing
        Ok(vec![TransportRoute::WebAssembly { 
            estimated_latency_ms: 50 
        }])
    }
    
    #[cfg(not(target_arch = "wasm32"))]
    async fn test_direct_tcp(&self, target: &str) -> Result<TransportRoute> {
        // Try common ports for EMRP TCP transport
        let ports = vec![8080, 8443, 9090, 7777];
        
        for port in ports {
            let address = format!("{}:{}", target, port);
            let start = Instant::now();
            
            match timeout(self.discovery_timeout, tokio::net::TcpStream::connect(&address)).await {
                Ok(Ok(_stream)) => {
                    let latency = start.elapsed();
                    return Ok(TransportRoute::DirectTcp {
                        address: target.to_string(),
                        port,
                        latency_ms: latency.as_millis() as u32,
                        established_at: Instant::now(),
                    });
                }
                _ => continue,
            }
        }
        
        Err(crate::error::SynapseError::TransportError("No direct TCP connection available".into()))
    }
    
    #[cfg(not(target_arch = "wasm32"))]
    async fn test_direct_udp(&self, target: &str) -> Result<TransportRoute> {
        // Try common ports for EMRP UDP transport
        let ports = vec![8080, 8443, 9090, 7777];
        
        for port in ports {
            let address = format!("{}:{}", target, port);
            let start = Instant::now();
            
            match timeout(
                self.discovery_timeout,
                tokio::net::UdpSocket::bind("0.0.0.0:0")
            ).await {
                Ok(Ok(socket)) => {
                    // Test UDP connectivity with a ping packet
                    if socket.connect(&address).await.is_ok() {
                        let latency = start.elapsed();
                        return Ok(TransportRoute::DirectUdp {
                            address: target.to_string(),
                            port,
                            latency_ms: latency.as_millis() as u32,
                            established_at: Instant::now(),
                        });
                    }
                }
                _ => continue,
            }
        }
        
        Err(crate::error::SynapseError::TransportError("No direct UDP connection available".into()))
    }
    
    #[cfg(not(target_arch = "wasm32"))]
    async fn discover_mdns_peer(&self, _target: &str) -> Result<TransportRoute> {
        // mDNS discovery implementation will be in mdns.rs
        // For now, return an error indicating it's not available
        Err(crate::error::SynapseError::TransportError("mDNS discovery not yet implemented".into()))
    }
    
    #[cfg(not(target_arch = "wasm32"))]
    async fn establish_nat_traversal(&self, _target: &str) -> Result<Vec<TransportRoute>> {
        // NAT traversal implementation will be in nat_traversal.rs
        // For now, return empty vec
        Ok(Vec::new())
    }
}

/// Transport selection algorithm
pub struct TransportSelector {
    discovery: TransportDiscovery,
    performance_weights: TransportWeights,
}

#[derive(Debug, Clone)]
pub struct TransportWeights {
    pub latency_weight: f32,
    pub reliability_weight: f32, 
    pub throughput_weight: f32,
    pub setup_time_weight: f32,
}

impl Default for TransportWeights {
    fn default() -> Self {
        Self {
            latency_weight: 0.4,
            reliability_weight: 0.3,
            throughput_weight: 0.2,
            setup_time_weight: 0.1,
        }
    }
}

impl TransportSelector {
    pub fn new() -> Self {
        Self {
            discovery: TransportDiscovery::new(),
            performance_weights: TransportWeights::default(),
        }
    }
    
    /// Choose the optimal transport for a message (non-WASM platforms)
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn choose_optimal_transport(
        &mut self, 
        target: &str, 
        urgency: abstraction::MessageUrgency
    ) -> Result<TransportRoute> {
        let available_routes = self.discovery.discover_transports(target).await?;
        
        match urgency {
            abstraction::MessageUrgency::Critical | abstraction::MessageUrgency::RealTime => {
                // Only use transports with <100ms latency
                for route in available_routes {
                    match route {
                        TransportRoute::DirectTcp { latency_ms, .. } 
                        | TransportRoute::DirectUdp { latency_ms, .. }
                        | TransportRoute::LocalMdns { latency_ms, .. } 
                        | TransportRoute::NatTraversal { latency_ms, .. } 
                            if latency_ms < 100 => return Ok(route),
                        _ => continue,
                    }
                }
                Err(crate::error::SynapseError::TransportError("No real-time transport available".into()))
            }
            abstraction::MessageUrgency::Interactive => {
                // Accept up to 1s latency, prefer faster options
                self.select_best_route(available_routes, 1000).await
            }
            abstraction::MessageUrgency::Background | abstraction::MessageUrgency::Batch => {
                // Prefer reliability over speed
                self.select_most_reliable_route(available_routes).await
            }
        }
    }
    
    /// Choose the optimal transport for a message (WASM platforms)
    #[cfg(target_arch = "wasm32")]
    pub async fn choose_optimal_transport(
        &mut self, 
        _target: &str, 
        _urgency: abstraction::MessageUrgency
    ) -> Result<TransportRoute> {
        // WASM currently only supports basic message passing
        Ok(TransportRoute::WebAssembly { 
            estimated_latency_ms: 50 
        })
    }
    
    #[cfg(not(target_arch = "wasm32"))]
    async fn select_best_route(&self, routes: Vec<TransportRoute>, max_latency_ms: u32) -> Result<TransportRoute> {
        let mut best_route = None;
        let mut best_score = f32::MIN;
        
        for route in routes {
            let latency_ms = self.get_route_latency(&route);
            if latency_ms <= max_latency_ms {
                let score = self.calculate_route_score(&route);
                if score > best_score {
                    best_score = score;
                    best_route = Some(route);
                }
            }
        }
        
        best_route.ok_or_else(|| {
            crate::error::SynapseError::TransportError("No suitable transport found".into())
        })
    }
    
    #[cfg(not(target_arch = "wasm32"))]
    async fn select_most_reliable_route(&self, routes: Vec<TransportRoute>) -> Result<TransportRoute> {
        // Prefer email and other reliable transports
        for route in &routes {
            match route {
                TransportRoute::StandardEmail { .. } => return Ok(route.clone()),
                TransportRoute::FastEmailRelay { .. } => return Ok(route.clone()),
                _ => continue,
            }
        }
        
        // If no email available, use the best alternative
        self.select_best_route(routes, u32::MAX).await
    }
    
    #[cfg(not(target_arch = "wasm32"))]
    fn get_route_latency(&self, route: &TransportRoute) -> u32 {
        match route {
            TransportRoute::DirectTcp { latency_ms, .. } 
            | TransportRoute::DirectUdp { latency_ms, .. }
            | TransportRoute::LocalMdns { latency_ms, .. }
            | TransportRoute::NatTraversal { latency_ms, .. } => *latency_ms,
            TransportRoute::FastEmailRelay { estimated_latency_ms, .. } => *estimated_latency_ms,
            TransportRoute::StandardEmail { estimated_latency_min } => estimated_latency_min * 60 * 1000, // Convert minutes to ms
            TransportRoute::EmailDiscovery { .. } => 30_000, // 30s for discovery
            TransportRoute::Udp { .. } => 5, // 5ms estimated for UDP
            TransportRoute::WebSocket { .. } => 20, // 20ms estimated for WebSocket
            TransportRoute::Quic { .. } => 10, // 10ms estimated for QUIC
        }
    }
    
    #[cfg(not(target_arch = "wasm32"))]
    fn calculate_route_score(&self, route: &TransportRoute) -> f32 {
        let latency_ms = self.get_route_latency(route) as f32;
        let reliability = self.get_route_reliability(route);
        let setup_time = self.get_route_setup_time(route);
        
        // Lower latency and setup time = higher score
        // Higher reliability = higher score
        let latency_score = 1.0 / (1.0 + latency_ms / 1000.0); // Normalize to 0-1
        let setup_score = 1.0 / (1.0 + setup_time);
        
        latency_score * self.performance_weights.latency_weight +
        reliability * self.performance_weights.reliability_weight +
        setup_score * self.performance_weights.setup_time_weight
    }
    
    #[cfg(not(target_arch = "wasm32"))]
    fn get_route_reliability(&self, route: &TransportRoute) -> f32 {
        match route {
            TransportRoute::StandardEmail { .. } => 0.95,
            TransportRoute::FastEmailRelay { .. } => 0.90,
            TransportRoute::DirectTcp { .. } => 0.85,
            TransportRoute::DirectUdp { .. } => 0.80,
            TransportRoute::LocalMdns { .. } => 0.95,
            TransportRoute::NatTraversal { .. } => 0.70,
            TransportRoute::EmailDiscovery { .. } => 0.95,
            TransportRoute::Udp { .. } => 0.80,
            TransportRoute::WebSocket { .. } => 0.85,
            TransportRoute::Quic { .. } => 0.90,
        }
    }
    
    #[cfg(not(target_arch = "wasm32"))]
    fn get_route_setup_time(&self, route: &TransportRoute) -> f32 {
        match route {
            TransportRoute::DirectTcp { .. } => 0.1, // 100ms
            TransportRoute::DirectUdp { .. } => 0.05, // 50ms
            TransportRoute::LocalMdns { .. } => 0.05, // 50ms
            TransportRoute::NatTraversal { .. } => 2.0, // 2s
            TransportRoute::FastEmailRelay { .. } => 1.0, // 1s
            TransportRoute::StandardEmail { .. } => 1.0, // 1s
            TransportRoute::EmailDiscovery { .. } => 30.0, // 30s
            TransportRoute::Udp { .. } => 0.05, // 50ms
            TransportRoute::WebSocket { .. } => 0.2, // 200ms
            TransportRoute::Quic { .. } => 0.1, // 100ms
        }
    }
}

// Re-export all major components for easy access
pub use abstraction::*;
pub use manager::*;

// Unified transport implementations (temporarily disabled)
// #[cfg(not(target_arch = "wasm32"))]
// pub use tcp_unified::{TcpTransportImpl, TcpTransportFactory};
#[cfg(not(target_arch = "wasm32"))]
pub use udp_unified::{UdpTransportImpl, UdpTransportFactory};
#[cfg(not(target_arch = "wasm32"))]
#[cfg(feature = "http")]
pub use http_unified::{HttpTransportImpl, HttpTransportFactory};

// Simple transport implementations
#[cfg(not(target_arch = "wasm32"))]
pub use tcp_simple::{SimpleTcpTransport, SimpleTcpTransportFactory};

// Platform-specific re-exports (only on non-WASM platforms)
#[cfg(not(target_arch = "wasm32"))]
pub use tcp::TcpTransport;
#[cfg(not(target_arch = "wasm32"))]
pub use tcp_enhanced::EnhancedTcpTransport;
// #[cfg(not(target_arch = "wasm32"))]
// #[cfg(feature = "mdns")]
// pub use mdns::{MdnsTransport, MdnsAdvertiser};
#[cfg(not(target_arch = "wasm32"))]
pub use mdns_enhanced::{EnhancedMdnsTransport, MdnsConfig};
#[cfg(not(target_arch = "wasm32"))]
pub use nat_traversal::{NatTraversalTransport, IceCandidate};
#[cfg(not(target_arch = "wasm32"))]
#[cfg(feature = "email")]
pub use email_enhanced::{EmailEnhancedTransport, FastEmailRelay};
#[cfg(not(target_arch = "wasm32"))]
pub use router::MultiTransportRouter;
#[cfg(not(target_arch = "wasm32"))]
pub use llm_discovery::{
    LlmDiscoveryManager, LlmDiscoveryConfig, DiscoveredLlm, LlmConnection,
    LlmModelInfo, LlmConnectionInfo, LlmPerformanceMetrics, LlmStatus,
    LlmRequest, LlmResponse, LlmResponseMetadata
};
