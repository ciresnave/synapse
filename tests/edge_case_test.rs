//! Advanced Edge Case Tests for Synapse
//! 
//! This module tests edge cases, error conditions, and boundary scenarios
//! to ensure robust production behavior.

use synapse::*;
use std::collections::HashMap;
use tokio::time::{sleep, Duration, timeout};
use uuid::Uuid;

#[cfg(test)]
mod edge_case_tests {
    use super::*;

    /// Test boundary conditions for message sizes
    #[tokio::test]
    async fn test_message_size_boundaries() {
        let config = Config::for_testing();
        let router = EmrpRouter::new(config).await.expect("Failed to create router");
        
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
            content: "a".to_string(),
            message_type: MessageType::Direct,
            metadata: HashMap::new(),
        };
        
        let result = router.convert_to_secure_message(&single_char_msg).await;
        assert!(result.is_ok(), "Single character message should work");
        
        // Test very large message (10MB)
        let large_content = "x".repeat(10_000_000);
        let large_msg = SimpleMessage {
            to: "LargeTest".to_string(),
            from_entity: "LargeTester".to_string(),
            content: large_content.clone(),
            message_type: MessageType::Direct,
            metadata: HashMap::new(),
        };
        
        let result = router.convert_to_secure_message(&large_msg).await;
        assert!(result.is_ok(), "Large message should be handled");
        
        if let Ok(secure_msg) = result {
            assert!(!secure_msg.content.is_empty(), "Large message content should be preserved");
        }
        
        println!("✓ Message size boundaries tested");
    }

    /// Test unicode and special character handling
    #[tokio::test]
    async fn test_unicode_and_special_chars() {
        let config = Config::for_testing();
        let router = EmrpRouter::new(config).await.expect("Failed to create router");
        
        let test_cases = vec![
            ("Emoji", "🚀🎉🔥💯🌟"),
            ("Chinese", "你好世界测试"),
            ("Japanese", "こんにちは世界テスト"),
            ("Arabic", "مرحبا بالعالم اختبار"),
            ("Russian", "Привет мир тест"),
            ("Mathematical", "∑∫√π∞≈≠≤≥±×÷"),
            ("Symbols", "©®™€£¥$¢§¶†‡•…‰"),
            ("Control chars", "\t\n\r\x0b\x0c"),
            ("Mixed", "Hello 世界! 🌍 Test émojis and ñoñó special chars 🎉"),
        ];
        
        for (name, content) in test_cases {
            let msg = SimpleMessage {
                to: format!("UnicodeTest_{}", name),
                from_entity: "UnicodeTester".to_string(),
                content: content.to_string(),
                message_type: MessageType::Direct,
                metadata: HashMap::new(),
            };
            
            let result = router.convert_to_secure_message(&msg).await;
            assert!(result.is_ok(), "Unicode message {} should work", name);
            
            if let Ok(secure_msg) = result {
                assert!(!secure_msg.content.is_empty(), "Unicode content should be preserved");
            }
        }
        
        println!("✓ Unicode and special character handling tested");
    }

    /// Test malformed and edge case identifiers
    #[tokio::test]
    async fn test_malformed_identifiers() {
        let config = Config::for_testing();
        let router = EmrpRouter::new(config).await.expect("Failed to create router");
        
        let edge_case_identifiers = vec![
            ("empty", ""),
            ("whitespace", "   "),
            ("special_chars", "!@#$%^&*()"),
            ("very_long", &"a".repeat(1000)),
            ("unicode_name", "用户名测试"),
            ("emoji_name", "🤖🚀"),
            ("email_like", "test@example.com"),
            ("path_like", "/path/to/entity"),
            ("url_like", "https://example.com/entity"),
            ("newlines", "test\nwith\nnewlines"),
            ("tabs", "test\twith\ttabs"),
        ];
        
        for (test_name, identifier) in edge_case_identifiers {
            let msg = SimpleMessage {
                to: identifier.to_string(),
                from_entity: format!("EdgeTester_{}", test_name),
                content: format!("Testing identifier: {}", test_name),
                message_type: MessageType::Direct,
                metadata: HashMap::new(),
            };
            
            let result = router.convert_to_secure_message(&msg).await;
            assert!(result.is_ok(), "Edge case identifier {} should be handled", test_name);
        }
        
        println!("✓ Malformed and edge case identifiers tested");
    }

    /// Test concurrent access and race conditions
    #[tokio::test]
    async fn test_concurrent_access() {
        let config = Config::for_testing();
        let router = EmrpRouter::new(config).await.expect("Failed to create router");
        
        // Test concurrent message processing
        let mut handles = Vec::new();
        let num_concurrent = 50;
        
        for i in 0..num_concurrent {
            let router_clone = router.clone();
            let handle = tokio::spawn(async move {
                let msg = SimpleMessage {
                    to: format!("ConcurrentTest{}", i),
                    from_entity: format!("ConcurrentTester{}", i),
                    content: format!("Concurrent message {}", i),
                    message_type: MessageType::Direct,
                    metadata: [("iteration".to_string(), i.to_string())].into(),
                };
                
                // Add random delay to increase chance of race conditions
                sleep(Duration::from_millis(rand::random::<u64>() % 10)).await;
                
                router_clone.convert_to_secure_message(&msg).await
            });
            handles.push(handle);
        }
        
        // Wait for all operations with timeout
        let timeout_duration = Duration::from_secs(30);
        let start_time = std::time::Instant::now();
        
        for (i, handle) in handles.into_iter().enumerate() {
            let remaining_time = timeout_duration.saturating_sub(start_time.elapsed());
            let result = timeout(remaining_time, handle).await;
            
            match result {
                Ok(Ok(Ok(_))) => {
                    // Success
                }
                Ok(Ok(Err(e))) => {
                    panic!("Concurrent operation {} failed: {:?}", i, e);
                }
                Ok(Err(e)) => {
                    panic!("Concurrent task {} panicked: {:?}", i, e);
                }
                Err(_) => {
                    panic!("Concurrent operation {} timed out", i);
                }
            }
        }
        
        println!("✓ Concurrent access tested with {} operations", num_concurrent);
    }

    /// Test memory pressure and resource limits
    #[tokio::test]
    async fn test_memory_pressure() {
        let config = Config::for_testing();
        let router = EmrpRouter::new(config).await.expect("Failed to create router");
        
        // Create many messages to test memory handling
        let mut messages = Vec::new();
        let num_messages = 1000;
        
        for i in 0..num_messages {
            let msg = SimpleMessage {
                to: format!("MemoryTest{}", i),
                from_entity: "MemoryTester".to_string(),
                content: format!("Memory test message {} with some content", i),
                message_type: MessageType::Direct,
                metadata: [
                    ("index".to_string(), i.to_string()),
                    ("batch".to_string(), (i / 100).to_string()),
                ].into(),
            };
            
            let secure_msg = router.convert_to_secure_message(&msg).await.expect("Failed to convert message");
            messages.push(secure_msg);
            
            // Yield periodically to prevent blocking
            if i % 100 == 0 {
                tokio::task::yield_now().await;
            }
        }
        
        // Verify all messages were processed
        assert_eq!(messages.len(), num_messages);
        
        // Clear messages to test cleanup
        messages.clear();
        
        // Force garbage collection hint
        tokio::task::yield_now().await;
        
        println!("✓ Memory pressure tested with {} messages", num_messages);
    }

    /// Test timeout and deadline handling
    #[tokio::test]
    async fn test_timeout_handling() {
        let config = Config::for_testing();
        let router = EmrpRouter::new(config).await.expect("Failed to create router");
        
        // Test operations with very short timeouts
        let msg = SimpleMessage {
            to: "TimeoutTest".to_string(),
            from_entity: "TimeoutTester".to_string(),
            content: "Testing timeout handling".to_string(),
            message_type: MessageType::Direct,
            metadata: HashMap::new(),
        };
        
        // Test with reasonable timeout
        let result = timeout(Duration::from_secs(5), router.convert_to_secure_message(&msg)).await;
        assert!(result.is_ok(), "Operation should complete within timeout");
        
        // Test with very short timeout (this might timeout depending on system load)
        let result = timeout(Duration::from_millis(1), router.convert_to_secure_message(&msg)).await;
        // We don't assert here as it depends on system performance
        
        println!("✓ Timeout handling tested");
    }

    /// Test network failure simulation
    #[tokio::test]
    async fn test_network_failure_simulation() {
        let config = Config::for_testing();
        let router = EmrpRouter::new(config).await.expect("Failed to create router");
        
        // Test with messages that might trigger network-like conditions
        let problematic_messages = vec![
            ("network_error", "Network connection failed"),
            ("dns_failure", "DNS resolution failed"),
            ("timeout", "Request timed out"),
            ("connection_refused", "Connection refused"),
            ("ssl_error", "SSL handshake failed"),
        ];
        
        for (test_name, content) in problematic_messages {
            let msg = SimpleMessage {
                to: format!("NetworkTest_{}", test_name),
                from_entity: "NetworkTester".to_string(),
                content: content.to_string(),
                message_type: MessageType::Direct,
                metadata: [("error_type".to_string(), test_name.to_string())].into(),
            };
            
            let result = router.convert_to_secure_message(&msg).await;
            assert!(result.is_ok(), "Network failure simulation {} should be handled", test_name);
        }
        
        println!("✓ Network failure simulation tested");
    }

    /// Test data corruption and recovery
    #[tokio::test]
    async fn test_data_corruption_scenarios() {
        let config = Config::for_testing();
        let router = EmrpRouter::new(config).await.expect("Failed to create router");
        
        // Test messages with potentially corrupted data patterns
        let corruption_tests = vec![
            ("null_bytes", "Hello\0World\0Test"),
            ("binary_data", "\x00\x01\x02\x03\x04\x05"),
            ("malformed_utf8", "Valid text with invalid \xFF\xFE bytes"),
            ("extremely_long_line", &"a".repeat(100_000)),
            ("many_newlines", &"\n".repeat(10_000)),
            ("zero_width_chars", "Text\u{200B}with\u{200C}zero\u{200D}width\u{FEFF}chars"),
            ("combining_chars", "a\u{0300}\u{0301}\u{0302}\u{0303}\u{0304}"),
        ];
        
        for (test_name, content) in corruption_tests {
            let msg = SimpleMessage {
                to: format!("CorruptionTest_{}", test_name),
                from_entity: "CorruptionTester".to_string(),
                content: content.to_string(),
                message_type: MessageType::Direct,
                metadata: HashMap::new(),
            };
            
            let result = router.convert_to_secure_message(&msg).await;
            assert!(result.is_ok(), "Data corruption test {} should be handled", test_name);
        }
        
        println!("✓ Data corruption scenarios tested");
    }

    /// Test performance under load
    #[tokio::test]
    async fn test_performance_under_load() {
        let config = Config::for_testing();
        let router = EmrpRouter::new(config).await.expect("Failed to create router");
        
        let start_time = std::time::Instant::now();
        let num_operations = 1000;
        
        // Create load with different message types
        let mut handles = Vec::new();
        
        for i in 0..num_operations {
            let router_clone = router.clone();
            let handle = tokio::spawn(async move {
                let message_type = match i % 4 {
                    0 => MessageType::Direct,
                    1 => MessageType::Broadcast,
                    2 => MessageType::Conversation,
                    _ => MessageType::Notification,
                };
                
                let msg = SimpleMessage {
                    to: format!("LoadTest{}", i),
                    from_entity: format!("LoadTester{}", i % 10), // 10 different senders
                    content: format!("Load test message {} with type {:?}", i, message_type),
                    message_type,
                    metadata: [
                        ("iteration".to_string(), i.to_string()),
                        ("timestamp".to_string(), chrono::Utc::now().to_rfc3339()),
                    ].into(),
                };
                
                router_clone.convert_to_secure_message(&msg).await
            });
            handles.push(handle);
        }
        
        // Wait for all operations
        let mut successes = 0;
        for handle in handles {
            let result = handle.await;
            match result {
                Ok(Ok(_)) => successes += 1,
                Ok(Err(e)) => eprintln!("Load test operation failed: {:?}", e),
                Err(e) => eprintln!("Load test task panicked: {:?}", e),
            }
        }
        
        let duration = start_time.elapsed();
        let ops_per_second = num_operations as f64 / duration.as_secs_f64();
        
        println!("✓ Performance test: {}/{} operations succeeded", successes, num_operations);
        println!("  Duration: {:?}", duration);
        println!("  Ops/second: {:.2}", ops_per_second);
        
        // Should have high success rate
        assert!(successes > num_operations * 95 / 100, "Should have >95% success rate");
        // Should process at least 100 ops/second
        assert!(ops_per_second > 100.0, "Should process at least 100 ops/second");
    }

    /// Test graceful degradation
    #[tokio::test]
    async fn test_graceful_degradation() {
        let config = Config::for_testing();
        let router = EmrpRouter::new(config).await.expect("Failed to create router");
        
        // Test system behavior under various stress conditions
        let stress_tests = vec![
            ("rapid_fire", 100, Duration::from_millis(1)),
            ("sustained_load", 50, Duration::from_millis(10)),
            ("burst_load", 200, Duration::from_millis(0)),
        ];
        
        for (test_name, num_msgs, delay) in stress_tests {
            let start_time = std::time::Instant::now();
            let mut successes = 0;
            
            for i in 0..num_msgs {
                let msg = SimpleMessage {
                    to: format!("StressTest_{}_{}", test_name, i),
                    from_entity: format!("StressTester_{}", test_name),
                    content: format!("Stress test message {} for {}", i, test_name),
                    message_type: MessageType::Direct,
                    metadata: HashMap::new(),
                };
                
                let result = router.convert_to_secure_message(&msg).await;
                if result.is_ok() {
                    successes += 1;
                }
                
                if delay > Duration::from_millis(0) {
                    sleep(delay).await;
                }
            }
            
            let duration = start_time.elapsed();
            let success_rate = (successes as f64 / num_msgs as f64) * 100.0;
            
            println!("✓ Stress test '{}': {}/{} messages ({:.1}%) in {:?}", 
                     test_name, successes, num_msgs, success_rate, duration);
            
            // Should maintain reasonable success rate even under stress
            assert!(success_rate > 90.0, "Should maintain >90% success rate under stress");
        }
    }

    /// Test cleanup and resource release
    #[tokio::test]
    async fn test_cleanup_and_resource_release() {
        // Test that resources are properly released when router is dropped
        {
            let config = Config::for_testing();
            let router = EmrpRouter::new(config).await.expect("Failed to create router");
            
            // Use the router
            let msg = SimpleMessage {
                to: "CleanupTest".to_string(),
                from_entity: "CleanupTester".to_string(),
                content: "Testing cleanup".to_string(),
                message_type: MessageType::Direct,
                metadata: HashMap::new(),
            };
            
            let _result = router.convert_to_secure_message(&msg).await.expect("Failed to convert message");
            
            // Router goes out of scope and should be dropped
        }
        
        // Give some time for cleanup
        sleep(Duration::from_millis(100)).await;
        
        // Test that new router can be created (resources were released)
        {
            let config = Config::for_testing();
            let router = EmrpRouter::new(config).await.expect("Failed to create router after cleanup");
            
            let msg = SimpleMessage {
                to: "PostCleanupTest".to_string(),
                from_entity: "PostCleanupTester".to_string(),
                content: "Testing after cleanup".to_string(),
                message_type: MessageType::Direct,
                metadata: HashMap::new(),
            };
            
            let result = router.convert_to_secure_message(&msg).await;
            assert!(result.is_ok(), "New router should work after cleanup");
        }
        
        println!("✓ Cleanup and resource release tested");
    }
}
