// Synapse Neural Communication Network
// Core module for federated identity and blockchain trust system

pub mod models;
pub mod services;
pub mod storage;
pub mod blockchain;
pub mod api;
pub mod telemetry;
pub mod transport;

// Re-export key types for convenience
pub use models::{
    participant::{ParticipantProfile, EntityType, DiscoverabilityLevel},
    trust::{TrustRatings, NetworkTrustRating, TrustBalance},
};

use anyhow::Result;
use std::sync::Arc;

/// Core Synapse network node
pub struct SynapseNode {
    pub registry: Arc<services::registry::ParticipantRegistry>,
    pub trust_manager: Arc<services::trust_manager::TrustManager>,
    pub blockchain: Arc<blockchain::SynapseBlockchain>,
    pub discovery: Arc<services::discovery::DiscoveryService>,
    pub error_telemetry: Arc<telemetry::ErrorTelemetry>,
}

impl SynapseNode {
    /// Create a new Synapse node with all components
    pub async fn new(config: SynapseConfig) -> Result<Self> {
        // Initialize storage layer
        let database = Arc::new(storage::database::Database::new(&config.database_url).await?);
        let cache = Arc::new(storage::cache::Cache::new(&config.redis_url).await?);
        
        // Initialize blockchain
        let blockchain = Arc::new(blockchain::SynapseBlockchain::new(config.blockchain_config).await?);
        
        // Initialize services
        let trust_manager = Arc::new(services::trust_manager::TrustManager::new(
            database.clone(),
            blockchain.clone(),
        ).await?);
        
        let registry = Arc::new(services::registry::ParticipantRegistry::new(
            database.clone(),
            cache.clone(),
            trust_manager.clone(),
        ).await?);
        
        let discovery = Arc::new(services::discovery::DiscoveryService::new(
            database.clone(),
            cache.clone(),
        ));
        
        // Initialize error telemetry
        let error_telemetry = Arc::new(telemetry::ErrorTelemetry::new(
            telemetry::ErrorTelemetryConfig {
                remote_endpoint: config.telemetry_endpoint,
                ..Default::default()
            }
        ));
        
        Ok(Self {
            registry,
            trust_manager,
            blockchain,
            discovery,
            error_telemetry,
        })
    }
    
    /// Start the Synapse node
    pub async fn start(&self) -> Result<()> {
        // Start blockchain consensus
        self.blockchain.start_consensus().await?;
        
        // Start trust point decay scheduler
        self.trust_manager.start_decay_scheduler().await?;
        
        // Discovery services are ready (no explicit start required)
        tracing::info!("Discovery service initialized");
        
        tracing::info!("Synapse node started successfully");
        Ok(())
    }
}

/// Configuration for Synapse node
#[derive(Debug, Clone)]
pub struct SynapseConfig {
    pub database_url: String,
    pub redis_url: String,
    pub blockchain_config: blockchain::BlockchainConfig,
    pub node_id: String,
    pub private_key: Vec<u8>,
    pub network_port: u16,
    pub telemetry_endpoint: Option<String>,
}

impl Default for SynapseConfig {
    fn default() -> Self {
        Self {
            database_url: "postgresql://localhost/synapse".to_string(),
            redis_url: "redis://localhost:6379".to_string(),
            blockchain_config: blockchain::BlockchainConfig::default(),
            node_id: uuid::Uuid::new_v4().to_string(),
            private_key: vec![], // Should be generated or loaded
            network_port: 8080,
            telemetry_endpoint: None, // No remote telemetry by default
        }
    }
}
