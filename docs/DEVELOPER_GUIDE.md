# 🛠️ Synapse Developer Guide

Welcome to the Synapse Neural Communication Network development! This guide will help you get started with building applications on top of Synapse's powerful communication system.

## 🚀 Getting Started

### Prerequisites
- Rust 1.70+ (2021 edition)
- PostgreSQL 12+
- Tokio async runtime familiarity
- Basic understanding of networking concepts

### Installation

Add Synapse to your `Cargo.toml`:
```toml
[dependencies]
synapse = "1.0.0"
tokio = { version = "1.0", features = ["full"] }
```

### Your First Synapse Application

```rust
use synapse::api::ParticipantAPI;
use synapse::config::SynapseConfig;
use synapse::models::{MessageContent, MessageOptions, Priority};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Create configuration
    let config = SynapseConfig::builder()
        .database_url("postgres://user:password@localhost/synapse_db")
        .http_port(8080)
        .websocket_port(8082)
        .enable_circuit_breaker(true)
        .build();
    
    // 2. Initialize the Synapse API
    let api = ParticipantAPI::new(config).await?;
    
    // 3. Start all services
    api.start().await?;
    
    // 4. Send a message
    let recipient_id = "p-123456";
    let content = MessageContent::Text("Hello, Synapse world!".to_string());
    let options = MessageOptions::default()
        .with_priority(Priority::Normal);
        
    let message_id = api.send_message(recipient_id, content, options).await?;
    println!("Message sent with ID: {}", message_id);
    
    // 5. Set up message handler
    api.on_message(|message| {
        println!("Received: {} from {}", message.content, message.sender_id);
        Ok(())
    }).await?;
        "Hello from my EMRP app!",        // Message content
        MessageType::Direct,              // Communication type
        SecurityLevel::Authenticated,     // Security level
        MessageUrgency::Interactive,      // Speed preference
    ).await?;
    
    println!("✅ Message sent successfully!");
    
    // 6. Listen for incoming messages
    let mut receiver = router.get_message_receiver().await?;
    while let Some(message) = receiver.recv().await {
        println!("📨 Received: {} from {}", 
                 message.content, 
                 message.from_entity);
    }
    
    Ok(())
}
```

## 🏗️ Core Concepts

### 1. Identity System

EMRP's identity system has three layers:

```rust
// Layer 1: Simple Names (what you use)
"Alice", "Claude", "MyBot"

// Layer 2: Global IDs (email-based)
"alice@company.com", "claude@anthropic.com"

// Layer 3: Network Addresses (auto-discovered)
"192.168.1.100:8080", "relay.company.com:587"
```

#### Handling Unknown Names

When you try to contact someone not in your registry, EMRP provides several intelligent lookup mechanisms:

```rust
// Basic approach - will attempt auto-discovery
router.send_message_smart("Unknown Person", "Hello!", MessageType::Direct, 
                         SecurityLevel::Basic, MessageUrgency::Interactive).await?;

// Enhanced approach with contextual hints
let lookup_request = ContactLookupRequest {
    name: "Dr. Alice Smith".to_string(),
    hints: vec![
        ContactHint::Organization("AI Research Lab".to_string()),
        ContactHint::Domain("university.edu".to_string()),
        ContactHint::Role("Machine Learning Researcher".to_string()),
    ],
    requester_context: RequesterContext {
        from_entity: "MyBot@company.com".to_string(),
        purpose: "Research collaboration inquiry".to_string(),
        urgency: MessageUrgency::Interactive,
    },
};

match router.resolve_contact_with_context(lookup_request).await? {
    ResolutionResult::Direct(global_id) => {
        // Found exact match, send directly
        router.send_message_smart(&global_id, "Hello Dr. Smith!", 
                                 MessageType::Direct, SecurityLevel::Authenticated,
                                 MessageUrgency::Interactive).await?;
    }
    
    ResolutionResult::ContactRequestRequired(candidates) => {
        // Found potential matches, need permission to contact
        for candidate in candidates {
            let request_id = router.send_contact_request(
                &candidate,
                "Hello, I'm interested in discussing ML research collaboration.",
                vec![Permission::Conversation(Duration::hours(24))]
            ).await?;
            println!("Contact request sent: {}", request_id);
        }
    }
    
    ResolutionResult::Suggestions(similar) => {
        // Show similar names for clarification
        println!("Did you mean one of these?");
        for suggestion in similar {
            println!("  - {}", suggestion);
        }
    }
    
    ResolutionResult::NotFound => {
        println!("Could not find anyone matching that description");
    }
}
```

#### Discovery Methods

EMRP uses multiple strategies to find unknown contacts:

1. **DNS-based Discovery**: Tries common email patterns with domain hints
2. **Peer Network Queries**: Asks known trusted contacts if they know the person
3. **Directory Lookups**: Searches LDAP, Active Directory, or custom registries
4. **Domain Inference**: Guesses email addresses based on organization hints
5. **Fuzzy Matching**: Finds similar names in your existing contacts

#### Privacy and Consent

All contact attempts to unknown entities require explicit permission:

```rust
// Configure discovery privacy settings
let discovery_config = DiscoveryConfig {
    allow_being_discovered: true,
    discovery_permissions: DiscoveryPermissions {
        discoverable_by_domain: vec!["company.com".to_string()],
        discoverable_by_entity_type: vec![EntityType::AiModel, EntityType::Human],
        require_introduction: false,
        public_profile_info: ProfileInfo {
            name: "Alice".to_string(),
            role: Some("AI Researcher".to_string()),
            organization: Some("University AI Lab".to_string()),
            bio: Some("Working on computer vision and ML".to_string()),
        },
    },
    auto_approval_rules: vec![
        AutoApprovalRule {
            condition: ApprovalCondition::FromDomain("trusted-university.edu".to_string()),
            action: ApprovalAction::AutoApprove(vec![Permission::SingleMessage]),
            priority: 1,
        }
    ],
    max_discovery_requests_per_hour: 10,
    max_pending_requests: 5,
};

router.configure_discovery(discovery_config).await?;
```

### 2. Message Types
Choose the right message type for your use case:

```rust
// Direct: One-to-one private communication
MessageType::Direct

// Broadcast: One-to-many announcements  
MessageType::Broadcast

// Conversation: Multi-party discussions
MessageType::Conversation

// Notification: System alerts and updates
MessageType::Notification
```

### 3. Security Levels
EMRP provides multiple security options:

```rust
// None: No encryption (testing only)
SecurityLevel::None

// Basic: Simple encryption
SecurityLevel::Basic

// Authenticated: Verified sender identity
SecurityLevel::Authenticated

// HighSecurity: Full end-to-end encryption
SecurityLevel::HighSecurity
```

### 4. Message Urgency
Control speed vs reliability tradeoffs:

```rust
// RealTime: <100ms, uses TCP/UDP
MessageUrgency::RealTime

// Interactive: <1s, tries fast then email
MessageUrgency::Interactive  

// Background: Reliable, uses email backbone
MessageUrgency::Background

// Discovery: For finding peers
MessageUrgency::Discovery
```

## 🔧 Advanced Features

### Custom Transport Configuration

```rust
let config = Config::builder()
    // Network settings
    .tcp_port(8080)
    .udp_port(8081)
    .enable_mdns(true)
    
    // Email settings
    .smtp_server("smtp.company.com".to_string())
    .smtp_port(587)
    .email_username("mybot@company.com".to_string())
    .email_password("app_password".to_string())
    
    // Security settings
    .require_authentication(true)
    .enable_encryption(true)
    .key_rotation_interval(Duration::from_secs(3600))
    
    // Performance tuning
    .max_retries(3)
    .connection_timeout(Duration::from_secs(10))
    .discovery_interval(Duration::from_secs(30))
    
    .build();
```

### Message Metadata and Custom Fields

```rust
use std::collections::HashMap;

let mut metadata = HashMap::new();
metadata.insert("priority".to_string(), "high".to_string());
metadata.insert("department".to_string(), "engineering".to_string());
metadata.insert("project_id".to_string(), "proj-123".to_string());

let message = SimpleMessage {
    to: "Alice".to_string(),
    from_entity: "ProjectBot".to_string(),
    content: "Code review ready for proj-123".to_string(),
    message_type: MessageType::Notification,
    metadata,
};

router.send_message_detailed(message, 
                           SecurityLevel::Authenticated,
                           MessageUrgency::Interactive).await?;
```

### Peer Discovery and Registration

```rust
// Manual registration with full details
router.register_identity(GlobalIdentity {
    local_name: "AliceBot".to_string(),
    global_id: "alice-bot@ai-lab.company.com".to_string(),
    entity_type: EntityType::AiModel,
    capabilities: vec!["real-time".to_string(), "file-transfer".to_string()],
    public_key: Some(alice_public_key),
    created_at: chrono::Utc::now(),
}).await?;

// Auto-discovery on local network
router.start_peer_discovery().await?;

// Search for peers by pattern
let peers = router.search_peers("*@ai-lab.company.com").await?;
for peer in peers {
    println!("Found peer: {} at {}", peer.local_name, peer.global_id);
}
```

### Message Filtering and Routing

```rust
// Set up message filters
router.add_message_filter(|message| {
    // Only accept messages from verified AI models
    message.from_entity_type == Some(EntityType::AiModel) &&
    message.security_level.unwrap_or(SecurityLevel::None) >= SecurityLevel::Authenticated
}).await?;

// Custom routing logic
router.set_routing_strategy(RoutingStrategy::Custom(Box::new(
    |destination, message_urgency| {
        match message_urgency {
            MessageUrgency::RealTime => TransportType::TCP,
            MessageUrgency::Interactive => {
                if is_local_network(&destination) {
                    TransportType::UDP
                } else {
                    TransportType::Email
                }
            },
            _ => TransportType::Email,
        }
    }
))).await?;
```

## 🧪 Testing Your Application

### Unit Testing with Mock Transport

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use message_routing_system::testing::*;

    #[tokio::test]
    async fn test_message_sending() {
        // Create test router with mock transport
        let config = Config::test_config();
        let router = EnhancedEmrpRouter::new_with_mock_transport(
            config, 
            "test@example.com".to_string()
        ).await.unwrap();
        
        // Register test peer
        router.register_peer("TestPeer", "peer@example.com").await.unwrap();
        
        // Send test message
        let result = router.send_message_smart(
            "TestPeer",
            "Test message",
            MessageType::Direct,
            SecurityLevel::Basic,
            MessageUrgency::Interactive,
        ).await;
        
        assert!(result.is_ok());
        
        // Verify message was sent
        let sent_messages = router.get_mock_sent_messages().await;
        assert_eq!(sent_messages.len(), 1);
        assert_eq!(sent_messages[0].content, "Test message");
    }
}
```

### Integration Testing

```rust
#[tokio::test]
async fn test_full_message_flow() {
    // Start two routers for full integration test
    let config1 = Config::test_config().with_tcp_port(8080);
    let config2 = Config::test_config().with_tcp_port(8081);
    
    let router1 = EnhancedEmrpRouter::new(config1, "alice@test.com".to_string()).await.unwrap();
    let router2 = EnhancedEmrpRouter::new(config2, "bob@test.com".to_string()).await.unwrap();
    
    // Cross-register
    router1.register_peer("Bob", "bob@test.com").await.unwrap();
    router2.register_peer("Alice", "alice@test.com").await.unwrap();
    
    // Start both routers
    router1.start().await.unwrap();
    router2.start().await.unwrap();
    
    // Send message from Alice to Bob
    router1.send_message_smart(
        "Bob",
        "Hello Bob!",
        MessageType::Direct,
        SecurityLevel::Basic,
        MessageUrgency::RealTime,
    ).await.unwrap();
    
    // Verify Bob receives the message
    let mut receiver = router2.get_message_receiver().await.unwrap();
    let message = tokio::time::timeout(
        Duration::from_secs(5),
        receiver.recv()
    ).await.unwrap().unwrap();
    
    assert_eq!(message.content, "Hello Bob!");
    assert_eq!(message.from_entity, "Alice");
}
```

## 🔍 Debugging and Monitoring

### Logging Configuration

```rust
// Enable detailed logging
env_logger::Builder::from_default_env()
    .filter_level(log::LevelFilter::Debug)
    .init();

// Or use tracing for structured logging
use tracing_subscriber;

tracing_subscriber::fmt()
    .with_max_level(tracing::Level::DEBUG)
    .with_target(false)
    .init();
```

### Router Status and Health Checks

```rust
// Get detailed router status
let status = router.get_detailed_status().await?;
println!("Router Status: {:#?}", status);

// Check transport health
for transport in router.get_active_transports().await? {
    let health = transport.health_check().await?;
    println!("Transport {}: {:?}", transport.name(), health);
}

// Monitor message statistics
let stats = router.get_message_statistics().await?;
println!("Messages sent: {}, received: {}, failed: {}", 
         stats.sent_count, stats.received_count, stats.failed_count);
```

### Performance Monitoring

```rust
// Track message latency
router.enable_latency_tracking(true).await?;

// Get transport performance metrics
let metrics = router.get_transport_metrics().await?;
for (transport_name, metric) in metrics {
    println!("{}: avg_latency={}ms, success_rate={}%", 
             transport_name, metric.avg_latency_ms, metric.success_rate);
}
```

## 🚀 Deployment Patterns

### Local Development
```rust
let config = Config::builder()
    .tcp_port(8080)
    .udp_port(8081)
    .enable_mdns(true)
    .email_mode(EmailMode::MockOnly)  // No real emails during development
    .build();
```

### Production Deployment
```rust
let config = Config::builder()
    .tcp_port(443)  // Standard HTTPS port
    .enable_tls(true)
    .smtp_server(std::env::var("SMTP_SERVER")?)
    .smtp_port(587)
    .email_username(std::env::var("EMAIL_USERNAME")?)
    .email_password(std::env::var("EMAIL_PASSWORD")?)
    .require_authentication(true)
    .enable_encryption(true)
    .build();
```

### Cloud/Container Deployment
```rust
let config = Config::builder()
    .tcp_port(std::env::var("PORT")?.parse()?)
    .external_ip(std::env::var("EXTERNAL_IP").ok())
    .enable_nat_traversal(true)
    .discovery_servers(vec![
        "stun.l.google.com:19302".to_string(),
        "stun1.l.google.com:19302".to_string(),
    ])
    .build();
```

## 📚 Next Steps

1. **Read the API Reference**: Generate with `cargo doc --open`
2. **Study the Examples**: Check out `examples/` directory
3. **Join the Community**: See `CONTRIBUTING.md` for development guidelines
4. **Performance Tuning**: See `docs/PERFORMANCE_GUIDE.md`
5. **Security Best Practices**: See `docs/SECURITY_GUIDE.md`

## 🆘 Getting Help

- **Documentation**: Full API docs at `docs/`
- **Examples**: Working examples in `examples/`
- **Issues**: Report bugs and feature requests on GitHub
- **Discussions**: Join the community discussions

Happy coding with EMRP! 🚀
