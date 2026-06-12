//! Synapse Client Binary
//!
//! A command-line client for interacting with the Synapse network.
//! Supports sending messages, discovering peers, and managing identity.

use anyhow::Result;
use chrono::Utc;
use clap::{Parser, Subcommand};
use std::io::{self, Write};
use synapse::{
    Config, SynapseRouter,
    types::{MessageType, SimpleMessage},
};
use tokio::time::Duration;
use tracing::{error, info, warn};

#[derive(Parser)]
#[command(name = "synapse-client")]
#[command(about = "Synapse Network Client - Send and receive messages")]
#[command(version = "1.1.0")]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Configuration file path
    #[arg(short, long, default_value = "config/client.toml")]
    config: String,

    /// Global ID for this client
    #[arg(short, long, default_value = "client@localhost")]
    global_id: String,

    /// Enable verbose logging
    #[arg(short, long)]
    verbose: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Send a message to another entity
    Send {
        /// Destination entity
        to: String,
        /// Message content
        message: String,
        /// Message type (direct, broadcast, system)
        #[arg(short, long, default_value = "direct")]
        msg_type: String,
    },
    /// Start interactive chat session
    Chat {
        /// Entity to chat with
        with: String,
    },
    /// Listen for incoming messages
    Listen {
        /// Duration to listen in seconds
        #[arg(short, long, default_value_t = 60)]
        duration: u64,
    },
    /// Manage identity and contacts
    Identity {
        #[command(subcommand)]
        action: IdentityAction,
    },
    /// Test connectivity
    Test {
        /// Target entity
        target: String,
    },
    /// Show client status
    Status,
}

#[derive(Subcommand)]
enum IdentityAction {
    /// Show our identity information
    Show,
    /// Add a contact
    Add {
        /// Contact name
        name: String,
        /// Contact global ID
        global_id: String,
    },
    /// List all contacts
    List,
    /// Generate a new keypair
    GenerateKeys,
    /// Import a public key
    ImportKey {
        /// Entity global ID
        entity: String,
        /// Path to public key file
        key_file: String,
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

    info!("💬 Starting Synapse Client v1.1.0");
    info!("Global ID: {}", cli.global_id);

    // Load configuration
    let config = match load_config(&cli.config).await {
        Ok(cfg) => cfg,
        Err(e) => {
            warn!("Failed to load config from {}: {}", cli.config, e);
            Config::for_testing() // Fall back to test config
        }
    };

    // Initialize client (using SynapseRouter as the communication layer)
    let client = match SynapseRouter::new(config, cli.global_id.clone()).await {
        Ok(r) => r,
        Err(e) => {
            error!("Failed to initialize client: {}", e);
            return Err(e.into());
        }
    };

    // Start the client
    if let Err(e) = client.start().await {
        error!("Failed to start client: {}", e);
        return Err(e.into());
    }

    match cli.command {
        Commands::Send {
            to,
            message,
            msg_type,
        } => send_message(client, &to, &message, &msg_type).await,
        Commands::Chat { with } => start_chat_session(client, &with).await,
        Commands::Listen { duration } => listen_for_messages(client, duration).await,
        Commands::Identity { action } => handle_identity_action(client, action).await,
        Commands::Test { target } => test_connectivity(client, &target).await,
        Commands::Status => show_status(client).await,
    }
}

async fn listen_for_messages(client: SynapseRouter, duration: u64) -> Result<()> {
    info!("👂 Listening for messages for {} seconds...", duration);
    let start_time = std::time::Instant::now();
    let listen_duration = std::time::Duration::from_secs(duration);

    while start_time.elapsed() < listen_duration {
        match client.receive_messages().await {
            Ok(messages) => {
                for message in messages {
                    info!(
                        "📨 Received from {}: {}",
                        message.from_entity, message.content
                    );
                }
            }
            Err(e) => {
                error!("Error receiving messages: {}", e);
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }

    info!("⏰ Listening period ended");
    Ok(())
}
async fn load_config(config_path: &str) -> Result<Config> {
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

async fn send_message(
    client: SynapseRouter,
    to: &str,
    message: &str,
    msg_type: &str,
) -> Result<()> {
    info!("📤 Sending {} message to {}: {}", msg_type, to, message);

    let message_type = match msg_type.to_lowercase().as_str() {
        "direct" => MessageType::Direct,
        "broadcast" => MessageType::Broadcast,
        "system" => MessageType::System,
        "tool_call" => MessageType::ToolCall,
        "tool_response" => MessageType::ToolResponse,
        "stream_chunk" => MessageType::StreamChunk,
        _ => {
            warn!("Unknown message type '{}', using Direct", msg_type);
            MessageType::Direct
        }
    };

    let simple_msg = SimpleMessage {
        to: to.to_string(),
        from_entity: client.get_our_global_id().to_string(),
        content: message.to_string(),
        message_type,
        metadata: std::collections::HashMap::new(),
    };

    match client.send_message(simple_msg, to.to_string()).await {
        Ok(_) => {
            info!("✅ Message sent successfully to {}", to);
            Ok(())
        }
        Err(e) => {
            error!("❌ Failed to send message to {}: {}", to, e);
            Err(e.into())
        }
    }
}

async fn start_chat_session(client: SynapseRouter, with: &str) -> Result<()> {
    info!("💬 Starting chat session with {}", with);
    info!("Type 'quit' to exit the chat session");

    // Start message listener in background
    let client_clone = client.clone();
    let with_clone = with.to_string();
    tokio::spawn(async move {
        loop {
            match client_clone.receive_messages().await {
                Ok(messages) => {
                    for message in messages {
                        if message.from_entity == with_clone {
                            println!("{}: {}", message.from_entity, message.content);
                        }
                    }
                }
                Err(e) => {
                    error!("Error receiving messages: {}", e);
                }
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    });

    // Interactive input loop
    loop {
        print!("> ");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let input = input.trim();

        if input.is_empty() {
            continue;
        }

        if input.to_lowercase() == "quit" {
            info!("👋 Ending chat session");
            break;
        }

        let chat_message = SimpleMessage {
            to: with.to_string(),
            from_entity: client.get_our_global_id().to_string(),
            content: input.to_string(),
            message_type: MessageType::Direct,
            metadata: std::collections::HashMap::new(),
        };
        match client.send_message(chat_message, with.to_string()).await {
            Ok(_) => {
                info!("✅ Message sent to {}", with);
            }
            Err(e) => {
                error!("❌ Failed to send message to {}: {}", with, e);
            }
        }
    }
    Ok(())
}

async fn handle_identity_action(client: SynapseRouter, action: IdentityAction) -> Result<()> {
    match action {
        IdentityAction::Show => {
            info!("🆔 Identity Information:");
            info!("   Global ID: {}", client.get_our_global_id());

            let health = client.get_health().await;
            info!("   Known Keys: {}", health.known_keys);
            info!("   Known Peers: {}", health.known_peers);
        }
        IdentityAction::Add { name, global_id } => {
            info!("➕ Adding contact: {} ({})", name, global_id);
            if let Err(e) = client.register_entity(&global_id, &name, None).await {
                error!("Failed to add contact: {}", e);
                return Err(e.into());
            }
            info!("✅ Contact added successfully");
        }
        IdentityAction::List => {
            info!("📋 Contact List:");
            let health = client.get_health().await;
            info!("   Total contacts: {}", health.known_peers);
            // Note: IdentityRegistry doesn't expose list method, so we show count only
        }
        IdentityAction::GenerateKeys => {
            info!("🔑 Generating new keypair...");
            match client.generate_keypair().await {
                Ok((public_key, private_key)) => {
                    info!("✅ Keypair generated successfully");
                    info!("Public key (share this):");
                    println!("{public_key}");
                    info!("Private key (keep this secret):");
                    println!("{private_key}");
                }
                Err(e) => {
                    error!("Failed to generate keypair: {}", e);
                    return Err(e.into());
                }
            }
        }
        IdentityAction::ImportKey { entity, key_file } => {
            info!("📥 Importing public key for {} from {}", entity, key_file);
            match std::fs::read_to_string(&key_file) {
                Ok(public_key) => {
                    if let Err(e) = client.register_peer_key(&entity, &public_key).await {
                        error!("Failed to import key: {}", e);
                        return Err(e.into());
                    }
                    info!("✅ Public key imported successfully");
                }
                Err(e) => {
                    error!("Failed to read key file {}: {}", key_file, e);
                    return Err(e.into());
                }
            }
        }
    }

    Ok(())
}

async fn test_connectivity(client: SynapseRouter, target: &str) -> Result<()> {
    info!("🧪 Testing connectivity to {}", target);

    let test_message = SimpleMessage {
        to: target.to_string(),
        from_entity: client.get_our_global_id().to_string(),
        content: format!(
            "Connectivity test from {} at {}",
            client.get_our_global_id(),
            Utc::now()
        ),
        message_type: MessageType::Direct,
        metadata: std::collections::HashMap::new(),
    };
    match client.send_message(test_message, target.to_string()).await {
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

async fn show_status(client: SynapseRouter) -> Result<()> {
    info!("📊 Client Status:");

    let health = client.get_health().await;

    info!("   Global ID: {}", health.our_global_id);
    info!("   Status: {}", health.status);
    info!("   Crypto Available: {}", health.crypto_available);
    info!("   Email Available: {}", health.email_available);
    info!("   Known Peers: {}", health.known_peers);
    info!("   Known Keys: {}", health.known_keys);

    if health.status == "healthy" {
        info!("✅ Client is ready for communication");
    } else {
        warn!("⚠️ Client status: {}", health.status);
    }

    Ok(())
}
