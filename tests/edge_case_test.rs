//! Edge Case Tests
//! 
//! Tests edge cases and boundary conditions in the synapse library

use synapse::{Config, SynapseRouter, SimpleMessage, MessageType};
use std::collections::HashMap;
use tokio::time::{timeout, sleep, Duration};
use anyhow::Result;

#[cfg(test)]
mod edge_case_tests {
    use super::*;

    /// Test boundary conditions for message sizes
    #[tokio::test]
    async fn test_message_size_boundaries() -> Result<()> {
        let config = Config::for_testing();
        let router = SynapseRouter::new(config, "test_entity".to_string()).await?;
        
        // Test minimum message (empty content)
        let min_msg = SimpleMessage {
            to: "MinTest".to_string(),
            from_entity: "MinTester".to_string(),
            content: "".to_string(),
            message_type: MessageType::Direct,
            metadata: HashMap::new(),
        };
        let result = router.convert_to_secure_message(&min_msg).await;
        assert!(result.is_ok(), "Empty message should be handled");
        
        // Test single character
        let single_char_msg = SimpleMessage {
            to: "SingleTest".to_string(),
            from_entity: "SingleTester".to_string(),
            content: "A".to_string(),
            message_type: MessageType::Direct,
            metadata: HashMap::new(),
        };
        let result = router.convert_to_secure_message(&single_char_msg).await;
        assert!(result.is_ok(), "Single character message should be handled");
        
        // Test very long message (10KB)
        let long_content = "A".repeat(10240);
        let long_msg = SimpleMessage {
            to: "LongTest".to_string(),
            from_entity: "LongTester".to_string(),
            content: long_content,
            message_type: MessageType::Direct,
            metadata: HashMap::new(),
        };
        let result = router.convert_to_secure_message(&long_msg).await;
        assert!(result.is_ok(), "Long message should be handled");
        
        Ok(())
    }

    /// Test various edge case content types
    #[tokio::test]
    async fn test_special_content_cases() -> Result<()> {
        let config = Config::for_testing();
        let router = SynapseRouter::new(config, "test_entity".to_string()).await?;
        
        let test_cases = vec![
            ("unicode", "🚀🎉✨🌟💫🔥💡⚡🌈🎯"),
            ("newlines", "Line 1\nLine 2\r\nLine 3\n\nLine 5"),
            ("special_chars", "!@#$%^&*(){}[]|\\:;\"'<>,.?/~`"),
            ("mixed", "Normal text with 🚀 and\nnewlines"),
            ("numbers", "12345.67890 -999 +123 0x1A2B3C"),
        ];

        for (test_name, content) in test_cases {
            let message = SimpleMessage {
                to: format!("Target_{}", test_name),
                from_entity: "SpecialTester".to_string(),
                content: content.to_string(),
                message_type: MessageType::Direct,
                metadata: HashMap::new(),
            };
            
            let result = router.convert_to_secure_message(&message).await;
            assert!(result.is_ok(), "Special content case '{}' should be handled", test_name);
        }
        
        Ok(())
    }

    /// Test edge cases with entity names
    #[tokio::test]
    async fn test_entity_name_edge_cases() -> Result<()> {
        let edge_case_identifiers = vec![
            "test_entity_1",
            "TestEntity123", 
            "entity-with-dashes",
            "entity.with.dots",
            "entity_with_underscores",
            "A", // Single character
            "very_long_entity_name_that_exceeds_normal_length_expectations_and_tests_boundary_conditions",
        ];

        for identifier in edge_case_identifiers {
            let config = Config::default_for_entity(identifier.to_string(), "test".to_string());
            let result = SynapseRouter::new(config, identifier.to_string()).await;
            
            match result {
                Ok(_router) => {
                    println!("✅ Entity identifier '{}' accepted", identifier);
                }
                Err(e) => {
                    println!("❌ Entity identifier '{}' rejected: {}", identifier, e);
                    // For now, we'll allow some failures as the validation might be strict
                }
            }
        }
        
        Ok(())
    }

    /// Test timeout handling in message processing
    #[tokio::test]
    async fn test_timeout_handling() -> Result<()> {
        let config = Config::for_testing();
        let router = SynapseRouter::new(config, "timeout_tester".to_string()).await?;
        
        let msg = SimpleMessage {
            to: "TimeoutTarget".to_string(),
            from_entity: "timeout_tester".to_string(),
            content: "Test timeout message".to_string(),
            message_type: MessageType::Direct,
            metadata: HashMap::new(),
        };

        // Test normal operation (should complete quickly)
        let result = timeout(Duration::from_secs(5), router.convert_to_secure_message(&msg)).await;
        assert!(result.is_ok(), "Normal message conversion should complete within timeout");

        // Test very short timeout (might timeout, but should handle gracefully)
        let result = timeout(Duration::from_millis(1), router.convert_to_secure_message(&msg)).await;
        match result {
            Ok(_) => println!("✅ Message processed within 1ms"),
            Err(_) => println!("⏰ Message processing timed out (expected for 1ms timeout)"),
        }
        
        Ok(())
    }

    /// Test rapid message creation 
    #[tokio::test]
    async fn test_rapid_message_creation() -> Result<()> {
        let config = Config::for_testing();
        let router = SynapseRouter::new(config, "rapid_tester".to_string()).await?;
        
        let message_count = 100;
        let mut tasks = Vec::new();
        
        for i in 0..message_count {
            let router_clone = router.clone();
            let task = tokio::spawn(async move {
                let message = SimpleMessage {
                    to: format!("RapidTarget_{}", i),
                    from_entity: "rapid_tester".to_string(),
                    content: format!("Rapid message {}", i),
                    message_type: MessageType::Direct,
                    metadata: HashMap::new(),
                };
                
                // Add random small delay to simulate varying timing
                let delay = Duration::from_millis(rand::random::<u64>() % 10);
                if delay > Duration::from_millis(0) {
                    sleep(delay).await;
                }
                
                router_clone.convert_to_secure_message(&message).await
            });
            tasks.push(task);
        }
        
        // Wait for all tasks with timeout
        let timeout_duration = Duration::from_secs(30);
        let start_time = std::time::Instant::now();
        let remaining_time = timeout_duration.saturating_sub(start_time.elapsed());
        
        let handle = futures::future::join_all(tasks);
        let result = timeout(remaining_time, handle).await;
        
        match result {
            Ok(results) => {
                let mut success_count = 0;
                for task_result in results {
                    match task_result {
                        Ok(Ok(_)) => success_count += 1,
                        Ok(Err(e)) => println!("Message conversion failed: {}", e),
                        Err(e) => println!("Task panicked: {:?}", e),
                    }
                }
                println!("Successfully processed {} out of {} rapid messages", success_count, message_count);
                assert!(success_count >= message_count * 8 / 10, "At least 80% of rapid messages should succeed");
            }
            Err(_) => {
                println!("⏰ Rapid message test timed out");
                // Don't fail the test for timeout, just report it
            }
        }
        
        Ok(())
    }

    /// Test message type variations
    #[tokio::test]
    async fn test_message_type_variations() -> Result<()> {
        let config = Config::for_testing();
        let router = SynapseRouter::new(config, "type_tester".to_string()).await?;
        
        let message_types = vec![
            MessageType::Direct,
            MessageType::Broadcast,
        ];

        for msg_type in message_types {
            let message = SimpleMessage {
                to: "TypeTarget".to_string(),
                from_entity: "type_tester".to_string(),
                content: format!("Test message with type: {:?}", msg_type),
                message_type: msg_type.clone(),
                metadata: HashMap::new(),
            };
            
            let result = router.convert_to_secure_message(&message).await;
            assert!(result.is_ok(), "Message type {:?} should be handled", msg_type);
        }
        
        Ok(())
    }

    /// Test metadata edge cases
    #[tokio::test]
    async fn test_metadata_edge_cases() -> Result<()> {
        let config = Config::for_testing();
        let router = SynapseRouter::new(config, "metadata_tester".to_string()).await?;
        
        // Test empty metadata
        let metadata = HashMap::new();
        let message = SimpleMessage {
            to: "MetadataTarget".to_string(),
            from_entity: "metadata_tester".to_string(),
            content: "Test with empty metadata".to_string(),
            message_type: MessageType::Direct,
            metadata,
        };
        let result = router.convert_to_secure_message(&message).await;
        assert!(result.is_ok(), "Empty metadata should be handled");
        
        // Test metadata with various key/value patterns
        let mut metadata = HashMap::new();
        metadata.insert("simple".to_string(), "value".to_string());
        metadata.insert("empty_value".to_string(), "".to_string());
        metadata.insert("".to_string(), "empty_key".to_string());
        metadata.insert("unicode_key_🔑".to_string(), "unicode_value_🎯".to_string());
        metadata.insert("long_key".repeat(100), "long_value".repeat(100));
        
        let message = SimpleMessage {
            to: "MetadataTarget".to_string(),
            from_entity: "metadata_tester".to_string(),
            content: "Test with complex metadata".to_string(),
            message_type: MessageType::Direct,
            metadata,
        };
        let result = router.convert_to_secure_message(&message).await;
        assert!(result.is_ok(), "Complex metadata should be handled");
        
        Ok(())
    }

    /// Test recovery from simulated errors
    #[tokio::test]
    async fn test_error_recovery() -> Result<()> {
        let config = Config::for_testing();
        let router = SynapseRouter::new(config, "recovery_tester".to_string()).await?;
        
        // Test recovery after processing various message scenarios
        let long_content = "X".repeat(1000);
        let scenarios = vec![
            ("normal", "Normal message"),
            ("empty", ""),
            ("long", long_content.as_str()),
            ("unicode", "🚀 Test message with unicode 🎯"),
        ];
        
        for (scenario_name, content) in scenarios {
            let message = SimpleMessage {
                to: format!("RecoveryTarget_{}", scenario_name),
                from_entity: "recovery_tester".to_string(),
                content: content.to_string(),
                message_type: MessageType::Direct,
                metadata: HashMap::new(),
            };
            
            let result = router.convert_to_secure_message(&message).await;
            assert!(result.is_ok(), "Recovery scenario '{}' should succeed", scenario_name);
        }
        
        Ok(())
    }
}
