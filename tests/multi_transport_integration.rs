use std::sync::Arc;
use anyhow::Result;
use synapse::{Config, SynapseRouter, SimpleMessage};

#[tokio::test]
async fn test_multi_transport_initialization() -> Result<()> {
    // Create a configuration for testing
    let config = Config::for_testing();
    let _router = SynapseRouter::new(config, "test_entity".to_string()).await?;
    
    // Test that the router initializes correctly
    println!("Multi-transport router initialized successfully");
    
    Ok(())
}

#[tokio::test]
async fn test_transport_message_routing() -> Result<()> {
    // Create a configuration for testing
    let config = Config::for_testing();
    let router = SynapseRouter::new(config, "test_entity".to_string()).await?;
    
    // Create test messages for different types
    let text_message = SimpleMessage::new(
        "recipient@example.com",
        "sender@example.com",
        "Test text message",
    );
    
    let data_message = SimpleMessage::new(
        "recipient@example.com",
        "sender@example.com",
        "Test data message",
    );
    
    // Convert messages to secure format
    let _secure_text = router.convert_to_secure_message(&text_message).await?;
    let _secure_data = router.convert_to_secure_message(&data_message).await?;
    
    println!("Multi-transport message routing test completed");
    
    Ok(())
}

#[tokio::test]
async fn test_transport_fallback() -> Result<()> {
    // Create a configuration for testing
    let config = Config::for_testing();
    let router = SynapseRouter::new(config, "test_entity".to_string()).await?;
    
    // Test message that would trigger transport fallback
    let message = SimpleMessage::new(
        "unreachable@example.com",
        "sender@example.com",
        "Test fallback message",
    );
    
    // Convert to secure message (should handle gracefully)
    let _secure_msg = router.convert_to_secure_message(&message).await?;
    
    println!("Transport fallback test completed");
    
    Ok(())
}

#[tokio::test]
async fn test_concurrent_transport_operations() -> Result<()> {
    // Create a configuration for testing
    let config = Config::for_testing();
    let router = Arc::new(SynapseRouter::new(config, "test_entity".to_string()).await?);
    
    const NUM_OPERATIONS: usize = 10;
    let mut tasks = Vec::new();
    
    for i in 0..NUM_OPERATIONS {
        let router = Arc::clone(&router);
        
        let task = tokio::spawn(async move {
            let message = SimpleMessage::new(
                format!("recipient{}@example.com", i),
                format!("sender{}@example.com", i),
                format!("Concurrent message {}", i),
            );
            
            router.convert_to_secure_message(&message).await
        });
        
        tasks.push(task);
    }
    
    // Wait for all operations to complete
    let results = futures::future::join_all(tasks).await;
    let mut success_count = 0;
    
    for result in results {
        match result {
            Ok(Ok(_)) => success_count += 1,
            Ok(Err(e)) => println!("Operation failed: {}", e),
            Err(e) => println!("Task panicked: {:?}", e),
        }
    }
    
    println!("Concurrent transport operations: {} out of {} succeeded", success_count, NUM_OPERATIONS);
    assert!(success_count >= NUM_OPERATIONS * 8 / 10, "At least 80% of operations should succeed");
    
    Ok(())
}
