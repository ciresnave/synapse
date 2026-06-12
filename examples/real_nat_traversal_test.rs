use chrono::Utc;
use std::collections::HashMap;
use synapse::transport::abstraction::{Transport, TransportTarget};
use synapse::transport::nat_traversal::NatTraversalTransport;
use synapse::types::{DateTimeWrapper, SecureMessage, SecurityLevel, UuidWrapper};
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    println!("Testing Real NAT Traversal Implementation");
    println!("=========================================");

    // Create a real NAT traversal transport
    let mut transport = NatTraversalTransport::new(12345).await?;

    println!("✅ Created NAT traversal transport on port 12345");

    // Test capabilities
    let capabilities = transport.capabilities();
    println!("✅ Transport capabilities:");
    println!(
        "   - Max message size: {} bytes",
        capabilities.max_message_size
    );
    println!("   - Real-time: {}", capabilities.real_time);
    println!("   - Network spanning: {}", capabilities.network_spanning);
    println!("   - Features: {:?}", capabilities.features);

    // Start the transport (this will initialize real networking)
    transport.start().await?;
    println!("✅ Transport started successfully");

    // Test real STUN discovery
    println!("\n🔍 Testing real STUN discovery...");
    match transport.discover_external_address().await {
        Ok(external_addr) => {
            println!("✅ STUN discovery successful!");
            println!("   External address: {}", external_addr);
        }
        Err(e) => {
            println!("⚠️  STUN discovery failed: {}", e);
            println!("   (This is normal if no internet or STUN servers are blocked)");
        }
    }

    // Test real UPnP discovery
    println!("\n🏠 Testing real UPnP discovery...");
    match transport.setup_upnp_mapping().await {
        Ok(mapping) => {
            println!("✅ UPnP mapping successful!");
            println!("   External port: {}", mapping.external_port);
            println!("   Internal port: {}", mapping.internal_port);
            println!("   Protocol: {}", mapping.protocol);
        }
        Err(e) => {
            println!("⚠️  UPnP discovery failed: {}", e);
            println!("   (This is normal if no UPnP gateway is available)");
        }
    }

    // Generate real ICE candidates
    println!("\n🧊 Generating real ICE candidates...");
    match transport.generate_ice_candidates().await {
        Ok(candidates) => {
            println!("✅ Generated {} ICE candidates:", candidates.len());
            for (i, candidate) in candidates.iter().enumerate() {
                println!(
                    "   {}. {:?} - {} (priority: {})",
                    i + 1,
                    candidate.candidate_type,
                    candidate.address,
                    candidate.priority
                );
            }
        }
        Err(e) => {
            println!("❌ Failed to generate ICE candidates: {}", e);
        }
    }

    // Test connectivity to a target
    println!("\n📡 Testing connectivity...");
    let target = TransportTarget {
        identifier: "test_target".to_string(),
        address: Some("8.8.8.8:53".to_string()), // Google DNS
        metadata: HashMap::new(),
    };

    let connectivity_result = transport.test_connectivity(&target).await?;
    println!("✅ Connectivity test results:");
    println!("   Connected: {}", connectivity_result.connected);
    println!("   Quality: {:.1}%", connectivity_result.quality * 100.0);
    if let Some(rtt) = connectivity_result.rtt {
        println!("   RTT: {:?}", rtt);
    }

    // Test metrics
    println!("\n📊 Transport metrics:");
    let metrics = transport.metrics().await;
    println!("   Status: {:?}", transport.status().await);
    println!("   Average latency: {} ms", metrics.average_latency_ms);
    println!(
        "   Reliability score: {:.1}%",
        metrics.reliability_score * 100.0
    );

    for (key, value) in &metrics.custom_metrics {
        println!("   {}: {}", key, value);
    }

    // Test message creation and parsing
    println!("\n📨 Testing message handling...");
    let test_message = SecureMessage {
        message_id: UuidWrapper::new(Uuid::new_v4()),
        to_global_id: "test_recipient".to_string(),
        from_global_id: "test_sender".to_string(),
        encrypted_content: b"Hello, NAT traversal world!".to_vec(),
        signature: Vec::new(),
        timestamp: DateTimeWrapper::new(Utc::now()),
        security_level: SecurityLevel::Public,
        routing_path: Vec::new(),
        metadata: {
            let mut metadata = HashMap::new();
            metadata.insert("test".to_string(), "real_implementation".to_string());
            metadata
        },
    };

    // Test sending to localhost (this should work)
    let localhost_target = TransportTarget {
        identifier: "localhost_test".to_string(),
        address: Some("127.0.0.1:12346".to_string()),
        metadata: HashMap::new(),
    };

    match transport
        .send_message(&localhost_target, &test_message)
        .await
    {
        Ok(receipt) => {
            println!("✅ Message sent successfully!");
            println!("   Message ID: {}", receipt.message_id);
            println!("   Delivery time: {:?}", receipt.delivery_time);
            println!("   Target reached: {}", receipt.target_reached);
        }
        Err(e) => {
            println!("ℹ️  Message send test: {}", e);
            println!("   (This is expected without a receiver)");
        }
    }

    // Stop the transport (cleanup)
    transport.stop().await?;
    println!("\n✅ Transport stopped successfully");
    println!("\n🎉 Real NAT Traversal Implementation Test Complete!");
    println!("    This implementation includes:");
    println!("    • Real STUN protocol implementation (RFC 5389)");
    println!("    • Real UPnP discovery using SSDP");
    println!("    • Actual UDP socket operations");
    println!("    • ICE candidate generation");
    println!("    • Proper resource management (start/stop)");
    println!("    • Real network connectivity testing");

    Ok(())
}
