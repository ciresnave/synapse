//! AI Assistant Stub Example
//! 
//! This example demonstrates how to create a simple AI assistant stub
//! that can respond to basic queries using the synapse library.

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

    info!("🤖 Starting AI Assistant Stub Example");

    // Create an AI assistant configuration
    let config = Config::default_for_entity("assistant_stub".to_string(), "ai_model".to_string());
    
    // Initialize the router
    let router = SynapseRouter::new(config, "assistant_stub".to_string()).await?;
    info!("✅ AI Assistant stub initialized successfully");

    // Simulate receiving a query
    let query_message = SimpleMessage {
        to: "assistant_stub".to_string(),
        from_entity: "user".to_string(),
        content: "Hello, can you help me?".to_string(),
        message_type: MessageType::Direct,
        metadata: HashMap::new(),
    };

    info!("📨 Received query: {}", query_message.content);
    debug!("Query details: from={}, type={:?}", query_message.from_entity, query_message.message_type);

    // Generate a response
    let response_content = generate_response(&query_message.content);
    
    let response_message = SimpleMessage {
        to: query_message.from_entity,
        from_entity: "assistant_stub".to_string(),
        content: response_content,
        message_type: MessageType::Direct,
        metadata: HashMap::new(),
    };

    info!("📤 Generated response: {}", response_message.content);
    
    // Convert to secure message format (for demonstration)
    match router.convert_to_secure_message(&response_message).await {
        Ok(secure_msg) => {
            info!("✅ Response successfully prepared for secure transmission");
            debug!("Secure message ID: {}", secure_msg.message_id);
        }
        Err(e) => {
            info!("⚠️  Response prepared (secure conversion failed: {})", e);
        }
    }

    info!("🎉 AI Assistant Stub example completed successfully!");
    
    Ok(())
}

/// Generate a simple response to a query
fn generate_response(query: &str) -> String {
    let query_lower = query.to_lowercase();
    
    if query_lower.contains("hello") || query_lower.contains("hi") {
        "Hello! I'm an AI assistant. How can I help you today?".to_string()
    } else if query_lower.contains("help") {
        "I'm here to help! I can answer questions and assist with various tasks.".to_string()
    } else if query_lower.contains("time") {
        format!("The current time is approximately {}", 
                chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC"))
    } else if query_lower.contains("weather") {
        "I don't have access to real-time weather data, but I'd be happy to help with other questions!".to_string()
    } else {
        "That's an interesting question! I'm a simple AI stub, so my responses are quite basic. Is there something specific I can help you with?".to_string()
    }
}
