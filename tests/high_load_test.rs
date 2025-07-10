//! High Load Test
//! This test validates that Synapse can handle high concurrency with 100+ users
//! and maintain performance and stability under load.

use std::sync::Arc;
use anyhow::Result;
use futures::future::join_all;
use tokio::sync::Barrier;
use synapse::{
    Config,
    identity::{Identity, KeyPair},
    services::{
        registry::RegistryService,
        trust_manager::TrustManager,
    },
    storage::database::Database,
    transport::{
        UnifiedTransportManager, 
        Transport, 
        MessageUrgency,
        TransportResult,
        MessageDeliveryStatus,
    },
};
use async_trait::async_trait;
use std::time::{Duration, Instant};

/// A mock transport for high-volume testing
#[derive(Clone)]
struct HighLoadTransport {
    id: String,
}

impl HighLoadTransport {
    fn new(id: &str) -> Self {
        Self {
            id: id.to_string(),
        }
    }
}

#[async_trait]
impl Transport for HighLoadTransport {
    fn get_id(&self) -> &str {
        &self.id
    }
    
    async fn send(&self, to: &str, _message: &[u8]) -> TransportResult<MessageDeliveryStatus> {
        // Simulate variable network latency based on system load
        let latency = rand::random::<u64>() % 50 + 10; // 10-60ms
        tokio::time::sleep(Duration::from_millis(latency)).await;
        
        Ok(MessageDeliveryStatus {
            delivered: true,
            recipient: to.to_string(),
            timestamp: chrono::Utc::now(),
            transport_id: self.get_id().to_string(),
            latency_ms: latency as u32,
        })
    }
    
    async fn can_reach(&self, _participant_id: &str) -> bool {
        true
    }
    
    async fn get_estimated_latency(&self, _participant_id: &str) -> Option<Duration> {
        Some(Duration::from_millis(30))
    }
    
    fn get_metrics(&self) -> synapse::transport::TransportMetrics {
        synapse::transport::TransportMetrics {
            transport_id: self.get_id().to_string(),
            messages_sent: 0,
            messages_received: 0,
            failures: 0,
            average_latency_ms: 30.0,
            uptime_seconds: 0,
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 16)]
async fn test_high_load_concurrent_users() -> Result<()> {
    // Number of concurrent users
    const NUM_USERS: usize = 150;
    // Number of messages per user
    const MESSAGES_PER_USER: usize = 10;
    // Number of registry operations per user
    const REGISTRY_OPS_PER_USER: usize = 5;
    
    // Set up a shared registry
    let admin_keypair = KeyPair::generate();
    let admin_identity = Identity::from_keypair(&admin_keypair)?;
    let db = Database::new_in_memory()?;
    let registry = Arc::new(RegistryService::new(db.clone(), admin_identity.clone()));
    
    // Create identities for all users
    let mut users = Vec::with_capacity(NUM_USERS);
    for i in 0..NUM_USERS {
        let keypair = KeyPair::generate();
        let identity = Identity::from_keypair(&keypair)?;
        
        // Add metadata to make each user unique
        let mut identity_with_meta = identity.clone();
        identity_with_meta.set_metadata("user_index", &i.to_string())?;
        identity_with_meta.set_metadata("name", &format!("User {}", i))?;
        
        // Register user with registry
        registry.register_participant(&identity_with_meta)?;
        
        users.push(identity_with_meta);
    }
    
    // Create transport manager
    let transport = Arc::new(HighLoadTransport::new("high_load_transport"));
    let transport_manager = UnifiedTransportManager::new_with_transports(vec![
        transport as Arc<dyn Transport>,
    ]);
    
    // Create shared barrier for coordinated start
    let barrier = Arc::new(Barrier::new(NUM_USERS));
    
    // Spawn tasks for each user
    let start_time = Instant::now();
    let mut tasks = Vec::with_capacity(NUM_USERS);
    
    for (idx, user) in users.iter().enumerate() {
        let user_id = user.get_id().clone();
        let registry = registry.clone();
        let transport_manager = transport_manager.clone();
        let users = users.clone();
        let barrier = barrier.clone();
        
        let task = tokio::spawn(async move {
            let mut results = Vec::new();
            
            // Wait for all tasks to be ready for fair comparison
            barrier.wait().await;
            
            // Messaging operations
            for msg_idx in 0..MESSAGES_PER_USER {
                // Select random recipient (not self)
                let recipient_idx = (idx + 1 + (msg_idx * 7) % (NUM_USERS - 1)) % NUM_USERS;
                let recipient = &users[recipient_idx];
                
                // Send a message
                let message = format!("High load test message {} from user {}", msg_idx, idx);
                let send_result = transport_manager.send_message(
                    &recipient.get_id(),
                    message.as_bytes(),
                    MessageUrgency::Interactive,
                ).await;
                
                results.push(("send", send_result.is_ok()));
            }
            
            // Registry operations
            for op_idx in 0..REGISTRY_OPS_PER_USER {
                // Alternate between different registry operations
                match op_idx % 5 {
                    0 => {
                        // Get user profile
                        let lookup_result = registry.get_participant(&user_id);
                        results.push(("lookup", lookup_result.is_ok()));
                    },
                    1 => {
                        // Search by metadata
                        let search_result = registry.search_by_metadata("name", "User");
                        results.push(("search", search_result.is_ok()));
                    },
                    2 => {
                        // Update metadata
                        if let Ok(Some(mut profile)) = registry.get_participant(&user_id) {
                            profile.set_metadata("last_active", &chrono::Utc::now().to_rfc3339())?;
                            let update_result = registry.update_participant(&profile);
                            results.push(("update", update_result.is_ok()));
                        }
                    },
                    3 => {
                        // List all participants (heavy operation)
                        let list_result = registry.list_all_participants();
                        results.push(("list", list_result.is_ok()));
                    },
                    4 => {
                        // Get participants by capability (another heavy operation)
                        let capability_result = registry.get_participants_by_capability("test");
                        results.push(("capability", capability_result.is_ok()));
                    },
                    _ => unreachable!(),
                }
            }
            
            // Return success rate for this user
            (idx, results)
        });
        
        tasks.push(task);
    }
    
    // Wait for all tasks to complete
    let results = join_all(tasks).await;
    let elapsed = start_time.elapsed();
    
    // Analyze results
    let mut success_count = 0;
    let mut total_operations = 0;
    let mut failed_operations = Vec::new();
    
    for result in results {
        if let Ok((user_idx, operations)) = result {
            for (op_type, success) in operations {
                total_operations += 1;
                if success {
                    success_count += 1;
                } else {
                    failed_operations.push((user_idx, op_type));
                }
            }
        } else {
            // Task panicked
            panic!("Task failed: {:?}", result);
        }
    }
    
    // Calculate metrics
    let success_rate = success_count as f64 / total_operations as f64;
    let operations_per_second = total_operations as f64 / elapsed.as_secs_f64();
    
    println!("High Load Test Results:");
    println!("  Users: {}", NUM_USERS);
    println!("  Total Operations: {}", total_operations);
    println!("  Elapsed Time: {:.2?}", elapsed);
    println!("  Success Rate: {:.2}%", success_rate * 100.0);
    println!("  Operations/second: {:.2}", operations_per_second);
    
    // Assert high success rate
    assert!(success_rate > 0.95, "Expected success rate > 95%, got {:.2}%", success_rate * 100.0);
    
    // Assert reasonable throughput (adjust based on machine capabilities)
    assert!(operations_per_second > 100.0, "Expected at least 100 ops/sec, got {:.2}", operations_per_second);
    
    Ok(())
}
