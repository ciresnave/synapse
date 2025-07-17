//! Simple integration test to verify basic functionality

use synapse::{Config, SynapseRouter, SimpleMessage, MessageType};
use tokio;

#[cfg(test)]
mod basic_integration_tests {
    use super::*;

    #[tokio::test]
    async fn test_basic_configuration() {
        // Test that we can create basic configurations
        let config = Config::for_testing();
        assert!(config.validate().is_ok(), "Test config should be valid");
        
        let gmail_config = Config::gmail_config("test", "ai_model", "test@gmail.com", "password");
        assert!(gmail_config.validate().is_ok(), "Gmail config should be valid");
    }

    #[tokio::test]
    async fn test_router_creation() {
        // Test basic router creation
        let config = Config::for_testing();
        let router = SynapseRouter::new(config, "test-entity".to_string()).await;
        assert!(router.is_ok(), "Router creation should succeed");
    }

    #[tokio::test]
    async fn test_simple_message_creation() {
        // Test basic message creation
        let message = SimpleMessage {
            to: "bob".to_string(),
            from_entity: "alice".to_string(),
            content: "Hello, Bob!".to_string(),
            message_type: MessageType::Direct,
            metadata: std::collections::HashMap::new(),
        };
        
        assert_eq!(message.to, "bob");
        assert_eq!(message.from_entity, "alice");
        assert_eq!(message.content, "Hello, Bob!");
    }

    #[tokio::test]
    async fn test_message_conversion() {
        // Test converting SimpleMessage to SecureMessage
        let config = Config::for_testing();
        let router = SynapseRouter::new(config, "test-entity".to_string()).await
            .expect("Router creation should succeed");
        
        let simple_message = SimpleMessage {
            to: "bob".to_string(),
            from_entity: "alice".to_string(),
            content: "Test message".to_string(),
            message_type: MessageType::Direct,
            metadata: std::collections::HashMap::new(),
        };
        
        let secure_message = router.convert_to_secure_message(&simple_message).await;
        assert!(secure_message.is_ok(), "Message conversion should succeed");
    }
}
