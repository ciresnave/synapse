//! Demo of the unified transport manager with multiple transport types
//! 
//! This example shows how to:
//! 1. Set up a transport manager with multiple transports
//! 2. Send messages using automatic transport selection
//! 3. Receive messages from multiple transports
//! 4. Monitor transport metrics and status

use synapse::{
    transport::{
        TransportManagerBuilder, TransportTarget,
        TcpTransportFactory, UdpTransportFactory, TransportType, TransportSelectionPolicy,
    },
    types::{SecureMessage, SecurityLevel},
    error::Result,
};
use std::{time::Duration, collections::HashMap};
use tokio::time::sleep;
use tracing::{info, warn, error};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .init();

    info!("Starting unified transport manager demo");

    // Create transport manager with custom configuration
    let manager = TransportManagerBuilder::new()
        .enable_transport(TransportType::Tcp)
        .enable_transport(TransportType::Udp)
        .selection_policy(TransportSelectionPolicy::Adaptive)
        .operation_timeout(Duration::from_secs(30))
        .transport_config(TransportType::Tcp, {
            let mut config = HashMap::new();
            config.insert("listen_port".to_string(), "8080".to_string());
            config.insert("connection_timeout_ms".to_string(), "5000".to_string());
            config
        })
        .transport_config(TransportType::Udp, {
            let mut config = HashMap::new();
            config.insert("bind_port".to_string(), "8081".to_string());
            config.insert("max_message_size".to_string(), "8192".to_string());
            config
        })
        .build();

    // Register transport factories
    manager.register_factory(Box::new(TcpTransportFactory)).await?;
    manager.register_factory(Box::new(UdpTransportFactory)).await?;

    // Start the transport manager
    info!("Starting transport manager...");
    manager.start().await?;

    // Wait for transports to start up
    sleep(Duration::from_secs(1)).await;

    // Check transport status
    let status = manager.get_transport_status().await;
    info!("Transport status: {:?}", status);

    // Demo 1: Send a real-time message (should prefer UDP)
    info!("\n--- Demo 1: Real-time message ---");
    let realtime_target = TransportTarget::new("localhost".to_string())
        .with_address("127.0.0.1:8081".to_string())
        .with_urgency(synapse::transport::abstraction::MessageUrgency::RealTime);

    let realtime_message = SecureMessage::new(
        "demo-recipient".to_string(),
        "demo-sender".to_string(),
        "This is a real-time message!".as_bytes().to_vec(),
        vec![], // signature placeholder
        SecurityLevel::Secure,
    );

    match manager.send_message(&realtime_target, &realtime_message).await {
        Ok(receipt) => {
            info!("Real-time message sent successfully!");
            info!("  Transport used: {:?}", receipt.transport_used);
            info!("  Delivery time: {:?}", receipt.delivery_time);
            info!("  Target reached: {}", receipt.target_reached);
        }
        Err(e) => {
            warn!("Failed to send real-time message: {}", e);
        }
    }

    // Demo 2: Send a background message (should prefer TCP for reliability)
    info!("\n--- Demo 2: Background message ---");
    let background_target = TransportTarget::new("localhost".to_string())
        .with_address("127.0.0.1:8080".to_string())
        .with_urgency(synapse::transport::abstraction::MessageUrgency::Background);

    let background_message = SecureMessage::new(
        "demo-recipient".to_string(),
        "demo-sender".to_string(),
        "This is a background message with more data that requires reliability.".as_bytes().to_vec(),
        vec![], // signature placeholder
        SecurityLevel::Secure,
    );

    match manager.send_message(&background_target, &background_message).await {
        Ok(receipt) => {
            info!("Background message sent successfully!");
            info!("  Transport used: {:?}", receipt.transport_used);
            info!("  Delivery time: {:?}", receipt.delivery_time);
            info!("  Target reached: {}", receipt.target_reached);
        }
        Err(e) => {
            warn!("Failed to send background message: {}", e);
        }
    }

    // Demo 3: Test connectivity to various targets
    info!("\n--- Demo 3: Connectivity tests ---");
    let test_targets = vec![
        ("TCP Local", TransportTarget::new("localhost".to_string()).with_address("127.0.0.1:8080".to_string())),
        ("UDP Local", TransportTarget::new("localhost".to_string()).with_address("127.0.0.1:8081".to_string())),
        ("Invalid Target", TransportTarget::new("invalid".to_string()).with_address("999.999.999.999:9999".to_string())),
    ];

    for (name, target) in test_targets {
        info!("Testing connectivity to {}", name);
        
        // Note: Direct access to transport factories is not available in public API
        // This would require using the transport manager's public methods instead
        info!("  Connectivity test would be performed via transport manager");
    }

    // Demo 4: Monitor metrics for a period
    info!("\n--- Demo 4: Monitoring metrics ---");
    for i in 0..5 {
        sleep(Duration::from_secs(2)).await;
        
        let metrics = manager.get_metrics().await;
        info!("Metrics snapshot {} :", i + 1);
        info!("  Total messages sent: {}", metrics.total_messages_sent);
        info!("  Total messages received: {}", metrics.total_messages_received);
        info!("  Total failures: {}", metrics.total_failures);
        info!("  Overall reliability: {:.2}%", metrics.overall_reliability * 100.0);
        info!("  Average latency: {:?}", metrics.average_latency);
        
        // Show per-transport metrics
        for (transport_type, transport_metrics) in &metrics.transport_metrics {
            info!("  {:?}: sent={}, received={}, failures={}, reliability={:.2}%", 
                  transport_type,
                  transport_metrics.messages_sent,
                  transport_metrics.messages_received,
                  transport_metrics.send_failures,
                  transport_metrics.reliability_score * 100.0);
        }
    }

    // Demo 5: Check for received messages
    info!("\n--- Demo 5: Checking for received messages ---");
    match manager.receive_messages().await {
        Ok(messages) => {
            if messages.is_empty() {
                info!("No messages received");
            } else {
                info!("Received {} messages:", messages.len());
                for msg in messages {
                    let content_str = String::from_utf8_lossy(&msg.message.encrypted_content);
                    info!("  Message from {} via {:?}: {}", 
                          msg.source, msg.transport_type, content_str);
                }
            }
        }
        Err(e) => {
            error!("Failed to receive messages: {}", e);
        }
    }

    // Demo 6: Transport failover simulation
    info!("\n--- Demo 6: Failover simulation ---");
    
    // Try to send to an unreachable target to trigger failover
    let unreachable_target = TransportTarget::new("unreachable".to_string())
        .with_address("192.0.2.1:9999".to_string()) // RFC 5737 test address
        .with_urgency(synapse::transport::abstraction::MessageUrgency::Interactive);

    let failover_message = SecureMessage::new(
        "demo-recipient".to_string(),
        "demo-sender".to_string(),
        "This message should trigger failover".as_bytes().to_vec(),
        vec![], // signature placeholder
        SecurityLevel::Secure,
    );

    match manager.send_message(&unreachable_target, &failover_message).await {
        Ok(receipt) => {
            info!("Failover message sent (unexpected success):");
            info!("  Transport used: {:?}", receipt.transport_used);
        }
        Err(e) => {
            info!("Failover test completed (expected failure): {}", e);
        }
    }

    // Clean shutdown
    info!("\n--- Shutting down transport manager ---");
    manager.stop().await?;
    
    info!("Unified transport manager demo completed successfully!");
    Ok(())
}

/// Helper function to create a sample message
fn create_sample_message(id: &str, content: &str) -> SecureMessage {
    SecureMessage::new(
        "demo-recipient".to_string(),
        "demo-sender".to_string(),
        content.as_bytes().to_vec(),
        vec![], // signature placeholder
        SecurityLevel::Secure,
    )
}
