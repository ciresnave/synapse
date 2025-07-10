//! Circuit Breaker Integration Demo
//!
//! This example demonstrates the circuit breaker functionality
//! integrated into the mDNS transport.

use synapse::{
    transport::{Transport, mdns_enhanced::{EnhancedMdnsTransport, MdnsConfig}},
    types::{SecureMessage, SecurityLevel},
    error::Result,
};
use tokio::time::{sleep, Duration};
use tracing::{info, warn};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt::init();
    
    info!("🔄 Circuit Breaker Integration Demo Starting");
    
    // Create an mDNS transport with circuit breaker
    let mut mdns_transport = EnhancedMdnsTransport::new(
        "demo-entity".to_string(),
        8080,
        Some(MdnsConfig::default()),
    ).await?;
    
    // Start the mDNS service
    mdns_transport.start().await?;
    
    info!("✅ mDNS Transport with Circuit Breaker created and started");
    
    // Get circuit breaker reference
    let circuit_breaker = mdns_transport.get_circuit_breaker();
    
    // Subscribe to circuit breaker events
    let mut event_receiver = circuit_breaker.subscribe_events();
    
    // Spawn a task to monitor circuit breaker events
    let event_monitor = tokio::spawn(async move {
        info!("📡 Starting circuit breaker event monitor");
        while let Ok(event) = event_receiver.recv().await {
            match event {
                synapse::circuit_breaker::CircuitEvent::Opened { reason, failure_count, .. } => {
                    warn!("🔴 Circuit breaker OPENED: {} (failures: {})", reason, failure_count);
                }
                synapse::circuit_breaker::CircuitEvent::HalfOpened { .. } => {
                    info!("🟡 Circuit breaker HALF-OPENED - testing recovery");
                }
                synapse::circuit_breaker::CircuitEvent::Closed { .. } => {
                    info!("🟢 Circuit breaker CLOSED - normal operation resumed");
                }
                synapse::circuit_breaker::CircuitEvent::RequestRejected { .. } => {
                    warn!("❌ Request rejected by circuit breaker");
                }
                synapse::circuit_breaker::CircuitEvent::ExternalTriggerActivated { trigger_name, .. } => {
                    info!("⚡ External trigger activated: {}", trigger_name);
                }
            }
        }
    });
    
    // Test normal circuit breaker operation
    info!("🧪 Testing circuit breaker states");
    
    // Check initial state
    let initial_stats = mdns_transport.get_circuit_stats();
    info!("📊 Initial circuit state: {:?}", initial_stats.state);
    info!("📊 Initial stats: {} requests, {} failures, {} successes", 
          initial_stats.total_requests, 
          initial_stats.failure_count, 
          initial_stats.success_count);
    
    // Test connectivity to a non-existent peer (should fail and trigger circuit breaker)
    info!("🔍 Testing connectivity to non-existent peer (expecting failures)");
    
    for i in 1..=5 {
        info!("📡 Attempt {} - testing connectivity to 'non-existent-peer'", i);
        
        let result = mdns_transport.test_connectivity("non-existent-peer").await;
        match result {
            Ok(metrics) => {
                info!("✅ Connectivity test succeeded: latency={:?}, reliability={:.2}", 
                      metrics.latency, metrics.reliability_score);
            }
            Err(e) => {
                warn!("❌ Connectivity test failed: {}", e);
            }
        }
        
        // Check if circuit is open
        if mdns_transport.is_circuit_open().await {
            warn!("🔴 Circuit breaker is now OPEN - further requests will be rejected immediately");
            break;
        }
        
        // Brief pause between attempts
        sleep(Duration::from_millis(500)).await;
    }
    
    // Show final statistics
    let final_stats = mdns_transport.get_circuit_stats();
    info!("📊 Final circuit state: {:?}", final_stats.state);
    info!("📊 Final stats: {} requests, {} failures, {} successes, {} rejections", 
          final_stats.total_requests, 
          final_stats.failure_count, 
          final_stats.success_count,
          final_stats.rejection_count);
    
    // Test sending a message (should be rejected if circuit is open)
    info!("📤 Testing message sending with circuit breaker protection");
    
    let test_message = SecureMessage::new(
        "non-existent-peer".to_string(),
        "demo-entity".to_string(),
        b"Hello, this is a test message".to_vec(),
        b"fake-signature".to_vec(),
        SecurityLevel::Public,
    );
    
    match mdns_transport.send_message("non-existent-peer", &test_message).await {
        Ok(message_id) => {
            info!("✅ Message sent successfully: {}", message_id);
        }
        Err(e) => {
            warn!("❌ Message sending failed: {}", e);
            if e.to_string().contains("Circuit breaker") {
                info!("🔄 This failure was prevented by the circuit breaker - protecting the system!");
            }
        }
    }
    
    // Wait for recovery timeout and test recovery
    info!("⏱️ Waiting for circuit breaker recovery timeout (10 seconds)...");
    sleep(Duration::from_secs(11)).await;
    
    info!("🔄 Testing circuit breaker recovery");
    match mdns_transport.test_connectivity("non-existent-peer").await {
        Ok(_) => {
            info!("✅ Recovery test succeeded");
        }
        Err(e) => {
            warn!("❌ Recovery test failed: {}", e);
            info!("🔄 Circuit breaker is now in half-open state, testing recovery");
        }
    }
    
    // Show final statistics after recovery attempt
    let recovery_stats = mdns_transport.get_circuit_stats();
    info!("📊 After recovery attempt - state: {:?}", recovery_stats.state);
    info!("📊 After recovery stats: {} requests, {} failures, {} successes, {} rejections", 
          recovery_stats.total_requests, 
          recovery_stats.failure_count, 
          recovery_stats.success_count,
          recovery_stats.rejection_count);
    
    // Clean shutdown
    event_monitor.abort();
    
    info!("🏁 Circuit Breaker Demo completed successfully!");
    info!("💡 Key benefits demonstrated:");
    info!("   - Automatic failure detection and circuit opening");
    info!("   - Protection against cascading failures");
    info!("   - Automatic recovery attempts");
    info!("   - Real-time monitoring and statistics");
    info!("   - Integration with transport layer");
    
    Ok(())
}
