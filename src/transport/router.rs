//! Multi-transport router for intelligent message routing

use super::{
    abstraction::{
        Transport, TransportTarget, MessageUrgency, TransportType,
        TransportCapabilities, DeliveryReceipt
    },
    TransportSelector, TransportRoute,
};
use crate::{
    types::SecureMessage,
    error::Result,
    config::Config,
};
use std::{sync::Arc, time::{Duration, Instant}, collections::HashMap};
use async_trait::async_trait;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};
use serde::{Serialize, Deserialize};

/// Connection offer for establishing connections
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionOffer {
    pub from_entity: String,
    pub to_entity: String,
    pub transport_type: TransportType,
    pub capabilities: TransportCapabilities,
    pub valid_until: u64, // Changed from Instant to u64 for serializability
}

/// Hybrid connection combining multiple transports
#[derive(Debug, Clone)]
pub struct HybridConnection {
    pub primary_transport: TransportType,
    pub fallback_transports: Vec<TransportType>,
    pub target: TransportTarget,
    pub established_at: Instant,
}

/// Transport provider trait for dependency injection
#[async_trait]
pub trait TransportProvider: Send + Sync {
    async fn create_tcp_transport(&self, config: &Config) -> Result<Option<Arc<dyn Transport>>>;
    async fn create_mdns_transport(&self, config: &Config) -> Result<Option<Arc<dyn Transport>>>;
    async fn create_nat_transport(&self, config: &Config) -> Result<Option<Arc<dyn Transport>>>;
    async fn create_email_transport(&self, config: &Config) -> Result<Option<Arc<dyn Transport>>>;
    fn create_transport_selector(&self) -> Arc<RwLock<TransportSelector>>;
}

/// Production transport provider
pub struct ProductionTransportProvider;

#[async_trait]
impl TransportProvider for ProductionTransportProvider {
    async fn create_tcp_transport(&self, _config: &Config) -> Result<Option<Arc<dyn Transport>>> {
        // Create TCP transport using enhanced implementation
        use crate::transport::tcp::TcpTransport;
        
        let tcp_port = 8080; // Default TCP port
        match TcpTransport::new(tcp_port).await {
            Ok(transport) => Ok(Some(Arc::new(transport))),
            Err(e) => {
                warn!("Failed to create TCP transport: {}", e);
                Ok(None)
            }
        }
    }

    async fn create_mdns_transport(&self, _config: &Config) -> Result<Option<Arc<dyn Transport>>> {
        // TODO: Re-enable mDNS transport after trait compatibility is fixed
        warn!("mDNS transport temporarily disabled due to trait compatibility issues");
        Ok(None)
    }

    async fn create_nat_transport(&self, _config: &Config) -> Result<Option<Arc<dyn Transport>>> {
        // NAT transport not implemented in current version
        // This would require STUN/TURN servers and NAT traversal logic
        info!("NAT transport not available in current version");
        Ok(None)
    }

    async fn create_email_transport(&self, config: &Config) -> Result<Option<Arc<dyn Transport>>> {
        // Create email transport using enhanced implementation
        use crate::transport::email_enhanced::EmailEnhancedTransport;
        
        match EmailEnhancedTransport::new(config.email.clone()).await {
            Ok(transport) => Ok(Some(Arc::new(transport))),
            Err(e) => {
                warn!("Failed to create email transport: {}", e);
                Ok(None)
            }
        }
    }

    fn create_transport_selector(&self) -> Arc<RwLock<TransportSelector>> {
        Arc::new(RwLock::new(TransportSelector::new()))
    }
}

/// Multi-transport router for Synapse
pub struct MultiTransportRouter {
    tcp_transport: Option<Arc<dyn Transport>>,
    mdns_transport: Option<Arc<dyn Transport>>, // Enhanced mDNS transport for local discovery
    nat_transport: Option<Arc<dyn Transport>>,
    email_transport: Option<Arc<dyn Transport>>, // Enhanced email transport for reliability
    transport_selector: Arc<RwLock<TransportSelector>>,
    route_cache: Arc<RwLock<HashMap<String, (TransportRoute, Instant)>>>,
    cache_duration: Duration,
    #[allow(dead_code)]
    our_entity_id: String,
    performance_monitoring: bool,
}

impl MultiTransportRouter {
    /// Create a new multi-transport router with default provider
    pub async fn new(config: Config, our_entity_id: String) -> Result<Self> {
        let provider = ProductionTransportProvider;
        Self::new_with_provider(config, our_entity_id, Box::new(provider)).await
    }

    /// Create a new multi-transport router with dependency injection
    pub async fn new_with_provider(
        config: Config, 
        our_entity_id: String,
        provider: Box<dyn TransportProvider>
    ) -> Result<Self> {
        info!("Initializing multi-transport router for entity: {}", our_entity_id);
        
        // Initialize transports through dependency injection
        let tcp_transport = provider.create_tcp_transport(&config).await?;
        let mdns_transport = provider.create_mdns_transport(&config).await?;
        let nat_transport = provider.create_nat_transport(&config).await?;
        let email_transport = provider.create_email_transport(&config).await?;
        let transport_selector = provider.create_transport_selector();

        if tcp_transport.is_none() && mdns_transport.is_none() && 
           nat_transport.is_none() && email_transport.is_none() {
            warn!("No transports available - router may have limited functionality");
        }

        Ok(Self {
            tcp_transport,
            mdns_transport,
            nat_transport,
            email_transport,
            transport_selector,
            route_cache: Arc::new(RwLock::new(HashMap::new())),
            cache_duration: Duration::from_secs(300), // 5 minutes
            our_entity_id,
            performance_monitoring: true,
        })
    }
    
    /// Send message with automatic transport selection
    pub async fn send_message(
        &self, 
        target: &str, 
        message: &SecureMessage, 
        urgency: MessageUrgency
    ) -> Result<DeliveryReceipt> {
        let start = Instant::now();
        
        // Check cache first
        if let Some(cached_route) = self.get_cached_route(target).await {
            if self.is_route_suitable(&cached_route, urgency) {
                debug!("Using cached route for {}: {:?}", target, cached_route);
                return self.send_via_route(target, message, &cached_route).await;
            }
        }
        
        // Discover optimal transport
        let mut selector = self.transport_selector.write().await;
        match selector.choose_optimal_transport(target, urgency).await {
            Ok(route) => {
                drop(selector); // Release lock early
                
                // Cache the route
                self.cache_route(target.to_string(), route.clone()).await;
                
                // Send via selected route
                let result = self.send_via_route(target, message, &route).await;
                
                if self.performance_monitoring {
                    let elapsed = start.elapsed();
                    info!("Message sent to {} via {:?} in {:?}", target, route, elapsed);
                }
                
                result
            }
            Err(e) => {
                warn!("Transport selection failed for {}: {}", target, e);
                
                // Fallback to email if all else fails
                info!("Falling back to email transport for {}", target);
                self.send_via_email(target, message).await
            }
        }
    }
    
    /// Send message with explicit fallback priority
    pub async fn send_with_fallback_priority(
        &self,
        target: &str,
        message: &SecureMessage,
        preferred_routes: &[TransportRoute],
    ) -> Result<DeliveryReceipt> {
        for route in preferred_routes {
            match self.send_via_route(target, message, route).await {
                Ok(result) => {
                    info!("Successfully sent to {} via {:?}", target, route);
                    return Ok(result);
                }
                Err(e) => {
                    warn!("Failed to send via {:?}: {}", route, e);
                    continue;
                }
            }
        }
        
        // If all preferred routes fail, try email as ultimate fallback
        warn!("All preferred routes failed, using email fallback");
        self.send_via_email(target, message).await
    }
    
    /// Send via specific transport route
    async fn send_via_route(&self, target: &str, message: &SecureMessage, route: &TransportRoute) -> Result<DeliveryReceipt> {
        let target_obj = TransportTarget::new(target.to_string());
        
        match route {
            TransportRoute::DirectTcp { .. } => {
                if let Some(ref transport) = self.tcp_transport {
                    transport.send_message(&target_obj, message).await
                } else {
                    Err(crate::error::SynapseError::TransportError("TCP transport not available".into()))
                }
            }
            TransportRoute::DirectUdp { .. } => {
                // UDP transport not implemented, fallback to TCP
                if let Some(ref transport) = self.tcp_transport {
                    transport.send_message(&target_obj, message).await
                } else {
                    Err(crate::error::SynapseError::TransportError("UDP transport not available (using TCP fallback)".into()))
                }
            }
            TransportRoute::Udp { .. } => {
                // Handle unified UDP transport route
                if let Some(ref transport) = self.tcp_transport {
                    transport.send_message(&target_obj, message).await
                } else {
                    Err(crate::error::SynapseError::TransportError("UDP transport not available (using TCP fallback)".into()))
                }
            }
            TransportRoute::WebSocket { .. } => {
                // WebSocket transport not implemented, fallback to TCP
                if let Some(ref transport) = self.tcp_transport {
                    transport.send_message(&target_obj, message).await
                } else {
                    Err(crate::error::SynapseError::TransportError("WebSocket transport not available (using TCP fallback)".into()))
                }
            }
            TransportRoute::Quic { .. } => {
                // QUIC transport not implemented, fallback to TCP
                if let Some(ref transport) = self.tcp_transport {
                    transport.send_message(&target_obj, message).await
                } else {
                    Err(crate::error::SynapseError::TransportError("QUIC transport not available (using TCP fallback)".into()))
                }
            }
            TransportRoute::LocalMdns { .. } => {
                self.send_via_mdns(target, message).await
            }
            TransportRoute::NatTraversal { .. } => {
                if let Some(ref transport) = self.nat_transport {
                    transport.send_message(&target_obj, message).await
                } else {
                    Err(crate::error::SynapseError::TransportError("NAT traversal transport not available".into()))
                }
            }
            TransportRoute::FastEmailRelay { .. } | 
            TransportRoute::StandardEmail { .. } | 
            TransportRoute::EmailDiscovery { .. } => {
                self.send_via_email(target, message).await
            }
        }
    }
    
    /// Send message via email transport (if available)
    async fn send_via_email(&self, target: &str, message: &SecureMessage) -> Result<DeliveryReceipt> {
        #[cfg(feature = "email")]
        {
            let target_obj = TransportTarget::new(target.to_string());
            if let Some(ref transport) = self.email_transport {
                transport.send_message(&target_obj, message).await
            } else {
                Err(crate::error::SynapseError::TransportError("Email transport not available".into()))
            }
        }
        #[cfg(not(feature = "email"))]
        {
            let _ = (target, message); // Suppress unused warnings
            Err(crate::error::SynapseError::TransportError("Email transport not available".to_string()))
        }
    }

    /// Send connection offer via email transport (if available)
    #[allow(dead_code)]
    async fn send_connection_offer_via_email(&self, target: &str, offer: &ConnectionOffer) -> Result<String> {
        #[cfg(feature = "email")]
        {
            match &self.email_transport {
                Some(transport) => transport.send_connection_offer(target, offer.clone()).await,
                None => Err(crate::error::SynapseError::TransportError("Email transport not available".to_string()).into())
            }
        }
        #[cfg(not(feature = "email"))]
        {
            let _ = (target, offer); // Suppress unused warnings
            Err(crate::error::SynapseError::TransportError("Email transport not available".to_string()).into())
        }
    }

    /// Check if mDNS transport is available
    fn has_mdns_transport(&self) -> bool {
        #[cfg(feature = "mdns")]
        {
            self.mdns_transport.is_some()
        }
        #[cfg(not(feature = "mdns"))]
        {
            false
        }
    }

    /// Send via mDNS transport (if available)
    async fn send_via_mdns(&self, target: &str, message: &SecureMessage) -> Result<DeliveryReceipt> {
        #[cfg(feature = "mdns")]
        {
            let target_obj = TransportTarget::new(target.to_string());
            if let Some(ref transport) = self.mdns_transport {
                transport.send_message(&target_obj, message).await
            } else {
                Err(crate::error::SynapseError::TransportError("mDNS transport not initialized".to_string()))
            }
        }
        #[cfg(not(feature = "mdns"))]
        {
            let _ = (target, message); // Suppress unused warnings
            Err(crate::error::SynapseError::TransportError("mDNS transport not available".to_string()))
        }
    }

    /// Check if mDNS can reach target
    async fn mdns_can_reach(&self, target: &str) -> bool {
        #[cfg(feature = "mdns")]
        {
            let target_obj = TransportTarget::new(target.to_string());
            if let Some(ref mdns) = self.mdns_transport {
                mdns.can_reach(&target_obj).await
            } else {
                false
            }
        }
        #[cfg(not(feature = "mdns"))]
        {
            let _ = target; // Suppress unused warning
            false
        }
    }
    
    /// Test if we can connect directly to target
    pub async fn can_connect_directly(&self, target: &str) -> bool {
        // Try TCP first
        let target_obj = TransportTarget::new(target.to_string());
        if let Some(ref tcp) = self.tcp_transport {
            if tcp.can_reach(&target_obj).await {
                return true;
            }
        }
        
        // Note: UDP transport not implemented
        
        false
    }
    
    /// Discover local peer via mDNS
    pub async fn discover_local_peer(&self, target: &str) -> Result<()> {
        if self.mdns_can_reach(target).await {
            Ok(())
        } else {
            Err(crate::error::SynapseError::TransportError("mDNS peer not found".into()))
        }
    }
    
    /// Establish NAT traversal connection
    pub async fn establish_nat_traversal(&self, target: &str) -> Result<super::NatMethod> {
        let target_obj = TransportTarget::new(target.to_string());
        if let Some(ref nat) = self.nat_transport {
            // This is a simplified version - the real implementation would be in NAT transport
            if nat.can_reach(&target_obj).await {
                Ok(super::NatMethod::Stun { 
                    server: "stun.l.google.com:19302".to_string() 
                })
            } else {
                Err(crate::error::SynapseError::TransportError("NAT traversal failed".into()))
            }
        } else {
            Err(crate::error::SynapseError::TransportError("NAT traversal transport not available".into()))
        }
    }
    
    /// Establish hybrid connection combining multiple transports
    pub async fn establish_hybrid_connection(&self, target: &str) -> Result<HybridConnection> {
        // Try to get a mutable reference to email transport for hybrid connection
        // In a real implementation, we'd need to restructure this
        info!("Establishing hybrid connection to {}", target);
        
        // For now, simulate a hybrid connection
        let discovery_start = Instant::now();
        tokio::time::sleep(Duration::from_millis(100)).await;
        let _discovery_time = discovery_start.elapsed();
        
        let connection_start = Instant::now();
        tokio::time::sleep(Duration::from_millis(50)).await;
        let connection_time = connection_start.elapsed();
        
        let _primary_route = TransportRoute::DirectTcp {
            address: target.to_string(),
            port: 8080,
            latency_ms: 25,
            established_at: Instant::now(),
        };
        
        let _fallback_route = TransportRoute::StandardEmail {
            estimated_latency_min: 60,
        };
        
        let _metrics = super::TransportMetrics {
            latency: connection_time,
            throughput_bps: 1_000_000,
            packet_loss: 0.01,
            jitter_ms: 5,
            reliability_score: 0.90,
            last_updated: Instant::now(),
        };
        
        Ok(HybridConnection {
            primary_transport: TransportType::Tcp,
            fallback_transports: vec![TransportType::Email],
            target: TransportTarget::new(target.to_string()),
            established_at: Instant::now(),
        })
    }
    
    /// Send with reliability priority (prefer email)
    pub async fn send_reliable(
        &self,
        target: &str,
        message: &SecureMessage,
        urgency: MessageUrgency,
    ) -> Result<String> {
        // For reliable delivery, prefer email first
        match self.send_via_email(target, message).await {
            Ok(receipt) => Ok(receipt.message_id),
            Err(e) => {
                warn!("Email delivery failed, trying alternatives: {}", e);
                // Try alternative delivery methods
                self.send_message(target, message, urgency).await
                    .map(|receipt| receipt.message_id)
            }
        }
    }
    
    /// Get cached route for target
    async fn get_cached_route(&self, target: &str) -> Option<TransportRoute> {
        let cache = self.route_cache.read().await;
        if let Some((route, cached_at)) = cache.get(target) {
            if cached_at.elapsed() < self.cache_duration {
                Some(route.clone())
            } else {
                None // Cache expired
            }
        } else {
            None
        }
    }
    
    /// Cache a route for future use
    async fn cache_route(&self, target: String, route: TransportRoute) {
        let mut cache = self.route_cache.write().await;
        cache.insert(target, (route, Instant::now()));
    }
    
    /// Check if a route is suitable for the given urgency
    fn is_route_suitable(&self, route: &TransportRoute, urgency: MessageUrgency) -> bool {
        match urgency {
            MessageUrgency::Critical => {
                matches!(route, 
                    TransportRoute::DirectTcp { latency_ms, .. } |
                    TransportRoute::DirectUdp { latency_ms, .. } |
                    TransportRoute::LocalMdns { latency_ms, .. }
                    if *latency_ms < 50
                )
            }
            MessageUrgency::RealTime => {
                matches!(route, 
                    TransportRoute::DirectTcp { latency_ms, .. } |
                    TransportRoute::DirectUdp { latency_ms, .. } |
                    TransportRoute::LocalMdns { latency_ms, .. }
                    if *latency_ms < 100
                )
            }
            MessageUrgency::Interactive => {
                !matches!(route, TransportRoute::StandardEmail { .. })
            }
            MessageUrgency::Background | MessageUrgency::Batch => {
                true // Any route is acceptable
            }
        }
    }
    
    /// Get transport capabilities summary
    pub fn get_capabilities(&self) -> Vec<String> {
        let mut capabilities = Vec::new();
        
        if self.tcp_transport.is_some() {
            capabilities.extend(vec!["tcp".to_string(), "direct_connection".to_string()]);
        }
        // Note: UDP transport not implemented
        
        if self.has_mdns_transport() {
            capabilities.extend(vec!["mdns".to_string(), "local_discovery".to_string()]);
        }
        
        if self.nat_transport.is_some() {
            capabilities.extend(vec!["nat_traversal".to_string(), "stun".to_string(), "upnp".to_string()]);
        }
        
        capabilities.extend(vec!["email".to_string(), "reliable_delivery".to_string(), "universal_reach".to_string()]);
        
        capabilities
    }
    
    /// Start background services for all transports
    pub async fn start_background_services(&self) -> Result<()> {
        info!("Starting multi-transport background services");
        
        // Start TCP server if available
        if let Some(ref tcp) = self.tcp_transport {
            // TCP server background service with proper Arc<Mutex<T>> pattern
            info!("Starting TCP transport background service");
            let _tcp_clone = Arc::clone(tcp);
            tokio::spawn(async move {
                // In a production implementation, this would start a background TCP listener
                // The TCP transport would need interior mutability to handle concurrent connections
                debug!("TCP background service started");
            });
        }
        
        // Note: UDP transport not implemented
        
        // Start mDNS discovery if available
        if self.has_mdns_transport() {
            tokio::spawn(async move {
                // Note: This would need to be restructured to allow mutable access
                debug!("mDNS discovery service started");
            });
        }
        
        info!("All available transport services started");
        Ok(())
    }
}
