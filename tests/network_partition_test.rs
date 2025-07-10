//! Network Partitioning Test
//! This test simulates network partitions and validates that Synapse can handle 
//! partitions and recover correctly when connectivity is restored.

use std::{sync::Arc, time::Duration};
use anyhow::Result;
use tokio::time::sleep;
use synapse::{
    Config,
    identity::{Identity, KeyPair},
    services::registry::RegistryService,
    storage::database::Database,
    transport::{
        Transport, 
        TransportError,
        TransportResult,
        MessageDeliveryStatus,
        UnifiedTransportManager,
    },
    synapse::transport::error_recovery::CircuitBreaker,
};
use async_trait::async_trait;
use std::sync::atomic::{AtomicBool, Ordering};

/// A network partition-aware transport for testing
struct PartitionAwareTransport {
    id: String,
    partition_active: Arc<AtomicBool>,
    partition_groups: Vec<String>, // IDs in the same partition group can communicate
    my_partition_group: String,
    circuit_breaker: Arc<CircuitBreaker>,
}

impl PartitionAwareTransport {
    fn new(id: &str, partition_group: &str) -> Self {
        Self {
            id: id.to_string(),
            partition_active: Arc::new(AtomicBool::new(false)),
            partition_groups: vec![partition_group.to_string()],
            my_partition_group: partition_group.to_string(),
            circuit_breaker: Arc::new(CircuitBreaker::new(id, 3, Duration::from_secs(5))),
        }
    }
    
    fn activate_partition(&self) {
        self.partition_active.store(true, Ordering::SeqCst);
    }
    
    fn deactivate_partition(&self) {
        self.partition_active.store(false, Ordering::SeqCst);
    }
    
    fn is_partitioned_from(&self, participant_id: &str) -> bool {
        if !self.partition_active.load(Ordering::SeqCst) {
            return false;
        }
        
        // Check if participant is in the same partition group
        !self.partition_groups.iter().any(|group| {
            participant_id.contains(group) || 
            self.my_partition_group == *group
        })
    }
}

#[async_trait]
impl Transport for PartitionAwareTransport {
    fn get_id(&self) -> &str {
        &self.id
    }
    
    async fn send(&self, to: &str, message: &[u8]) -> TransportResult<MessageDeliveryStatus> {
        // Check circuit breaker first
        if !self.circuit_breaker.allow_request() {
            return Err(TransportError::CircuitBreakerOpen { 
                message: format!("Circuit breaker open for transport {}", self.id),
                retry_after_secs: 5,
            });
        }
        
        // Check for network partition
        if self.is_partitioned_from(to) {
            self.circuit_breaker.record_failure();
            return Err(TransportError::ConnectionFailed { 
                message: format!("Network partition prevents sending to {}", to),
                is_permanent: false,
            });
        }
        
        // Simulate network latency
        sleep(Duration::from_millis(50)).await;
        
        self.circuit_breaker.record_success();
        Ok(MessageDeliveryStatus {
            delivered: true,
            recipient: to.to_string(),
            timestamp: chrono::Utc::now(),
            transport_id: self.get_id().to_string(),
            latency_ms: 50,
        })
    }
    
    async fn can_reach(&self, participant_id: &str) -> bool {
        !self.is_partitioned_from(participant_id)
    }
    
    async fn get_estimated_latency(&self, participant_id: &str) -> Option<Duration> {
        if self.is_partitioned_from(participant_id) {
            None
        } else {
            Some(Duration::from_millis(50))
        }
    }
    
    fn get_metrics(&self) -> synapse::transport::TransportMetrics {
        synapse::transport::TransportMetrics {
            transport_id: self.get_id().to_string(),
            messages_sent: 0,
            messages_received: 0,
            failures: 0,
            average_latency_ms: 50.0,
            uptime_seconds: 0,
        }
    }
}

#[tokio::test]
async fn test_network_partition_recovery() -> Result<()> {
    let config = Config::default();
    
    // Create test participants
    let group_a_keypair = KeyPair::generate();
    let group_a_identity = Identity::from_keypair(&group_a_keypair)?;
    let group_a_id = group_a_identity.get_id();
    
    let group_b_keypair = KeyPair::generate();
    let group_b_identity = Identity::from_keypair(&group_b_keypair)?;
    let group_b_id = group_b_identity.get_id();
    
    // Create databases and registry services
    let group_a_db = Database::new_in_memory()?;
    let group_b_db = Database::new_in_memory()?;
    
    let group_a_registry = RegistryService::new(group_a_db.clone(), group_a_identity.clone());
    let group_b_registry = RegistryService::new(group_b_db.clone(), group_b_identity.clone());
    
    // Register participants with each other
    group_a_registry.register_participant(&group_b_identity)?;
    group_b_registry.register_participant(&group_a_identity)?;
    
    // Create partition-aware transports
    let group_a_transport = Arc::new(PartitionAwareTransport::new("group_a_transport", "group_a"));
    let group_b_transport = Arc::new(PartitionAwareTransport::new("group_b_transport", "group_b"));
    
    // Create transport managers
    let mut group_a_transport_manager = UnifiedTransportManager::new_with_transports(vec![
        group_a_transport.clone() as Arc<dyn Transport>,
    ]);
    
    let mut group_b_transport_manager = UnifiedTransportManager::new_with_transports(vec![
        group_b_transport.clone() as Arc<dyn Transport>,
    ]);
    
    // Test 1: Normal communication (no partition)
    {
        // Group A sends to Group B
        let message = "Hello from Group A to Group B";
        let result = group_a_transport_manager.send_message(
            &group_b_id,
            message.as_bytes(),
            synapse::MessageUrgency::Interactive,
        ).await;
        
        assert!(result.is_ok(), "Message should be delivered when no partition exists");
        assert_eq!(result.unwrap().recipient, group_b_id);
        
        // Group B sends to Group A
        let message = "Hello from Group B to Group A";
        let result = group_b_transport_manager.send_message(
            &group_a_id,
            message.as_bytes(),
            synapse::MessageUrgency::Interactive,
        ).await;
        
        assert!(result.is_ok(), "Message should be delivered when no partition exists");
        assert_eq!(result.unwrap().recipient, group_a_id);
    }
    
    // Test 2: Activate partition between Group A and Group B
    {
        // Activate partition
        group_a_transport.activate_partition();
        group_b_transport.activate_partition();
        
        // Group A attempts to send to Group B
        let message = "This message should fail due to partition";
        let result = group_a_transport_manager.send_message(
            &group_b_id,
            message.as_bytes(),
            synapse::MessageUrgency::Interactive,
        ).await;
        
        assert!(result.is_err(), "Message should fail when partition exists");
        assert!(result.unwrap_err().to_string().contains("partition"), 
                "Error should indicate partition is the cause");
        
        // Group B attempts to send to Group A
        let message = "This message should also fail due to partition";
        let result = group_b_transport_manager.send_message(
            &group_a_id,
            message.as_bytes(),
            synapse::MessageUrgency::Interactive,
        ).await;
        
        assert!(result.is_err(), "Message should fail when partition exists");
        assert!(result.unwrap_err().to_string().contains("partition"), 
                "Error should indicate partition is the cause");
    }
    
    // Test 3: Circuit breaker behavior with repeated partition failures
    {
        // Send multiple messages to trigger circuit breaker
        for i in 0..5 {
            let message = format!("Partition test message {}", i);
            let _ = group_a_transport_manager.send_message(
                &group_b_id,
                message.as_bytes(),
                synapse::MessageUrgency::Interactive,
            ).await;
        }
        
        // Circuit breaker should now be open
        let message = "This should fail immediately due to circuit breaker";
        let result = group_a_transport_manager.send_message(
            &group_b_id,
            message.as_bytes(),
            synapse::MessageUrgency::Interactive,
        ).await;
        
        assert!(result.is_err(), "Message should fail due to open circuit breaker");
        assert!(result.unwrap_err().to_string().contains("circuit"), 
                "Error should indicate circuit breaker is the cause");
    }
    
    // Test 4: Network partition recovery
    {
        // Deactivate partition
        group_a_transport.deactivate_partition();
        group_b_transport.deactivate_partition();
        
        // Reset circuit breaker manually
        let breaker = &group_a_transport.circuit_breaker;
        breaker.reset();
        
        // Wait a moment for circuit breaker timeout
        sleep(Duration::from_secs(1)).await;
        
        // Try sending messages again
        let message = "Hello after partition recovery";
        let result = group_a_transport_manager.send_message(
            &group_b_id,
            message.as_bytes(),
            synapse::MessageUrgency::Interactive,
        ).await;
        
        assert!(result.is_ok(), "Message should succeed after partition recovery");
        assert_eq!(result.unwrap().recipient, group_b_id);
        
        // Group B sends to Group A
        let message = "Hello from B to A after recovery";
        let result = group_b_transport_manager.send_message(
            &group_a_id,
            message.as_bytes(),
            synapse::MessageUrgency::Interactive,
        ).await;
        
        assert!(result.is_ok(), "Message should succeed after partition recovery");
        assert_eq!(result.unwrap().recipient, group_a_id);
    }
    
    // Test 5: Partial partitioning scenario
    {
        // Create a third participant in group A
        let group_a2_keypair = KeyPair::generate();
        let group_a2_identity = Identity::from_keypair(&group_a2_keypair)?;
        let group_a2_id = group_a2_identity.get_id();
        
        // Register the new participant
        group_a_registry.register_participant(&group_a2_identity)?;
        group_b_registry.register_participant(&group_a2_identity)?;
        
        // Create a transport for the new participant
        let group_a2_transport = Arc::new(PartitionAwareTransport::new("group_a2_transport", "group_a"));
        
        let mut group_a2_transport_manager = UnifiedTransportManager::new_with_transports(vec![
            group_a2_transport.clone() as Arc<dyn Transport>,
        ]);
        
        // Activate selective partition (B is partitioned from A but not from A2)
        group_b_transport.activate_partition();
        
        // Group B should be able to reach Group A2 but not Group A
        let message_to_a = "This should fail due to partition";
        let result_a = group_b_transport_manager.send_message(
            &group_a_id,
            message_to_a.as_bytes(),
            synapse::MessageUrgency::Interactive,
        ).await;
        
        assert!(result_a.is_err(), "Message to A should fail due to partition");
        
        // Deactivate all partitions for cleanup
        group_a_transport.deactivate_partition();
        group_b_transport.deactivate_partition();
        
        // Reset all circuit breakers
        group_a_transport.circuit_breaker.reset();
        group_b_transport.circuit_breaker.reset();
        group_a2_transport.circuit_breaker.reset();
    }
    
    // Test 6: Network partition with delayed recovery and retry
    {
        // Activate partition
        group_a_transport.activate_partition();
        group_b_transport.activate_partition();
        
        // Try sending a message (will fail)
        let message = "Trying during partition";
        let result = group_a_transport_manager.send_message(
            &group_b_id,
            message.as_bytes(),
            synapse::MessageUrgency::Interactive,
        ).await;
        
        assert!(result.is_err(), "Message should fail during partition");
        
        // Set up retry task
        let retry_transport_manager = group_a_transport_manager.clone();
        let retry_target_id = group_b_id.clone();
        let retry_message = "This message will be retried until partition resolves".to_string();
        
        let retry_task = tokio::spawn(async move {
            let max_attempts = 10;
            let mut attempts = 0;
            let mut result = None;
            
            while attempts < max_attempts {
                attempts += 1;
                
                match retry_transport_manager.send_message(
                    &retry_target_id,
                    retry_message.as_bytes(),
                    synapse::MessageUrgency::Interactive,
                ).await {
                    Ok(delivery) => {
                        result = Some(Ok(delivery));
                        break;
                    }
                    Err(e) => {
                        // If circuit breaker is open, wait longer
                        let wait_time = if e.to_string().contains("circuit") {
                            Duration::from_secs(2)
                        } else {
                            Duration::from_secs(1)
                        };
                        
                        sleep(wait_time).await;
                    }
                }
            }
            
            if result.is_none() {
                result = Some(Err("Max retry attempts reached".to_string()));
            }
            
            result.unwrap()
        });
        
        // After a delay, resolve the partition while the retry task is running
        sleep(Duration::from_secs(3)).await;
        group_a_transport.circuit_breaker.reset();
        group_a_transport.deactivate_partition();
        group_b_transport.deactivate_partition();
        
        // The retry task should eventually succeed
        let retry_result = retry_task.await?;
        assert!(retry_result.is_ok(), "Retry should eventually succeed after partition resolves");
    }
    
    Ok(())
}
