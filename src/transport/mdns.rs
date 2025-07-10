//! mDNS (Multicast DNS) transport for local network discovery

use super::{Transport, TransportMetrics};
use crate::{types::SecureMessage, error::Result};
use async_trait::async_trait;
use std::{time::{Duration, Instant}, collections::HashMap, net::{SocketAddr, IpAddr}};
use tracing::{info, debug, warn};
#[cfg(feature = "mdns")]
use trust_dns_resolver::{AsyncResolver, TokioAsyncResolver};
#[cfg(feature = "mdns")]
use trust_dns_resolver::config::{ResolverConfig, ResolverOpts};

/// mDNS service discovery and communication
#[cfg(feature = "mdns")]
pub struct MdnsTransport {
    service_name: String,
    #[allow(dead_code)]
    local_port: u16,
    resolver: TokioAsyncResolver,
    discovered_peers: HashMap<String, MdnsPeer>,
    discovery_interval: Duration,
}

/// Discovered mDNS peer information
#[derive(Debug, Clone)]
pub struct MdnsPeer {
    pub entity_id: String,
    pub service_instance: String,
    pub address: SocketAddr,
    pub txt_records: HashMap<String, String>,
    pub discovered_at: Instant,
    pub last_seen: Instant,
    pub priority: u16,
    pub weight: u16,
}

#[cfg(feature = "mdns")]
impl MdnsTransport {
    pub async fn new(service_name: String, local_port: u16) -> Result<Self> {
        let resolver = AsyncResolver::tokio(
            ResolverConfig::default(),
            ResolverOpts::default()
        );
        
        info!("Created mDNS transport for service '{}' on port {}", service_name, local_port);
        
        Ok(Self {
            service_name,
            local_port,
            resolver,
            discovered_peers: HashMap::new(),
            discovery_interval: Duration::from_secs(30),
        })
    }
    
    /// Start mDNS service discovery
    pub async fn start_discovery(&mut self) -> Result<()> {
        info!("Starting mDNS discovery for service '{}'", self.service_name);
        
        loop {
            match self.discover_peers().await {
                Ok(peers) => {
                    debug!("Discovered {} mDNS peers", peers.len());
                    for peer in peers {
                        self.discovered_peers.insert(peer.entity_id.clone(), peer);
                    }
                }
                Err(e) => {
                    warn!("mDNS discovery error: {}", e);
                }
            }
            
            // Clean up old peers
            self.cleanup_stale_peers();
            
            tokio::time::sleep(self.discovery_interval).await;
        }
    }
    
    /// Discover EMRP peers on the local network
    async fn discover_peers(&self) -> Result<Vec<MdnsPeer>> {
        let mut peers = Vec::new();
        
        // Look for _emrp._tcp.local services
        let service_query = format!("_emrp._tcp.local");
        
        match self.resolver.srv_lookup(&service_query).await {
            Ok(srv_records) => {
                for srv in srv_records.iter() {
                    debug!("Found SRV record: {} -> {}:{}", 
                           srv.target(), srv.target(), srv.port());
                    
                    // Resolve the target to an IP address
                    if let Ok(a_records) = self.resolver.ipv4_lookup(srv.target().clone()).await {
                        for a_record in a_records.iter() {
                            // Parse TXT records if available
                            let mut txt_records = HashMap::new();
                            
                            // Query for TXT records for this service instance
                            if let Ok(txt_lookup) = self.resolver.txt_lookup(srv.target().clone()).await {
                                for txt_record in txt_lookup.iter() {
                                    for txt_data in txt_record.iter() {
                                        if let Ok(txt_str) = std::str::from_utf8(txt_data) {
                                            // Parse key=value pairs from TXT records
                                            if let Some(eq_pos) = txt_str.find('=') {
                                                let key = txt_str[..eq_pos].to_string();
                                                let value = txt_str[eq_pos + 1..].to_string();
                                                txt_records.insert(key, value);
                                            } else {
                                                // Handle boolean flags without values
                                                txt_records.insert(txt_str.to_string(), "true".to_string());
                                            }
                                        }
                                    }
                                }
                            }
                            
                            let peer = MdnsPeer {
                                entity_id: srv.target().to_string().trim_end_matches('.').to_string(),
                                service_instance: service_query.clone(),
                                address: SocketAddr::new(IpAddr::V4(a_record.0), srv.port()),
                                txt_records,
                                discovered_at: Instant::now(),
                                last_seen: Instant::now(),
                                priority: srv.priority(),
                                weight: srv.weight(),
                            };
                            
                            peers.push(peer);
                            break; // Use first IP address
                        }
                    }
                }
            }
            Err(e) => {
                debug!("No SRV records found for {}: {}", service_query, e);
            }
        }
        
        // Also look for IPv6 addresses
        match self.resolver.srv_lookup(&service_query).await {
            Ok(srv_records) => {
                for srv in srv_records.iter() {
                    if let Ok(aaaa_records) = self.resolver.ipv6_lookup(srv.target().clone()).await {
                        for aaaa_record in aaaa_records.iter() {
                            let peer = MdnsPeer {
                                entity_id: format!("ipv6-{}", srv.target().to_string().trim_end_matches('.')),
                                service_instance: service_query.clone(),
                                address: SocketAddr::new(IpAddr::V6(aaaa_record.0), srv.port()),
                                txt_records: HashMap::new(),
                                discovered_at: Instant::now(),
                                last_seen: Instant::now(),
                                priority: srv.priority(),
                                weight: srv.weight(),
                            };
                            
                            peers.push(peer);
                            break; // Use first IPv6 address
                        }
                    }
                }
            }
            Err(_) => {
                // IPv6 lookup failed, ignore
            }
        }
        
        Ok(peers)
    }
    
    /// Clean up peers that haven't been seen recently
    fn cleanup_stale_peers(&mut self) {
        let stale_threshold = Duration::from_secs(300); // 5 minutes
        let now = Instant::now();
        
        self.discovered_peers.retain(|entity_id, peer| {
            if now.duration_since(peer.last_seen) > stale_threshold {
                debug!("Removing stale mDNS peer: {}", entity_id);
                false
            } else {
                true
            }
        });
    }
    
    /// Find a specific peer by entity ID
    pub fn find_peer(&self, entity_id: &str) -> Option<&MdnsPeer> {
        self.discovered_peers.get(entity_id)
    }
    
    /// Get all discovered peers
    pub fn get_all_peers(&self) -> Vec<&MdnsPeer> {
        self.discovered_peers.values().collect()
    }
    
    /// Send message to a discovered peer
    pub async fn send_to_peer(&self, peer: &MdnsPeer, message: &SecureMessage) -> Result<String> {
        // Use TCP to send to the peer's address
        let tcp_transport = super::tcp::TcpTransport::new(0).await?;
        
        match tcp_transport.connect(&peer.address.ip().to_string(), peer.address.port()).await {
            Ok(mut stream) => {
                tcp_transport.send_via_stream(&mut stream, message).await?;
                info!("Sent message to mDNS peer {} at {}", peer.entity_id, peer.address);
                Ok(format!("mdns://{}@{}", peer.entity_id, peer.address))
            }
            Err(e) => {
                warn!("Failed to connect to mDNS peer {}: {}", peer.entity_id, e);
                Err(e)
            }
        }
    }
    
    /// Test connectivity to a peer
    pub async fn test_peer_connectivity(&self, peer: &MdnsPeer) -> Result<TransportMetrics> {
        let start = Instant::now();
        let tcp_transport = super::tcp::TcpTransport::new(0).await?;
        
        match tcp_transport.connect(&peer.address.ip().to_string(), peer.address.port()).await {
            Ok(_stream) => {
                let latency = start.elapsed();
                Ok(TransportMetrics {
                    latency,
                    throughput_bps: 10_000_000, // 10Mbps for local network
                    packet_loss: 0.001, // 0.1% packet loss on LAN
                    jitter_ms: 1,
                    reliability_score: 0.95,
                    last_updated: Instant::now(),
                })
            }
            Err(e) => {
                Err(crate::error::EmrpError::Transport(format!("mDNS peer connectivity test failed: {}", e)))
            }
        }
    }
}

#[async_trait]
#[cfg(feature = "mdns")]
impl Transport for MdnsTransport {
    async fn send_message(&self, target: &str, message: &SecureMessage) -> Result<String> {
        // Look for the target in discovered peers
        if let Some(peer) = self.find_peer(target) {
            return self.send_to_peer(peer, message).await;
        }
        
        // If not found, try to discover it first
        debug!("Target {} not in mDNS cache, attempting discovery", target);
        
        // For a production implementation, we might wait for discovery
        // For now, return an error
        Err(crate::error::EmrpError::Transport(
            format!("mDNS peer '{}' not found", target)
        ))
    }
    
    async fn receive_messages(&self) -> Result<Vec<SecureMessage>> {
        // mDNS transport relies on TCP/UDP for actual message transport
        // Discovery happens separately, message receiving is handled by TCP transport
        Ok(Vec::new())
    }
    
    async fn test_connectivity(&self, target: &str) -> Result<TransportMetrics> {
        if let Some(peer) = self.find_peer(target) {
            self.test_peer_connectivity(peer).await
        } else {
            Err(crate::error::EmrpError::Transport(
                format!("mDNS peer '{}' not found for connectivity test", target)
            ))
        }
    }
    
    async fn can_reach(&self, target: &str) -> bool {
        self.find_peer(target).is_some()
    }
    
    fn get_capabilities(&self) -> Vec<String> {
        vec![
            "mdns".to_string(),
            "local_network".to_string(),
            "discovery".to_string(),
            "low_latency".to_string(),
        ]
    }
    
    fn estimated_latency(&self) -> Duration {
        Duration::from_millis(5) // 5ms for local network
    }
    
    fn reliability_score(&self) -> f32 {
        0.95 // Very reliable on local network
    }
}

/// mDNS service announcement for advertising EMRP availability
pub struct MdnsAdvertiser {
    service_name: String,
    port: u16,
    txt_records: HashMap<String, String>,
}

#[cfg(feature = "mdns")]
impl MdnsAdvertiser {
    pub fn new(service_name: String, port: u16) -> Self {
        let mut txt_records = HashMap::new();
        txt_records.insert("version".to_string(), "1.0".to_string());
        txt_records.insert("protocol".to_string(), "emrp".to_string());
        
        Self {
            service_name,
            port,
            txt_records,
        }
    }
    
    /// Add a TXT record for service announcement
    pub fn add_txt_record(&mut self, key: String, value: String) {
        self.txt_records.insert(key, value);
    }
    
    /// Start advertising the EMRP service via mDNS
    pub async fn start_advertising(&self) -> Result<()> {
        info!("Starting mDNS advertisement for service '{}' on port {}", 
               self.service_name, self.port);
        
        // In a real implementation, this would use an mDNS library to announce the service
        // For now, we'll just log that we would be advertising
        info!("Would advertise: _emrp._tcp.local -> {}:{}", self.service_name, self.port);
        
        for (key, value) in &self.txt_records {
            debug!("TXT record: {}={}", key, value);
        }
        
        // Keep advertising (in real implementation, this would be handled by the mDNS library)
        loop {
            tokio::time::sleep(Duration::from_secs(60)).await;
            debug!("mDNS advertisement heartbeat for {}", self.service_name);
        }
    }
    
    /// Stop advertising the service
    pub async fn stop_advertising(&self) -> Result<()> {
        info!("Stopping mDNS advertisement for service '{}'", self.service_name);
        // In a real implementation, this would deregister the service
        Ok(())
    }
}
