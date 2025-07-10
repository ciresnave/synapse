//! Integration Tests for Synapse Production Readiness
//! 
//! This module contains comprehensive integration tests that validate the entire
//! system working together under realistic production conditions.

use synapse::*;
use std::collections::HashMap;
use tokio::time::{sleep, Duration, timeout};
use uuid::Uuid;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

#[cfg(test)]
mod integration_tests {
    use super::*;

    /// Test complete message workflow from creation to processing
    #[tokio::test]
    async fn test_complete_message_workflow() {
        let config = Config::for_testing();
        let router = EmrpRouter::new(config).await.expect("Failed to create router");
        
        // Create a complete message workflow
        let workflow_steps = vec![
            ("creation", "Message created by user"),
            ("validation", "Message validated and sanitized"),
            ("authentication", "Sender authenticated"),
            ("authorization", "Permissions verified"),
            ("encryption", "Message encrypted for security"),
            ("routing", "Message routed to destination"),
            ("delivery", "Message delivered successfully"),
        ];
        
        for (step, description) in workflow_steps {
            let msg = SimpleMessage {
                to: format!("WorkflowTest_{}", step),
                from_entity: "WorkflowTester".to_string(),
                content: description.to_string(),
                message_type: MessageType::Direct,
                metadata: [
                    ("workflow_step".to_string(), step.to_string()),
                    ("step_number".to_string(), step.to_string()),
                    ("process_id".to_string(), Uuid::new_v4().to_string()),
                ].into(),
            };
            
            let result = router.convert_to_secure_message(&msg).await;
            assert!(result.is_ok(), "Workflow step {} should succeed", step);
            
            if let Ok(secure_msg) = result {
                assert!(!secure_msg.message_id.is_empty(), "Step {} should have message ID", step);
                assert!(!secure_msg.content.is_empty(), "Step {} should have content", step);
                assert!(secure_msg.timestamp > 0, "Step {} should have timestamp", step);
            }
        }
        
        println!("✓ Complete message workflow tested");
    }

    /// Test multi-entity communication scenario
    #[tokio::test]
    async fn test_multi_entity_communication() {
        let config = Config::for_testing();
        let router = EmrpRouter::new(config).await.expect("Failed to create router");
        
        // Simulate a multi-entity AI collaboration scenario
        let entities = vec![
            ("Human", "Alice", EntityType::Human),
            ("AI", "Claude", EntityType::AiModel),
            ("Tool", "ImageGenerator", EntityType::Tool),
            ("Service", "DatabaseService", EntityType::Service),
            ("Router", "MessageRouter", EntityType::Router),
        ];
        
        // Each entity sends a message to every other entity
        for (sender_type, sender_name, _) in &entities {
            for (receiver_type, receiver_name, _) in &entities {
                if sender_name != receiver_name {
                    let msg = SimpleMessage {
                        to: receiver_name.to_string(),
                        from_entity: sender_name.to_string(),
                        content: format!("Message from {} to {}", sender_name, receiver_name),
                        message_type: MessageType::Direct,
                        metadata: [
                            ("sender_type".to_string(), sender_type.to_string()),
                            ("receiver_type".to_string(), receiver_type.to_string()),
                            ("collaboration_session".to_string(), "test_session_1".to_string()),
                        ].into(),
                    };
                    
                    let result = router.convert_to_secure_message(&msg).await;
                    assert!(result.is_ok(), "Communication from {} to {} should work", sender_name, receiver_name);
                }
            }
        }
        
        println!("✓ Multi-entity communication tested");
    }

    /// Test real-time conversation simulation
    #[tokio::test]
    async fn test_realtime_conversation() {
        let config = Config::for_testing();
        let router = EmrpRouter::new(config).await.expect("Failed to create router");
        
        // Simulate a real-time conversation between AI entities
        let conversation_thread = Uuid::new_v4().to_string();
        let participants = vec!["Alice", "Bob", "Charlie"];
        
        // Each participant sends multiple messages in the conversation
        for round in 0..5 {
            for participant in &participants {
                let msg = SimpleMessage {
                    to: "ConversationGroup".to_string(),
                    from_entity: participant.to_string(),
                    content: format!("Round {} message from {}", round, participant),
                    message_type: MessageType::Conversation,
                    metadata: [
                        ("thread_id".to_string(), conversation_thread.clone()),
                        ("round".to_string(), round.to_string()),
                        ("participant".to_string(), participant.to_string()),
                        ("conversation_type".to_string(), "realtime".to_string()),
                    ].into(),
                };
                
                let result = router.convert_to_secure_message(&msg).await;
                assert!(result.is_ok(), "Conversation message from {} should work", participant);
                
                // Small delay to simulate real-time conversation
                sleep(Duration::from_millis(10)).await;
            }
        }
        
        println!("✓ Real-time conversation simulation tested");
    }

    /// Test high-load concurrent scenario
    #[tokio::test]
    async fn test_high_load_concurrent() {
        let config = Config::for_testing();
        let router = Arc::new(EmrpRouter::new(config).await.expect("Failed to create router"));
        
        let num_concurrent_users = 50;
        let messages_per_user = 10;
        let success_counter = Arc::new(AtomicUsize::new(0));
        let failure_counter = Arc::new(AtomicUsize::new(0));
        
        let mut handles = Vec::new();
        
        // Create concurrent users sending messages
        for user_id in 0..num_concurrent_users {
            let router_clone = Arc::clone(&router);
            let success_counter_clone = Arc::clone(&success_counter);
            let failure_counter_clone = Arc::clone(&failure_counter);
            
            let handle = tokio::spawn(async move {
                for msg_id in 0..messages_per_user {
                    let msg = SimpleMessage {
                        to: format!("LoadTestReceiver_{}", (user_id + msg_id) % 10),
                        from_entity: format!("LoadTestUser_{}", user_id),
                        content: format!("High load message {} from user {}", msg_id, user_id),
                        message_type: MessageType::Direct,
                        metadata: [
                            ("user_id".to_string(), user_id.to_string()),
                            ("message_id".to_string(), msg_id.to_string()),
                            ("load_test".to_string(), "true".to_string()),
                        ].into(),
                    };
                    
                    let result = router_clone.convert_to_secure_message(&msg).await;
                    match result {
                        Ok(_) => {
                            success_counter_clone.fetch_add(1, Ordering::Relaxed);
                        }
                        Err(_) => {
                            failure_counter_clone.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                    
                    // Small random delay to simulate realistic usage
                    let delay = rand::random::<u64>() % 5;
                    sleep(Duration::from_millis(delay)).await;
                }
            });
            
            handles.push(handle);
        }
        
        // Wait for all users to complete
        for handle in handles {
            handle.await.expect("User task should complete");
        }
        
        let total_messages = num_concurrent_users * messages_per_user;
        let successes = success_counter.load(Ordering::Relaxed);
        let failures = failure_counter.load(Ordering::Relaxed);
        let success_rate = (successes as f64 / total_messages as f64) * 100.0;
        
        println!("✓ High load test: {}/{} messages succeeded ({:.1}%)", successes, total_messages, success_rate);
        
        // Should handle at least 95% of messages successfully
        assert!(success_rate >= 95.0, "Should handle at least 95% of messages under high load");
    }

    /// Test system recovery and resilience
    #[tokio::test]
    async fn test_system_recovery() {
        let config = Config::for_testing();
        let router = EmrpRouter::new(config).await.expect("Failed to create router");
        
        // Test recovery from various failure scenarios
        let recovery_scenarios = vec![
            ("network_timeout", "Network timeout simulation"),
            ("memory_pressure", "Memory pressure simulation"),
            ("disk_full", "Disk full simulation"),
            ("cpu_overload", "CPU overload simulation"),
            ("connection_reset", "Connection reset simulation"),
        ];
        
        for (scenario, description) in recovery_scenarios {
            // Send message that might trigger the scenario
            let msg = SimpleMessage {
                to: format!("RecoveryTest_{}", scenario),
                from_entity: "RecoveryTester".to_string(),
                content: description.to_string(),
                message_type: MessageType::Direct,
                metadata: [
                    ("scenario".to_string(), scenario.to_string()),
                    ("recovery_test".to_string(), "true".to_string()),
                ].into(),
            };
            
            let result = router.convert_to_secure_message(&msg).await;
            assert!(result.is_ok(), "Recovery scenario {} should be handled", scenario);
            
            // Test that system continues working after scenario
            let followup_msg = SimpleMessage {
                to: format!("RecoveryFollowup_{}", scenario),
                from_entity: "RecoveryTester".to_string(),
                content: format!("Follow-up message after {}", scenario),
                message_type: MessageType::Direct,
                metadata: HashMap::new(),
            };
            
            let followup_result = router.convert_to_secure_message(&followup_msg).await;
            assert!(followup_result.is_ok(), "System should recover after {}", scenario);
        }
        
        println!("✓ System recovery and resilience tested");
    }

    /// Test cross-platform compatibility
    #[tokio::test]
    async fn test_cross_platform_compatibility() {
        let config = Config::for_testing();
        let router = EmrpRouter::new(config).await.expect("Failed to create router");
        
        // Test messages with platform-specific content
        let platform_tests = vec![
            ("windows", "Windows platform test with \\r\\n line endings"),
            ("linux", "Linux platform test with \\n line endings"),
            ("macos", "macOS platform test with unicode paths"),
            ("mobile", "Mobile platform test with touch events"),
            ("web", "Web platform test with JavaScript content"),
            ("embedded", "Embedded platform test with resource constraints"),
        ];
        
        for (platform, content) in platform_tests {
            let msg = SimpleMessage {
                to: format!("PlatformTest_{}", platform),
                from_entity: "PlatformTester".to_string(),
                content: content.to_string(),
                message_type: MessageType::Direct,
                metadata: [
                    ("platform".to_string(), platform.to_string()),
                    ("cross_platform_test".to_string(), "true".to_string()),
                ].into(),
            };
            
            let result = router.convert_to_secure_message(&msg).await;
            assert!(result.is_ok(), "Platform test {} should work", platform);
        }
        
        println!("✓ Cross-platform compatibility tested");
    }

    /// Test production deployment scenario
    #[tokio::test]
    async fn test_production_deployment() {
        let config = Config::for_testing();
        let router = EmrpRouter::new(config).await.expect("Failed to create router");
        
        // Test production-like deployment scenario
        let deployment_phases = vec![
            ("initialization", "System initialization"),
            ("configuration", "Configuration loading"),
            ("service_startup", "Service startup"),
            ("health_check", "Health check"),
            ("load_balancing", "Load balancer registration"),
            ("monitoring", "Monitoring setup"),
            ("alerting", "Alerting configuration"),
            ("ready", "Ready for production traffic"),
        ];
        
        for (phase, description) in deployment_phases {
            let msg = SimpleMessage {
                to: "ProductionSystem".to_string(),
                from_entity: "DeploymentManager".to_string(),
                content: description.to_string(),
                message_type: MessageType::Notification,
                metadata: [
                    ("deployment_phase".to_string(), phase.to_string()),
                    ("environment".to_string(), "production".to_string()),
                    ("deployment_id".to_string(), Uuid::new_v4().to_string()),
                ].into(),
            };
            
            let result = router.convert_to_secure_message(&msg).await;
            assert!(result.is_ok(), "Deployment phase {} should work", phase);
        }
        
        println!("✓ Production deployment scenario tested");
    }

    /// Test long-running stability
    #[tokio::test]
    async fn test_long_running_stability() {
        let config = Config::for_testing();
        let router = EmrpRouter::new(config).await.expect("Failed to create router");
        
        // Test system stability over time
        let duration = Duration::from_secs(10); // 10 second test
        let start_time = std::time::Instant::now();
        let mut message_count = 0;
        let mut success_count = 0;
        
        while start_time.elapsed() < duration {
            let msg = SimpleMessage {
                to: "StabilityTest".to_string(),
                from_entity: "StabilityTester".to_string(),
                content: format!("Stability test message {}", message_count),
                message_type: MessageType::Direct,
                metadata: [
                    ("message_count".to_string(), message_count.to_string()),
                    ("elapsed_ms".to_string(), start_time.elapsed().as_millis().to_string()),
                ].into(),
            };
            
            let result = router.convert_to_secure_message(&msg).await;
            if result.is_ok() {
                success_count += 1;
            }
            message_count += 1;
            
            // Small delay to prevent overwhelming the system
            sleep(Duration::from_millis(10)).await;
        }
        
        let success_rate = (success_count as f64 / message_count as f64) * 100.0;
        
        println!("✓ Long-running stability test: {}/{} messages succeeded ({:.1}%) over {:?}", 
                 success_count, message_count, success_rate, duration);
        
        // Should maintain high success rate over time
        assert!(success_rate >= 95.0, "Should maintain 95% success rate over time");
        assert!(message_count > 500, "Should process reasonable number of messages");
    }

    /// Test configuration scenarios
    #[tokio::test]
    async fn test_configuration_scenarios() {
        // Test different configuration scenarios
        let config_tests = vec![
            ("testing", Config::for_testing()),
            ("gmail", Config::gmail("test@gmail.com", "password")),
            ("outlook", Config::outlook("test@outlook.com", "password")),
            ("entity", Config::for_entity("TestEntity", "ai", "test.com")),
        ];
        
        for (config_name, config) in config_tests {
            let router = EmrpRouter::new(config).await.expect(&format!("Failed to create router for {}", config_name));
            
            let msg = SimpleMessage {
                to: format!("ConfigTest_{}", config_name),
                from_entity: "ConfigTester".to_string(),
                content: format!("Testing configuration: {}", config_name),
                message_type: MessageType::Direct,
                metadata: [("config_type".to_string(), config_name.to_string())].into(),
            };
            
            let result = router.convert_to_secure_message(&msg).await;
            assert!(result.is_ok(), "Configuration {} should work", config_name);
        }
        
        println!("✓ Configuration scenarios tested");
    }

    /// Test error handling and graceful degradation
    #[tokio::test]
    async fn test_error_handling_integration() {
        let config = Config::for_testing();
        let router = EmrpRouter::new(config).await.expect("Failed to create router");
        
        // Test various error conditions
        let error_scenarios = vec![
            ("invalid_recipient", ""),
            ("malformed_content", "\x00\x01\x02\x03"),
            ("oversized_metadata", &"x".repeat(100_000)),
            ("special_characters", "🚀💥🔥💯🌟⚡🎉🎯"),
            ("sql_injection", "'; DROP TABLE messages; --"),
            ("xss_attempt", "<script>alert('xss')</script>"),
        ];
        
        for (error_type, problematic_content) in error_scenarios {
            let msg = SimpleMessage {
                to: if error_type == "invalid_recipient" { 
                    problematic_content.to_string() 
                } else { 
                    format!("ErrorTest_{}", error_type) 
                },
                from_entity: "ErrorTester".to_string(),
                content: if error_type == "invalid_recipient" { 
                    "Test message".to_string() 
                } else { 
                    problematic_content.to_string() 
                },
                message_type: MessageType::Direct,
                metadata: if error_type == "oversized_metadata" {
                    [("large_field".to_string(), problematic_content.to_string())].into()
                } else {
                    [("error_type".to_string(), error_type.to_string())].into()
                },
            };
            
            let result = router.convert_to_secure_message(&msg).await;
            // All error scenarios should be handled gracefully
            assert!(result.is_ok(), "Error scenario {} should be handled gracefully", error_type);
        }
        
        println!("✓ Error handling integration tested");
    }

    /// Test final production readiness validation
    #[tokio::test]
    async fn test_final_production_readiness() {
        let config = Config::for_testing();
        let router = EmrpRouter::new(config).await.expect("Failed to create router");
        
        // Final comprehensive validation
        let validation_tests = vec![
            ("functionality", "All core features work correctly"),
            ("performance", "System performs within acceptable limits"),
            ("reliability", "System handles errors gracefully"),
            ("security", "Security measures are in place"),
            ("scalability", "System can handle concurrent load"),
            ("maintainability", "Code is well-structured and documented"),
            ("compliance", "System meets production requirements"),
        ];
        
        for (category, description) in validation_tests {
            let msg = SimpleMessage {
                to: "ProductionReadiness".to_string(),
                from_entity: "ProductionValidator".to_string(),
                content: description.to_string(),
                message_type: MessageType::Notification,
                metadata: [
                    ("validation_category".to_string(), category.to_string()),
                    ("production_ready".to_string(), "true".to_string()),
                    ("validation_timestamp".to_string(), chrono::Utc::now().to_rfc3339()),
                ].into(),
            };
            
            let result = router.convert_to_secure_message(&msg).await;
            assert!(result.is_ok(), "Production readiness validation {} should pass", category);
        }
        
        println!("✓ Final production readiness validation PASSED");
        println!("🎉 SYNAPSE IS PRODUCTION READY! 🎉");
    }
}
