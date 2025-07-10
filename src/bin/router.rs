//! EMRP Router - Standalone message routing service
//!
//! This binary provides a standalone router service that can be run
//! to handle message routing for multiple entities.

use synapse::{
    router::EmrpRouter, config::Config, init_logging,
    types::{EmailConfig, SmtpConfig, ImapConfig},
};
use clap::{Arg, Command};
use std::path::PathBuf;
use tokio::signal;
use tracing::{info, error, warn};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    init_logging();

    // Parse command line arguments
    let matches = Command::new("emrp-router")
        .version("0.1.0")
        .author("EMRP Development Team")
        .about("Email-Based Message Routing Protocol Router")
        .arg(
            Arg::new("config")
                .short('c')
                .long("config")
                .value_name("FILE")
                .help("Path to configuration file")
                .num_args(1),
        )
        .arg(
            Arg::new("global-id")
                .short('i')
                .long("global-id")
                .value_name("ID")
                .help("Global ID for this router (email address)")
                .required(true)
                .num_args(1),
        )
        .arg(
            Arg::new("smtp-host")
                .long("smtp-host")
                .value_name("HOST")
                .help("SMTP server hostname")
                .num_args(1),
        )
        .arg(
            Arg::new("smtp-port")
                .long("smtp-port")
                .value_name("PORT")
                .help("SMTP server port")
                .value_parser(clap::value_parser!(u16))
                .num_args(1),
        )
        .arg(
            Arg::new("smtp-user")
                .long("smtp-user")
                .value_name("USER")
                .help("SMTP username")
                .num_args(1),
        )
        .arg(
            Arg::new("smtp-pass")
                .long("smtp-pass")
                .value_name("PASS")
                .help("SMTP password")
                .num_args(1),
        )
        .arg(
            Arg::new("imap-host")
                .long("imap-host")
                .value_name("HOST")
                .help("IMAP server hostname")
                .num_args(1),
        )
        .arg(
            Arg::new("imap-port")
                .long("imap-port")
                .value_name("PORT")
                .help("IMAP server port")
                .value_parser(clap::value_parser!(u16))
                .num_args(1),
        )
        .arg(
            Arg::new("imap-user")
                .long("imap-user")
                .value_name("USER")
                .help("IMAP username")
                .num_args(1),
        )
        .arg(
            Arg::new("imap-pass")
                .long("imap-pass")
                .value_name("PASS")
                .help("IMAP password")
                .num_args(1),
        )
        .get_matches();

    let global_id = matches.get_one::<String>("global-id").unwrap().clone();

    // Load configuration
    let config = if let Some(config_path) = matches.get_one::<String>("config") {
        load_config_from_file(config_path).await?
    } else {
        create_config_from_args(&matches)?
    };

    info!("Starting EMRP Router for entity: {}", global_id);

    // Create and start router
    let router = EmrpRouter::new(config, global_id.clone()).await?;

    // Generate keypair if we don't have one
    match router.generate_our_keypair().await {
        Ok((_private_pem, public_pem)) => {
            info!("Generated new keypair for {}", global_id);
            info!("Public key:\n{}", public_pem);
            warn!("IMPORTANT: Save the private key securely!");
        }
        Err(e) => {
            error!("Failed to generate keypair: {}", e);
            return Err(e.into());
        }
    }

    // Start the router
    router.start().await?;

    // Print status
    let status = router.status().await;
    info!("Router Status:");
    info!("  Global ID: {}", status.our_global_id);
    info!("  Known Entities: {}", status.known_entities);
    info!("  Known Keys: {}", status.known_keys);
    info!("  Email Configured: {}", status.email_configured);

    info!("EMRP Router is running. Press Ctrl+C to stop.");

    // Wait for shutdown signal
    match signal::ctrl_c().await {
        Ok(()) => {
            info!("Received shutdown signal");
        }
        Err(err) => {
            error!("Unable to listen for shutdown signal: {}", err);
        }
    }

    // Stop the router
    router.stop().await?;
    info!("EMRP Router stopped");

    Ok(())
}

/// Load configuration from file
async fn load_config_from_file(path: &str) -> Result<Config, Box<dyn std::error::Error>> {
    let config_path = PathBuf::from(path);
    
    if !config_path.exists() {
        return Err(format!("Configuration file not found: {}", path).into());
    }

    let config_content = tokio::fs::read_to_string(&config_path).await?;
    let config: Config = toml::from_str(&config_content)?;
    
    Ok(config)
}

/// Create configuration from command line arguments
fn create_config_from_args(matches: &clap::ArgMatches) -> Result<Config, Box<dyn std::error::Error>> {
    let smtp_config = SmtpConfig {
        host: matches
            .get_one::<String>("smtp-host")
            .unwrap_or(&"localhost".to_string())
            .clone(),
        port: *matches.get_one::<u16>("smtp-port").unwrap_or(&587),
        username: matches
            .get_one::<String>("smtp-user")
            .unwrap_or(&"".to_string())
            .clone(),
        password: matches
            .get_one::<String>("smtp-pass")
            .unwrap_or(&"".to_string())
            .clone(),
        use_tls: true,
        use_ssl: false,
    };

    let imap_config = ImapConfig {
        host: matches
            .get_one::<String>("imap-host")
            .unwrap_or(&"localhost".to_string())
            .clone(),
        port: *matches.get_one::<u16>("imap-port").unwrap_or(&993),
        username: matches
            .get_one::<String>("imap-user")
            .unwrap_or(&"".to_string())
            .clone(),
        password: matches
            .get_one::<String>("imap-pass")
            .unwrap_or(&"".to_string())
            .clone(),
        use_ssl: true,
    };

    let email_config = EmailConfig {
        smtp: smtp_config,
        imap: imap_config,
    };

    // Create a default config and update the email section
    let mut config = Config::default_for_entity("router", "router");
    config.email = email_config;

    Ok(config)
}
