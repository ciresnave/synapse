//! Synapse Router Binary
//!
//! A standalone router that can relay messages between entities in the Synapse network.
//! This router supports multiple transports and can act as a bridge between different
//! network segments or protocols.

use anyhow::Result;
use chrono::Utc;
use clap::{Parser, Subcommand};
use std::time::Duration;
use synapse::{
    Config, SynapseRouter,
    types::{MessageType, SimpleMessage},
};
use tokio::signal;
use tracing::{error, info, warn};

#[derive(Parser)]
#[command(name = "synapse-router")]
#[command(about = "Synapse Network Router - Routes messages between entities")]
#[command(version = "1.1.0")]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Configuration file path
    #[arg(short, long, default_value = "config/router.toml")]
    config: String,

    /// Global ID for this router
    #[arg(short, long, default_value = "router@localhost")]
    global_id: String,

    /// Enable verbose logging
    #[arg(short, long)]
    verbose: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the router daemon
    Start {
        /// Port to listen on
        #[arg(short, long, default_value_t = 8080)]
        port: u16,

        /// Enable email server
        #[arg(short, long)]
        email_server: bool,
    },
    /// Check router status
    Status,
    /// Test router connectivity
    Test {
        /// Target entity to test
        target: String,
    },
    /// Send a test message
    Send {
        /// Destination entity
        to: String,
        /// Message content
        message: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize logging
    let log_level = if cli.verbose {
        tracing::Level::DEBUG
    } else {
        tracing::Level::INFO
    };

    tracing_subscriber::fmt().with_max_level(log_level).init();

    info!("🚀 Starting Synapse Router v1.1.0");
    info!("Global ID: {}", cli.global_id);

    // Load configuration
    let config = match load_config(&cli.config).await {
        Ok(cfg) => cfg,
        Err(e) => {
            error!("Failed to load config from {}: {}", cli.config, e);
            Config::for_testing() // Fall back to test config
        }
    };

    // Initialize router
    let router = match SynapseRouter::new(config, cli.global_id.clone()).await {
        Ok(r) => r,
        Err(e) => {
            error!("Failed to initialize router: {}", e);
            return Err(e.into());
        }
    };

    match cli.command {
        Commands::Start { port, email_server } => start_router(router, port, email_server).await,
        Commands::Status => check_status(router).await,
        Commands::Test { target } => test_connectivity(router, &target).await,
        Commands::Send { to, message } => send_test_message(router, &to, &message).await,
    }
}

async fn load_config(config_path: &str) -> Result<Config> {
    // Try to load from file, fall back to default if not found
    match std::fs::read_to_string(config_path) {
        Ok(content) => {
            info!("Loading configuration from {}", config_path);
            let config: Config = toml::from_str(&content)?;
            Ok(config)
        }
        Err(_) => {
            warn!(
                "Config file {} not found, using default configuration",
                config_path
            );
            Ok(Config::default())
        }
    }
}

async fn start_router(router: SynapseRouter, port: u16, email_server: bool) -> Result<()> {
    info!(
        "🔧 Starting router on port {} (email_server: {})",
        port, email_server
    );

    // Start the router
    if let Err(e) = router.start().await {
        error!("Failed to start router: {}", e);
        return Err(e.into());
    }

    info!("✅ Router started successfully");
    info!("📡 Listening for connections on port {}", port);

    if email_server {
        info!("📧 Email server capabilities enabled");
    }

    // Set up message processing loop
    let router_clone = router.clone();
    tokio::spawn(async move {
        loop {
            match router_clone.receive_messages().await {
                Ok(messages) => {
                    for message in messages {
                        info!(
                            "📨 Received message from {}: {}",
                            message.from_entity, message.content
                        );

                        // Process and potentially route the message
                        if let Err(e) = process_message(&router_clone, message).await {
                            error!("Failed to process message: {}", e);
                        }
                    }
                }
                Err(e) => {
                    error!("Error receiving messages: {}", e);
                }
            }

            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    });

    // Wait for shutdown signal
    info!("🛡️ Router running. Press Ctrl+C to shutdown gracefully.");

    match signal::ctrl_c().await {
        Ok(()) => {
            info!("📡 Shutdown signal received");
        }
        Err(err) => {
            error!("Unable to listen for shutdown signal: {}", err);
        }
    }

    // Graceful shutdown
    info!("🔄 Shutting down router...");
    if let Err(e) = router.stop().await {
        error!("Error during shutdown: {}", e);
    } else {
        info!("✅ Router shutdown complete");
    }

    Ok(())
}

async fn process_message(_router: &SynapseRouter, message: SimpleMessage) -> Result<()> {
    info!(
        "🔄 Processing message: {} -> {}",
        message.from_entity, message.to
    );

    // For now, just log the message
    // In a real router, this would implement routing logic
    info!("📝 Message content: {}", message.content);

    Ok(())
}

async fn check_status(router: SynapseRouter) -> Result<()> {
    info!("🔍 Checking router status...");

    let health = router.get_health().await;

    info!("📊 Router Status Report:");
    info!("   Status: {}", health.status);
    info!("   Global ID: {}", health.our_global_id);
    info!("   Crypto Available: {}", health.crypto_available);
    info!("   Email Available: {}", health.email_available);
    info!("   Known Peers: {}", health.known_peers);
    info!("   Known Keys: {}", health.known_keys);

    if health.status == "healthy" {
        info!("✅ Router is healthy and operational");
    } else {
        warn!("⚠️ Router status: {}", health.status);
    }

    Ok(())
}

async fn test_connectivity(router: SynapseRouter, target: &str) -> Result<()> {
    info!("🧪 Testing connectivity to {}", target);

    let test_message = SimpleMessage {
        to: target.to_string(),
        from_entity: router.get_our_global_id().to_string(),
        content: format!("Test message from router at {}", Utc::now()),
        message_type: MessageType::Direct,
        metadata: std::collections::HashMap::new(),
    };

    match router.send_message(test_message, target.to_string()).await {
        Ok(()) => {
            info!("✅ Test message sent successfully to {}", target);
        }
        Err(e) => {
            error!("❌ Failed to send test message to {}: {}", target, e);
            return Err(e.into());
        }
    }

    Ok(())
}

async fn send_test_message(router: SynapseRouter, to: &str, message: &str) -> Result<()> {
    info!("📤 Sending message to {}: {}", to, message);

    let simple_message = SimpleMessage {
        to: to.to_string(),
        from_entity: router.get_our_global_id().to_string(),
        content: message.to_string(),
        message_type: MessageType::Direct,
        metadata: std::collections::HashMap::new(),
    };

    match router.send_message(simple_message, to.to_string()).await {
        Ok(()) => {
            info!("✅ Message sent successfully");
        }
        Err(e) => {
            error!("❌ Failed to send message: {}", e);
            return Err(e.into());
        }
    }

    Ok(())
}
