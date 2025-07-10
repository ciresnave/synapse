//! Basic Chat Example for Synapse
//!
//! This example demonstrates simple message exchange between two entities.

use synapse::{
    Config,
    types::{SimpleMessage, MessageType},
};
use anyhow::Result;
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    info!("💬 Starting Basic Chat Demo");

    // Create configurations for two entities
    let alice_config = Config::default_for_entity("Alice", "Human");
    let bob_config = Config::default_for_entity("Bob", "Human");
    
    info!("✅ Configurations created:");
    info!("   Alice: {} ({})", alice_config.entity.local_name, alice_config.entity.entity_type);
    info!("   Bob: {} ({})", bob_config.entity.local_name, bob_config.entity.entity_type);

    // Simulate a conversation
    demo_conversation().await?;
    
    info!("💬 Basic Chat Demo completed!");
    Ok(())
}

async fn demo_conversation() -> Result<()> {
    // Alice sends a message to Bob
    let message1 = SimpleMessage::new(
        "Alice",
        "Bob",
        "Hey Bob! How are you doing?"
    );

    info!("📤 Alice → Bob: {}", message1.content);

    // Bob responds to Alice
    let message2 = SimpleMessage::new(
        "Bob",
        "Alice",
        "Hi Alice! I'm doing great, thanks for asking. How about you?"
    );

    info!("📤 Bob → Alice: {}", message2.content);

    // Alice replies
    let message3 = SimpleMessage::new(
        "Alice",
        "Bob",
        "I'm doing well too! Want to grab coffee later?"
    );

    info!("📤 Alice → Bob: {}", message3.content);

    // Bob confirms
    let message4 = SimpleMessage::new(
        "Bob",
        "Alice",
        "Sounds great! How about 3 PM at the usual place?"
    );

    info!("📤 Bob → Alice: {}", message4.content);

    info!("☕ Conversation complete - coffee date planned!");
    info!("🎯 This demonstrates basic message creation and structure");
    
    Ok(())
}
