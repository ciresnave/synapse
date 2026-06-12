// Synapse Neural Communication Network
// Core module for federated identity and blockchain trust system

pub mod api;
pub mod blockchain;
pub mod models;
pub mod services;
pub mod storage;
pub mod telemetry;
pub mod transport;

pub mod auth;

// Re-export key types for convenience
pub use models::{
    participant::{DiscoverabilityLevel, EntityType, ParticipantProfile},
    trust::{NetworkTrustRating, TrustBalance, TrustRatings},
};

use anyhow::Result;
use std::sync::Arc;
use uuid::Uuid;

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
        // Initialize blockchain
        let blockchain =
            Arc::new(blockchain::SynapseBlockchain::new(config.blockchain_config.clone()).await?);

        let database = Arc::new(storage::Database::new(&config.database_url).await?);
        let cache = Arc::new(storage::Cache::new(&config.redis_url).await?);

        // Trust manager
        let trust_manager = Arc::new(
            services::trust_manager::TrustManager::new(database.clone(), blockchain.clone())
                .await?,
        );

        // Discovery service
        let discovery = Arc::new(services::discovery::DiscoveryService::new(
            database.clone(),
            cache.clone(),
        ));

        // Participant registry
        let registry = Arc::new(
            services::registry::ParticipantRegistry::new(
                database.clone(),
                cache.clone(),
                trust_manager.clone(),
            )
            .await?,
        );

        // Error telemetry (always initialize for monolithic build)
        let error_telemetry = Arc::new(telemetry::ErrorTelemetry::new(
            telemetry::error_reporting::ErrorTelemetryConfig::default(),
        ));

        Ok(SynapseNode {
            registry,
            trust_manager,
            blockchain,
            discovery,
            error_telemetry,
        })
    }

    /// Start the Synapse node
    pub async fn start(&self) -> Result<()> {
        // Start blockchain consensus (always for monolithic build)
        Arc::as_ref(&self.blockchain).start_consensus().await?;

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
            node_id: Uuid::new_v4().to_string(),
            private_key: vec![], // Should be generated or loaded
            network_port: 8080,
            telemetry_endpoint: None, // No remote telemetry by default
        }
    }
}
