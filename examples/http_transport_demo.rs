// SPDX-License-Identifier: MIT OR Apache-2.0
//! HTTP Transport Demo
//!
//! This example demonstrates how the HTTP transport can be used to communicate
//! through firewalls that only allow HTTP/HTTPS traffic.
use chrono::Utc;
use std::{collections::HashMap, time::Duration};
use synapse::{
    error::Result,
    transport::{
        HttpTransportFactory, TransportManagerBuilder, TransportSelectionPolicy, TransportTarget,
        TransportType, abstraction::MessageUrgency,
    },
    types::{SecureMessage, SecurityLevel},
};
use tracing::{info, warn};
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .init();

    info!("🚀 Starting HTTP Transport Demo");
    info!("This demo shows how HTTP transport can pierce firewalls!");

    // Create transport manager with HTTP transport
    let manager = TransportManagerBuilder::new()
        .enable_transport(TransportType::Http)
        .selection_policy(TransportSelectionPolicy::FirstAvailable)
        .operation_timeout(Duration::from_secs(30))
        .transport_config(TransportType::Http, {
            let mut config = HashMap::new();
            config.insert("use_https".to_string(), "true".to_string());
            config.insert("server_port".to_string(), "8443".to_string()); // Enable server
            config.insert("server_address".to_string(), "127.0.0.1".to_string());
            config.insert("timeout_ms".to_string(), "15000".to_string());
            config.insert("max_message_size".to_string(), "5242880".to_string()); // 5MB
            config.insert("user_agent".to_string(), "Synapse-Demo/1.0".to_string());
            config
        })
        .build();

    // Register HTTP transport factory
    manager
        .register_factory(Box::new(HttpTransportFactory))
        .await?;

    // Start the transport manager
    info!("📡 Starting HTTP transport manager...");
    manager.start().await?;

    // Demo 1: Send message to public web service
    info!("\n--- Demo 1: Public Web Service Communication ---");

    let web_target = TransportTarget::new("httpbin.org".to_string())
        .with_address("https://httpbin.org/post".to_string())
        .with_urgency(MessageUrgency::Interactive);

    let web_message = create_sample_message("web-001", "Hello Web Service!");

    info!("📤 Sending message to httpbin.org...");
    match manager.send_message(&web_target, &web_message).await {
        Ok(receipt) => {
            info!("✅ Message sent successfully!");
            info!("   • Message ID: {}", receipt.message_id);
            info!("   • Target: {}", receipt.target_reached);
            info!("   • Transport: {}", receipt.transport_used);
            info!("   • Latency: {:?}", receipt.delivery_time);
            info!("   • Confirmation: {:?}", receipt.confirmation);
        }
        Err(e) => {
            warn!("❌ Failed to send message: {}", e);
        }
    }

    // Demo 2: Send message to local service
    info!("\n--- Demo 2: Local Service Communication ---");
    let local_target = TransportTarget::new("localhost".to_string())
        .with_address("https://localhost:8443".to_string())
        .with_urgency(MessageUrgency::Background);

    let local_message = create_sample_message("local-001", "Hello Local Service!");

    info!("📤 Sending message to local service...");
    match manager.send_message(&local_target, &local_message).await {
        Ok(receipt) => {
            info!("✅ Message sent to local service!");
            info!("   • Message ID: {}", receipt.message_id);
            info!("   • Latency: {:?}", receipt.delivery_time);
        }
        Err(e) => {
            info!("ℹ️  Local service not available (expected): {}", e);
        }
    }

    // Demo 3: Test connectivity to various endpoints
    info!("\n--- Demo 3: Connectivity Testing ---");
    let test_endpoints = vec![
        "https://httpbin.org",
        "https://jsonplaceholder.typicode.com",
        "https://api.github.com",
        "https://localhost:8443",
    ];

    for endpoint in test_endpoints {
        let target = TransportTarget::new(endpoint.to_string()).with_address(endpoint.to_string());

        info!("🔍 Testing connectivity to {}...", endpoint);

        // Test connectivity by sending a small test message
        let test_message = create_sample_message("connectivity-test", "ping");

        match manager.send_message(&target, &test_message).await {
            Ok(receipt) => {
                info!(
                    "✅ {} is reachable ({}ms)",
                    endpoint,
                    receipt.delivery_time.as_millis()
                );
            }
            Err(e) => {
                info!("❌ {} is unreachable: {}", endpoint, e);
            }
        }
    }

    // Demo 4: Show transport capabilities
    info!("\n--- Demo 4: Transport Capabilities ---");
    if let Some(capabilities) = manager
        .get_transport_capabilities(TransportType::Http)
        .await
    {
        info!("🔧 HTTP Transport Capabilities:");
        info!("   • Supports encryption: {}", capabilities.encrypted);
        info!(
            "   • Supports bidirectional: {}",
            capabilities.bidirectional
        );
        info!("   • Supports real-time: {}", capabilities.real_time);
        info!("   • Supports reliable delivery: {}", capabilities.reliable);
        info!("   • Supports broadcast: {}", capabilities.broadcast);
        info!("   • Network spanning: {}", capabilities.network_spanning);
        info!(
            "   • Max message size: {} bytes",
            capabilities.max_message_size
        );
        info!(
            "   • Supported urgencies: {:?}",
            capabilities.supported_urgencies
        );
    }

    // Demo 5: Performance metrics
    info!("\n--- Demo 5: Performance Metrics ---");
    let metrics = manager.get_metrics().await;
    info!("📈 HTTP Transport Metrics:");
    if let Some(http_metrics) = metrics.transport_metrics.get(&TransportType::Http) {
        info!("   • Messages sent: {}", http_metrics.messages_sent);
        info!("   • Messages received: {}", http_metrics.messages_received);
        info!("   • Bytes sent: {}", http_metrics.bytes_sent);
        info!("   • Bytes received: {}", http_metrics.bytes_received);
        info!(
            "   • Average latency: {}ms",
            http_metrics.average_latency_ms
        );
        info!(
            "   • Reliability: {:.2}%",
            http_metrics.reliability_score * 100.0
        );
        info!(
            "   • Active connections: {}",
            http_metrics.active_connections
        );
    } else {
        info!("   • No HTTP metrics available yet");
    }

    info!("📊 Overall Metrics:");
    info!("   • Total messages sent: {}", metrics.total_messages_sent);
    info!(
        "   • Total messages received: {}",
        metrics.total_messages_received
    );
    info!(
        "   • Overall reliability: {:.2}%",
        metrics.overall_reliability * 100.0
    );
    info!("   • Average latency: {:?}", metrics.average_latency);

    // Demonstrate firewall-friendly characteristics
    info!("\n--- Firewall-Friendly Characteristics ---");
    info!("🛡️  HTTP Transport Advantages:");
    info!("   • Uses standard HTTP/HTTPS ports (80/443)");
    info!("   • Recognized as normal web traffic by firewalls");
    info!("   • Works through corporate proxies");
    info!("   • Can use existing web infrastructure");
    info!("   • Compatible with load balancers and CDNs");
    info!("   • Standard protocol with wide tooling support");

    info!("\n⚠️  HTTP Transport Considerations:");
    info!("   • Higher latency than direct TCP/UDP");
    info!("   • Request/response pattern (not true streaming)");
    info!("   • May have connection limits per host");
    info!("   • Requires HTTP server for receiving messages");

    // Clean shutdown
    info!("\n--- Cleanup ---");
    manager.stop().await?;
    info!("🛑 HTTP transport demo completed!");

    Ok(())
}

/// Helper function to create a sample message
fn create_sample_message(id: &str, content: &str) -> SecureMessage {
    SecureMessage {
        message_id: synapse::blockchain::serialization::UuidWrapper(Uuid::new_v4()),
        to_global_id: "http-demo-recipient".to_string(),
        from_global_id: "http-demo-sender".to_string(),
        encrypted_content: content.as_bytes().to_vec(),
        signature: vec![0u8; 64], // Placeholder signature
        timestamp: synapse::blockchain::serialization::DateTimeWrapper(Utc::now()),
        security_level: SecurityLevel::Public,
        routing_path: Vec::new(),
        metadata: {
            let mut metadata = HashMap::new();
            metadata.insert("content_type".to_string(), "text/plain".to_string());
            metadata.insert("demo_id".to_string(), id.to_string());
            metadata
        },
    }
}
