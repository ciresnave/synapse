use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Barrier;
use futures::future::join_all;
use anyhow::Result;
use synapse::{Config, SynapseRouter, SimpleMessage};

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn test_high_load_routing() -> Result<()> {
    // Create a configuration for testing
    let config = Config::for_testing();
    let router = Arc::new(SynapseRouter::new(config, "test_entity".to_string()).await?);
    
    // Number of concurrent routers and operations
    const NUM_ROUTERS: usize = 50;
    const OPERATIONS_PER_ROUTER: usize = 20;
    
    // Shared barrier to coordinate start time
    let barrier = Arc::new(Barrier::new(NUM_ROUTERS));
    
    let mut tasks = Vec::new();
    
    for router_id in 0..NUM_ROUTERS {
        let router = Arc::clone(&router);
        let barrier = Arc::clone(&barrier);
        
        let task = tokio::spawn(async move {
            // Wait for all tasks to be ready
            barrier.wait().await;
            
            // Perform operations
            for op_id in 0..OPERATIONS_PER_ROUTER {
                // Create a test message
                let content = format!("High load test message {} from router {}", op_id, router_id);
                let simple_msg = SimpleMessage::new(
                    format!("test_recipient_{}", (router_id + 1) % NUM_ROUTERS),
                    format!("test_sender_{}", router_id),
                    content,
                );
                
                // Convert to secure message
                let _secure_msg = router.convert_to_secure_message(&simple_msg).await?;
                
                // Add some artificial load
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
            
            Ok::<(), anyhow::Error>(())
        });
        
        tasks.push(task);
    }

    // Wait for all tasks with timeout
    let start_time = Instant::now();
    let results = join_all(tasks).await;
    let duration = start_time.elapsed();
    
    // Verify results
    let mut success_count = 0;
    for task_result in results {
        match task_result {
            Ok(Ok(())) => success_count += 1,
            Ok(Err(e)) => println!("High load task failed: {}", e),
            Err(e) => println!("Task panicked: {:?}", e),
        }
    }
    
    println!("High load test completed in {:?}", duration);
    println!("Successfully processed {} out of {} router load tests", success_count, NUM_ROUTERS);
    
    assert!(success_count >= NUM_ROUTERS * 8 / 10, "At least 80% of high load tests should succeed");
    assert!(duration < Duration::from_secs(30), "High load test should complete within 30 seconds");
    
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_concurrent_message_conversion() -> Result<()> {
    // Create a configuration for testing
    let config = Config::for_testing();
    let router = Arc::new(SynapseRouter::new(config, "test_entity".to_string()).await?);
    
    // Number of concurrent operations
    const NUM_OPERATIONS: usize = 100;
    
    // Shared barrier to coordinate start time
    let barrier = Arc::new(Barrier::new(NUM_OPERATIONS));
    
    let mut tasks = Vec::new();
    
    for op_id in 0..NUM_OPERATIONS {
        let router = Arc::clone(&router);
        let barrier = Arc::clone(&barrier);
        
        let task = tokio::spawn(async move {
            // Wait for all tasks to be ready
            barrier.wait().await;
            
            // Create a test message
            let content = format!("Concurrent test message {}", op_id);
            let simple_msg = SimpleMessage::new(
                format!("recipient_{}", (op_id + 1) % NUM_OPERATIONS),
                format!("sender_{}", op_id),
                content,
            );
            
            // Convert to secure message
            let _secure_msg = router.convert_to_secure_message(&simple_msg).await?;
            
            Ok::<(), anyhow::Error>(())
        });
        
        tasks.push(task);
    }

    // Wait for all tasks
    let start_time = Instant::now();
    let results = join_all(tasks).await;
    let duration = start_time.elapsed();
    
    // Verify results
    let mut success_count = 0;
    for task_result in results {
        match task_result {
            Ok(Ok(())) => success_count += 1,
            Ok(Err(e)) => println!("Concurrent operation failed: {}", e),
            Err(e) => println!("Task panicked: {:?}", e),
        }
    }
    
    println!("Concurrent message conversion test completed in {:?}", duration);
    println!("Successfully processed {} out of {} operations", success_count, NUM_OPERATIONS);
    
    assert!(success_count >= NUM_OPERATIONS * 9 / 10, "At least 90% of operations should succeed");
    assert!(duration < Duration::from_secs(10), "Test should complete within 10 seconds");
    
    Ok(())
}
