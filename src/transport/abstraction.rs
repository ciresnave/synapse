//! Unified Transport Abstraction Layer for Synapse
//! 
//! This module provides a unified interface that sits above all transport mechanisms,
//! making applications transport-agnostic while providing intelligent transport selection,
//! failover, and optimization capabilities.

use crate::{
    types::SecureMessage,
    error::Result,
};
use async_trait::async_trait;
use std::{
    time::{Duration, Instant},
    collections::HashMap,
};
use dashmap::DashMap;
use serde::{Serialize, Deserialize};


/// Unified transport interface that all transport mechanisms must implement
#[async_trait]
pub trait Transport: Send + Sync {
    /// Get the transport type identifier
    fn transport_type(&self) -> TransportType;
    
    /// Get transport capabilities
    fn capabilities(&self) -> TransportCapabilities;
    
    /// Check if this transport can reach a specific target
    async fn can_reach(&self, target: &TransportTarget) -> bool;
    
    /// Get estimated metrics for reaching a target
    async fn estimate_metrics(&self, target: &TransportTarget) -> Result<TransportEstimate>;
    
    /// Send a message via this transport
    async fn send_message(&self, target: &TransportTarget, message: &SecureMessage) -> Result<DeliveryReceipt>;
    
    /// Receive messages from this transport
    async fn receive_messages(&self) -> Result<Vec<IncomingMessage>>;
    
    /// Test connectivity to a target
    async fn test_connectivity(&self, target: &TransportTarget) -> Result<ConnectivityResult>;
    
    /// Start the transport (if needed)
    async fn start(&self) -> Result<()>;
    
    /// Stop the transport gracefully
    async fn stop(&self) -> Result<()>;
    
    /// Get current transport status
    async fn status(&self) -> TransportStatus;
    
    /// Get transport metrics
    async fn metrics(&self) -> TransportMetrics;
}

/// Transport type identifiers
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TransportType {
    Tcp,
    Udp,
    WebSocket,
    Http,
    Email,
    Mdns,
    Quic,
    Custom(u32), // For extensibility
}

impl std::fmt::Display for TransportType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TransportType::Tcp => write!(f, "TCP"),
            TransportType::Udp => write!(f, "UDP"),
            TransportType::WebSocket => write!(f, "WebSocket"),
            TransportType::Http => write!(f, "HTTP"),
            TransportType::Email => write!(f, "Email"),
            TransportType::Mdns => write!(f, "mDNS"),
            TransportType::Quic => write!(f, "QUIC"),
            TransportType::Custom(id) => write!(f, "Custom({})", id),
        }
    }
}

/// Transport capabilities
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransportCapabilities {
    /// Maximum message size supported
    pub max_message_size: usize,
    /// Whether transport supports reliable delivery
    pub reliable: bool,
    /// Whether transport supports real-time communication
    pub real_time: bool,
    /// Whether transport supports broadcast/multicast
    pub broadcast: bool,
    /// Whether transport supports bidirectional communication
    pub bidirectional: bool,
    /// Whether transport supports encryption
    pub encrypted: bool,
    /// Whether transport works across networks (not just local)
    pub network_spanning: bool,
    /// Supported message urgency levels
    pub supported_urgencies: Vec<MessageUrgency>,
    /// Transport-specific features
    pub features: Vec<String>,
}

/// Message urgency levels for transport selection
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageUrgency {
    /// Immediate delivery required (< 100ms)
    Critical,
    /// Real-time delivery preferred (< 1s)
    RealTime,
    /// Interactive response time (< 5s)
    Interactive,
    /// Background processing acceptable
    Background,
    /// Store and forward acceptable
    Batch,
}

/// Transport target specification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransportTarget {
    /// Primary identifier (entity ID, email, IP, etc.)
    pub identifier: String,
    /// Optional specific address/endpoint
    pub address: Option<String>,
    /// Preferred transport types (in order of preference)
    pub preferred_transports: Vec<TransportType>,
    /// Required capabilities
    pub required_capabilities: Vec<String>,
    /// Urgency level for this target
    pub urgency: MessageUrgency,
}

impl TransportTarget {
    pub fn new(identifier: String) -> Self {
        Self {
            identifier,
            address: None,
            preferred_transports: Vec::new(),
            required_capabilities: Vec::new(),
            urgency: MessageUrgency::Interactive,
        }
    }
    
    pub fn with_address(mut self, address: String) -> Self {
        self.address = Some(address);
        self
    }
    
    pub fn with_urgency(mut self, urgency: MessageUrgency) -> Self {
        self.urgency = urgency;
        self
    }
    
    pub fn prefer_transport(mut self, transport: TransportType) -> Self {
        self.preferred_transports.push(transport);
        self
    }
    
    pub fn require_capability(mut self, capability: String) -> Self {
        self.required_capabilities.push(capability);
        self
    }
}

/// Transport performance estimate
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransportEstimate {
    /// Estimated latency
    pub latency: Duration,
    /// Estimated reliability (0.0-1.0)
    pub reliability: f64,
    /// Estimated bandwidth (bytes/sec)
    pub bandwidth: u64,
    /// Estimated cost (arbitrary units)
    pub cost: f64,
    /// Whether the transport is currently available
    pub available: bool,
    /// Confidence in these estimates (0.0-1.0)
    pub confidence: f64,
}

/// Delivery receipt from successful message send
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeliveryReceipt {
    /// Unique message ID
    pub message_id: String,
    /// Transport used for delivery
    pub transport_used: TransportType,
    /// Actual delivery time
    pub delivery_time: Duration,
    /// Target that was reached
    pub target_reached: String,
    /// Delivery confirmation level
    pub confirmation: DeliveryConfirmation,
    /// Additional metadata
    pub metadata: HashMap<String, String>,
}

/// Levels of delivery confirmation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeliveryConfirmation {
    /// Message was sent to transport layer
    Sent,
    /// Message was delivered to target network
    Delivered,
    /// Message was received by target application
    Received,
    /// Message was acknowledged by target
    Acknowledged,
}

/// Incoming message with transport context
#[derive(Debug, Clone)]
pub struct IncomingMessage {
    /// The message content
    pub message: SecureMessage,
    /// Transport that received the message
    pub transport_type: TransportType,
    /// Source address/identifier
    pub source: String,
    /// When the message was received (as timestamp)
    pub received_timestamp: u64,
    /// Additional transport-specific metadata
    pub metadata: HashMap<String, String>,
}

impl IncomingMessage {
    /// Create a new incoming message with current timestamp
    pub fn new(message: SecureMessage, transport_type: TransportType, source: String) -> Self {
        Self {
            message,
            transport_type,
            source,
            received_timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            metadata: HashMap::new(),
        }
    }
    
    /// Get received time as Duration since epoch
    pub fn received_at(&self) -> Duration {
        Duration::from_secs(self.received_timestamp)
    }
}

/// Result of connectivity test
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectivityResult {
    /// Whether connection was successful
    pub connected: bool,
    /// Round-trip time if successful
    pub rtt: Option<Duration>,
    /// Error message if failed
    pub error: Option<String>,
    /// Connection quality score (0.0-1.0)
    pub quality: f64,
    /// Additional test results
    pub details: HashMap<String, String>,
}

/// Current transport status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransportStatus {
    /// Transport is not yet started
    Stopped,
    /// Transport is starting up
    Starting,
    /// Transport is running normally
    Running,
    /// Transport is experiencing issues but functional
    Degraded,
    /// Transport is not functional
    Failed,
    /// Transport is shutting down
    Stopping,
}

/// Transport performance metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransportMetrics {
    /// Transport type
    pub transport_type: TransportType,
    /// Number of messages sent
    pub messages_sent: u64,
    /// Number of messages received
    pub messages_received: u64,
    /// Number of send failures
    pub send_failures: u64,
    /// Number of receive failures
    pub receive_failures: u64,
    /// Total bytes sent
    pub bytes_sent: u64,
    /// Total bytes received
    pub bytes_received: u64,
    /// Average latency in milliseconds
    pub average_latency_ms: u64,
    /// Current reliability score (0.0-1.0)
    pub reliability_score: f64,
    /// Number of active connections
    pub active_connections: u32,
    /// Last update time as Unix timestamp
    pub last_updated_timestamp: u64,
    /// Transport-specific metrics
    pub custom_metrics: HashMap<String, f64>,
}

impl Default for TransportMetrics {
    fn default() -> Self {
        Self {
            transport_type: TransportType::Tcp,
            messages_sent: 0,
            messages_received: 0,
            send_failures: 0,
            receive_failures: 0,
            bytes_sent: 0,
            bytes_received: 0,
            average_latency_ms: 0,
            reliability_score: 1.0,
            active_connections: 0,
            last_updated_timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            custom_metrics: HashMap::new(),
        }
    }
}

impl TransportMetrics {
    /// Get average latency as Duration
    pub fn average_latency(&self) -> Duration {
        Duration::from_millis(self.average_latency_ms)
    }
    
    /// Set average latency from Duration
    pub fn set_average_latency(&mut self, latency: Duration) {
        self.average_latency_ms = latency.as_millis() as u64;
    }
    
    /// Get last updated time as Instant (approximate)
    pub fn last_updated(&self) -> Instant {
        // This is approximate since we can't perfectly convert back to Instant
        Instant::now() // For now, just return current time
    }
    
    /// Update the last updated timestamp to now
    pub fn touch(&mut self) {
        self.last_updated_timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
    }
}

/// Delivery estimate for UI display
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeliveryEstimate {
    /// Estimated latency
    pub latency: Duration,
    /// Estimated reliability (0.0-1.0)  
    pub reliability: f64,
    /// Estimated throughput in bytes per second
    pub throughput_estimate: u64,
    /// Cost score (lower is better)
    pub cost_score: f64,
}

/// Helper implementations for common capability sets
impl TransportCapabilities {
    /// TCP transport capabilities
    pub fn tcp() -> Self {
        Self {
            max_message_size: 64 * 1024 * 1024, // 64MB
            reliable: true,
            real_time: false,
            broadcast: false,
            bidirectional: true,
            encrypted: false, // Depends on TLS
            network_spanning: true,
            supported_urgencies: vec![
                MessageUrgency::Interactive,
                MessageUrgency::Background,
                MessageUrgency::Batch,
            ],
            features: vec![
                "connection_oriented".to_string(),
                "stream_based".to_string(),
                "flow_control".to_string(),
            ],
        }
    }
    
    /// UDP transport capabilities
    pub fn udp() -> Self {
        Self {
            max_message_size: 65507, // Max UDP payload
            reliable: false,
            real_time: true,
            broadcast: true,
            bidirectional: true,
            encrypted: false,
            network_spanning: true,
            supported_urgencies: vec![
                MessageUrgency::Critical,
                MessageUrgency::RealTime,
                MessageUrgency::Interactive,
            ],
            features: vec![
                "connectionless".to_string(),
                "datagram_based".to_string(),
                "low_overhead".to_string(),
                "multicast".to_string(),
            ],
        }
    }
    
    /// Email transport capabilities
    pub fn email() -> Self {
        Self {
            max_message_size: 25 * 1024 * 1024, // 25MB typical limit
            reliable: true,
            real_time: false,
            broadcast: true,
            bidirectional: true,
            encrypted: true, // Usually TLS
            network_spanning: true,
            supported_urgencies: vec![
                MessageUrgency::Background,
                MessageUrgency::Batch,
            ],
            features: vec![
                "store_and_forward".to_string(),
                "federation".to_string(),
                "authentication".to_string(),
                "persistent".to_string(),
            ],
        }
    }
    
    /// mDNS transport capabilities
    pub fn mdns() -> Self {
        Self {
            max_message_size: 1024, // mDNS packet size limit
            reliable: false,
            real_time: true,
            broadcast: true,
            bidirectional: true,
            encrypted: false,
            network_spanning: false, // Local network only
            supported_urgencies: vec![
                MessageUrgency::Critical,
                MessageUrgency::RealTime,
            ],
            features: vec![
                "service_discovery".to_string(),
                "zero_configuration".to_string(),
                "local_network".to_string(),
                "multicast".to_string(),
            ],
        }
    }
    
    /// WebSocket transport capabilities
    pub fn websocket() -> Self {
        Self {
            max_message_size: 16 * 1024 * 1024, // 16MB typical
            reliable: true,
            real_time: true,
            broadcast: false,
            bidirectional: true,
            encrypted: true, // WSS
            network_spanning: true,
            supported_urgencies: vec![
                MessageUrgency::Critical,
                MessageUrgency::RealTime,
                MessageUrgency::Interactive,
            ],
            features: vec![
                "web_compatible".to_string(),
                "full_duplex".to_string(),
                "frame_based".to_string(),
                "http_upgrade".to_string(),
            ],
        }
    }
    
    /// QUIC transport capabilities
    pub fn quic() -> Self {
        Self {
            max_message_size: 1024 * 1024 * 1024, // 1GB theoretical
            reliable: true,
            real_time: true,
            broadcast: false,
            bidirectional: true,
            encrypted: true, // Built-in TLS 1.3
            network_spanning: true,
            supported_urgencies: vec![
                MessageUrgency::Critical,
                MessageUrgency::RealTime,
                MessageUrgency::Interactive,
                MessageUrgency::Background,
            ],
            features: vec![
                "multiplexed_streams".to_string(),
                "zero_rtt".to_string(),
                "connection_migration".to_string(),
                "modern_crypto".to_string(),
                "congestion_control".to_string(),
            ],
        }
    }
    
    /// HTTP transport capabilities
    pub fn http() -> Self {
        Self {
            max_message_size: 10 * 1024 * 1024, // 10MB typical limit
            reliable: true,
            real_time: false, // HTTP has higher latency
            broadcast: false,
            bidirectional: true,
            encrypted: false, // Depends on HTTPS
            network_spanning: true,
            supported_urgencies: vec![
                MessageUrgency::Interactive,
                MessageUrgency::Background,
                MessageUrgency::Batch,
            ],
            features: vec![
                "firewall_friendly".to_string(),
                "web_compatible".to_string(),
                "request_response".to_string(),
                "standard_protocol".to_string(),
            ],
        }
    }
    
    /// HTTPS transport capabilities
    pub fn https() -> Self {
        let mut caps = Self::http();
        caps.encrypted = true;
        caps.features.push("encrypted".to_string());
        caps.features.push("authenticated".to_string());
        caps
    }
}

/// Transport factory trait for creating transport instances
#[async_trait]
pub trait TransportFactory: Send + Sync {
    /// Create a new transport instance
    async fn create_transport(&self, config: &HashMap<String, String>) -> Result<Box<dyn Transport>>;
    
    /// Get the transport type this factory creates
    fn transport_type(&self) -> TransportType;
    
    /// Get default configuration for this transport
    fn default_config(&self) -> HashMap<String, String>;
    
    /// Validate configuration for this transport
    fn validate_config(&self, config: &HashMap<String, String>) -> Result<()>;
}

// Factory implementations for unified transports

/// TCP Transport Factory
pub struct TcpTransportFactory;

#[async_trait]
impl TransportFactory for TcpTransportFactory {
    async fn create_transport(&self, _config: &HashMap<String, String>) -> Result<Box<dyn Transport>> {
        let transport = crate::transport::tcp_simple::SimpleTcpTransport::new();
        Ok(Box::new(transport))
    }
    
    fn transport_type(&self) -> TransportType {
        TransportType::Tcp
    }
    
    fn default_config(&self) -> HashMap<String, String> {
        let mut config = HashMap::new();
        config.insert("listen_port".to_string(), "0".to_string());
        config.insert("connection_timeout_ms".to_string(), "30000".to_string());
        config.insert("max_message_size".to_string(), "1048576".to_string()); // 1MB
        config
    }
    
    fn validate_config(&self, config: &HashMap<String, String>) -> Result<()> {
        if let Some(port_str) = config.get("listen_port") {
            if port_str.parse::<u16>().is_err() {
                return Err(crate::error::EmrpError::Config(crate::error::ConfigError::ValidationFailed(
                    "Invalid port number".to_string()
                )));
            }
        }
        Ok(())
    }
}

/// UDP Transport Factory (temporarily disabled)
// pub struct UdpTransportFactory;
// 
// #[async_trait]
// impl TransportFactory for UdpTransportFactory {
//     async fn create_transport(&self, config: &HashMap<String, String>) -> Result<Box<dyn Transport>> {
//         // TODO: Implement simple UDP transport
//         Err(crate::error::EmrpError::Transport("UDP transport not yet implemented".to_string()))
//     }
//     
//     fn transport_type(&self) -> TransportType {
//         TransportType::Udp
//     }
//     
//     fn default_config(&self) -> HashMap<String, String> {
//         HashMap::new()
//     }
//
//     fn validate_config(&self, config: &HashMap<String, String>) -> Result<()> {
//         Ok(())
//     }
// }

/// Email Transport Factory (temporarily disabled)
// pub struct EmailTransportFactory;

// #[async_trait]
// impl TransportFactory for EmailTransportFactory {
//     async fn create_transport(&self, config: &HashMap<String, String>) -> Result<Box<dyn Transport>> {
//         Err(crate::error::EmrpError::Transport("Email transport not yet implemented".to_string()))
//     }
//     
//     fn transport_type(&self) -> TransportType {
//         TransportType::Email
//     }
//     
//     fn default_config(&self) -> HashMap<String, String> {
//         HashMap::new()
//     }
//     
//     fn validate_config(&self, config: &HashMap<String, String>) -> Result<()> {
//         Ok(())
//     }
// }

// mDNS Transport Factory (COMMENTED OUT - TO BE FIXED)
/*
pub struct MdnsTransportFactory;

#[async_trait]
impl TransportFactory for MdnsTransportFactory {
    async fn create_transport(&self, config: &HashMap<String, String>) -> Result<Box<dyn Transport>> {
        let transport = crate::transport::mdns_unified::MdnsTransportImpl::new(config).await?;
        Ok(Box::new(transport))
    }
    
    fn transport_type(&self) -> TransportType {
        TransportType::Mdns
    }
    
    fn default_config(&self) -> HashMap<String, String> {
        let mut config = HashMap::new();
        config.insert("service_name".to_string(), "_synapse._tcp.local".to_string());
        config.insert("local_port".to_string(), "0".to_string());
        config.insert("discovery_timeout_ms".to_string(), "5000".to_string());
        config.insert("max_message_size".to_string(), "65507".to_string()); // Max UDP
        config
    }
    
    fn validate_config(&self, config: &HashMap<String, String>) -> Result<()> {
        if let Some(service_name) = config.get("service_name") {
            if !service_name.contains("._tcp.") && !service_name.contains("._udp.") {
                return Err(crate::error::EmrpError::Config(crate::error::ConfigError::ValidationFailed(
                    "Service name must include protocol (_tcp. or _udp.)".to_string()
                )));
            }
        }
        Ok(())
    }
}
*/

// WebSocket Transport Factory (COMMENTED OUT - TO BE FIXED)
/*
pub struct WebSocketTransportFactory;

#[async_trait]
impl TransportFactory for WebSocketTransportFactory {
    async fn create_transport(&self, config: &HashMap<String, String>) -> Result<Box<dyn Transport>> {
        let transport = crate::transport::websocket_unified::WebSocketTransportImpl::new(config).await?;
        Ok(Box::new(transport))
    }
    
    fn transport_type(&self) -> TransportType {
        TransportType::WebSocket
    }
    
    fn default_config(&self) -> HashMap<String, String> {
        let mut config = HashMap::new();
        config.insert("local_port".to_string(), "0".to_string());
        config.insert("connection_timeout_ms".to_string(), "30000".to_string());
        config.insert("max_message_size".to_string(), "16777216".to_string()); // 16MB
        config
    }
    
    fn validate_config(&self, config: &HashMap<String, String>) -> Result<()> {
        if let Some(port_str) = config.get("local_port") {
            if port_str.parse::<u16>().is_err() {
                return Err(crate::error::EmrpError::Config(crate::error::ConfigError::ValidationFailed(
                    "Invalid port number".to_string()
                )));
            }
        }
        Ok(())
    }
}
*/

// QUIC Transport Factory (COMMENTED OUT - TO BE FIXED)
/*
pub struct QuicTransportFactory;

#[async_trait]
impl TransportFactory for QuicTransportFactory {
    async fn create_transport(&self, config: &HashMap<String, String>) -> Result<Box<dyn Transport>> {
        let transport = crate::transport::quic_unified::QuicTransportImpl::new(config).await?;
        Ok(Box::new(transport))
    }
    
    fn transport_type(&self) -> TransportType {
        TransportType::Quic
    }
    
    fn default_config(&self) -> HashMap<String, String> {
        let mut config = HashMap::new();
        config.insert("bind_address".to_string(), "0.0.0.0:0".to_string());
        config.insert("connection_timeout_ms".to_string(), "10000".to_string());
        config.insert("max_concurrent_streams".to_string(), "1000".to_string());
        config.insert("max_message_size".to_string(), "10485760".to_string()); // 10MB
        config
    }
    
    fn validate_config(&self, config: &HashMap<String, String>) -> Result<()> {
        if let Some(addr_str) = config.get("bind_address") {
            if addr_str.parse::<SocketAddr>().is_err() {
                return Err(crate::error::EmrpError::Config(crate::error::ConfigError::ValidationFailed(
                    "Invalid socket address".to_string()
                )));
            }
        }
        Ok(())
    }
}
*/

/// HTTP Transport Factory
pub struct HttpTransportFactory;

#[async_trait]
impl TransportFactory for HttpTransportFactory {
    async fn create_transport(&self, config: &HashMap<String, String>) -> Result<Box<dyn Transport>> {
        let transport = crate::transport::http_unified::HttpTransportImpl::new(config).await?;
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
                return Err(crate::error::EmrpError::Config(crate::error::ConfigError::ValidationFailed(
                    "Invalid server port number".to_string()
                )));
            }
        }
        
        // Validate timeout
        if let Some(timeout_str) = config.get("timeout_ms") {
            if timeout_str.parse::<u64>().is_err() {
                return Err(crate::error::EmrpError::Config(crate::error::ConfigError::ValidationFailed(
                    "Invalid timeout value".to_string()
                )));
            }
        }
        
        // Validate max message size
        if let Some(size_str) = config.get("max_message_size") {
            if size_str.parse::<usize>().is_err() {
                return Err(crate::error::EmrpError::Config(crate::error::ConfigError::ValidationFailed(
                    "Invalid max message size".to_string()
                )));
            }
        }
        
        Ok(())
    }
}

/// Unified Transport Manager
/// Manages multiple transport mechanisms and provides intelligent routing
pub struct UnifiedTransportManager {
    transports: HashMap<TransportType, Box<dyn Transport>>,
    target_preferences: HashMap<String, TransportType>, // Target ID -> preferred transport
    metrics_cache: DashMap<String, HashMap<TransportType, TransportEstimate>>,
    failover_policies: HashMap<TransportType, Vec<TransportType>>, // Failover order
    config: UnifiedTransportConfig,
}

/// Configuration for the unified transport manager
#[derive(Debug, Clone)]
pub struct UnifiedTransportConfig {
    pub default_transport: TransportType,
    pub enable_automatic_failover: bool,
    pub prefer_real_time_for_sync: bool,
    pub metrics_cache_seconds: u64,
    pub optimize_for_bandwidth: bool,
}

impl Default for UnifiedTransportConfig {
    fn default() -> Self {
        Self {
            default_transport: TransportType::WebSocket,
            enable_automatic_failover: true,
            prefer_real_time_for_sync: true,
            metrics_cache_seconds: 60,
            optimize_for_bandwidth: false,
        }
    }
}

impl UnifiedTransportManager {
    /// Create new transport manager
    pub async fn new(config: UnifiedTransportConfig) -> Result<Self> {
        let mut manager = Self {
            transports: HashMap::new(),
            target_preferences: HashMap::new(),
            metrics_cache: DashMap::new(),
            failover_policies: HashMap::new(),
            config,
        };
        
        // Set up default failover policies
        manager.setup_default_failover_policies();
        
        Ok(manager)
    }
    
    /// Register a transport implementation
    pub fn register_transport(&mut self, transport: Box<dyn Transport>) -> Result<()> {
        let transport_type = transport.transport_type();
        
        if self.transports.contains_key(&transport_type) {
            return Err(crate::error::EmrpError::Transport(format!("Transport already registered: {}", transport_type)));
        }
        
        self.transports.insert(transport_type, transport);
        
        Ok(())
    }
    
    /// Send a message using the best available transport
    pub async fn send_message(&self, target: &TransportTarget, message: &SecureMessage) -> Result<DeliveryReceipt> {
        // Check if we have a preferred transport for this target
        let transport_type = if let Some(preferred) = self.target_preferences.get(&target.identifier) {
            *preferred
        } else {
            // No preference, try to determine best transport
            self.determine_best_transport(target, message).await?
        };
        
        // Try to send with selected transport
        if let Some(transport) = self.transports.get(&transport_type) {
            match transport.send_message(target, message).await {
                Ok(receipt) => {
                    return Ok(receipt);
                }
                Err(err) => {
                    // Transport failed, try failover if enabled
                    if self.config.enable_automatic_failover {
                        return self.try_failover_transports(target, message, transport_type).await;
                    }
                    return Err(err);
                }
            }
        }
        
        // No suitable transport found
        Err(crate::error::EmrpError::Transport(format!("No suitable transport found for target: {}", target.identifier)))
    }
    
    /// Try to send using failover transports
    async fn try_failover_transports(
        &self, 
        target: &TransportTarget, 
        message: &SecureMessage,
        failed_transport: TransportType,
    ) -> Result<DeliveryReceipt> {
        if let Some(failover_list) = self.failover_policies.get(&failed_transport) {
            for transport_type in failover_list {
                if let Some(transport) = self.transports.get(transport_type) {
                    if transport.can_reach(target).await {
                        match transport.send_message(target, message).await {
                            Ok(receipt) => return Ok(receipt),
                            Err(_) => continue, // Try next failover transport
                        }
                    }
                }
            }
        }
        
        Err(crate::error::EmrpError::Transport(format!("All transports failed for target: {}", target.identifier)))
    }
    
    /// Determine best transport for a target and message
    async fn determine_best_transport(&self, target: &TransportTarget, _message: &SecureMessage) -> Result<TransportType> {
        // If target specifies preferred transports and we have one, use the first available
        if !target.preferred_transports.is_empty() {
            for preferred in &target.preferred_transports {
                if self.transports.contains_key(preferred) {
                    return Ok(*preferred);
                }
            }
        }
        
        // Collect metrics for all transports that can reach this target
        let mut available_transports = Vec::new();
        
        for (transport_type, transport) in &self.transports {
            if transport.can_reach(target).await {
                let metrics = transport.estimate_metrics(target).await?;
                available_transports.push((*transport_type, metrics));
            }
        }
        
        if available_transports.is_empty() {
            return Err(crate::error::EmrpError::Transport(format!("No transports can reach target: {}", target.identifier)));
        }
        
        // Sort by best metrics based on configuration and message
        available_transports.sort_by(|(_, a), (_, b)| {
            // For now, we'll use latency as the primary sorting criterion
            // since we don't have synchronous response field on SecureMessage
            // and real_time_capability field on TransportEstimate
            
            // First compare by availability
            match (a.available, b.available) {
                (true, false) => return std::cmp::Ordering::Less,
                (false, true) => return std::cmp::Ordering::Greater,
                _ => {} // Both same availability, continue with other metrics
            }
            
            // Then by reliability
            let reliability_cmp = b.reliability.partial_cmp(&a.reliability).unwrap_or(std::cmp::Ordering::Equal);
            if reliability_cmp != std::cmp::Ordering::Equal {
                return reliability_cmp;
            }
            
            // Finally by latency (lower is better)
            a.latency.partial_cmp(&b.latency).unwrap_or(std::cmp::Ordering::Equal)
        });
        
        // Return the best transport
        Ok(available_transports[0].0)
    }
    
    /// Set up default failover policies
    fn setup_default_failover_policies(&mut self) {
        // WebSocket failover
        self.failover_policies.insert(
            TransportType::WebSocket,
            vec![TransportType::Http, TransportType::Email]
        );
        
        // HTTP failover
        self.failover_policies.insert(
            TransportType::Http,
            vec![TransportType::WebSocket, TransportType::Email]
        );
        
        // Email failover (last resort)
        self.failover_policies.insert(
            TransportType::Email,
            vec![TransportType::Http, TransportType::WebSocket]
        );
        
        // QUIC failover
        self.failover_policies.insert(
            TransportType::Quic,
            vec![TransportType::WebSocket, TransportType::Http, TransportType::Email]
        );
    }
    
    /// Receive messages from all transports
    pub async fn receive_messages(&self) -> Result<Vec<IncomingMessage>> {
        let mut all_messages = Vec::new();
        
        for transport in self.transports.values() {
            match transport.receive_messages().await {
                Ok(mut messages) => all_messages.append(&mut messages),
                Err(_) => continue, // Skip transports with errors
            }
        }
        
        Ok(all_messages)
    }
    
    /// Receive messages from a specific transport
    pub async fn receive_from_transport(&self, transport_type: TransportType) -> Result<Vec<IncomingMessage>> {
        if let Some(transport) = self.transports.get(&transport_type) {
            return transport.receive_messages().await;
        }
        
        Err(crate::error::EmrpError::Transport(format!("Transport not found: {}", transport_type)))
    }
    
    /// Start all transports
    pub async fn start_all_transports(&self) -> Result<()> {
        for transport in self.transports.values() {
            if let Err(e) = transport.start().await {
                // Log the error but continue with other transports
                tracing::error!("Failed to start transport {}: {}", transport.transport_type(), e);
            }
        }
        
        Ok(())
    }
    
    /// Stop all transports
    pub async fn stop_all_transports(&self) -> Result<()> {
        for transport in self.transports.values() {
            if let Err(e) = transport.stop().await {
                // Log the error but continue stopping other transports
                tracing::error!("Failed to stop transport {}: {}", transport.transport_type(), e);
            }
        }
        
        Ok(())
    }
}

/// Convenience function to create all standard transport factories
pub fn create_standard_factories() -> Vec<Box<dyn TransportFactory>> {
    vec![
        Box::new(TcpTransportFactory),
        Box::new(super::udp_unified::UdpTransportFactory),
        // TODO: Re-enable other transports once they're fixed
        // Box::new(EmailTransportFactory),
        // Box::new(MdnsTransportFactory),
        // Box::new(WebSocketTransportFactory),
        // Box::new(QuicTransportFactory),
        Box::new(HttpTransportFactory),
    ]
}
