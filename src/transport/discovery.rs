use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use auto_discovery::{
    config::DiscoveryConfig,
    discovery::ServiceDiscovery,
    service::ServiceInfo,
    types::{ServiceType, ProtocolType},
};
use dashmap::DashMap;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use crate::transport::abstraction::{
    Transport, TransportType, TransportCapabilities, TransportMetrics,
    TransportStatus, TransportTarget, TransportEstimate, IncomingMessage,
    MessageUrgency, DeliveryReceipt, ConnectivityResult, DeliveryConfirmation
};
use crate::error::{Result, SynapseError};
use crate::types::SecureMessage;

/// Configuration for auto-discovery transport
#[derive(Debug, Clone)]
pub struct SynapseDiscoveryConfig {
    pub identifier: String,
    pub service_type: String,
    pub port: u16,
    pub scan_interval: Duration,
    pub protocols: Vec<String>,
}

impl SynapseDiscoveryConfig {
    pub fn new(identifier: String, port: u16) -> Self {
        Self {
            identifier,
            service_type: "_synapse._tcp".to_string(),
            port,
            scan_interval: Duration::from_secs(60),
            protocols: vec!["mdns".to_string()],
        }
    }
    
    pub fn with_service_type(mut self, service_type: String) -> Self {
        self.service_type = service_type;
        self
    }
    
    pub fn with_scan_interval(mut self, interval: Duration) -> Self {
        self.scan_interval = interval;
        self
    }
    
    pub fn with_protocols(mut self, protocols: Vec<String>) -> Self {
        self.protocols = protocols;
        self
    }
}

/// Auto-discovery based transport implementation
pub struct DiscoveryTransport {
    config: SynapseDiscoveryConfig,
    discovery: Arc<ServiceDiscovery>,
    discovered_services: Arc<DashMap<String, ServiceInfo>>,
    metrics: Arc<RwLock<TransportMetrics>>,
    last_discovery: Arc<RwLock<Instant>>,
}

impl DiscoveryTransport {
    pub async fn new(config: SynapseDiscoveryConfig) -> Result<Self> {
        let discovery_config = DiscoveryConfig::new()
            .with_service_type(ServiceType::new(&config.service_type)?)
            .with_protocol(ProtocolType::Mdns)
            .with_timeout(config.scan_interval);

        let discovery = ServiceDiscovery::new(discovery_config)
            .await
            .map_err(|e| SynapseError::TransportError(e.to_string()))?;

        // Create service info for this instance
        let service_info = ServiceInfo::new(
            &config.identifier,
            &config.service_type,
            config.port,
            Some(vec![("type", "synapse")]),
        ).map_err(|e| SynapseError::TransportError(e.to_string()))?;

        // Register our service
        discovery.register_service(service_info)
            .await
            .map_err(|e| SynapseError::TransportError(e.to_string()))?;

        Ok(Self {
            config,
            discovery: Arc::new(discovery),
            discovered_services: Arc::new(DashMap::new()),
            metrics: Arc::new(RwLock::new(TransportMetrics::default())),
            last_discovery: Arc::new(RwLock::new(Instant::now())),
        })
    }

    async fn scan_services(&self) -> Result<()> {
        debug!("Scanning for services...");
        *self.last_discovery.write().await = Instant::now();

        // Discover services of our type
        let discovered = self.discovery.discover_services(Some(ProtocolType::Mdns))
            .await
            .map_err(|e| SynapseError::TransportError(e.to_string()))?;

        for service in discovered {
            if service.service_type() != &ServiceType::new(&self.config.service_type)? {
                continue;
            }

            // Skip our own service
            if service.name() == self.config.identifier {
                continue;  
            }

            // Add/update service
            info!("Discovered synapse service: {} at {}:{}", 
                service.name(), service.address(), service.port());
            self.discovered_services.insert(service.name().to_string(), service);
        }

        Ok(())
    }
}

#[async_trait]
impl Transport for DiscoveryTransport {
    fn transport_type(&self) -> TransportType {
        TransportType::AutoDiscovery
    }

    fn capabilities(&self) -> TransportCapabilities {
        TransportCapabilities {
            reliable: true,
            real_time: true,
            broadcast: true,
            bidirectional: true,
            encrypted: true,
            network_spanning: true,
            max_message_size: 1024 * 1024, // 1MB
            features: vec![],
            supported_urgencies: vec![MessageUrgency::RealTime, MessageUrgency::Interactive],
        }
    }

    async fn can_reach(&self, target: &TransportTarget) -> bool {
        // Scan for services if needed
        if self.last_discovery.read().await.elapsed() > self.config.scan_interval {
            if let Err(e) = self.scan_services().await {
                warn!("Service scan failed: {}", e);
                return false;
            }
        }
        self.discovered_services.contains_key(&target.identifier)
    }

    async fn estimate_metrics(&self, target: &TransportTarget) -> Result<TransportEstimate> {
        if let Some(_service) = self.discovered_services.get(&target.identifier) {
            Ok(TransportEstimate {
                latency: Duration::from_millis(50),
                bandwidth: 1024 * 1024, // 1MB/s
                reliability: 0.95,
                cost: 0.1,
                available: true,
                confidence: 0.9,
            })
        } else {
            Err(SynapseError::TransportError("Service not found".to_string()))
        }
    }

    async fn start(&self) -> Result<()> {
        info!("Starting discovery transport");
        self.scan_services().await?;
        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        info!("Stopping discovery transport");
        Ok(())
    }

    async fn send_message(&self, target: &TransportTarget, message: &SecureMessage) -> Result<DeliveryReceipt> {
        if let Some(_service) = self.discovered_services.get(&target.identifier) {
            // TODO: Implement actual message sending using service.address() and service.port()
            // For now return placeholder receipt
            Ok(DeliveryReceipt {
                message_id: message.message_id.0.to_string(),
                transport_used: self.transport_type(),
                delivery_time: Duration::from_secs(0),
                target_reached: "success".to_string(),
                confirmation: DeliveryConfirmation::Delivered,
                metadata: HashMap::new(),
            })
        } else {
            Err(SynapseError::TransportError("Service not found".to_string()))
        }
    }

    async fn receive_messages(&self) -> Result<Vec<IncomingMessage>> {
        // TODO: Implement message receiving based on the auto-discovery service
        Ok(Vec::new())
    }

    async fn test_connectivity(&self, target: &TransportTarget) -> Result<ConnectivityResult> {
        if let Some(service) = self.discovered_services.get(&target.identifier) {
            Ok(ConnectivityResult {
                connected: true,
                rtt: Some(Duration::from_millis(50)), // Placeholder latency
                quality: 0.95,
                error: None,
                details: {
                    let mut details = HashMap::new();
                    details.insert("type".to_string(), "mdns".to_string());
                    details.insert("address".to_string(), service.address().to_string());
                    details.insert("port".to_string(), service.port().to_string());
                    details
                },
            })
        } else {
            Ok(ConnectivityResult {
                connected: false,
                rtt: None,
                quality: 0.0,
                error: Some("Service not found".to_string()),
                details: HashMap::new(),
            })
        }
    }

    async fn status(&self) -> TransportStatus {
        TransportStatus::Running
    }

    async fn metrics(&self) -> TransportMetrics {
        self.metrics.read().await.clone()
    }
}
