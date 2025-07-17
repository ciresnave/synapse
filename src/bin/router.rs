//! Synapse Router - Standalone message routing service
//!
//! This binary provides a standalone router service that can be run
//! to handle message routing for multiple entities.

use synapse::{
    router::SynapseRouter, config::Config, init_logging,
    types::{EmailConfig, SmtpConfig, ImapConfig},
    config::{EntityConfig, RouterConfig, SecurityConfig, LoggingConfig},
};
use tokio::signal;
use tracing::{info, error};
use std::sync::atomic::{AtomicBool, Ordering};
use std::error::Error;
use synapse::error::SynapseError;

static RUNNING: AtomicBool = AtomicBool::new(true);

async fn handle_shutdown_signals() {
    let _ = signal::ctrl_c().await;
    RUNNING.store(false, Ordering::SeqCst);
}

fn should_exit() -> bool {
    !RUNNING.load(Ordering::SeqCst)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Initialize logging
    init_logging();

    // Handle Ctrl+C gracefully
    tokio::spawn(handle_shutdown_signals());

    let args: Vec<String> = std::env::args().collect();
    let port = args.get(1).map(|p| p.parse::<u16>().unwrap_or(7000)).unwrap_or(7000);
    let global_id = format!("router-{}", port);

    // Create a basic config
    let config = Config {
        entity: EntityConfig {
            local_name: format!("router-{}", port),
            entity_type: "router".to_string(),
            domain: "local".to_string(),
            capabilities: vec!["routing".to_string()],
            display_name: Some(format!("Router {}", port)),
        },
        email: EmailConfig {
            smtp: SmtpConfig {
                host: "smtp.example.com".to_string(),
                port: 587,
                username: "test@example.com".to_string(),
                password: "password".to_string(),
                use_tls: true,
                use_ssl: false,
            },
            imap: ImapConfig {
                host: "imap.example.com".to_string(),
                port: 993,
                username: "test@example.com".to_string(),
                password: "password".to_string(),
                use_ssl: true,
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
            default_security_level: "authenticated".to_string(),
            trusted_domains: vec![],
            require_encryption_for: vec![],
        },
        logging: LoggingConfig {
            level: "info".to_string(),
            format: "pretty".to_string(),
            file: None,
            log_message_content: false,
        },
    };

    info!("Starting EMRP router on port {}", port);
    info!("Entity ID: {}", global_id);

    // Create and start router
    let router = match SynapseRouter::new(config, global_id.clone()).await {
        Ok(r) => r,
        Err(e) => return Err(Box::new(e) as Box<dyn Error>),
    };

    let (public_key, private_key) = match router.generate_keypair().await {
        Ok(keys) => keys,
        Err(e) => {
            error!("Failed to generate keypair: {}", e);
            return Err(Box::new(e));
        }
    };

    info!("Generated keypair:");
    info!("  Public Key: {}", public_key);
    info!("  Private Key: {}", private_key);

    match router.start().await {
        Ok(_) => Ok(()),
        Err(e) => Err(Box::new(e) as Box<dyn Error>),
    }?;

    let health = router.get_health().await;
    info!("Router started successfully.");
    info!("Router Status:");
    info!("  Status: {}", health.status);
    info!("  Global ID: {}", router.get_our_global_id());
    info!("  Known Peers: {}", health.known_peers);
    info!("  Known Keys: {}", health.known_keys);
    info!("  Email Configured: {}", if health.email_available { "✓" } else { "✗" });

    info!("Starting message processing loop...");
    loop {
        if should_exit() {
            info!("Received exit signal");
            break;
        }

        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }

    info!("Stopping router...");
    router.stop().await?;
    info!("Router stopped cleanly.");

    Ok(())
        .map_err(|e: SynapseError| Box::new(e) as Box<dyn Error>)
}
