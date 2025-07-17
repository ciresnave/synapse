//! Tool Interaction Example
//! 
//! This example demonstrates how to create tool interactions
//! using the synapse library for AI-tool communication.

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

    info!("🔧 Starting Tool Interaction Example");

    // Create AI and tool entities
    let ai_config = Config::default_for_entity("ai_assistant".to_string(), "ai_model".to_string());
    let tool_config = Config::default_for_entity("calculator_tool".to_string(), "tool".to_string());

    // Initialize routers
    let ai_router = SynapseRouter::new(ai_config, "ai_assistant".to_string()).await?;
    let tool_router = SynapseRouter::new(tool_config, "calculator_tool".to_string()).await?;
    
    info!("✅ AI assistant and calculator tool initialized");

    // Simulate tool interaction workflow
    let interactions = vec![
        ("list_capabilities", "What can you do?"),
        ("calculate", "14 + 28"),
        ("calculate", "150 / 3"),
        ("calculate", "2^10"),
        ("validate", "Is 42 a valid number?"),
    ];

    info!("🤖 Starting AI-Tool interaction simulation...");

    for (operation, query) in interactions {
        // AI sends request to tool
        let request = SimpleMessage {
            to: "calculator_tool".to_string(),
            from_entity: "ai_assistant".to_string(),
            content: format!("{}:{}", operation, query),
            message_type: MessageType::Direct,
            metadata: create_tool_metadata(operation),
        };

        info!("🔄 AI → Tool: {} ({})", query, operation);
        debug!("Request: {}", request.content);

        // Convert to secure message
        match ai_router.convert_to_secure_message(&request).await {
            Ok(secure_msg) => {
                debug!("✅ Request secured: {}", secure_msg.message_id);
            }
            Err(e) => {
                debug!("⚠️  Request prepared (secure conversion failed: {})", e);
            }
        }

        // Tool generates response
        let response_content = generate_tool_response(operation, query);
        
        let response = SimpleMessage {
            to: "ai_assistant".to_string(),
            from_entity: "calculator_tool".to_string(),
            content: response_content.clone(),
            message_type: MessageType::Direct,
            metadata: create_response_metadata(operation),
        };

        info!("🔧 Tool → AI: {}", response_content);

        // Convert tool response to secure message
        match tool_router.convert_to_secure_message(&response).await {
            Ok(secure_msg) => {
                debug!("✅ Response secured: {}", secure_msg.message_id);
            }
            Err(e) => {
                debug!("⚠️  Response prepared (secure conversion failed: {})", e);
            }
        }

        // Add delay between interactions
        tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
    }

    info!("🎉 Tool Interaction example completed successfully!");
    
    Ok(())
}

/// Create metadata for tool operations
fn create_tool_metadata(operation: &str) -> HashMap<String, String> {
    let mut metadata = HashMap::new();
    
    metadata.insert("operation".to_string(), operation.to_string());
    metadata.insert("tool_type".to_string(), "calculator".to_string());
    metadata.insert("timestamp".to_string(), 
                    chrono::Utc::now().to_rfc3339());
    metadata.insert("request_id".to_string(), 
                    format!("req-{}", uuid::Uuid::new_v4()));
    
    metadata
}

/// Create metadata for tool responses
fn create_response_metadata(operation: &str) -> HashMap<String, String> {
    let mut metadata = HashMap::new();
    
    metadata.insert("response_to".to_string(), operation.to_string());
    metadata.insert("tool_type".to_string(), "calculator".to_string());
    metadata.insert("timestamp".to_string(), 
                    chrono::Utc::now().to_rfc3339());
    metadata.insert("response_id".to_string(), 
                    format!("resp-{}", uuid::Uuid::new_v4()));
    
    metadata
}

/// Generate tool responses based on operation and query
fn generate_tool_response(operation: &str, query: &str) -> String {
    match operation {
        "list_capabilities" => {
            "I can perform basic mathematical operations: addition (+), subtraction (-), multiplication (*), division (/), and exponentiation (^)".to_string()
        }
        "calculate" => {
            if query.contains("14 + 28") {
                "Result: 42".to_string()
            } else if query.contains("150 / 3") {
                "Result: 50".to_string()
            } else if query.contains("2^10") {
                "Result: 1024".to_string()
            } else {
                "Error: Unable to parse mathematical expression".to_string()
            }
        }
        "validate" => {
            if query.contains("42") {
                "Yes, 42 is a valid number and quite famous in certain circles!".to_string()
            } else {
                "Validation result depends on the specific context".to_string()
            }
        }
        _ => {
            "Error: Unknown operation".to_string()
        }
    }
}
