//! Connectivity Demo for Synapse
//!
//! This example demonstrates basic connectivity concepts and configuration.

use anyhow::Result;
use synapse::{Config, types::SimpleMessage};
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    info!("🌐 Starting Connectivity Demo");

    // Create different types of configurations
    demo_different_config_types().await?;

    info!("🌐 Connectivity Demo completed!");
    Ok(())
}

async fn demo_different_config_types() -> Result<()> {
    // Test configuration
    let test_config = Config::for_testing();
    info!("🧪 Test config created: {}", test_config.entity.local_name);

    // Gmail configuration example
    let gmail_config =
        Config::gmail_config("MyBot", "Tool", "mybot@gmail.com", "app_password_here");
    info!(
        "📧 Gmail config created: {}",
        gmail_config.entity.local_name
    );

    // Outlook configuration example
    let outlook_config = Config::outlook_config(
        "OfficeBot",
        "Service",
        "officebot@outlook.com",
        "password_here",
    );
    info!(
        "🏢 Outlook config created: {}",
        outlook_config.entity.local_name
    );

    // Default configuration for an entity
    let entity_config = Config::default_for_entity("NetworkBot", "AiModel");
    info!(
        "🤖 Entity config created: {}",
        entity_config.entity.local_name
    );

    // Create a sample message for the demo
    let message = SimpleMessage::new(
        "ConnectivityDemo",
        "NetworkTest",
        "Testing connectivity configurations",
    );

    info!("📤 Demo message created: {}", message.content);
    info!("🎯 Demonstrated different configuration types for various network setups");

    Ok(())
}
