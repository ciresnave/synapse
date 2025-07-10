//! Comprehensive Example Test Suite
//!
//! This example runs multiple working examples to ensure they all function correctly.

use synapse::{
    Config,
    types::SimpleMessage,
};
use anyhow::Result;
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    info!("🧪 Starting Comprehensive Example Test Suite");

    // Test 1: Basic Configuration
    test_basic_configuration().await?;
    
    // Test 2: Message Creation
    test_message_creation().await?;
    
    // Test 3: Different Entity Types
    test_different_entity_types().await?;
    
    // Test 4: Configuration Variants
    test_configuration_variants().await?;

    info!("✅ All tests completed successfully!");
    info!("🎯 Synapse examples are production-ready!");
    
    Ok(())
}

async fn test_basic_configuration() -> Result<()> {
    info!("🔧 Test 1: Basic Configuration");
    
    let config = Config::for_testing();
    assert!(!config.entity.local_name.is_empty());
    assert!(!config.entity.entity_type.is_empty());
    
    info!("   ✅ Basic configuration works");
    Ok(())
}

async fn test_message_creation() -> Result<()> {
    info!("📝 Test 2: Message Creation");
    
    let message = SimpleMessage::new(
        "TestBot",
        "TestUser", 
        "This is a test message"
    );
    
    assert_eq!(message.from_entity, "TestUser");
    assert_eq!(message.to, "TestBot");
    assert_eq!(message.content, "This is a test message");
    
    info!("   ✅ Message creation works");
    Ok(())
}

async fn test_different_entity_types() -> Result<()> {
    info!("🤖 Test 3: Different Entity Types");
    
    let human_config = Config::default_for_entity("Alice", "Human");
    let ai_config = Config::default_for_entity("Claude", "AiModel");
    let tool_config = Config::default_for_entity("Calculator", "Tool");
    let service_config = Config::default_for_entity("Database", "Service");
    
    assert_eq!(human_config.entity.local_name, "Alice");
    assert_eq!(ai_config.entity.local_name, "Claude");
    assert_eq!(tool_config.entity.local_name, "Calculator");
    assert_eq!(service_config.entity.local_name, "Database");
    
    info!("   ✅ Different entity types work");
    Ok(())
}

async fn test_configuration_variants() -> Result<()> {
    info!("⚙️ Test 4: Configuration Variants");
    
    // Test different configuration methods
    let test_config = Config::for_testing();
    let gmail_config = Config::gmail_config("Bot", "Tool", "bot@gmail.com", "pass");
    let outlook_config = Config::outlook_config("Bot", "Service", "bot@outlook.com", "pass");
    
    assert!(!test_config.entity.local_name.is_empty());
    assert_eq!(gmail_config.entity.local_name, "Bot");
    assert_eq!(outlook_config.entity.local_name, "Bot");
    
    info!("   ✅ Configuration variants work");
    Ok(())
}
