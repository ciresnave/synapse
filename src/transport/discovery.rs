use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use tokio::sync::{Mutex, RwLock};
use tracing::{debug, error, info, warn};

#[cfg(feature = "mdns")]
use auto_discovery::{DiscoveryConfig, ProtocolType, ServiceDiscovery, ServiceInfo, ServiceType};

use crate::error::{Result, SynapseError};
use crate::transport::abstraction::{
    ConnectivityResult, DeliveryConfirmation, DeliveryReceipt, IncomingMessage, MessageUrgency,
    Transport, TransportCapabilities, TransportEstimate, TransportMetrics, TransportStatus,
    TransportTarget, TransportType,
};
use crate::types::SecureMessage;

/// Discovered service information
#[derive(Debug, Clone)]
pub struct DiscoveredService {
    pub name: String,
    pub service_type: String,
    pub host: String,
    pub port: u16,
    pub addresses: Vec<IpAddr>,
    pub txt_records: HashMap<String, String>,
    pub last_seen: Instant,
}

impl DiscoveredService {
    pub fn socket_addrs(&self) -> Vec<SocketAddr> {
        self.addresses
            .iter()
            .map(|addr| SocketAddr::new(*addr, self.port))
            .collect()
    }
}

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
    #[cfg(feature = "mdns")]
    discovery: Arc<Mutex<Option<ServiceDiscovery>>>,
    discovered_services: Arc<RwLock<HashMap<String, DiscoveredService>>>,
    metrics: Arc<RwLock<TransportMetrics>>,
    last_discovery: Arc<RwLock<Instant>>,
    running: Arc<RwLock<bool>>,
}

impl DiscoveryTransport {
    pub async fn new(config: SynapseDiscoveryConfig) -> Result<Self> {
        info!(
            "Creating DiscoveryTransport with service type: {}",
            config.service_type
        );

        let transport = Self {
            config,
            #[cfg(feature = "mdns")]
            discovery: Arc::new(Mutex::new(None)),
            discovered_services: Arc::new(RwLock::new(HashMap::new())),
            metrics: Arc::new(RwLock::new(TransportMetrics::default())),
            last_discovery: Arc::new(RwLock::new(Instant::now())),
            running: Arc::new(RwLock::new(false)),
        };

        Ok(transport)
    }

    #[cfg(feature = "mdns")]
    async fn initialize_discovery(&self) -> Result<()> {
        let service_type = ServiceType::new(&self.config.service_type)
            .map_err(|e| SynapseError::TransportError(format!("Invalid service type: {}", e)))?;

        let discovery_config = DiscoveryConfig::new()
            .with_service_type(service_type)
            .with_protocol(ProtocolType::Mdns)
            .with_timeout(self.config.scan_interval);

        let discovery = ServiceDiscovery::new(discovery_config).await.map_err(|e| {
            SynapseError::TransportError(format!("Failed to create discovery: {}", e))
        })?;

        // Register our own service
        let service_info = ServiceInfo::new(
            &self.config.identifier,
            &self.config.service_type,
            self.config.port,
            Some(vec![
                ("synapse", "true"),
                ("version", "1.1.0"),
                ("protocols", &self.config.protocols.join(",")),
            ]),
        )
        .map_err(|e| {
            SynapseError::TransportError(format!("Failed to create service info: {}", e))
        })?;

        discovery
            .register_service(service_info)
            .await
            .map_err(|e| {
                SynapseError::TransportError(format!("Failed to register service: {}", e))
            })?;

        info!(
            "Registered service: {} on port {}",
            self.config.identifier, self.config.port
        );

        *self.discovery.lock().await = Some(discovery);
        Ok(())
    }

    #[cfg(not(feature = "mdns"))]
    async fn initialize_discovery(&self) -> Result<()> {
        warn!("mDNS discovery not available - compiled without mdns feature");
        Ok(())
    }

    async fn discover_services(&self) -> Result<()> {
        #[cfg(feature = "mdns")]
        {
            if let Some(discovery) = self.discovery.lock().await.as_ref() {
                debug!("Discovering services...");

                match discovery.discover_services(Some(ProtocolType::Mdns)).await {
                    Ok(services) => {
                        let mut discovered = self.discovered_services.write().await;
                        let now = Instant::now();

                        for service in services {
                            let discovered_service = DiscoveredService {
                                name: service.name().to_string(),
                                service_type: service.service_type().to_string(),
                                host: service.address().to_string(),
                                port: service.port(),
                                addresses: vec![service.address()],
                                txt_records: HashMap::new(), // auto-discovery doesn't expose txt records directly
                                last_seen: now,
                            };

                            debug!(
                                "Discovered service: {} at {}:{}",
                                discovered_service.name,
                                discovered_service.host,
                                discovered_service.port
                            );

                            discovered.insert(discovered_service.name.clone(), discovered_service);
                        }

                        // Update metrics - skip last_updated as it's not available in TransportMetrics
                        info!("Discovery found {} services", discovered.len());
                    }
                    Err(e) => {
                        error!("Discovery failed: {}", e);
                        return Err(SynapseError::TransportError(format!(
                            "Discovery failed: {}",
                            e
                        )));
                    }
                }
            }
        }

        *self.last_discovery.write().await = Instant::now();
        Ok(())
    }

    pub async fn get_discovered_services(&self) -> HashMap<String, DiscoveredService> {
        self.discovered_services.read().await.clone()
    }

    pub async fn find_service_by_name(&self, name: &str) -> Option<DiscoveredService> {
        self.discovered_services.read().await.get(name).cloned()
    }

    pub async fn find_services_by_type(&self, service_type: &str) -> Vec<DiscoveredService> {
        self.discovered_services
            .read()
            .await
            .values()
            .filter(|service| service.service_type == service_type)
            .cloned()
            .collect()
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
        // Check if we have discovered a service that matches the target
        let services = self.discovered_services.read().await;

        // Look for exact match by identifier
        if services.contains_key(&target.identifier) {
            return true;
        }

        // Look for services with matching capabilities
        for service in services.values() {
            if target.required_capabilities.iter().all(|cap| {
                service.txt_records.contains_key(cap)
                    || service
                        .txt_records
                        .get("protocols")
                        .map_or(false, |p| p.contains(cap))
            }) {
                return true;
            }
        }

        false
    }

    async fn estimate_metrics(&self, target: &TransportTarget) -> Result<TransportEstimate> {
        let services = self.discovered_services.read().await;

        // Find best matching service
        let service = if services.contains_key(&target.identifier) {
            services.get(&target.identifier)
        } else {
            services.values().find(|service| {
                target.required_capabilities.iter().all(|cap| {
                    service.txt_records.contains_key(cap)
                        || service
                            .txt_records
                            .get("protocols")
                            .map_or(false, |p| p.contains(cap))
                })
            })
        };

        if let Some(service) = service {
            // Estimate based on service age and location
            let age = service.last_seen.elapsed();
            let reliability = if age < Duration::from_secs(30) {
                0.95
            } else {
                0.8
            };

            // Local network services are typically fast
            Ok(TransportEstimate {
                latency: Duration::from_millis(10),
                bandwidth: 10_000_000, // 10 MB/s for local network
                reliability,
                cost: 0.1,
                available: true,
                confidence: 0.9,
            })
        } else {
            Ok(TransportEstimate {
                latency: Duration::from_millis(1000),
                bandwidth: 0,
                reliability: 0.0,
                cost: 1.0,
                available: false,
                confidence: 0.1,
            })
        }
    }

    async fn start(&self) -> Result<()> {
        info!("Starting DiscoveryTransport");

        *self.running.write().await = true;

        // Initialize mDNS discovery
        self.initialize_discovery().await?;

        // Start discovery loop - create a weak reference to avoid lifetime issues
        let discovered_services = self.discovered_services.clone();
        let discovery = self.discovery.clone();
        let running = self.running.clone();
        let scan_interval = self.config.scan_interval;

        tokio::spawn(async move {
            while *running.read().await {
                #[cfg(feature = "mdns")]
                {
                    if let Some(disco) = discovery.lock().await.as_ref() {
                        if let Ok(services) =
                            disco.discover_services(Some(ProtocolType::Mdns)).await
                        {
                            let mut discovered = discovered_services.write().await;
                            let now = Instant::now();

                            for service in services {
                                let discovered_service = DiscoveredService {
                                    name: service.name().to_string(),
                                    service_type: service.service_type().to_string(),
                                    host: service.address().to_string(),
                                    port: service.port(),
                                    addresses: vec![service.address()],
                                    txt_records: HashMap::new(),
                                    last_seen: now,
                                };

                                discovered
                                    .insert(discovered_service.name.clone(), discovered_service);
                            }
                        }
                    }
                }

                tokio::time::sleep(scan_interval).await;
            }
        });

        info!("DiscoveryTransport started successfully");
        Ok(())
    }
    async fn stop(&self) -> Result<()> {
        info!("Stopping DiscoveryTransport");

        *self.running.write().await = false;

        #[cfg(feature = "mdns")]
        {
            // Take ownership of the discovery service to drop it
            *self.discovery.lock().await = None;
        }

        info!("DiscoveryTransport stopped");
        Ok(())
    }
    async fn send_message(
        &self,
        target: &TransportTarget,
        message: &SecureMessage,
    ) -> Result<DeliveryReceipt> {
        // Discovery transport doesn't send messages directly - it's used for finding other transports
        // The abstraction layer will use discovered services to route via appropriate transports
        let services = self.discovered_services.read().await;

        if let Some(service) = services.values().next() {
            // Create a delivery receipt indicating the service was found
            Ok(DeliveryReceipt {
                message_id: message.message_id.to_string(),
                transport_used: TransportType::AutoDiscovery,
                delivery_time: Duration::from_millis(1),
                target_reached: format!("{}:{}", service.host, service.port),
                confirmation: DeliveryConfirmation::Sent,
                metadata: {
                    let mut metadata = HashMap::new();
                    metadata.insert(
                        "discovered_services".to_string(),
                        services.len().to_string(),
                    );
                    metadata.insert("service_name".to_string(), service.name.clone());
                    metadata
                },
            })
        } else {
            Err(SynapseError::TransportError(
                "No discovered services available for message delivery".to_string(),
            ))
        }
    }

    async fn receive_messages(&self) -> Result<Vec<IncomingMessage>> {
        // Discovery transport doesn't receive messages directly
        // It provides service information for other transports to use
        Ok(Vec::new())
    }

    async fn test_connectivity(&self, target: &TransportTarget) -> Result<ConnectivityResult> {
        let services = self.discovered_services.read().await;

        // Find matching service
        let service = if services.contains_key(&target.identifier) {
            services.get(&target.identifier)
        } else {
            services.values().find(|service| {
                target
                    .required_capabilities
                    .iter()
                    .all(|cap| service.txt_records.contains_key(cap))
            })
        };

        if let Some(service) = service {
            let age = service.last_seen.elapsed();
            let connected = age < Duration::from_secs(60); // Consider fresh if seen in last minute

            Ok(ConnectivityResult {
                connected,
                rtt: Some(Duration::from_millis(10)), // Local network RTT
                quality: if connected { 1.0 } else { 0.0 },
                error: if connected {
                    None
                } else {
                    Some("Service not recently seen".to_string())
                },
                details: {
                    let mut details = HashMap::new();
                    details.insert("service_name".to_string(), service.name.clone());
                    details.insert("host".to_string(), service.host.clone());
                    details.insert("port".to_string(), service.port.to_string());
                    details.insert("last_seen".to_string(), format!("{:?} ago", age));
                    details
                },
            })
        } else {
            Ok(ConnectivityResult {
                connected: false,
                rtt: None,
                quality: 0.0,
                error: Some("No matching service discovered".to_string()),
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
