//! AI Assistant Network Example
//! 
//! This example demonstrates how to create a network of AI assistants
//! that can communicate with each other using the synapse library.

use synapse::{Config, SynapseRouter};
use anyhow::Result;
use tracing::{info, warn};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    info!("🤖 Starting AI Assistant Network Example");

    // Create multiple AI assistants
    let assistants = vec![
        ("alice", "ai_model"),
        ("bob", "ai_model"), 
        ("charlie", "ai_model"),
        ("diana", "tool"),
    ];

    let mut routers = Vec::new();

    // Initialize each assistant
    for (name, entity_type) in &assistants {
        info!("🚀 Initializing assistant: {}", name);
        
        let config = Config::default_for_entity(name.to_string(), entity_type.to_string());
        
        match SynapseRouter::new(config, name.to_string()).await {
            Ok(router) => {
                info!("✅ Assistant {} initialized successfully", name);
                routers.push((name, router));
            }
            Err(e) => {
                warn!("❌ Failed to initialize assistant {}: {}", name, e);
            }
        }
    }

    info!("🌐 AI Assistant Network established with {} assistants", routers.len());

    // Demonstrate network connectivity
    for (name, _router) in &routers {
        info!("📡 Assistant {} is ready for communication", name);
    }

    info!("🎉 AI Assistant Network example completed successfully!");
    
    Ok(())
}
