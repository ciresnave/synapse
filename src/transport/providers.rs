// SPDX-License-Identifier: MIT OR Apache-2.0
//! Transport providers for dependency injection
//!
//! This module provides trait-based dependency injection for transport implementations,
//! making the system much more testable and flexible.

use super::{TransportSelector, abstraction, abstraction::Transport};
use crate::{config::Config, error::Result, types::SecureMessage};
use async_trait::async_trait;
use std::sync::Arc;
use tracing::{info, warn};

/// Transport provider trait for dependency injection
#[async_trait]
pub trait TransportProvider: Send + Sync {
    /// Create a TCP transport instance
    async fn create_tcp_transport(&self, config: &Config) -> Result<Option<Arc<dyn Transport>>>;

    /// Create an mDNS transport instance
    async fn create_mdns_transport(&self, config: &Config) -> Result<Option<Arc<dyn Transport>>>;

    /// Create a NAT traversal transport instance
    async fn create_nat_transport(&self, config: &Config) -> Result<Option<Arc<dyn Transport>>>;

    /// Create an email transport instance
    async fn create_email_transport(&self, config: &Config) -> Result<Option<Arc<dyn Transport>>>;

    /// Create a transport selector
    fn create_transport_selector(&self) -> Arc<tokio::sync::RwLock<TransportSelector>>;
}

/// Default production transport provider
pub struct ProductionTransportProvider;

#[async_trait]
impl TransportProvider for ProductionTransportProvider {
    async fn create_tcp_transport(&self, config: &Config) -> Result<Option<Arc<dyn Transport>>> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            use super::tcp_unified::TcpTransportImpl;
            use std::collections::HashMap;

            let mut tcp_config = HashMap::new();
            tcp_config.insert("listen_port".to_string(), "8080".to_string());
            tcp_config.insert("connection_timeout_ms".to_string(), "10000".to_string());
            tcp_config.insert(
                crate::network_scope::BIND_SCOPE_KEY.to_string(),
                config.network.bind_scope.config_value().to_string(),
            );

            match TcpTransportImpl::new(&tcp_config).await {
                Ok(transport) => {
                    tracing::info!("TCP transport initialized");
                    Ok(Some(Arc::new(transport)))
                }
                Err(e) => {
                    tracing::warn!("Failed to initialize TCP transport: {}", e);
                    Ok(None)
                }
            }
        }
        #[cfg(target_arch = "wasm32")]
        Ok(None)
    }

    async fn create_mdns_transport(&self, config: &Config) -> Result<Option<Arc<dyn Transport>>> {
        use crate::transport::mdns_enhanced::{EnhancedMdnsTransport, MdnsConfig};
        let bind_scope = config.network.bind_scope;
        if !bind_scope.is_all_interfaces() {
            info!(
                "mDNS transport not created: set network.bind_scope = \"all_interfaces\" to enable it"
            );
            return Ok(None);
        }
        let mdns_config = MdnsConfig {
            bind_scope,
            ..MdnsConfig::default()
        };
        match EnhancedMdnsTransport::new("_synapse._tcp.local".to_string(), 8080, Some(mdns_config))
            .await
        {
            Ok(transport) => {
                tracing::info!("mDNS transport initialized");
                Ok(Some(Arc::new(transport) as Arc<dyn abstraction::Transport>))
            }
            Err(e) => {
                tracing::warn!("Failed to initialize mDNS transport: {}", e);
                Err(e)
            }
        }
    }

    async fn create_nat_transport(&self, config: &Config) -> Result<Option<Arc<dyn Transport>>> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            use super::nat_traversal::NatTraversalTransport;
            match NatTraversalTransport::new_with_scope(8080, config.network.bind_scope).await {
                Ok(transport) => {
                    tracing::info!("NAT traversal transport initialized");
                    Ok(Some(Arc::new(transport)))
                }
                Err(e) => {
                    tracing::warn!("Failed to initialize NAT traversal transport: {}", e);
                    Ok(None)
                }
            }
        }
        #[cfg(target_arch = "wasm32")]
        Ok(None)
    }

    async fn create_email_transport(&self, config: &Config) -> Result<Option<Arc<dyn Transport>>> {
        use crate::transport::email_simple::SimpleEmailTransport;
        info!("Creating simplified email transport");

        match SimpleEmailTransport::new(config.email.clone()) {
            Ok(transport) => {
                info!("Simple email transport created successfully");
                Ok(Some(Arc::new(transport) as Arc<dyn abstraction::Transport>))
            }
            Err(e) => {
                warn!("Failed to create simple email transport: {}", e);
                Ok(None)
            }
        }
    }

    fn create_transport_selector(&self) -> Arc<tokio::sync::RwLock<TransportSelector>> {
        Arc::new(tokio::sync::RwLock::new(TransportSelector::new()))
    }
}

/// Test transport provider for dependency injection in tests
pub struct TestTransportProvider {
    pub tcp_transport: Option<Arc<dyn Transport>>,
    pub mdns_transport: Option<Arc<dyn Transport>>,
    pub nat_transport: Option<Arc<dyn Transport>>,
    pub email_transport: Option<Arc<dyn Transport>>,
}

impl TestTransportProvider {
    pub fn new() -> Self {
        Self {
            tcp_transport: None,
            mdns_transport: None,
            nat_transport: None,
            email_transport: None,
        }
    }

    pub fn with_tcp_transport(mut self, transport: Arc<dyn Transport>) -> Self {
        self.tcp_transport = Some(transport);
        self
    }

    pub fn with_mdns_transport(mut self, transport: Arc<dyn Transport>) -> Self {
        self.mdns_transport = Some(transport);
        self
    }

    pub fn with_nat_transport(mut self, transport: Arc<dyn Transport>) -> Self {
        self.nat_transport = Some(transport);
        self
    }

    pub fn with_email_transport(mut self, transport: Arc<dyn Transport>) -> Self {
        self.email_transport = Some(transport);
        self
    }
}

impl Default for TestTransportProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl TransportProvider for TestTransportProvider {
    async fn create_tcp_transport(&self, _config: &Config) -> Result<Option<Arc<dyn Transport>>> {
        Ok(self.tcp_transport.clone())
    }

    async fn create_mdns_transport(&self, _config: &Config) -> Result<Option<Arc<dyn Transport>>> {
        Ok(self.mdns_transport.clone())
    }

    async fn create_nat_transport(&self, _config: &Config) -> Result<Option<Arc<dyn Transport>>> {
        Ok(self.nat_transport.clone())
    }

    async fn create_email_transport(&self, _config: &Config) -> Result<Option<Arc<dyn Transport>>> {
        Ok(self.email_transport.clone())
    }

    fn create_transport_selector(&self) -> Arc<tokio::sync::RwLock<TransportSelector>> {
        Arc::new(tokio::sync::RwLock::new(TransportSelector::new()))
    }
}

/// Simple mock transport for testing
#[derive(Debug, Clone)]
pub struct MockTransport {
    pub id: String,
    pub latency: std::time::Duration,
    pub reliability: f32,
    pub capabilities: Vec<String>,
}

impl MockTransport {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            latency: std::time::Duration::from_millis(50),
            reliability: 0.95,
            capabilities: vec!["mock".to_string(), "test".to_string()],
        }
    }

    pub fn with_latency(mut self, latency: std::time::Duration) -> Self {
        self.latency = latency;
        self
    }

    pub fn with_reliability(mut self, reliability: f32) -> Self {
        self.reliability = reliability;
        self
    }

    pub fn with_capabilities(mut self, capabilities: Vec<String>) -> Self {
        self.capabilities = capabilities;
        self
    }
}

#[async_trait]
impl Transport for MockTransport {
    fn transport_type(&self) -> abstraction::TransportType {
        abstraction::TransportType::Tcp
    }

    fn capabilities(&self) -> abstraction::TransportCapabilities {
        abstraction::TransportCapabilities {
            max_message_size: 1024 * 1024, // 1MB
            reliable: true,
            real_time: false,
            broadcast: false,
            bidirectional: true,
            encrypted: false,
            network_spanning: true,
            supported_urgencies: vec![abstraction::MessageUrgency::Interactive],
            features: vec![],
        }
    }

    async fn can_reach(&self, target: &abstraction::TransportTarget) -> bool {
        // Mock implementation: can reach if target identifier is not empty and not failing
        !target.identifier.is_empty() && self.reliability > 0.5
    }

    async fn estimate_metrics(
        &self,
        _target: &abstraction::TransportTarget,
    ) -> Result<abstraction::TransportEstimate> {
        Ok(abstraction::TransportEstimate {
            latency: self.latency,
            reliability: self.reliability as f64,
            bandwidth: 1_000_000,
            cost: 1.0,
            available: true,
            confidence: 0.8,
        })
    }

    async fn send_message(
        &self,
        target: &abstraction::TransportTarget,
        _message: &SecureMessage,
    ) -> Result<abstraction::DeliveryReceipt> {
        // Simulate latency
        tokio::time::sleep(self.latency).await;
        Ok(abstraction::DeliveryReceipt {
            message_id: format!("mock-{}-sent-to-{}", self.id, target.identifier),
            transport_used: abstraction::TransportType::Tcp,
            delivery_time: self.latency,
            target_reached: target.identifier.clone(),
            confirmation: abstraction::DeliveryConfirmation::Delivered,
            metadata: std::collections::HashMap::new(),
        })
    }

    async fn receive_messages(&self) -> Result<Vec<abstraction::IncomingMessage>> {
        Ok(vec![]) // Simple mock - no messages
    }

    async fn test_connectivity(
        &self,
        _target: &abstraction::TransportTarget,
    ) -> Result<abstraction::ConnectivityResult> {
        Ok(abstraction::ConnectivityResult {
            connected: self.reliability > 0.5,
            rtt: Some(self.latency),
            error: None,
            quality: self.reliability as f64,
            details: std::collections::HashMap::new(),
        })
    }

    async fn start(&self) -> Result<()> {
        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        Ok(())
    }

    async fn status(&self) -> abstraction::TransportStatus {
        abstraction::TransportStatus::Running
    }

    async fn metrics(&self) -> abstraction::TransportMetrics {
        abstraction::TransportMetrics::default()
    }
}
