//! Transport Error Handling Test
//! Tests error handling and recovery in the transport layer

use synapse::{
    transport::{Transport, TransportError, TransportResult, MessageDeliveryStatus, UnifiedTransportManager},
    Config, MessageUrgency,
};
use anyhow::Result;
use async_trait::async_trait;
use std::{sync::{Arc, atomic::{AtomicBool, Ordering}}, time::Duration};
use tokio::time::sleep;

// Mock transport that simulates failures
struct MockFailingTransport {
    id: String,
    should_fail: Arc<AtomicBool>,
}

impl MockFailingTransport {
    fn new(id: &str) -> Self {
        Self {
            id: id.to_string(),
            should_fail: Arc::new(AtomicBool::new(false)),
        }
    }
    
    fn set_failing(&self, should_fail: bool) {
        self.should_fail.store(should_fail, Ordering::SeqCst);
    }
}

#[async_trait]
impl Transport for MockFailingTransport {
    fn get_id(&self) -> &str {
        &self.id
    }
    
    async fn send(&self, to: &str, message: &[u8]) -> TransportResult<MessageDeliveryStatus> {
        if self.should_fail.load(Ordering::SeqCst) {
            Err(TransportError::ConnectionFailed { 
                message: "Simulated transport failure".into(),
                is_permanent: false,
            })
        } else {
            // Simulate network delay
            sleep(Duration::from_millis(50)).await;
            Ok(MessageDeliveryStatus {
                delivered: true,
                recipient: to.to_string(),
                timestamp: chrono::Utc::now(),
                transport_id: self.get_id().to_string(),
                latency_ms: 50,
            })
        }
    }
    
    async fn can_reach(&self, _participant_id: &str) -> bool {
        !self.should_fail.load(Ordering::SeqCst)
    }
    
    async fn get_estimated_latency(&self, _participant_id: &str) -> Option<Duration> {
        if self.should_fail.load(Ordering::SeqCst) {
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
async fn test_transport_error_handling_and_recovery() -> Result<()> {
    // Create transports
    let primary_transport = Arc::new(MockFailingTransport::new("primary"));
    let backup_transport = Arc::new(MockFailingTransport::new("backup"));
    let fallback_transport = Arc::new(MockFailingTransport::new("fallback"));
    
    // Create transport manager with the mock transports
    let mut transport_manager = UnifiedTransportManager::new_with_transports(vec![
        primary_transport.clone() as Arc<dyn Transport>,
        backup_transport.clone() as Arc<dyn Transport>,
        fallback_transport.clone() as Arc<dyn Transport>,
    ]);
    
    // Test 1: All transports working
    {
        let result = transport_manager.send_message(
            "test-recipient", 
            "Test message 1".as_bytes(),
            MessageUrgency::Interactive,
        ).await;
        
        assert!(result.is_ok(), "Message should be sent successfully when all transports work");
        assert_eq!(result.unwrap().transport_id, "primary", "Should use primary transport when available");
    }
    
    // Test 2: Primary transport fails
    {
        primary_transport.set_failing(true);
        
        let result = transport_manager.send_message(
            "test-recipient", 
            "Test message 2".as_bytes(),
            MessageUrgency::Interactive,
        ).await;
        
        assert!(result.is_ok(), "Message should be sent successfully despite primary failure");
        assert_eq!(result.unwrap().transport_id, "backup", "Should fall back to backup transport");
    }
    
    // Test 3: Primary and backup transports fail
    {
        backup_transport.set_failing(true);
        
        let result = transport_manager.send_message(
            "test-recipient", 
            "Test message 3".as_bytes(),
            MessageUrgency::Interactive,
        ).await;
        
        assert!(result.is_ok(), "Message should be sent successfully despite multiple failures");
        assert_eq!(result.unwrap().transport_id, "fallback", "Should fall back to fallback transport");
    }
    
    // Test 4: All transports fail
    {
        fallback_transport.set_failing(true);
        
        let result = transport_manager.send_message(
            "test-recipient", 
            "Test message 4".as_bytes(),
            MessageUrgency::Interactive,
        ).await;
        
        assert!(result.is_err(), "Message should fail when all transports fail");
        
        // Verify error is a transport error with the right type
        match result {
            Err(err) => {
                let err_str = err.to_string();
                assert!(
                    err_str.contains("All transports failed"),
                    "Error should indicate all transports failed: {}", err_str
                );
            }
            Ok(_) => panic!("Expected error when all transports fail"),
        }
    }
    
    // Test 5: Recovery when primary becomes available again
    {
        primary_transport.set_failing(false);
        
        let result = transport_manager.send_message(
            "test-recipient", 
            "Test message 5".as_bytes(),
            MessageUrgency::Interactive,
        ).await;
        
        assert!(result.is_ok(), "Message should be sent successfully after recovery");
        assert_eq!(result.unwrap().transport_id, "primary", "Should use primary transport after recovery");
    }
    
    // Test 6: High urgency message with failing primary
    {
        primary_transport.set_failing(true);
        
        let result = transport_manager.send_message(
            "test-recipient", 
            "Test message 6".as_bytes(),
            MessageUrgency::RealTime,
        ).await;
        
        assert!(result.is_ok(), "High urgency message should be sent successfully");
    }
    
    // Test 7: Transport capability detection
    {
        primary_transport.set_failing(false);
        backup_transport.set_failing(false);
        fallback_transport.set_failing(false);
        
        let capabilities = transport_manager.get_available_transports("test-recipient").await?;
        assert_eq!(capabilities.len(), 3, "Should detect all three transports as available");
        
        primary_transport.set_failing(true);
        let capabilities = transport_manager.get_available_transports("test-recipient").await?;
        assert_eq!(capabilities.len(), 2, "Should detect two transports as available when primary fails");
    }
    
    Ok(())
}

#[tokio::test]
async fn test_transport_timeout_handling() -> Result<()> {
    // Create a custom transport that simulates timeouts
    struct TimeoutTransport;
    
    #[async_trait]
    impl Transport for TimeoutTransport {
        fn get_id(&self) -> &str {
            "timeout_transport"
        }
        
        async fn send(&self, _to: &str, _message: &[u8]) -> TransportResult<MessageDeliveryStatus> {
            // Simulate a long delay beyond reasonable timeout
            sleep(Duration::from_secs(5)).await;
            
            Ok(MessageDeliveryStatus {
                delivered: true,
                recipient: "test".to_string(),
                timestamp: chrono::Utc::now(),
                transport_id: self.get_id().to_string(),
                latency_ms: 5000,
            })
        }
        
        async fn can_reach(&self, _participant_id: &str) -> bool {
            true
        }
        
        async fn get_estimated_latency(&self, _participant_id: &str) -> Option<Duration> {
            Some(Duration::from_millis(5000))
        }
        
        fn get_metrics(&self) -> synapse::transport::TransportMetrics {
            synapse::transport::TransportMetrics {
                transport_id: self.get_id().to_string(),
                messages_sent: 0,
                messages_received: 0,
                failures: 0,
                average_latency_ms: 5000.0,
                uptime_seconds: 0,
            }
        }
    }
    
    // Create transport manager with custom timeout
    let timeout_transport = Arc::new(TimeoutTransport);
    let mut transport_manager = UnifiedTransportManager::new_with_transports(vec![
        timeout_transport as Arc<dyn Transport>
    ]);
    
    // Set a short timeout
    transport_manager.set_timeout(Duration::from_millis(500));
    
    // Test: Send message with timeout
    let result = transport_manager.send_message(
        "test-recipient", 
        "Timeout test message".as_bytes(),
        MessageUrgency::Interactive,
    ).await;
    
    assert!(result.is_err(), "Message should time out");
    
    // Verify error is a timeout error
    match result {
        Err(err) => {
            let err_str = err.to_string();
            assert!(
                err_str.contains("timeout") || err_str.contains("timed out"),
                "Error should indicate timeout: {}", err_str
            );
        }
        Ok(_) => panic!("Expected timeout error"),
    }
    
    Ok(())
}

#[tokio::test]
async fn test_concurrent_transport_failures() -> Result<()> {
    // Create a transport that randomly fails
    struct RandomFailureTransport {
        id: String,
        failure_rate: f64,
    }
    
    impl RandomFailureTransport {
        fn new(id: &str, failure_rate: f64) -> Self {
            Self {
                id: id.to_string(),
                failure_rate,
            }
        }
    }
    
    #[async_trait]
    impl Transport for RandomFailureTransport {
        fn get_id(&self) -> &str {
            &self.id
        }
        
        async fn send(&self, to: &str, _message: &[u8]) -> TransportResult<MessageDeliveryStatus> {
            // Random failure based on failure_rate
            if rand::random::<f64>() < self.failure_rate {
                Err(TransportError::ConnectionFailed { 
                    message: format!("Random failure to {}", to),
                    is_permanent: false,
                })
            } else {
                // Simulate variable network delay
                let latency = rand::random::<u64>() % 200 + 20; // 20-220ms
                sleep(Duration::from_millis(latency)).await;
                
                Ok(MessageDeliveryStatus {
                    delivered: true,
                    recipient: to.to_string(),
                    timestamp: chrono::Utc::now(),
                    transport_id: self.get_id().to_string(),
                    latency_ms: latency as u32,
                })
            }
        }
        
        async fn can_reach(&self, _participant_id: &str) -> bool {
            rand::random::<f64>() >= self.failure_rate
        }
        
        async fn get_estimated_latency(&self, _participant_id: &str) -> Option<Duration> {
            Some(Duration::from_millis(100))
        }
        
        fn get_metrics(&self) -> synapse::transport::TransportMetrics {
            synapse::transport::TransportMetrics {
                transport_id: self.get_id().to_string(),
                messages_sent: 0,
                messages_received: 0,
                failures: 0,
                average_latency_ms: 100.0,
                uptime_seconds: 0,
            }
        }
    }
    
    // Create transport manager with multiple random failure transports
    let transports: Vec<Arc<dyn Transport>> = vec![
        Arc::new(RandomFailureTransport::new("t1", 0.3)),
        Arc::new(RandomFailureTransport::new("t2", 0.3)),
        Arc::new(RandomFailureTransport::new("t3", 0.3)),
        Arc::new(RandomFailureTransport::new("t4", 0.3)),
    ];
    
    let transport_manager = UnifiedTransportManager::new_with_transports(transports);
    
    // Send multiple messages concurrently to test handling of intermittent failures
    let mut handles = vec![];
    
    for i in 0..20 {
        let tm = transport_manager.clone();
        let handle = tokio::spawn(async move {
            let recipient = format!("recipient-{}", i % 5);
            let message = format!("Test message {}", i);
            
            tm.send_message(
                &recipient,
                message.as_bytes(),
                MessageUrgency::Interactive,
            ).await
        });
        
        handles.push(handle);
    }
    
    // Wait for all messages to be processed
    let results = futures::future::join_all(handles).await;
    
    // Count successes and failures
    let successes = results.iter()
        .filter(|r| r.is_ok() && r.as_ref().unwrap().is_ok())
        .count();
    
    let failures = results.len() - successes;
    
    // With our failure rates, we should have some successes and likely some failures
    println!("Concurrent test results: {} successes, {} failures", successes, failures);
    
    // At least some messages should get through with our redundant transports
    assert!(successes > 0, "At least some messages should succeed");
    
    Ok(())
}
