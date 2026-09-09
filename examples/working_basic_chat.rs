//! Working Basic Chat Example
//!
//! This demonstrates simple message exchange using the actual implemented API

use anyhow::Result;
use synapse::{Config, types::SimpleMessage};
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    info!("💬 Starting Working Basic Chat Demo");

    // Create configurations
    let _config = Config::for_testing();

    info!("🔗 Configuration loaded successfully");

    // Demo message creation and processing
    demo_message_exchange().await?;

    info!("✅ Basic Chat Demo completed!");
    Ok(())
}

async fn demo_message_exchange() -> Result<()> {
    // Create a simple message
    let message = SimpleMessage::new("Bob", "Alice", "Hello Bob! How are you today?");

    info!(
        "✉️ Created message from {} to {}",
        message.from_entity, message.to
    );
    info!("📝 Content: {}", message.content);

    // Create a response message
    let response_message = SimpleMessage::new(
        "Alice",
        "Bob",
        "Hi Alice! I'm doing great, thanks for asking!",
    );

    info!(
        "📤 Created response from {} to {}",
        response_message.from_entity, response_message.to
    );
    info!("📝 Response: {}", response_message.content);

    // Show message exchange summary
    info!("� Message Exchange Summary:");
    info!(
        "   1. {} → {}: {}",
        message.from_entity, message.to, message.content
    );
    info!(
        "   2. {} → {}: {}",
        response_message.from_entity, response_message.to, response_message.content
    );

    info!("✅ Message exchange completed successfully!");
    Ok(())
}
