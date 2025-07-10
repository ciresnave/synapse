//! Tool Interaction Example for Synapse
//!
//! This example demonstrates how AI entities can call tools and receive responses.

use synapse::{
    Config,
    types::SimpleMessage,
};
use anyhow::Result;
use tracing::info;
use serde_json::json;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    info!("🔧 Starting Tool Interaction Demo");

    // Set up entity configurations
    demo_tool_interaction_setup().await?;
    
    info!("🔧 Tool Interaction Demo completed!");
    Ok(())
}

async fn demo_tool_interaction_setup() -> Result<()> {
    // Create configurations for different entities
    let ai_config = Config::default_for_entity("Claude", "AiModel");
    let tool_config = Config::default_for_entity("FileSystem", "Tool");
    let service_config = Config::default_for_entity("Calculator", "Service");

    info!("✅ Entity configurations created:");
    info!("   AI: {} ({})", ai_config.entity.local_name, ai_config.entity.entity_type);
    info!("   Tool: {} ({})", tool_config.entity.local_name, tool_config.entity.entity_type);
    info!("   Service: {} ({})", service_config.entity.local_name, service_config.entity.entity_type);

    // Simulate tool interaction scenarios
    demo_file_operations().await?;
    demo_calculation_request().await?;
    demo_tool_capabilities().await?;

    Ok(())
}

async fn demo_file_operations() -> Result<()> {
    info!("📁 === File Operations Demo ===");

    // AI requests file listing
    let list_request = SimpleMessage::new(
        "Claude",
        "FileSystem", 
        json!({
            "action": "list_files",
            "path": "/workspace",
            "options": {
                "include_hidden": false,
                "recursive": false
            }
        }).to_string()
    );

    info!("🤖 Claude → FileSystem: {}", "Requesting file list");

    // FileSystem responds with file listing
    let list_response = SimpleMessage::new(
        "FileSystem",
        "Claude",
        json!({
            "status": "success",
            "files": [
                {"name": "document.txt", "size": 1024, "type": "file"},
                {"name": "images", "size": 0, "type": "directory"},
                {"name": "config.json", "size": 512, "type": "file"}
            ]
        }).to_string()
    );

    info!("📁 FileSystem → Claude: Found 3 items");

    // AI requests file content
    let read_request = SimpleMessage::new(
        "Claude", 
        "FileSystem",
        json!({
            "action": "read_file",
            "path": "/workspace/document.txt"
        }).to_string()
    );

    info!("🤖 Claude → FileSystem: Reading document.txt");

    let read_response = SimpleMessage::new(
        "FileSystem",
        "Claude", 
        json!({
            "status": "success",
            "content": "This is the content of the document.",
            "encoding": "utf-8"
        }).to_string()
    );

    info!("📄 FileSystem → Claude: File content delivered");

    Ok(())
}

async fn demo_calculation_request() -> Result<()> {
    info!("🧮 === Calculation Demo ===");

    // AI requests mathematical calculation
    let calc_request = SimpleMessage::new(
        "Claude",
        "Calculator",
        json!({
            "operation": "complex_math",
            "expression": "sqrt(144) + pow(2, 3) * 5",
            "precision": 10
        }).to_string()
    );

    info!("🤖 Claude → Calculator: Requesting calculation");

    // Calculator responds with result
    let calc_response = SimpleMessage::new(
        "Calculator", 
        "Claude",
        json!({
            "status": "success", 
            "result": 52.0,
            "expression": "sqrt(144) + pow(2, 3) * 5",
            "steps": [
                "sqrt(144) = 12",
                "pow(2, 3) = 8", 
                "8 * 5 = 40",
                "12 + 40 = 52"
            ]
        }).to_string()
    );

    info!("🧮 Calculator → Claude: Result = 52.0");

    Ok(())
}

async fn demo_tool_capabilities() -> Result<()> {
    info!("⚙️ === Tool Capabilities Demo ===");

    // AI queries tool capabilities
    let capabilities_request = SimpleMessage::new(
        "Claude",
        "FileSystem",
        json!({
            "action": "get_capabilities"
        }).to_string()
    );

    info!("🤖 Claude → FileSystem: Requesting capabilities");

    // Tool responds with its capabilities
    let capabilities_response = SimpleMessage::new(
        "FileSystem",
        "Claude",
        json!({
            "status": "success",
            "capabilities": {
                "file_operations": ["read", "write", "list", "delete", "move"],
                "supported_formats": ["text", "json", "csv", "markdown"],
                "max_file_size": "10MB",
                "permissions": ["read", "write"],
                "features": ["recursive_search", "file_watching", "backup"]
            },
            "version": "1.0.0"
        }).to_string()
    );

    info!("⚙️ FileSystem → Claude: Capabilities shared");
    info!("🎯 Tool interaction patterns demonstrated successfully!");

    Ok(())
}
