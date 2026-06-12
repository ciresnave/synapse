//! Enhanced Synapse Router with Email Server Integration Demo
//! 
//! This example demonstrates:
//! - Automatic connectivity detection
//! - Email server startup when externally accessible
//! - Fallback to external providers when not accessible
//! - Multi-transport integration
//! - Smart message routing

use synapse::{
    EnhancedSynapseRouter,
    config::{Config, EntityConfig, RouterConfig, SecurityConfig, LoggingConfig},
    types::{MessageType, SecurityLevel, EmailConfig, SmtpConfig, ImapConfig},
    transport::abstraction::MessageUrgency,
    error::Result,
};
use tracing::{info, warn, error};
use std::time::Duration;
use tokio::time::sleep;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt::init();
    
    info!("🚀 Starting Enhanced Synapse Router with Email Server Integration Demo");
    
    // Create configuration
    let config = create_demo_config();
    let our_entity_id = "enhanced-demo@synapse.local".to_string();
    
    // Create enhanced router with email server integration
    info!("🔧 Initializing Enhanced Synapse Router...");
    let router = match EnhancedSynapseRouter::new(config, our_entity_id.clone()).await {
        Ok(router) => {
            info!("✅ Enhanced Synapse Router initialized successfully");
            router
        }
        Err(e) => {
            error!("❌ Failed to initialize Enhanced Synapse Router: {}", e);
            return Err(e);
        }
    };
    
    // Check status before starting
    let status = router.status().await;
    info!("📊 Router Status:");
    info!("  🆔 Our ID: {}", status.synapse_status.our_global_id);
    info!("  🚀 Multi-transport: {}", status.multi_transport_enabled);
    info!("  📧 Email server: {}", status.email_server_enabled);
    info!("  🔌 Available transports: {:?}", status.available_transports);
    
    // Show email server connectivity info
    if let Some(connectivity_info) = router.email_server_connectivity() {
        info!("🌐 Email server connectivity: {}", connectivity_info);
    }
    
    // Start all services
    info!("🎬 Starting all router services...");
    if let Err(e) = router.start().await {
        error!("❌ Failed to start router services: {}", e);
        return Err(e);
    }
    
    // Give services time to start
    sleep(Duration::from_secs(2)).await;
    
    // Test different message urgency levels
    let test_targets = vec![
        "alice@example.com",
        "bob@synapse.local",
        "claude@anthropic.com",
    ];
    
    for target in test_targets {
        info!("🎯 Testing connection to: {}", target);
        
        // Test connection capabilities
        let capabilities = router.test_connection(target).await;
        info!("  📡 Capabilities for {}:", target);
        info!("    📧 Email: {}", capabilities.email);
        info!("    🔗 Direct TCP: {}", capabilities.direct_tcp);
        info!("    📡 Direct UDP: {}", capabilities.direct_udp);
        info!("    🏠 mDNS Local: {}", capabilities.mdns_local);
        info!("    🌐 NAT Traversal: {}", capabilities.nat_traversal);
        info!("    ⏱️  Estimated latency: {}ms", capabilities.estimated_latency_ms);
        
        // Test benchmark
        let benchmarks = router.benchmark_transport(target).await;
        info!("  📈 Benchmarks for {}:", target);
        info!("    📧 Email: {}ms", benchmarks.email_latency_ms);
        if let Some(tcp) = benchmarks.tcp_latency_ms {
            info!("    🔗 TCP: {}ms", tcp);
        }
        if let Some(udp) = benchmarks.udp_latency_ms {
            info!("    📡 UDP: {}ms", udp);
        }
        if let Some(mdns) = benchmarks.mdns_latency_ms {
            info!("    🏠 mDNS: {}ms", mdns);
        }
        if let Some(nat) = benchmarks.nat_traversal_latency_ms {
            info!("    🌐 NAT: {}ms", nat);
        }
        
        // Send test messages with different urgency levels
        let urgency_tests = vec![
            (MessageUrgency::Background, "📝 Background task update"),
            (MessageUrgency::Batch, "� Batch processing message"),
            (MessageUrgency::Interactive, "⚡ Interactive response needed"),
            (MessageUrgency::RealTime, "🚨 Real-time alert!"),
        ];
        
        for (urgency, content) in urgency_tests {
            info!("  📤 Sending {:?} message to {}", urgency, target);
            
            match router.send_message_smart(
                target,
                content,
                MessageType::Direct,
                SecurityLevel::Authenticated,
                urgency,
            ).await {
                Ok(message_id) => {
                    info!("    ✅ Message sent successfully: {}", message_id);
                }
                Err(e) => {
                    warn!("    ⚠️  Failed to send message: {}", e);
                }
            }
        }
        
        println!(); // Add spacing between targets
    }
    
    // Email server specific tests
    if router.is_running_email_server() {
        info!("🏃 Email server is running locally!");
        
        if let Some(email_server) = router.email_server() {
            info!("📧 Email server features:");
            info!("  🏠 Can run local server: {}", email_server.should_use_local_server());
            info!("  🔄 Can relay for clients: {}", email_server.can_relay_for_clients());
            
            // Add test user
            let test_user = synapse::email_server::UserAccount {
                username: "testuser".to_string(),
                email: "testuser@synapse.local".to_string(),
                password_hash: "test_hash".to_string(),
                active: true,
                permissions: synapse::email_server::UserPermissions {
                    can_send: true,
                    can_receive: true,
                    can_relay: false,
                    is_admin: false,
                },
            };
            
            if let Err(e) = email_server.add_user(test_user) {
                warn!("⚠️  Failed to add test user: {}", e);
            } else {
                info!("👤 Test user added successfully");
            }
            
            // Add local domain
            if let Err(e) = email_server.add_local_domain("synapse.local") {
                warn!("⚠️  Failed to add local domain: {}", e);
            } else {
                info!("🏠 Local domain 'synapse.local' added");
            }
        }
    } else {
        info!("🌐 Using external email providers");
    }
    
    // Final status check
    let final_status = router.status().await;
    info!("🏁 Final Router Status:");
    info!("  🆔 Entity ID: {}", final_status.synapse_status.our_global_id);
    info!("  👥 Known peers: {}", final_status.synapse_status.known_peers);
    info!("  🔑 Known keys: {}", final_status.synapse_status.known_keys);
    info!("  📧 Email available: {}", final_status.synapse_status.email_available);
    info!("  🚀 Multi-transport: {}", final_status.multi_transport_enabled);
    info!("  🏃 Email server: {}", final_status.email_server_enabled);
    info!("  🔌 Transport count: {}", final_status.available_transports.len());
    
    info!("🎉 Enhanced Synapse Router Demo completed successfully!");
    
    // Keep running for a bit to show server activity
    info!("⏳ Keeping services running for 10 seconds to show activity...");
    sleep(Duration::from_secs(10)).await;
    
    info!("👋 Demo finished!");
    Ok(())
}

fn create_demo_config() -> Config {
    Config {
        entity: EntityConfig {
            local_name: "enhanced-demo".to_string(),
            entity_type: "AiModel".to_string(),
            domain: "synapse.local".to_string(),
            capabilities: vec!["messaging".to_string(), "email-server".to_string()],
            display_name: Some("Enhanced Demo Router".to_string()),
        },
        email: EmailConfig {
            smtp: SmtpConfig {
                host: "localhost".to_string(),
                port: 2525,
                username: "demo@synapse.local".to_string(),
                password: "demo_password".to_string(),
                use_tls: false,
                use_ssl: false,
            },
            imap: ImapConfig {
                host: "localhost".to_string(),
                port: 1143,
                username: "demo@synapse.local".to_string(),
                password: "demo_password".to_string(),
                use_ssl: false,
            },
        },
        router: RouterConfig {
            max_connections: 100,
            queue_size: 1000,
            connection_timeout: 30,
            max_retries: 3,
            enable_realtime: true,
            idle_timeout: 300,
        },
        security: SecurityConfig {
            private_key_path: None,
            public_key_path: None,
            auto_generate_keys: true,
            default_security_level: "Authenticated".to_string(),
            trusted_domains: vec!["synapse.local".to_string()],
            require_encryption_for: vec!["AiModel".to_string()],
        },
        logging: LoggingConfig {
            level: "info".to_string(),
            format: "pretty".to_string(),
            file: None,
            log_message_content: false,
        },
    }
}
