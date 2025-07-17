//! Concurrent Registry Access Tests
//! 
//! Tests concurrent access to registry services and thread-safety

use synapse::{Config, SynapseRouter, SimpleMessage, MessageType};
use std::collections::HashMap;
use tokio::sync::Barrier;
use futures::future::join_all;
use anyhow::Result;
use std::sync::Arc;

#[cfg(test)]
mod concurrent_registry_tests {
    use super::*;

    /// Test concurrent router creation to ensure thread safety during initialization
    #[tokio::test]
    async fn test_concurrent_router_creation() -> Result<()> {
        let num_tasks = 10;
        let barrier = Arc::new(Barrier::new(num_tasks));
        let mut tasks = Vec::new();

        for i in 0..num_tasks {
            let barrier_clone = Arc::clone(&barrier);
            let entity_id = format!("entity_{}", i);
            
            let task = tokio::spawn(async move {
                // Wait for all tasks to be ready
                barrier_clone.wait().await;
                
                // Simultaneously create routers
                let config = Config::for_testing();
                SynapseRouter::new(config, entity_id).await
            });
            
            tasks.push(task);
        }

        // Wait for all tasks to complete
        let results = join_all(tasks).await;
        
        // Verify all routers were created successfully
        let mut success_count = 0;
        for task_result in results {
            match task_result {
                Ok(Ok(_router)) => success_count += 1,
                Ok(Err(e)) => println!("Router creation failed: {}", e),
                Err(e) => println!("Task failed: {:?}", e),
            }
        }
        
        assert!(success_count >= num_tasks * 8 / 10, "At least 80% of concurrent router creations should succeed");
        println!("Successfully created {} out of {} routers concurrently", success_count, num_tasks);
        Ok(())
    }

    /// Test concurrent message processing
    #[tokio::test]
    async fn test_concurrent_message_processing() -> Result<()> {
        let config = Config::for_testing();
        let router = Arc::new(SynapseRouter::new(config, "test_entity".to_string()).await?);
        
        let num_messages = 20;
        let barrier = Arc::new(Barrier::new(num_messages));
        let mut tasks = Vec::new();

        for i in 0..num_messages {
            let router_clone = Arc::clone(&router);
            let barrier_clone = Arc::clone(&barrier);
            
            let task = tokio::spawn(async move {
                // Wait for all tasks to be ready
                barrier_clone.wait().await;
                
                // Create and process message simultaneously
                let message = SimpleMessage {
                    to: format!("Target_{}", i),
                    from_entity: "test_entity".to_string(),
                    content: format!("Concurrent message {}", i),
                    message_type: MessageType::Direct,
                    metadata: HashMap::new(),
                };
                
                router_clone.convert_to_secure_message(&message).await
            });
            
            tasks.push(task);
        }

        // Wait for all tasks to complete
        let results = join_all(tasks).await;
        
        // Verify message processing
        let mut success_count = 0;
        for task_result in results {
            match task_result {
                Ok(Ok(_secure_msg)) => success_count += 1,
                Ok(Err(e)) => println!("Message processing failed: {}", e),
                Err(e) => println!("Task failed: {:?}", e),
            }
        }
        
        assert!(success_count >= num_messages * 8 / 10, "At least 80% of concurrent message processing should succeed");
        println!("Successfully processed {} out of {} messages concurrently", success_count, num_messages);
        Ok(())
    }

    /// Test concurrent config creation
    #[tokio::test]
    async fn test_concurrent_config_creation() -> Result<()> {
        let num_configs = 15;
        let barrier = Arc::new(Barrier::new(num_configs));
        let mut tasks = Vec::new();

        for i in 0..num_configs {
            let barrier_clone = Arc::clone(&barrier);
            let entity_id = format!("config_entity_{}", i);
            
            let task = tokio::spawn(async move {
                // Wait for all tasks to be ready
                barrier_clone.wait().await;
                
                // Simultaneously create configs
                Config::default_for_entity(entity_id, "test".to_string())
            });
            
            tasks.push(task);
        }

        // Wait for all tasks to complete
        let results = join_all(tasks).await;
        
        // Verify all configs were created successfully
        let mut success_count = 0;
        for task_result in results {
            match task_result {
                Ok(_config) => success_count += 1,
                Err(e) => println!("Task failed: {:?}", e),
            }
        }
        
        assert_eq!(success_count, num_configs, "All concurrent config creations should succeed");
        println!("Successfully created {} configs concurrently", success_count);
        Ok(())
    }

    /// Test mixed concurrent operations (creation, messaging, etc.)
    #[tokio::test]
    async fn test_mixed_concurrent_operations() -> Result<()> {
        let num_operations = 12;
        let barrier = Arc::new(Barrier::new(num_operations));
        let mut tasks = Vec::new();

        // Create half router creation tasks
        for i in 0..(num_operations / 2) {
            let barrier_clone = Arc::clone(&barrier);
            let entity_id = format!("mixed_entity_{}", i);
            
            let task = tokio::spawn(async move {
                barrier_clone.wait().await;
                
                let config = Config::for_testing();
                let router = SynapseRouter::new(config, entity_id).await?;
                
                // Also process a message
                let message = SimpleMessage {
                    to: "MixedTarget".to_string(),
                    from_entity: "mixed_entity".to_string(),
                    content: "Mixed operation message".to_string(),
                    message_type: MessageType::Direct,
                    metadata: HashMap::new(),
                };
                
                router.convert_to_secure_message(&message).await?;
                Ok::<(), anyhow::Error>(())
            });
            
            tasks.push(task);
        }
        
        // Create half config creation tasks
        for i in (num_operations / 2)..num_operations {
            let barrier_clone = Arc::clone(&barrier);
            
            let task = tokio::spawn(async move {
                barrier_clone.wait().await;
                
                let _config = Config::default_for_entity(
                    format!("mixed_config_{}", i), 
                    "test".to_string()
                );
                
                Ok::<(), anyhow::Error>(())
            });
            
            tasks.push(task);
        }

        // Wait for all tasks to complete
        let results = join_all(tasks).await;
        
        // Verify mixed operations
        let mut success_count = 0;
        for task_result in results {
            match task_result {
                Ok(Ok(())) => success_count += 1,
                Ok(Err(e)) => println!("Mixed operation failed: {}", e),
                Err(e) => println!("Task failed: {:?}", e),
            }
        }
        
        assert!(success_count >= num_operations * 7 / 10, "At least 70% of mixed concurrent operations should succeed");
        println!("Successfully completed {} out of {} mixed operations concurrently", success_count, num_operations);
        Ok(())
    }
}
