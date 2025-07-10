//! Basic test to verify unified transport abstraction system is working

use synapse::{
    transport::{
        TransportManager, TransportManagerConfig,
        TcpTransportFactory, UdpTransportFactory,
        TransportTarget,
        abstraction::MessageUrgency,
    },
    types::{SecureMessage, SecurityLevel},
    error::Result,
};
use tokio;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt::init();
    
    println!("🚀 Testing Basic Unified Transport System");
    println!("==========================================");
    
    // Create a transport manager with minimal configuration
    let config = TransportManagerConfig::default();
    let manager = TransportManager::new(config);
    
    // Register transport factories
    println!("📡 Registering transport factories...");
    manager.register_factory(Box::new(TcpTransportFactory)).await?;
    manager.register_factory(Box::new(UdpTransportFactory)).await?;
    
    // Start the transport manager
    println!("🔄 Starting transport manager...");
    match manager.start().await {
        Ok(()) => println!("✅ Transport manager started successfully"),
        Err(e) => {
            println!("❌ Failed to start transport manager: {}", e);
            return Ok(()); // Continue with test even if start fails
        }
    }
    
    // Get available transports
    let available_transports = manager.list_available_transports().await;
    println!("📡 Available transport types: {:?}", available_transports);
    
    // Test transport capabilities
    for transport_type in &available_transports {
        if let Some(capabilities) = manager.get_transport_capabilities(*transport_type).await {
            println!("\n🔧 {} Transport Capabilities:", transport_type);
            println!("   • Max message size: {} bytes", capabilities.max_message_size);
            println!("   • Reliable: {}", capabilities.reliable);
            println!("   • Real-time: {}", capabilities.real_time);
            println!("   • Broadcast support: {}", capabilities.broadcast);
            println!("   • Bidirectional: {}", capabilities.bidirectional);
            println!("   • Encrypted: {}", capabilities.encrypted);
            println!("   • Network spanning: {}", capabilities.network_spanning);
            println!("   • Supported urgencies: {:?}", capabilities.supported_urgencies);
        }
    }
    
    // Test transport selection with a realistic target
    let target = TransportTarget::new("test-target@example.com".to_string())
        .with_address("127.0.0.1:8080".to_string())
        .with_urgency(MessageUrgency::Interactive);
    
    println!("\n🎯 Testing transport selection for target: {}", target.identifier);
    
    match manager.select_optimal_transport(&target).await {
        Ok(selected_type) => {
            println!("✅ Selected transport: {}", selected_type);
            
            // Test connectivity estimation
            if let Ok(estimate) = manager.estimate_delivery(&target, selected_type).await {
                println!("📊 Delivery estimate:");
                println!("   • Latency: {:?}", estimate.latency);
                println!("   • Reliability: {:.2}%", estimate.reliability * 100.0);
                println!("   • Throughput: {} bytes/s", estimate.throughput_estimate);
                println!("   • Cost score: {:.2}", estimate.cost_score);
            }
        }
        Err(e) => {
            println!("❌ Transport selection failed: {}", e);
        }
    }
    
    // Test sending a message (creating a simple secure message)
    let secure_message = SecureMessage::new(
        "test-target@example.com",
        "test-system@localhost",
        b"Hello from unified transport system!".to_vec(),
        Vec::new(), // empty signature for testing
        SecurityLevel::Authenticated
    );
    
    println!("\n📤 Testing message sending...");
    match manager.send_message(&target, &secure_message).await {
        Ok(receipt) => {
            println!("✅ Message sent successfully!");
            println!("   • Message ID: {}", receipt.message_id);
            println!("   • Transport: {}", receipt.transport_used);
            println!("   • Delivery time: {:?}", receipt.delivery_time);
            println!("   • Target reached: {}", receipt.target_reached);
            println!("   • Confirmation: {:?}", receipt.confirmation);
        }
        Err(e) => {
            println!("❌ Message sending failed: {}", e);
        }
    }
    
    // Test metrics collection
    println!("\n📊 Collecting transport metrics...");
    let metrics = manager.get_metrics_summary().await;
    if metrics.is_empty() {
        println!("⚠️  No metrics available yet");
    } else {
        for (transport_name, transport_metrics) in metrics {
            println!("📈 {} metrics:", transport_name);
            println!("   • Messages sent: {}", transport_metrics.messages_sent);
            println!("   • Messages received: {}", transport_metrics.messages_received);
            println!("   • Send failures: {}", transport_metrics.send_failures);
            println!("   • Average latency: {}ms", transport_metrics.average_latency_ms);
            println!("   • Reliability score: {:.2}%", transport_metrics.reliability_score * 100.0);
            println!("   • Active connections: {}", transport_metrics.active_connections);
        }
    }
    
    // Test graceful shutdown
    println!("\n🔄 Testing graceful shutdown...");
    match manager.stop().await {
        Ok(()) => println!("✅ Transport manager stopped successfully"),
        Err(e) => println!("⚠️  Warning during shutdown: {}", e),
    }
    
    println!("\n🎉 Unified transport abstraction test complete!");
    
    Ok(())
}
