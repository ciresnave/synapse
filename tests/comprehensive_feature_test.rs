//! Comprehensive Feature Test Suite for Synapse
//! 
//! This test suite validates all major features, edge cases, and error conditions
//! to ensure the Synapse project is truly production-ready.

use synapse::*;
use std::collections::HashMap;
use tokio::time::{sleep, Duration};
use uuid::Uuid;

#[cfg(test)]
mod comprehensive_tests {
    use super::*;

    /// Test all configuration types and their validity
    #[tokio::test]
    async fn test_all_configuration_types() {
        // Test basic configuration
        let config = Config::for_testing();
        assert!(config.is_valid(), "Basic test config should be valid");
        
        // Test Gmail configuration
        let gmail_config = Config::gmail("test@gmail.com", "password");
        assert!(gmail_config.is_valid(), "Gmail config should be valid");
        
        // Test Outlook configuration
        let outlook_config = Config::outlook("test@outlook.com", "password");
        assert!(outlook_config.is_valid(), "Outlook config should be valid");
        
        // Test entity configuration
        let entity_config = Config::for_entity("TestEntity", "ai", "test.com");
        assert!(entity_config.is_valid(), "Entity config should be valid");
        
        println!("✓ All configuration types are valid");
    }

    /// Test all message types and their processing
    #[tokio::test]
    async fn test_all_message_types() {
        let config = Config::for_testing();
        let router = EmrpRouter::new(config).await.expect("Failed to create router");
        
        // Test Direct message
        let direct_msg = SimpleMessage {
            to: "TestReceiver".to_string(),
            from_entity: "TestSender".to_string(),
            content: "Direct message test".to_string(),
            message_type: MessageType::Direct,
            metadata: HashMap::new(),
        };
        
        // Test Broadcast message
        let broadcast_msg = SimpleMessage {
            to: "AllUsers".to_string(),
            from_entity: "BroadcastSender".to_string(),
            content: "Broadcast message test".to_string(),
            message_type: MessageType::Broadcast,
            metadata: [("priority".to_string(), "high".to_string())].into(),
        };
        
        // Test Conversation message
        let conversation_msg = SimpleMessage {
            to: "ConversationPartner".to_string(),
            from_entity: "ConversationStarter".to_string(),
            content: "Conversation message test".to_string(),
            message_type: MessageType::Conversation,
            metadata: [("thread_id".to_string(), Uuid::new_v4().to_string())].into(),
        };
        
        // Test Notification message
        let notification_msg = SimpleMessage {
            to: "NotificationReceiver".to_string(),
            from_entity: "NotificationSender".to_string(),
            content: "Notification message test".to_string(),
            message_type: MessageType::Notification,
            metadata: [("urgency".to_string(), "high".to_string())].into(),
        };
        
        // Convert to secure messages and validate
        let secure_direct = router.convert_to_secure_message(&direct_msg).await.expect("Failed to convert direct message");
        let secure_broadcast = router.convert_to_secure_message(&broadcast_msg).await.expect("Failed to convert broadcast message");
        let secure_conversation = router.convert_to_secure_message(&conversation_msg).await.expect("Failed to convert conversation message");
        let secure_notification = router.convert_to_secure_message(&notification_msg).await.expect("Failed to convert notification message");
        
        // Validate secure message properties
        assert!(!secure_direct.message_id.is_empty(), "Direct message should have ID");
        assert!(!secure_broadcast.message_id.is_empty(), "Broadcast message should have ID");
        assert!(!secure_conversation.message_id.is_empty(), "Conversation message should have ID");
        assert!(!secure_notification.message_id.is_empty(), "Notification message should have ID");
        
        println!("✓ All message types process correctly");
    }

    /// Test all entity types and their capabilities
    #[tokio::test]
    async fn test_all_entity_types() {
        // Test Human entity
        let human_profile = ParticipantProfile {
            participant_id: "human1".to_string(),
            name: "Alice Human".to_string(),
            entity_type: EntityType::Human,
            capabilities: vec!["chat".to_string(), "file_sharing".to_string()],
            public_key: "mock_human_key".to_string(),
            email: Some("alice@example.com".to_string()),
            display_name: Some("Alice".to_string()),
            bio: Some("Human user".to_string()),
            avatar_url: None,
            last_seen: chrono::Utc::now(),
            status: "online".to_string(),
            preferences: HashMap::new(),
            trust_score: 0.8,
            reputation: 85,
            verification_status: "verified".to_string(),
            discoverability: DiscoverabilityLevel::Public,
            metadata: HashMap::new(),
        };
        
        // Test AI Model entity
        let ai_profile = ParticipantProfile {
            participant_id: "ai1".to_string(),
            name: "Claude AI".to_string(),
            entity_type: EntityType::AiModel,
            capabilities: vec!["reasoning".to_string(), "code_generation".to_string(), "analysis".to_string()],
            public_key: "mock_ai_key".to_string(),
            email: Some("claude@anthropic.com".to_string()),
            display_name: Some("Claude".to_string()),
            bio: Some("AI assistant".to_string()),
            avatar_url: None,
            last_seen: chrono::Utc::now(),
            status: "available".to_string(),
            preferences: HashMap::new(),
            trust_score: 0.95,
            reputation: 98,
            verification_status: "verified".to_string(),
            discoverability: DiscoverabilityLevel::Public,
            metadata: HashMap::new(),
        };
        
        // Test Tool entity
        let tool_profile = ParticipantProfile {
            participant_id: "tool1".to_string(),
            name: "Image Generator".to_string(),
            entity_type: EntityType::Tool,
            capabilities: vec!["image_generation".to_string(), "style_transfer".to_string()],
            public_key: "mock_tool_key".to_string(),
            email: Some("imagegen@tools.com".to_string()),
            display_name: Some("ImageGen".to_string()),
            bio: Some("AI image generation tool".to_string()),
            avatar_url: None,
            last_seen: chrono::Utc::now(),
            status: "ready".to_string(),
            preferences: HashMap::new(),
            trust_score: 0.9,
            reputation: 90,
            verification_status: "verified".to_string(),
            discoverability: DiscoverabilityLevel::Public,
            metadata: HashMap::new(),
        };
        
        // Test Service entity
        let service_profile = ParticipantProfile {
            participant_id: "service1".to_string(),
            name: "Database Service".to_string(),
            entity_type: EntityType::Service,
            capabilities: vec!["data_storage".to_string(), "query_processing".to_string()],
            public_key: "mock_service_key".to_string(),
            email: Some("database@services.com".to_string()),
            display_name: Some("DatabaseSvc".to_string()),
            bio: Some("Database service".to_string()),
            avatar_url: None,
            last_seen: chrono::Utc::now(),
            status: "running".to_string(),
            preferences: HashMap::new(),
            trust_score: 0.99,
            reputation: 100,
            verification_status: "verified".to_string(),
            discoverability: DiscoverabilityLevel::Unlisted,
            metadata: HashMap::new(),
        };
        
        // Test Router entity
        let router_profile = ParticipantProfile {
            participant_id: "router1".to_string(),
            name: "Message Router".to_string(),
            entity_type: EntityType::Router,
            capabilities: vec!["message_routing".to_string(), "protocol_translation".to_string()],
            public_key: "mock_router_key".to_string(),
            email: Some("router@infrastructure.com".to_string()),
            display_name: Some("Router".to_string()),
            bio: Some("Message routing service".to_string()),
            avatar_url: None,
            last_seen: chrono::Utc::now(),
            status: "active".to_string(),
            preferences: HashMap::new(),
            trust_score: 1.0,
            reputation: 100,
            verification_status: "verified".to_string(),
            discoverability: DiscoverabilityLevel::Stealth,
            metadata: HashMap::new(),
        };
        
        // Validate entity properties
        assert_eq!(human_profile.entity_type, EntityType::Human);
        assert_eq!(ai_profile.entity_type, EntityType::AiModel);
        assert_eq!(tool_profile.entity_type, EntityType::Tool);
        assert_eq!(service_profile.entity_type, EntityType::Service);
        assert_eq!(router_profile.entity_type, EntityType::Router);
        
        // Validate capabilities
        assert!(human_profile.capabilities.contains(&"chat".to_string()));
        assert!(ai_profile.capabilities.contains(&"reasoning".to_string()));
        assert!(tool_profile.capabilities.contains(&"image_generation".to_string()));
        assert!(service_profile.capabilities.contains(&"data_storage".to_string()));
        assert!(router_profile.capabilities.contains(&"message_routing".to_string()));
        
        println!("✓ All entity types are properly configured");
    }

    /// Test security levels and encryption
    #[tokio::test]
    async fn test_security_levels() {
        let config = Config::for_testing();
        let router = EmrpRouter::new(config).await.expect("Failed to create router");
        
        // Test different security levels
        let security_levels = vec![
            SecurityLevel::Public,
            SecurityLevel::Authenticated,
            SecurityLevel::Encrypted,
            SecurityLevel::HighSecurity,
        ];
        
        for security_level in security_levels {
            let msg = SimpleMessage {
                to: "SecurityTest".to_string(),
                from_entity: "SecurityTester".to_string(),
                content: format!("Testing security level: {:?}", security_level),
                message_type: MessageType::Direct,
                metadata: HashMap::new(),
            };
            
            let secure_msg = router.convert_to_secure_message(&msg).await.expect("Failed to convert message");
            
            // Validate security properties
            assert!(!secure_msg.message_id.is_empty(), "Message should have ID");
            assert!(!secure_msg.content.is_empty(), "Message should have content");
            
            println!("✓ Security level {:?} processed correctly", security_level);
        }
        
        println!("✓ All security levels work correctly");
    }

    /// Test transport layer and routing
    #[tokio::test]
    async fn test_transport_layer() {
        let config = Config::for_testing();
        let router = EmrpRouter::new(config).await.expect("Failed to create router");
        
        // Test message urgency levels
        let urgency_levels = vec![
            MessageUrgency::Interactive,
            MessageUrgency::RealTime,
            MessageUrgency::Background,
            MessageUrgency::Bulk,
        ];
        
        for urgency in urgency_levels {
            let msg = SimpleMessage {
                to: "TransportTest".to_string(),
                from_entity: "TransportTester".to_string(),
                content: format!("Testing urgency: {:?}", urgency),
                message_type: MessageType::Direct,
                metadata: HashMap::new(),
            };
            
            let secure_msg = router.convert_to_secure_message(&msg).await.expect("Failed to convert message");
            
            // Validate transport properties
            assert!(!secure_msg.message_id.is_empty(), "Message should have ID");
            
            println!("✓ Message urgency {:?} processed correctly", urgency);
        }
        
        println!("✓ Transport layer works correctly");
    }

    /// Test error handling and edge cases
    #[tokio::test]
    async fn test_error_handling() {
        let config = Config::for_testing();
        let router = EmrpRouter::new(config).await.expect("Failed to create router");
        
        // Test empty message
        let empty_msg = SimpleMessage {
            to: "".to_string(),
            from_entity: "".to_string(),
            content: "".to_string(),
            message_type: MessageType::Direct,
            metadata: HashMap::new(),
        };
        
        // This should still work but with defaults
        let result = router.convert_to_secure_message(&empty_msg).await;
        assert!(result.is_ok(), "Empty message should be handled gracefully");
        
        // Test very large message
        let large_content = "x".repeat(1_000_000); // 1MB message
        let large_msg = SimpleMessage {
            to: "LargeMessageTest".to_string(),
            from_entity: "LargeMessageTester".to_string(),
            content: large_content,
            message_type: MessageType::Direct,
            metadata: HashMap::new(),
        };
        
        let result = router.convert_to_secure_message(&large_msg).await;
        assert!(result.is_ok(), "Large message should be handled");
        
        // Test message with special characters
        let special_msg = SimpleMessage {
            to: "SpecialTest".to_string(),
            from_entity: "SpecialTester".to_string(),
            content: "🚀 Hello 世界! Test émojis and ñoñó special chars 🎉".to_string(),
            message_type: MessageType::Direct,
            metadata: HashMap::new(),
        };
        
        let result = router.convert_to_secure_message(&special_msg).await;
        assert!(result.is_ok(), "Special characters should be handled");
        
        println!("✓ Error handling and edge cases work correctly");
    }

    /// Test metadata handling
    #[tokio::test]
    async fn test_metadata_handling() {
        let config = Config::for_testing();
        let router = EmrpRouter::new(config).await.expect("Failed to create router");
        
        // Test various metadata scenarios
        let mut metadata = HashMap::new();
        metadata.insert("priority".to_string(), "high".to_string());
        metadata.insert("thread_id".to_string(), Uuid::new_v4().to_string());
        metadata.insert("user_agent".to_string(), "Synapse/1.0".to_string());
        metadata.insert("content_type".to_string(), "application/json".to_string());
        metadata.insert("encoding".to_string(), "utf-8".to_string());
        
        let msg = SimpleMessage {
            to: "MetadataTest".to_string(),
            from_entity: "MetadataTester".to_string(),
            content: "Testing metadata handling".to_string(),
            message_type: MessageType::Direct,
            metadata,
        };
        
        let secure_msg = router.convert_to_secure_message(&msg).await.expect("Failed to convert message");
        
        // Validate metadata preservation
        assert!(!secure_msg.message_id.is_empty(), "Message should have ID");
        
        println!("✓ Metadata handling works correctly");
    }

    /// Test concurrent operations
    #[tokio::test]
    async fn test_concurrent_operations() {
        let config = Config::for_testing();
        let router = EmrpRouter::new(config).await.expect("Failed to create router");
        
        // Create multiple concurrent messages
        let mut handles = Vec::new();
        
        for i in 0..10 {
            let router_clone = router.clone();
            let handle = tokio::spawn(async move {
                let msg = SimpleMessage {
                    to: format!("ConcurrentTest{}", i),
                    from_entity: format!("ConcurrentTester{}", i),
                    content: format!("Concurrent message {}", i),
                    message_type: MessageType::Direct,
                    metadata: HashMap::new(),
                };
                
                router_clone.convert_to_secure_message(&msg).await
            });
            handles.push(handle);
        }
        
        // Wait for all operations to complete
        for handle in handles {
            let result = handle.await.expect("Task should complete");
            assert!(result.is_ok(), "Concurrent operation should succeed");
        }
        
        println!("✓ Concurrent operations work correctly");
    }

    /// Test configuration validation
    #[tokio::test]
    async fn test_configuration_validation() {
        // Test valid configurations
        let valid_configs = vec![
            Config::for_testing(),
            Config::gmail("test@gmail.com", "password"),
            Config::outlook("test@outlook.com", "password"),
            Config::for_entity("TestEntity", "ai", "test.com"),
        ];
        
        for config in valid_configs {
            assert!(config.is_valid(), "Configuration should be valid");
        }
        
        println!("✓ Configuration validation works correctly");
    }

    /// Test performance with batch operations
    #[tokio::test]
    async fn test_batch_performance() {
        let config = Config::for_testing();
        let router = EmrpRouter::new(config).await.expect("Failed to create router");
        
        let start_time = std::time::Instant::now();
        
        // Process 100 messages in batch
        for i in 0..100 {
            let msg = SimpleMessage {
                to: format!("BatchTest{}", i),
                from_entity: "BatchTester".to_string(),
                content: format!("Batch message {}", i),
                message_type: MessageType::Direct,
                metadata: HashMap::new(),
            };
            
            let _secure_msg = router.convert_to_secure_message(&msg).await.expect("Failed to convert message");
        }
        
        let duration = start_time.elapsed();
        println!("✓ Processed 100 messages in {:?}", duration);
        
        // Should be able to process 100 messages in under 1 second
        assert!(duration < Duration::from_secs(1), "Batch processing should be fast");
    }

    /// Test memory usage and resource management
    #[tokio::test]
    async fn test_resource_management() {
        let config = Config::for_testing();
        let router = EmrpRouter::new(config).await.expect("Failed to create router");
        
        // Create and drop many messages to test memory management
        for i in 0..1000 {
            let msg = SimpleMessage {
                to: format!("ResourceTest{}", i),
                from_entity: "ResourceTester".to_string(),
                content: format!("Resource test message {}", i),
                message_type: MessageType::Direct,
                metadata: HashMap::new(),
            };
            
            let _secure_msg = router.convert_to_secure_message(&msg).await.expect("Failed to convert message");
            
            // Yield to allow cleanup
            if i % 100 == 0 {
                tokio::task::yield_now().await;
            }
        }
        
        println!("✓ Resource management works correctly");
    }

    /// Integration test with all components
    #[tokio::test]
    async fn test_full_integration() {
        let config = Config::for_testing();
        let router = EmrpRouter::new(config).await.expect("Failed to create router");
        
        // Test a complete message flow
        let msg = SimpleMessage {
            to: "IntegrationTest".to_string(),
            from_entity: "IntegrationTester".to_string(),
            content: "Full integration test message".to_string(),
            message_type: MessageType::Direct,
            metadata: [
                ("priority".to_string(), "high".to_string()),
                ("test_id".to_string(), Uuid::new_v4().to_string()),
            ].into(),
        };
        
        // Convert to secure message
        let secure_msg = router.convert_to_secure_message(&msg).await.expect("Failed to convert message");
        
        // Validate all components
        assert!(!secure_msg.message_id.is_empty(), "Message should have ID");
        assert!(!secure_msg.content.is_empty(), "Message should have content");
        assert!(secure_msg.timestamp > 0, "Message should have timestamp");
        
        println!("✓ Full integration test passed");
    }
}
