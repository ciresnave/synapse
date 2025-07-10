//! End-to-End Integration Test
//! This test validates the entire Synapse system from end to end, including:
//! - Participant registration in the registry
//! - Trust establishment
//! - Message routing with circuit breakers
//! - Transport fallback mechanisms
//! - Error recovery systems

use std::{sync::Arc, time::Duration};
use anyhow::Result;
use tokio::time::sleep;
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
        TransportError,
        TransportResult,
        MessageDeliveryStatus,
    },
    synapse::transport::error_recovery::{CircuitBreaker, RetryPolicy, ConnectionHealthMonitor},
};
use async_trait::async_trait;

/// A mock transport that can be controlled to simulate various failure scenarios
struct MockNetworkTransport {
    id: String,
    // Circuit breaker integration
    circuit_breaker: Arc<CircuitBreaker>,
    // Health monitor for this transport
    health_monitor: Arc<ConnectionHealthMonitor>,
    // Configurable failure behavior
    failure_rate: f64,
    introduce_latency_ms: u64,
    simulate_network_partition: bool,
}

impl MockNetworkTransport {
    fn new(id: &str) -> Self {
        Self {
            id: id.to_string(),
            circuit_breaker: Arc::new(CircuitBreaker::new(id, 3, Duration::from_secs(5))),
            health_monitor: Arc::new(ConnectionHealthMonitor::new()),
            failure_rate: 0.0,
            introduce_latency_ms: 0,
            simulate_network_partition: false,
        }
    }
    
    // Control functions to simulate different network conditions
    fn set_failure_rate(&mut self, rate: f64) {
        self.failure_rate = rate.clamp(0.0, 1.0);
    }
    
    fn set_latency(&mut self, latency_ms: u64) {
        self.introduce_latency_ms = latency_ms;
    }
    
    fn simulate_partition(&mut self, partition: bool) {
        self.simulate_network_partition = partition;
    }
    
    fn reset(&mut self) {
        self.failure_rate = 0.0;
        self.introduce_latency_ms = 0;
        self.simulate_network_partition = false;
        self.circuit_breaker.reset();
    }
    
    // Internal method to determine if a request should fail based on configuration
    async fn should_fail(&self) -> bool {
        // Check circuit breaker first
        if !self.circuit_breaker.allow_request() {
            return true;
        }
        
        // Simulate network partition
        if self.simulate_network_partition {
            return true;
        }
        
        // Apply probabilistic failure based on rate
        let fails = rand::random::<f64>() < self.failure_rate;
        
        // If configured, add latency
        if self.introduce_latency_ms > 0 {
            sleep(Duration::from_millis(self.introduce_latency_ms)).await;
        }
        
        fails
    }
}

#[async_trait]
impl Transport for MockNetworkTransport {
    fn get_id(&self) -> &str {
        &self.id
    }
    
    async fn send(&self, to: &str, message: &[u8]) -> TransportResult<MessageDeliveryStatus> {
        if self.should_fail().await {
            self.circuit_breaker.record_failure();
            self.health_monitor.record_failure();
            
            Err(TransportError::ConnectionFailed { 
                message: format!("Failed to send message to {}", to),
                is_permanent: false,
            })
        } else {
            self.circuit_breaker.record_success();
            self.health_monitor.record_success();
            
            Ok(MessageDeliveryStatus {
                delivered: true,
                recipient: to.to_string(),
                timestamp: chrono::Utc::now(),
                transport_id: self.get_id().to_string(),
                latency_ms: self.introduce_latency_ms as u32,
            })
        }
    }
    
    async fn can_reach(&self, _participant_id: &str) -> bool {
        !self.should_fail().await
    }
    
    async fn get_estimated_latency(&self, _participant_id: &str) -> Option<Duration> {
        if self.should_fail().await {
            None
        } else {
            Some(Duration::from_millis(self.introduce_latency_ms))
        }
    }
    
    fn get_metrics(&self) -> synapse::transport::TransportMetrics {
        synapse::transport::TransportMetrics {
            transport_id: self.get_id().to_string(),
            messages_sent: 0,
            messages_received: 0,
            failures: 0,
            average_latency_ms: self.introduce_latency_ms as f64,
            uptime_seconds: 0,
        }
    }
}

#[tokio::test]
async fn test_end_to_end_with_error_recovery() -> Result<()> {
    // Create basic test configuration
    let config = Config::default();
    
    // 1. Set up participants with identities
    let alice_keypair = KeyPair::generate();
    let alice_identity = Identity::from_keypair(&alice_keypair)?;
    let alice_id = alice_identity.get_id();
    
    let bob_keypair = KeyPair::generate();
    let bob_identity = Identity::from_keypair(&bob_keypair)?;
    let bob_id = bob_identity.get_id();
    
    let charlie_keypair = KeyPair::generate();
    let charlie_identity = Identity::from_keypair(&charlie_keypair)?;
    let charlie_id = charlie_identity.get_id();
    
    // 2. Set up in-memory databases
    let alice_db = Database::new_in_memory()?;
    let bob_db = Database::new_in_memory()?;
    let charlie_db = Database::new_in_memory()?;
    
    // 3. Create registry services
    let alice_registry = RegistryService::new(alice_db.clone(), alice_identity.clone());
    let bob_registry = RegistryService::new(bob_db.clone(), bob_identity.clone());
    let charlie_registry = RegistryService::new(charlie_db.clone(), charlie_identity.clone());
    
    // 4. Create trust managers
    let alice_trust = TrustManager::new(alice_db, alice_identity.clone());
    let bob_trust = TrustManager::new(bob_db, bob_identity.clone());
    let charlie_trust = TrustManager::new(charlie_db, charlie_identity.clone());
    
    // 5. Register participants in each other's registries
    alice_registry.register_participant(&bob_identity)?;
    alice_registry.register_participant(&charlie_identity)?;
    
    bob_registry.register_participant(&alice_identity)?;
    bob_registry.register_participant(&charlie_identity)?;
    
    charlie_registry.register_participant(&alice_identity)?;
    charlie_registry.register_participant(&bob_identity)?;
    
    // 6. Establish trust relationships
    alice_trust.trust_participant(&bob_id, 0.8)?;
    alice_trust.trust_participant(&charlie_id, 0.7)?;
    
    bob_trust.trust_participant(&alice_id, 0.9)?;
    bob_trust.trust_participant(&charlie_id, 0.6)?;
    
    charlie_trust.trust_participant(&alice_id, 0.8)?;
    charlie_trust.trust_participant(&bob_id, 0.7)?;
    
    // 7. Create transport managers with mock transports
    // Alice's transports
    let alice_primary = Arc::new(MockNetworkTransport::new("alice_primary"));
    let alice_secondary = Arc::new(MockNetworkTransport::new("alice_secondary"));
    let alice_fallback = Arc::new(MockNetworkTransport::new("alice_fallback"));
    
    let alice_transport_manager = UnifiedTransportManager::new_with_transports(vec![
        alice_primary.clone() as Arc<dyn Transport>,
        alice_secondary.clone() as Arc<dyn Transport>,
        alice_fallback.clone() as Arc<dyn Transport>,
    ]);
    
    // Bob's transports
    let bob_primary = Arc::new(MockNetworkTransport::new("bob_primary"));
    let bob_secondary = Arc::new(MockNetworkTransport::new("bob_secondary"));
    
    let bob_transport_manager = UnifiedTransportManager::new_with_transports(vec![
        bob_primary.clone() as Arc<dyn Transport>,
        bob_secondary.clone() as Arc<dyn Transport>,
    ]);
    
    // Charlie's transport
    let charlie_transport = Arc::new(MockNetworkTransport::new("charlie_transport"));
    
    let charlie_transport_manager = UnifiedTransportManager::new_with_transports(vec![
        charlie_transport.clone() as Arc<dyn Transport>,
    ]);
    
    // 8. Set up retry policy for transport operations
    let retry_policy = RetryPolicy::new(3, 100)
        .with_max_backoff(2)
        .with_jitter_factor(0.1);

    // Test 1: Normal communication - all systems operational
    {
        tracing::info!("Test 1: Normal communication path");
        let message = "Hello Bob, this is Alice!";
        
        let result = alice_transport_manager.send_message(
            &bob_id,
            message.as_bytes(),
            MessageUrgency::Interactive,
        ).await;
        
        assert!(result.is_ok(), "Message should be delivered successfully under normal conditions");
        
        let delivery = result.unwrap();
        assert_eq!(delivery.transport_id, "alice_primary", "Should use primary transport");
        assert_eq!(delivery.recipient, bob_id, "Recipient should be Bob");
        assert!(delivery.delivered, "Message should be delivered");
    }
    
    // Test 2: Primary transport failure with automatic failover
    {
        tracing::info!("Test 2: Primary transport failure with failover");
        
        // Make primary transport fail
        if let Some(transport) = alice_primary.as_any().downcast_ref::<MockNetworkTransport>() {
            let mut transport = unsafe { &mut *(transport as *const MockNetworkTransport as *mut MockNetworkTransport) };
            transport.set_failure_rate(1.0); // Always fail
        }
        
        let message = "Hello Bob, this is Alice with a failover message!";
        
        let result = alice_transport_manager.send_message(
            &bob_id,
            message.as_bytes(),
            MessageUrgency::Interactive,
        ).await;
        
        assert!(result.is_ok(), "Message should be delivered successfully via failover");
        
        let delivery = result.unwrap();
        assert_eq!(delivery.transport_id, "alice_secondary", "Should use secondary transport");
        assert_eq!(delivery.recipient, bob_id, "Recipient should be Bob");
        assert!(delivery.delivered, "Message should be delivered");
        
        // Reset primary transport
        if let Some(transport) = alice_primary.as_any().downcast_ref::<MockNetworkTransport>() {
            let mut transport = unsafe { &mut *(transport as *const MockNetworkTransport as *mut MockNetworkTransport) };
            transport.reset();
        }
    }
    
    // Test 3: High latency scenario with circuit breaker
    {
        tracing::info!("Test 3: High latency scenario with circuit breaker");
        
        // Make primary transport very slow
        if let Some(transport) = alice_primary.as_any().downcast_ref::<MockNetworkTransport>() {
            let mut transport = unsafe { &mut *(transport as *const MockNetworkTransport as *mut MockNetworkTransport) };
            transport.set_latency(1000); // 1 second latency
        }
        
        // Configure transport manager with a short timeout
        alice_transport_manager.set_timeout(Duration::from_millis(500));
        
        // Send multiple messages to trigger circuit breaker
        for i in 1..=5 {
            let message = format!("Test message {}", i);
            
            let _ = alice_transport_manager.send_message(
                &bob_id,
                message.as_bytes(),
                MessageUrgency::Interactive,
            ).await;
        }
        
        // Now the primary transport should be marked as unhealthy by the circuit breaker
        // The next message should use the secondary transport immediately
        let message = "This should use the secondary transport directly";
        
        let result = alice_transport_manager.send_message(
            &bob_id,
            message.as_bytes(),
            MessageUrgency::Interactive,
        ).await;
        
        assert!(result.is_ok(), "Message should be delivered via secondary after circuit breaker trips");
        assert_eq!(result.unwrap().transport_id, "alice_secondary", "Should use secondary transport due to circuit breaker");
        
        // Reset primary transport
        if let Some(transport) = alice_primary.as_any().downcast_ref::<MockNetworkTransport>() {
            let mut transport = unsafe { &mut *(transport as *const MockNetworkTransport as *mut MockNetworkTransport) };
            transport.reset();
        }
        
        // Reset timeout to default
        alice_transport_manager.set_timeout(Duration::from_secs(30));
    }
    
    // Test 4: Complete network partition and retry policy
    {
        tracing::info!("Test 4: Complete network partition and retry policy");
        
        // Simulate network partition for all Alice's transports
        if let Some(transport) = alice_primary.as_any().downcast_ref::<MockNetworkTransport>() {
            let mut transport = unsafe { &mut *(transport as *const MockNetworkTransport as *mut MockNetworkTransport) };
            transport.simulate_partition(true);
        }
        if let Some(transport) = alice_secondary.as_any().downcast_ref::<MockNetworkTransport>() {
            let mut transport = unsafe { &mut *(transport as *const MockNetworkTransport as *mut MockNetworkTransport) };
            transport.simulate_partition(true);
        }
        if let Some(transport) = alice_fallback.as_any().downcast_ref::<MockNetworkTransport>() {
            let mut transport = unsafe { &mut *(transport as *const MockNetworkTransport as *mut MockNetworkTransport) };
            transport.simulate_partition(true);
        }
        
        // Use retry policy to attempt sending message
        let message = "Hello Bob, hope this reaches you despite the network partition!";
        
        let result = retry_policy.execute(|| {
            let transport_manager = alice_transport_manager.clone();
            let bob_id = bob_id.clone();
            let message = message.to_string();
            
            Box::pin(async move {
                transport_manager.send_message(
                    &bob_id,
                    message.as_bytes(),
                    MessageUrgency::Critical, // Escalate to critical
                ).await
            })
        }).await;
        
        assert!(result.is_err(), "Message should fail after retry policy exhaustion");
        
        // Reset all transports
        for transport in [alice_primary.clone(), alice_secondary.clone(), alice_fallback.clone()] {
            if let Some(transport) = transport.as_any().downcast_ref::<MockNetworkTransport>() {
                let mut transport = unsafe { &mut *(transport as *const MockNetworkTransport as *mut MockNetworkTransport) };
                transport.reset();
            }
        }
    }
    
    // Test 5: Recovery after network partition
    {
        tracing::info!("Test 5: Recovery after network partition");
        
        // Ensure all transports are reset
        for transport in [alice_primary.clone(), alice_secondary.clone(), alice_fallback.clone()] {
            if let Some(transport) = transport.as_any().downcast_ref::<MockNetworkTransport>() {
                let mut transport = unsafe { &mut *(transport as *const MockNetworkTransport as *mut MockNetworkTransport) };
                transport.reset();
            }
        }
        
        // Wait for circuit breaker timeout to pass
        sleep(Duration::from_secs(5)).await;
        
        // Try sending a message after recovery
        let message = "Hello Bob, the network should be recovered now!";
        
        let result = alice_transport_manager.send_message(
            &bob_id,
            message.as_bytes(),
            MessageUrgency::Interactive,
        ).await;
        
        assert!(result.is_ok(), "Message should be delivered after network recovery");
        assert_eq!(result.unwrap().transport_id, "alice_primary", "Should use primary transport after recovery");
    }

    Ok(())
}
