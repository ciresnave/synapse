//! Basic Chat Example
//! 
//! This example demonstrates how to create a simple chat application
//! using the synapse library for message routing.

use synapse::{Config, SynapseRouter, SimpleMessage, MessageType};
use anyhow::Result;
use tracing::{info, debug};
use std::collections::HashMap;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    info!("💬 Starting Basic Chat Example");

    // Create two chat participants
    let alice_config = Config::default_for_entity("alice".to_string(), "user".to_string());
    let bob_config = Config::default_for_entity("bob".to_string(), "user".to_string());

    // Initialize routers for both participants
    let alice_router = SynapseRouter::new(alice_config, "alice".to_string()).await?;
    let bob_router = SynapseRouter::new(bob_config, "bob".to_string()).await?;
    
    info!("✅ Chat participants Alice and Bob initialized");

    // Simulate a conversation
    let messages = vec![
        ("alice", "bob", "Hello Bob! How are you today?"),
        ("bob", "alice", "Hi Alice! I'm doing great, thanks for asking. How about you?"),
        ("alice", "bob", "I'm doing well too! Are you working on anything interesting?"),
        ("bob", "alice", "Yes! I'm learning about distributed messaging systems. It's fascinating!"),
        ("alice", "bob", "That sounds really cool! I'd love to hear more about it sometime."),
    ];

    info!("🗨️  Starting conversation simulation...");

    for (from, to, content) in messages {
        // Create the message
        let message = SimpleMessage {
            to: to.to_string(),
            from_entity: from.to_string(),
            content: content.to_string(),
            message_type: MessageType::Direct,
            metadata: HashMap::new(),
        };

        info!("📨 {}: {}", from, content);
        debug!("Message details: from={} to={} type={:?}", from, to, message.message_type);

        // Convert to secure message format
        let router = if from == "alice" { &alice_router } else { &bob_router };
        
        match router.convert_to_secure_message(&message).await {
            Ok(secure_msg) => {
                debug!("✅ Message secured with ID: {}", secure_msg.message_id);
            }
            Err(e) => {
                debug!("⚠️  Message prepared (secure conversion failed: {})", e);
            }
        }

        // Add a small delay to simulate real conversation timing
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    }

    info!("💬 Conversation completed successfully!");
    info!("🎉 Basic Chat example finished!");
    
    Ok(())
}
