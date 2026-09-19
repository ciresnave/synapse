// SPDX-License-Identifier: MIT OR Apache-2.0
//! Unified Transport Abstraction Layer for Synapse
//!
//! This module provides a unified interface that sits above all transport mechanisms,
//! making applications transport-agnostic while providing intelligent transport selection,
//! failover, and optimization capabilities.

use super::router::ConnectionOffer;
use crate::{error::Result, types::SecureMessage};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    net::SocketAddr,
    time::{Duration, Instant},
};

/// Sealing machinery for [`TransportReceive::receive_raw`].
///
/// The module and the `Token` type are both `pub`: the type must be *nameable* from anywhere
/// (downstream crates need to spell it out in `impl TransportReceive for MyTransport`, and
/// sibling modules inside this crate such as `manager.rs` need to name it too). What makes the
/// token "sealed" is that it cannot be *constructed* anywhere but here: its single field is a
/// private (non-`pub`) unit `()`, so no other module can write the struct literal `Token(())`,
/// and its only constructor, `new`, is `pub(crate)` -- visible within this crate, invisible to
/// downstream crates. A downstream implementor receives a `Token` as a parameter and can ignore
/// it, but has no way to produce one to call `receive_raw` itself. This is the standard "sealed
/// trait" / "private token" pattern: the trait is public to *implement*, private to *call*.
pub mod private {
    /// Nameable everywhere; constructible only inside this crate (private field, `pub(crate)`
    /// constructor).
    #[derive(Debug)]
    pub struct Token(());

    impl Token {
        pub(crate) fn new() -> Self {
            Token(())
        }
    }
}

/// Raw, unverified receive -- callable only by [`TransportManager`], which pairs every message
/// with a `SenderVerdict` before handing it to applications. See the module-level docs on
/// [`private`] for how the sealing works.
#[async_trait]
pub trait TransportReceive: Send + Sync {
    /// Receive raw messages from the wire.
    ///
    /// ⚠️ Unverified: senders are not authenticated here. `TransportManager::receive_messages`
    /// pairs each message with a `SenderVerdict`; that is the only public way to receive from a
    /// transport.
    ///
    /// The `token` parameter exists only to make this method uncallable from outside this crate:
    /// `private::Token` cannot be constructed anywhere else. Implementors accept and ignore it.
    ///
    /// A downstream crate can implement this trait for its own transport:
    /// ```
    /// use async_trait::async_trait;
    /// use synapse::error::Result;
    /// use synapse::transport::{IncomingMessage, TransportReceive};
    ///
    /// struct MyTransport;
    ///
    /// #[async_trait]
    /// impl TransportReceive for MyTransport {
    ///     async fn receive_raw(
    ///         &self,
    ///         _token: synapse::transport::abstraction::private::Token,
    ///     ) -> Result<Vec<IncomingMessage>> {
    ///         Ok(Vec::new())
    ///     }
    /// }
    /// # fn main() {}
    /// ```
    ///
    /// But it cannot call `receive_raw` itself, because it cannot construct a `Token`:
    /// ```compile_fail
    /// use synapse::transport::{IncomingMessage, TransportReceive};
    ///
    /// async fn call_it(t: &dyn TransportReceive) {
    ///     // No public constructor for `Token` exists outside this crate, so this cannot compile.
    ///     let _ = t.receive_raw(synapse::transport::abstraction::private::Token::new()).await;
    /// }
    /// # fn main() {}
    /// ```
    async fn receive_raw(&self, token: private::Token) -> Result<Vec<IncomingMessage>>;
}

/// Unified transport interface that all transport mechanisms must implement
#[async_trait]
pub trait Transport: TransportReceive + Send + Sync {
    /// Get the transport type identifier
    fn transport_type(&self) -> TransportType;

    /// Get transport capabilities
    fn capabilities(&self) -> TransportCapabilities;

    /// Check if this transport can reach a specific target
    async fn can_reach(&self, target: &TransportTarget) -> bool;

    /// Get estimated metrics for reaching a target
    async fn estimate_metrics(&self, target: &TransportTarget) -> Result<TransportEstimate>;

    /// Send a message via this transport
    async fn send_message(
        &self,
        target: &TransportTarget,
        message: &SecureMessage,
    ) -> Result<DeliveryReceipt>;

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

    /// Send a connection offer to a target
    async fn send_connection_offer(&self, target: &str, offer: ConnectionOffer) -> Result<String> {
        let _ = (target, offer);
        Err(crate::error::SynapseError::TransportError(
            "Connection offers not supported by this transport".to_string(),
        ))
    }
}

/// Transport types supported by the system
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TransportType {
    Tcp,
    Udp,
    WebSocket,
    Http,
    Email,
    AutoDiscovery, // Replaces Mdns with more comprehensive discovery
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
            TransportType::AutoDiscovery => write!(f, "Auto-Discovery"),
            TransportType::Quic => write!(f, "QUIC"),
            TransportType::Custom(id) => write!(f, "Custom({id})"),
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
    /// The receiving application acknowledged the message with a verified, signed ack whose
    /// digest matches what was sent (P2 slice b).
    Acknowledged,
    /// No acknowledgement arrived within the tracking window (P2 slice e). It says nothing about
    /// whether the message was delivered.
    Expired,
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
            supported_urgencies: vec![MessageUrgency::Background, MessageUrgency::Batch],
            features: vec![
                "store_and_forward".to_string(),
                "federation".to_string(),
                "authentication".to_string(),
                "persistent".to_string(),
            ],
        }
    }

    /// Auto-Discovery transport capabilities
    pub fn auto_discovery() -> Self {
        Self {
            max_message_size: 1024, // Smallest for discovery
            reliable: false,
            real_time: true,
            broadcast: true,
            bidirectional: true,
            encrypted: false,
            network_spanning: false, // Local network only
            supported_urgencies: vec![MessageUrgency::Critical, MessageUrgency::RealTime],
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
    async fn create_transport(
        &self,
        config: &HashMap<String, String>,
    ) -> Result<Box<dyn Transport>>;

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
    async fn create_transport(
        &self,
        _config: &HashMap<String, String>,
    ) -> Result<Box<dyn Transport>> {
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
        if let Some(port_str) = config.get("listen_port")
            && port_str.parse::<u16>().is_err()
        {
            return Err(crate::error::SynapseError::Config(
                "Invalid port number".to_string(),
            ));
        }
        Ok(())
    }
}

/// UDP Transport Factory (temporarily disabled)
pub struct UdpTransportFactory;

#[async_trait]
impl TransportFactory for UdpTransportFactory {
    async fn create_transport(
        &self,
        config: &HashMap<String, String>,
    ) -> Result<Box<dyn Transport>> {
        // Remove SocketAddr creation, pass config directly
        let transport = crate::transport::udp_unified::UdpTransportImpl::new(config).await?;
        Ok(Box::new(transport) as Box<dyn Transport>)
    }

    fn transport_type(&self) -> TransportType {
        TransportType::Udp
    }

    fn default_config(&self) -> HashMap<String, String> {
        let mut cfg = HashMap::new();
        cfg.insert("listen_port".to_string(), "9000".to_string());
        cfg
    }

    fn validate_config(&self, config: &HashMap<String, String>) -> Result<()> {
        if let Some(port_str) = config.get("listen_port")
            && port_str.parse::<u16>().is_err()
        {
            return Err(crate::error::SynapseError::Config(
                "Invalid port number".to_string(),
            ));
        }
        Ok(())
    }
}

/// Email Transport Factory (temporarily disabled)
pub struct EmailTransportFactory;

#[async_trait]
impl TransportFactory for EmailTransportFactory {
    async fn create_transport(
        &self,
        _config: &HashMap<String, String>,
    ) -> Result<Box<dyn Transport>> {
        Err(crate::error::SynapseError::TransportError(
            "Email transport not yet implemented".to_string(),
        ))
    }

    fn transport_type(&self) -> TransportType {
        TransportType::Email
    }

    fn default_config(&self) -> HashMap<String, String> {
        HashMap::new()
    }

    fn validate_config(&self, _config: &HashMap<String, String>) -> Result<()> {
        Ok(())
    }
}
// }

// mDNS Transport Factory (COMMENTED OUT - TO BE FIXED)
pub struct MdnsTransportFactory;

#[async_trait]
impl TransportFactory for MdnsTransportFactory {
    async fn create_transport(
        &self,
        _config: &HashMap<String, String>,
    ) -> Result<Box<dyn Transport>> {
        use crate::transport::mdns_enhanced::{EnhancedMdnsTransport, MdnsConfig};
        let entity_id = "mdns-factory-instance".to_string();
        let local_port = 5353;
        let config = Some(MdnsConfig::default());
        match EnhancedMdnsTransport::new(entity_id, local_port, config).await {
            Ok(transport) => Ok(Box::new(transport) as Box<dyn Transport>),
            Err(e) => Err(crate::error::SynapseError::TransportError(format!(
                "Failed to create mDNS transport: {e}"
            ))),
        }
    }

    fn transport_type(&self) -> TransportType {
        TransportType::AutoDiscovery
    }

    fn default_config(&self) -> HashMap<String, String> {
        let mut config = HashMap::new();
        config.insert(
            "service_name".to_string(),
            "_synapse._tcp.local".to_string(),
        );
        config.insert("local_port".to_string(), "0".to_string());
        config.insert("discovery_timeout_ms".to_string(), "5000".to_string());
        config.insert("max_message_size".to_string(), "65507".to_string()); // Max UDP
        config
    }

    fn validate_config(&self, config: &HashMap<String, String>) -> Result<()> {
        if let Some(service_name) = config.get("service_name")
            && !service_name.contains("._tcp.")
            && !service_name.contains("._udp.")
        {
            return Err(crate::error::SynapseError::Config(
                "Service name must include protocol (_tcp. or _udp.)".to_string(),
            ));
        }
        Ok(())
    }
}

// WebSocket Transport Factory (RE-ENABLED)
pub struct WebSocketTransportFactory;

#[async_trait]
impl TransportFactory for WebSocketTransportFactory {
    async fn create_transport(
        &self,
        config: &HashMap<String, String>,
    ) -> Result<Box<dyn Transport>> {
        let transport =
            crate::transport::websocket_unified::WebSocketTransportImpl::new(config).await?;
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
        if let Some(port_str) = config.get("local_port")
            && port_str.parse::<u16>().is_err()
        {
            return Err(crate::error::SynapseError::Config(
                "Invalid port number".to_string(),
            ));
        }
        Ok(())
    }
}

// QUIC Transport Factory (RE-ENABLED)
pub struct QuicTransportFactory;

#[async_trait]
impl TransportFactory for QuicTransportFactory {
    async fn create_transport(
        &self,
        _config: &HashMap<String, String>,
    ) -> Result<Box<dyn Transport>> {
        Err(crate::error::SynapseError::TransportError(
            "QUIC is not implemented yet; it arrives in the QUIC slice".to_string(),
        ))
    }

    fn transport_type(&self) -> TransportType {
        TransportType::Quic
    }

    fn default_config(&self) -> HashMap<String, String> {
        let mut config = HashMap::new();
        config.insert(
            "bind_address".to_string(),
            crate::network_scope::BindScope::Loopback
                .listen_addr(0)
                .to_string(),
        );
        config.insert("connection_timeout_ms".to_string(), "10000".to_string());
        config.insert("max_concurrent_streams".to_string(), "1000".to_string());
        config.insert("max_message_size".to_string(), "10485760".to_string()); // 10MB
        config
    }

    fn validate_config(&self, config: &HashMap<String, String>) -> Result<()> {
        if let Some(addr_str) = config.get("bind_address")
            && addr_str.parse::<SocketAddr>().is_err()
        {
            return Err(crate::error::SynapseError::Config(
                "Invalid socket address".to_string(),
            ));
        }
        Ok(())
    }
}

/// HTTP Transport Factory
// Removed unexpected cfg condition
pub struct HttpTransportFactory;

// Removed unexpected cfg condition
#[async_trait]
impl TransportFactory for HttpTransportFactory {
    async fn create_transport(
        &self,
        config: &HashMap<String, String>,
    ) -> Result<Box<dyn Transport>> {
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
        config.insert(
            "user_agent".to_string(),
            "Synapse-HTTP-Transport/1.0".to_string(),
        );
        config
    }

    fn validate_config(&self, config: &HashMap<String, String>) -> Result<()> {
        // Validate server port
        if let Some(port_str) = config.get("server_port")
            && port_str.parse::<u16>().is_err()
        {
            return Err(crate::error::SynapseError::Config(
                "Invalid server port number".to_string(),
            ));
        }

        // Validate timeout
        if let Some(timeout_str) = config.get("timeout_ms")
            && timeout_str.parse::<u64>().is_err()
        {
            return Err(crate::error::SynapseError::Config(
                "Invalid timeout value".to_string(),
            ));
        }

        // Validate max message size
        if let Some(size_str) = config.get("max_message_size")
            && size_str.parse::<usize>().is_err()
        {
            return Err(crate::error::SynapseError::Config(
                "Invalid max message size".to_string(),
            ));
        }

        Ok(())
    }
}
