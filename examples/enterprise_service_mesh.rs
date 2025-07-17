//! Enterprise Service Mesh Example
//! 
//! This example demonstrates how to create an enterprise-grade service mesh
//! using the synapse library for distributed service communication.

use synapse::{Config, SynapseRouter, SimpleMessage, MessageType};
use anyhow::Result;
use tracing::{info, warn, debug};
use std::collections::HashMap;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    info!("🏢 Starting Enterprise Service Mesh Example");

    // Define enterprise services
    let services = vec![
        ("auth-service", "service"),
        ("user-service", "service"),
        ("order-service", "service"),
        ("payment-service", "service"),
        ("notification-service", "service"),
        ("api-gateway", "gateway"),
        ("load-balancer", "infrastructure"),
        ("monitoring", "tool"),
    ];

    let mut service_routers = Vec::new();

    // Initialize all services
    info!("🚀 Initializing enterprise services...");
    for (service_name, service_type) in &services {
        debug!("Initializing service: {}", service_name);
        
        let config = Config::default_for_entity(service_name.to_string(), service_type.to_string());
        
        match SynapseRouter::new(config, service_name.to_string()).await {
            Ok(router) => {
                info!("✅ Service '{}' ({}) initialized", service_name, service_type);
                service_routers.push((service_name, service_type, router));
            }
            Err(e) => {
                warn!("❌ Failed to initialize service '{}': {}", service_name, e);
            }
        }
    }

    info!("🌐 Service mesh established with {} services", service_routers.len());

    // Simulate enterprise workflow
    info!("📋 Simulating enterprise workflow...");
    
    let workflow_steps = vec![
        ("api-gateway", "auth-service", "authenticate_user", "User authentication request"),
        ("auth-service", "user-service", "get_user_profile", "Fetch user profile data"),
        ("api-gateway", "order-service", "create_order", "New order creation request"),
        ("order-service", "payment-service", "process_payment", "Payment processing request"),
        ("payment-service", "notification-service", "send_confirmation", "Payment confirmation"),
        ("monitoring", "load-balancer", "health_check", "Service health monitoring"),
    ];

    for (from_service, to_service, operation, description) in workflow_steps {
        let message = SimpleMessage {
            to: to_service.to_string(),
            from_entity: from_service.to_string(),
            content: format!("{}:{}", operation, description),
            message_type: MessageType::Direct,
            metadata: create_enterprise_metadata(operation),
        };

        info!("🔄 {} → {}: {}", from_service, to_service, description);
        debug!("Operation: {} | Message: {}", operation, message.content);

        // Find the router for the sending service
        if let Some((_, _, router)) = service_routers.iter()
            .find(|(name, _, _)| **name == from_service) {
            
            match router.convert_to_secure_message(&message).await {
                Ok(secure_msg) => {
                    debug!("✅ Enterprise message secured: {}", secure_msg.message_id);
                }
                Err(e) => {
                    debug!("⚠️  Enterprise message prepared (secure conversion failed: {})", e);
                }
            }
        }

        // Add delay to simulate processing time
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
    }

    // Show service mesh status
    info!("📊 Service Mesh Status Report:");
    for (name, service_type, _router) in &service_routers {
        info!("  ✓ {} ({}) - Active", name, service_type);
    }

    info!("🎉 Enterprise Service Mesh example completed successfully!");
    
    Ok(())
}

/// Create metadata for enterprise operations
fn create_enterprise_metadata(operation: &str) -> HashMap<String, String> {
    let mut metadata = HashMap::new();
    
    metadata.insert("operation".to_string(), operation.to_string());
    metadata.insert("timestamp".to_string(), 
                    chrono::Utc::now().to_rfc3339());
    metadata.insert("service_mesh".to_string(), "enterprise".to_string());
    metadata.insert("priority".to_string(), 
                    if operation.contains("payment") { "high" } else { "normal" }.to_string());
    
    // Add tracing information
    metadata.insert("trace_id".to_string(), 
                    format!("trace-{}", uuid::Uuid::new_v4()));
    metadata.insert("span_id".to_string(), 
                    format!("span-{}", uuid::Uuid::new_v4()));
    
    metadata
}
