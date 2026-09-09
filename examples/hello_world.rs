/*!
 * Hello World Example for Synapse
 *
 * This is the simplest possible Synapse application. It demonstrates:
 * - Basic configuration setup
 * - Creating simple messages
 * - Minimal working example
 *
 * Run with: cargo run --example hello_world
 */

use anyhow::Result;
use synapse::{Config, types::SimpleMessage};
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    info!("👋 Starting Hello World Synapse Demo");

    // Create a test configuration
    let config = Config::for_testing();

    info!("✅ Configuration created:");
    info!(
        "   Entity: {} ({})",
        config.entity.local_name, config.entity.entity_type
    );
    info!("   Email: {}", config.email.smtp.username);

    // Create a simple message
    let message = SimpleMessage::new(
        "HelloBot",
        "Alice",
        "Hello Alice! This is my first Synapse message.",
    );

    info!("📤 Created message:");
    info!("   From: {}", message.from_entity);
    info!("   To: {}", message.to);
    info!("   Content: {}", message.content);

    info!("👋 Hello World Demo completed!");
    info!("🎯 This demonstrates basic Synapse types and configuration");
    Ok(())
}
