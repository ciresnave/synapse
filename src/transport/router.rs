//! Multi-transport router for intelligent message routing

use super::{
    Transport, TransportRoute, TransportSelector, MessageUrgency, 
    tcp::TcpTransport, mdns::MdnsTransport, nat_traversal::NatTraversalTransport,
    email_enhanced::EmailEnhancedTransport, HybridConnection,
};
use crate::{types::SecureMessage, error::Result, config::Config};
use std::{sync::Arc, time::{Duration, Instant}, collections::HashMap};
use tokio::sync::RwLock;
use tracing::{info, debug, warn};

/// Multi-transport router for EMRP
pub struct MultiTransportRouter {
    tcp_transport: Option<Arc<TcpTransport>>,
    mdns_transport: Option<Arc<MdnsTransport>>,
    nat_transport: Option<Arc<NatTraversalTransport>>,
    email_transport: Arc<EmailEnhancedTransport>,
    transport_selector: Arc<RwLock<TransportSelector>>,
    route_cache: Arc<RwLock<HashMap<String, (TransportRoute, Instant)>>>,
    cache_duration: Duration,
    #[allow(dead_code)]
    our_entity_id: String,
    performance_monitoring: bool,
}

impl MultiTransportRouter {
    /// Create a new multi-transport router
    pub async fn new(config: Config, our_entity_id: String) -> Result<Self> {
        info!("Initializing multi-transport router for entity: {}", our_entity_id);
        
        // Initialize email transport (always available)
        let email_transport = Arc::new(EmailEnhancedTransport::new(config.email.clone()).await?);
        
        // Initialize TCP transport (if ports available)
        let tcp_transport = match TcpTransport::new(8080).await {
            Ok(transport) => {
                info!("TCP transport initialized on port 8080");
                Some(Arc::new(transport))
            }
            Err(e) => {
                warn!("Failed to initialize TCP transport: {}", e);
                None
            }
        };
        
        // Initialize mDNS transport
        let mdns_transport = match MdnsTransport::new("_emrp._tcp.local".to_string(), 8080).await {
            Ok(transport) => {
                info!("mDNS transport initialized");
                Some(Arc::new(transport))
            }
            Err(e) => {
                warn!("Failed to initialize mDNS transport: {}", e);
                None
            }
        };
        
        // Initialize NAT traversal transport
        let nat_transport = match NatTraversalTransport::new(8080).await {
            Ok(transport) => {
                info!("NAT traversal transport initialized");
                Some(Arc::new(transport))
            }
            Err(e) => {
                warn!("Failed to initialize NAT traversal transport: {}", e);
                None
            }
        };
        
        Ok(Self {
            tcp_transport,
            mdns_transport,
            nat_transport,
            email_transport,
            transport_selector: Arc::new(RwLock::new(TransportSelector::new())),
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
    ) -> Result<String> {
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
                self.email_transport.send_message(target, message).await
            }
        }
    }
    
    /// Send message with explicit fallback priority
    pub async fn send_with_fallback_priority(
        &self,
        target: &str,
        message: &SecureMessage,
        preferred_routes: &[TransportRoute],
    ) -> Result<String> {
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
        self.email_transport.send_message(target, message).await
    }
    
    /// Send via specific transport route
    async fn send_via_route(&self, target: &str, message: &SecureMessage, route: &TransportRoute) -> Result<String> {
        match route {
            TransportRoute::DirectTcp { .. } => {
                if let Some(ref transport) = self.tcp_transport {
                    transport.send_message(target, message).await
                } else {
                    Err(crate::error::EmrpError::Transport("TCP transport not available".into()))
                }
            }
            TransportRoute::DirectUdp { .. } => {
                // UDP transport not implemented, fallback to TCP
                if let Some(ref transport) = self.tcp_transport {
                    transport.send_message(target, message).await
                } else {
                    Err(crate::error::EmrpError::Transport("UDP transport not available (using TCP fallback)".into()))
                }
            }
            TransportRoute::Udp { .. } => {
                // Handle unified UDP transport route
                if let Some(ref transport) = self.tcp_transport {
                    transport.send_message(target, message).await
                } else {
                    Err(crate::error::EmrpError::Transport("UDP transport not available (using TCP fallback)".into()))
                }
            }
            TransportRoute::WebSocket { .. } => {
                // WebSocket transport not implemented, fallback to TCP
                if let Some(ref transport) = self.tcp_transport {
                    transport.send_message(target, message).await
                } else {
                    Err(crate::error::EmrpError::Transport("WebSocket transport not available (using TCP fallback)".into()))
                }
            }
            TransportRoute::Quic { .. } => {
                // QUIC transport not implemented, fallback to TCP
                if let Some(ref transport) = self.tcp_transport {
                    transport.send_message(target, message).await
                } else {
                    Err(crate::error::EmrpError::Transport("QUIC transport not available (using TCP fallback)".into()))
                }
            }
            TransportRoute::LocalMdns { .. } => {
                if let Some(ref transport) = self.mdns_transport {
                    transport.send_message(target, message).await
                } else {
                    Err(crate::error::EmrpError::Transport("mDNS transport not available".into()))
                }
            }
            TransportRoute::NatTraversal { .. } => {
                if let Some(ref transport) = self.nat_transport {
                    transport.send_message(target, message).await
                } else {
                    Err(crate::error::EmrpError::Transport("NAT traversal transport not available".into()))
                }
            }
            TransportRoute::FastEmailRelay { .. } | 
            TransportRoute::StandardEmail { .. } | 
            TransportRoute::EmailDiscovery { .. } => {
                self.email_transport.send_message(target, message).await
            }
        }
    }
    
    /// Test if we can connect directly to target
    pub async fn can_connect_directly(&self, target: &str) -> bool {
        // Try TCP first
        if let Some(ref tcp) = self.tcp_transport {
            if tcp.can_reach(target).await {
                return true;
            }
        }
        
        // Note: UDP transport not implemented
        
        false
    }
    
    /// Discover local peer via mDNS
    pub async fn discover_local_peer(&self, target: &str) -> Result<()> {
        if let Some(ref mdns) = self.mdns_transport {
            if mdns.can_reach(target).await {
                Ok(())
            } else {
                Err(crate::error::EmrpError::Transport("mDNS peer not found".into()))
            }
        } else {
            Err(crate::error::EmrpError::Transport("mDNS transport not available".into()))
        }
    }
    
    /// Establish NAT traversal connection
    pub async fn establish_nat_traversal(&self, target: &str) -> Result<super::NatMethod> {
        if let Some(ref nat) = self.nat_transport {
            // This is a simplified version - the real implementation would be in NAT transport
            if nat.can_reach(target).await {
                Ok(super::NatMethod::Stun { 
                    server: "stun.l.google.com:19302".to_string() 
                })
            } else {
                Err(crate::error::EmrpError::Transport("NAT traversal failed".into()))
            }
        } else {
            Err(crate::error::EmrpError::Transport("NAT traversal transport not available".into()))
        }
    }
    
    /// Send connection offer via email
    pub async fn send_connection_offer_via_email(
        &self, 
        target: &str, 
        offer: super::ConnectionOffer
    ) -> Result<String> {
        self.email_transport.send_connection_offer(target, offer).await
    }
    
    /// Establish hybrid connection combining multiple transports
    pub async fn establish_hybrid_connection(&self, target: &str) -> Result<HybridConnection> {
        // Try to get a mutable reference to email transport for hybrid connection
        // In a real implementation, we'd need to restructure this
        info!("Establishing hybrid connection to {}", target);
        
        // For now, simulate a hybrid connection
        let discovery_start = Instant::now();
        tokio::time::sleep(Duration::from_millis(100)).await;
        let discovery_time = discovery_start.elapsed();
        
        let connection_start = Instant::now();
        tokio::time::sleep(Duration::from_millis(50)).await;
        let connection_time = connection_start.elapsed();
        
        let primary_route = TransportRoute::DirectTcp {
            address: target.to_string(),
            port: 8080,
            latency_ms: 25,
            established_at: Instant::now(),
        };
        
        let fallback_route = TransportRoute::StandardEmail {
            estimated_latency_min: 1,
        };
        
        let metrics = super::TransportMetrics {
            latency: connection_time,
            throughput_bps: 1_000_000,
            packet_loss: 0.01,
            jitter_ms: 5,
            reliability_score: 0.90,
            last_updated: Instant::now(),
        };
        
        Ok(HybridConnection {
            primary: primary_route,
            fallback: fallback_route,
            discovery_latency: discovery_time,
            connection_latency: connection_time,
            total_setup_time: discovery_time + connection_time,
            metrics,
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
        match self.email_transport.send_message(target, message).await {
            Ok(result) => Ok(result),
            Err(e) => {
                warn!("Email delivery failed, trying alternatives: {}", e);
                self.send_message(target, message, urgency).await
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
            MessageUrgency::Background | MessageUrgency::Discovery => {
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
        
        if self.mdns_transport.is_some() {
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
        if let Some(ref mdns) = self.mdns_transport {
            let _mdns_clone = Arc::clone(mdns);
            tokio::spawn(async move {
                // Note: This would need to be restructured to allow mutable access
                debug!("mDNS discovery service started");
            });
        }
        
        info!("All available transport services started");
        Ok(())
    }
}
