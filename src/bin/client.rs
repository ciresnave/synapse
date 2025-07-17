//! Synapse Client - Interactive command-line client for the Synapse Neural Communication Network
//!
//! This binary provides a command-line interface for sending and receiving
//! messages using the Synapse protocol.

use synapse::{
    router::SynapseRouter, config::Config, init_logging,
    types::{MessageType, SecurityLevel, SimpleMessage},
};
use clap::{Arg, Command, ArgMatches};
use std::io::Write;
// use tokio::io::AsyncBufReadExt;
use tracing::{info, error, warn};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    init_logging();

    // Parse command line arguments
    let matches = Command::new("synapse-client")
        .version("1.0.0")
        .author("Synapse Development Team")
        .about("Synapse Neural Communication Network Client")
        .subcommand(
            Command::new("send")
                .about("Send a message to another entity")
                .arg(
                    Arg::new("to")
                        .short('t')
                        .long("to")
                        .value_name("ENTITY")
                        .help("Recipient entity name")
                        .required(true)
                        .num_args(1),
                )
                .arg(
                    Arg::new("message")
                        .short('m')
                        .long("message")
                        .value_name("TEXT")
                        .help("Message to send")
                        .required(true)
                        .num_args(1),
                )
                .arg(
                    Arg::new("type")
                        .long("type")
                        .value_name("TYPE")
                        .help("Message type (direct, tool_call, tool_response, system, broadcast)")
                        .default_value("direct")
                        .num_args(1),
                )
                .arg(
                    Arg::new("security")
                        .long("security")
                        .value_name("LEVEL")
                        .help("Security level (public, private, authenticated, secure)")
                        .default_value("private")
                        .num_args(1),
                ),
        )
        .subcommand(
            Command::new("receive")
                .about("Check for and display incoming messages")
                .arg(
                    Arg::new("continuous")
                        .short('c')
                        .long("continuous")
                        .help("Continuously poll for messages")
                        .action(clap::ArgAction::SetTrue),
                ),
        )
        .subcommand(
            Command::new("interactive")
                .about("Start interactive chat mode")
                .arg(
                    Arg::new("with")
                        .short('w')
                        .long("with")
                        .value_name("ENTITY")
                        .help("Entity to chat with")
                        .required(true)
                        .num_args(1),
                ),
        )
        .subcommand(
            Command::new("status")
                .about("Show client status and configuration"),
        )
        .subcommand(
            Command::new("add-entity")
                .about("Add a new entity to the registry")
                .arg(
                    Arg::new("global-id")
                        .short('i')
                        .long("global-id")
                        .value_name("ID")
                        .help("Global ID (email address)")
                        .required(true)
                        .num_args(1),
                )
                .arg(
                    Arg::new("name")
                        .short('n')
                        .long("name")
                        .value_name("NAME")
                        .help("Local name for the entity")
                        .required(true)
                        .num_args(1),
                )
                .arg(
                    Arg::new("public-key")
                        .short('k')
                        .long("public-key")
                        .value_name("KEY")
                        .help("Public key in PEM format")
                        .required(true)
                        .num_args(1),
                ),
        )
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
                .help("Your global ID (email address)")
                .required(true)
                .num_args(1),
        )
        .get_matches();

    let global_id = matches.get_one::<String>("global-id").unwrap().clone();

    // Load or create configuration
    let config = if let Some(config_path) = matches.get_one::<String>("config") {
        Config::from_file(config_path)?
    } else {
        info!("No config file specified, using default configuration");
        warn!("Please configure email settings for full functionality");
        Config::default_for_entity(&global_id, "human")
    };

    // Create router
    let router = SynapseRouter::new(config, global_id.clone()).await?;

    match matches.subcommand() {
        Some(("send", send_matches)) => {
            handle_send_command(&router, send_matches).await?;
        }
        Some(("receive", receive_matches)) => {
            handle_receive_command(&router, receive_matches).await?;
        }
        Some(("interactive", interactive_matches)) => {
            handle_interactive_command(&router, interactive_matches).await?;
        }
        Some(("status", _)) => {
            handle_status_command(&router).await?;
        }
        Some(("add-entity", add_matches)) => {
            handle_add_entity_command(&router, add_matches).await?;
        }
        _ => {
            println!("No subcommand specified. Use --help for usage information.");
            println!("Quick start:");
            println!("  synapse-client -i your@email.com send -t Claude -m \"Hello!\"");
            println!("  synapse-client -i your@email.com receive");
            println!("  synapse-client -i your@email.com interactive -w Claude");
        }
    }

    Ok(())
}

/// Handle the send command
async fn handle_send_command(
    router: &SynapseRouter,
    matches: &ArgMatches,
) -> Result<(), Box<dyn std::error::Error>> {
    let to = matches.get_one::<String>("to").unwrap();
    let message = matches.get_one::<String>("message").unwrap();
    let message_type = parse_message_type(matches.get_one::<String>("type").unwrap())?;
    let security_level = parse_security_level(matches.get_one::<String>("security").unwrap())?;

    info!("Sending message to {} (type: {:?}, security: {:?})", to, message_type, security_level);

    let simple_message = SimpleMessage {
        to: to.clone(),
        from_entity: router.get_our_global_id().to_string(),
        content: message.clone(),
        message_type,
        metadata: std::collections::HashMap::new(),
    };

    match router.send_message(simple_message, to.clone()).await {
        Ok(_) => {
            println!("✓ Message sent successfully");
            println!("  To: {}", to);
            println!("  Content: {}", message);
            Ok(())
        }
        Err(e) => {
            println!("✗ Failed to send message");
            println!("  Error: {}", e);
            Err(Box::new(e))
        }
    }
}

/// Handle the receive command
async fn handle_receive_command(
    router: &SynapseRouter,
    matches: &ArgMatches,
) -> Result<(), Box<dyn std::error::Error>> {
    let continuous = matches.get_flag("continuous");

    if continuous {
        println!("📨 Continuously checking for messages... (Press Ctrl+C to stop)");
        
        loop {
            match router.receive_messages().await {
                Ok(messages) => {
                    for message in messages {
                        print_message(&message);
                    }
                }
                Err(e) => {
                    error!("Error receiving messages: {}", e);
                }
            }
            
            // Wait 10 seconds before checking again
            tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
        }
    } else {
        println!("📨 Checking for new messages...");
        
        match router.receive_messages().await {
            Ok(messages) => {
                if messages.is_empty() {
                    println!("No new messages");
                } else {
                    println!("Found {} new message(s):", messages.len());
                    for message in messages {
                        print_message(&message);
                    }
                }
            }
            Err(e) => {
                error!("Error receiving messages: {}", e);
                println!("✗ Failed to receive messages: {}", e);
            }
        }
    }

    Ok(())
}

/// Handle the interactive command
async fn handle_interactive_command(
    router: &SynapseRouter,
    matches: &ArgMatches,
) -> Result<(), Box<dyn std::error::Error>> {
    let with_entity = matches.get_one::<String>("with").unwrap();
    println!("Starting interactive chat with {}", with_entity);
    println!("Type a message and press Enter to send. Type /quit to exit.");

    loop {
        print!("> ");
        std::io::stdout().flush()?;

        let mut message = String::new();
        std::io::stdin().read_line(&mut message)?;
        message = message.trim().to_string();

        if message == "/quit" {
            break;
        }

        let simple_message = SimpleMessage {
            to: with_entity.clone(),
            from_entity: router.get_our_global_id().to_string(),
            content: message.clone(),
            message_type: MessageType::Direct,
            metadata: std::collections::HashMap::new(),
        };

        match router.send_message(simple_message, with_entity.clone()).await {
            Ok(_) => {
                println!("✓ Message sent");
            }
            Err(e) => {
                println!("✗ Failed to send message: {}", e);
            }
        }

        // Poll for incoming messages
        match router.receive_messages().await {
            Ok(messages) => {
                for msg in messages {
                    println!("\nFrom {}: {}", msg.from_entity, msg.content);
                }
            }
            Err(e) => {
                warn!("Failed to receive messages: {}", e);
            }
        }
    }

    println!("Interactive session ended.");
    Ok(())
}

/// Handle the status command
async fn handle_status_command(router: &SynapseRouter) -> Result<(), Box<dyn std::error::Error>> {
    let status = router.get_health().await;
    
    println!("Synapse Client Status:");
    println!("Status: {}", status.status);
    println!("Global ID: {}", router.get_our_global_id());
    println!("Known Peers: {}", status.known_peers);
    println!("Known Keys: {}", status.known_keys);
    println!("Email Available: {}", if status.email_available { "✓" } else { "✗" });

    if !status.email_available {
        println!("\nWarning: Email transport not available!");
    }

    println!("\nEncryption Status:");
    println!("Crypto Support: {}", if status.crypto_available { "✓" } else { "✗" });

    Ok(())
}

/// Handle the add-entity command
async fn handle_add_entity_command(
    router: &SynapseRouter,
    matches: &ArgMatches,
) -> Result<(), Box<dyn std::error::Error>> {
    let global_id = matches.get_one::<String>("global-id").unwrap();
    let name = matches.get_one::<String>("name").unwrap();
    let public_key = matches.get_one::<String>("public-key").unwrap();

    // Add to identity registry
    router.register_entity(global_id, name, Some(name.clone())).await?;
    
    // Add public key
    router.add_entity_key(global_id, public_key).await?;

    println!("✓ Added entity '{}' ({})", name, global_id);

    Ok(())
}

/// Parse message type from string
fn parse_message_type(type_str: &str) -> Result<MessageType, Box<dyn std::error::Error>> {
    match type_str.to_lowercase().as_str() {
        "direct" => Ok(MessageType::Direct),
        "tool_call" => Ok(MessageType::ToolCall),
        "tool_response" => Ok(MessageType::ToolResponse),
        "system" => Ok(MessageType::System),
        "broadcast" => Ok(MessageType::Broadcast),
        "stream_chunk" => Ok(MessageType::StreamChunk),
        _ => Err(format!("Unknown message type: {}", type_str).into()),
    }
}

/// Parse security level from string
fn parse_security_level(level_str: &str) -> Result<SecurityLevel, Box<dyn std::error::Error>> {
    match level_str.to_lowercase().as_str() {
        "public" => Ok(SecurityLevel::Public),
        "private" => Ok(SecurityLevel::Private),
        "authenticated" => Ok(SecurityLevel::Authenticated),
        "secure" => Ok(SecurityLevel::Secure),
        _ => Err(format!("Unknown security level: {}", level_str).into()),
    }
}

/// Print a message in a nice format
fn print_message(message: &SimpleMessage) {
    println!("📩 New Message");
    println!("  From: {}", message.from_entity);
    println!("  To: {}", message.to);
    println!("  Type: {:?}", message.message_type);
    println!("  Content: {}", message.content);
    if !message.metadata.is_empty() {
        println!("  Metadata: {:?}", message.metadata);
    }
    println!("{}", "-".repeat(40));
}
