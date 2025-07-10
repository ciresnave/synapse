//! Simple Email Server Integration Test
//! 
//! A minimal test to verify the enhanced router with email server integration

use synapse::{
    router_enhanced::EnhancedEmrpRouter,
    config::{Config, EntityConfig, RouterConfig, SecurityConfig, LoggingConfig},
    types::{EmailConfig, SmtpConfig, ImapConfig},
    error::Result,
};

fn create_test_config() -> Config {
    Config {
        entity: EntityConfig {
            local_name: "test-router".to_string(),
            entity_type: "AiModel".to_string(),
            domain: "test.local".to_string(),
            capabilities: vec!["messaging".to_string()],
            display_name: Some("Test Router".to_string()),
        },
        email: EmailConfig {
            smtp: SmtpConfig {
                host: "localhost".to_string(),
                port: 2525,
                username: "test@test.local".to_string(),
                password: "test".to_string(),
                use_tls: false,
                use_ssl: false,
            },
            imap: ImapConfig {
                host: "localhost".to_string(),
                port: 1143,
                username: "test@test.local".to_string(),
                password: "test".to_string(),
                use_ssl: false,
            },
        },
        router: RouterConfig {
            max_connections: 10,
            queue_size: 100,
            connection_timeout: 30,
            max_retries: 3,
            enable_realtime: true,
            idle_timeout: 300,
        },
        security: SecurityConfig {
            private_key_path: None,
            public_key_path: None,
            auto_generate_keys: true,
            default_security_level: "Public".to_string(),
            trusted_domains: vec![],
            require_encryption_for: vec![],
        },
        logging: LoggingConfig {
            level: "info".to_string(),
            format: "compact".to_string(),
            file: None,
            log_message_content: false,
        },
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("🚀 Testing Enhanced EMRP Router with Email Server Integration");
    
    // Initialize router
    let config = create_test_config();
    let router = EnhancedEmrpRouter::new(config, "test@test.local".to_string()).await?;
    
    // Check status
    let status = router.status().await;
    println!("✅ Router initialized successfully");
    println!("📊 Status:");
    println!("  - Multi-transport: {}", status.multi_transport_enabled);
    println!("  - Email server: {}", status.email_server_enabled);
    println!("  - Transports: {:?}", status.available_transports);
    
    // Check email server specific features
    if router.is_running_email_server() {
        println!("📧 Email server is configured to run locally");
        
        if let Some(connectivity_info) = router.email_server_connectivity() {
            println!("🌐 Connectivity: {}", connectivity_info);
        }
    } else {
        println!("🌐 Using external email providers");
    }
    
    println!("🎉 Integration test completed successfully!");
    Ok(())
}
