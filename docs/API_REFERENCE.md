# 📚 Synapse API Reference Guide

This guide provides a comprehensive overview of the Synapse Neural Communication Network API, with practical examples and usage patterns. This document is intended for developers who want to integrate with the Synapse ecosystem or build applications on top of it.

## 📖 Quick Navigation

- [Core Components](#core-components)
- [Participant Registry](#participant-registry)
- [Trust System](#trust-system)
- [Blockchain](#blockchain)
- [Transport Layer](#transport-layer)
- [Storage](#storage)
- [WebAssembly Support](#webassembly-support)
- [Error Handling](#error-handling)
- [Configuration](#configuration)

## 🏗️ Core Components

### Synapse

The main entry point for the Synapse system.

```rust
use synapse::*;

// Create Synapse instance with configuration
let config = Config::default();
let synapse = Synapse::new(config).await?;

// Start all services
synapse.start().await?;

// Send message
router.send_message_smart(
    "Alice",                          // Target
    "Hello!",                         // Content
    MessageType::Direct,              // Type
    SecurityLevel::Authenticated,     // Security
    MessageUrgency::Interactive,      // Urgency
).await?;
```

**Key Methods:**

- `new(config, entity_id)` - Create new router instance
- `start()` - Start all services and transports
- `send_message_smart()` - Intelligent message sending
- `register_peer()` - Register a peer manually
- `get_message_receiver()` - Get message receiver channel

### Config

Configuration builder for customizing EMRP behavior.

```rust
let config = Config::builder()
    .tcp_port(8080)
    .udp_port(8081)
    .enable_encryption(true)
    .smtp_server("smtp.gmail.com".to_string())
    .smtp_port(587)
    .email_username("bot@gmail.com".to_string())
    .email_password("app_password".to_string())
    .build();
```

**Key Configuration Options:**

- Network: `tcp_port()`, `udp_port()`, `enable_mdns()`
- Email: `smtp_server()`, `email_username()`, `email_password()`
- Security: `enable_encryption()`, `require_authentication()`
- Performance: `max_retries()`, `connection_timeout()`

## 🚀 Router API

### Message Sending

#### Simple Message Sending

```rust
// Basic message
router.send_message_smart(
    "Alice",                      // Who to send to
    "Hello from EMRP!",          // Message content
    MessageType::Direct,         // Message type
    SecurityLevel::Basic,        // Security level
    MessageUrgency::Interactive, // Speed vs reliability
).await?;
```

#### Detailed Message Sending

```rust
use std::collections::HashMap;

let mut metadata = HashMap::new();
metadata.insert("priority".to_string(), "high".to_string());
metadata.insert("category".to_string(), "urgent".to_string());

let message = SimpleMessage {
    to: "Bob".to_string(),
    from_entity: "MyBot".to_string(),
    content: "Urgent system alert!".to_string(),
    message_type: MessageType::Notification,
    metadata,
};

router.send_message_detailed(
    message,
    SecurityLevel::HighSecurity,
    MessageUrgency::RealTime
).await?;
```

#### Broadcast Messages

```rust
// Send to multiple recipients
router.send_broadcast(
    vec!["Alice".to_string(), "Bob".to_string(), "Charlie".to_string()],
    "Team meeting in 10 minutes!".to_string(),
    SecurityLevel::Basic,
    MessageUrgency::Interactive,
).await?;
```

### Message Receiving

#### Basic Message Receiving

```rust
let mut receiver = router.get_message_receiver().await?;

while let Some(message) = receiver.recv().await {
    println!("From: {}", message.from_entity);
    println!("Content: {}", message.content);
    println!("Type: {:?}", message.message_type);
}
```

#### Filtered Message Receiving

```rust
let mut receiver = router.get_filtered_receiver(|msg| {
    msg.message_type == MessageType::Direct && 
    msg.from_entity.contains("@company.com")
}).await?;

while let Some(message) = receiver.recv().await {
    // Only receives direct messages from company domain
    handle_company_message(message).await?;
}
```

#### Message Receiving with Timeout

```rust
use tokio::time::{timeout, Duration};

let mut receiver = router.get_message_receiver().await?;

match timeout(Duration::from_secs(30), receiver.recv()).await {
    Ok(Some(message)) => {
        println!("Received: {}", message.content);
    },
    Ok(None) => {
        println!("Receiver closed");
    },
    Err(_) => {
        println!("Timeout waiting for message");
    }
}
```

### Router Status and Health

#### Basic Status

```rust
let status = router.get_basic_status().await?;
println!("Active transports: {:?}", status.active_transports);
println!("Peer count: {}", status.peer_count);
println!("Is healthy: {}", status.is_healthy);
```

#### Detailed Status

```rust
let detailed = router.get_detailed_status().await?;
println!("Router info: {:#?}", detailed);

// Check specific components
if detailed.email_server_status.is_some() {
    println!("Email server is running");
}

for (transport, metrics) in detailed.transport_metrics {
    println!("{}: {}ms avg latency", transport, metrics.avg_latency_ms);
}
```

#### Health Monitoring

```rust
// Continuous health monitoring
tokio::spawn(async move {
    loop {
        let health = router.check_health().await.unwrap();
        
        if !health.is_healthy {
            eprintln!("Router unhealthy: {:?}", health.issues);
        }
        
        tokio::time::sleep(Duration::from_secs(30)).await;
    }
});
```

## 📝 Message Types

### SimpleMessage Structure

```rust
use std::collections::HashMap;

let message = SimpleMessage {
    to: "recipient".to_string(),           // Target entity
    from_entity: "sender".to_string(),     // Sender entity
    content: "message content".to_string(), // Main content
    message_type: MessageType::Direct,     // Type of message
    metadata: HashMap::new(),              // Additional data
};
```

### Message Types

#### Direct Messages

```rust
MessageType::Direct  // One-to-one private communication
```

**Use Cases:** Private conversations, specific requests, personal communication

#### Broadcast Messages

```rust
MessageType::Broadcast  // One-to-many public announcements
```

**Use Cases:** Announcements, alerts, status updates

#### Conversation Messages

```rust
MessageType::Conversation  // Multi-party discussions
```

**Use Cases:** Group chats, team discussions, collaborative work

#### Notification Messages

```rust
MessageType::Notification  // System alerts and updates
```

**Use Cases:** System alerts, automated notifications, monitoring updates

### Security Levels

```rust
SecurityLevel::None            // No security (testing only)
SecurityLevel::Basic           // Basic encryption
SecurityLevel::Authenticated   // Verified sender identity
SecurityLevel::HighSecurity    // Full end-to-end encryption
```

### Message Urgency

```rust
MessageUrgency::RealTime      // <100ms, prefer TCP/UDP
MessageUrgency::Interactive   // <1s, try fast then email
MessageUrgency::Background    // Reliable, use email backbone
MessageUrgency::Discovery     // For peer discovery
```

## 👤 Identity Management

### Registering Peers

#### Quick Registration

```rust
// Simple peer registration
router.register_peer("Alice", "alice@company.com").await?;
```

#### Detailed Registration

```rust
router.register_identity(GlobalIdentity {
    local_name: "Alice".to_string(),
    global_id: "alice@ai-company.com".to_string(),
    entity_type: EntityType::AiModel,
    capabilities: vec!["analysis".to_string(), "reasoning".to_string()],
    public_key: Some(alice_public_key),
    created_at: chrono::Utc::now(),
}).await?;
```

### Entity Types

```rust
EntityType::Human    // Human users
EntityType::AiModel  // AI systems and language models
EntityType::Tool     // Utility services and tools
EntityType::Service  // Infrastructure services
EntityType::Router   // EMRP routing infrastructure
```

### Identity Resolution

#### Basic Name Resolution

```rust
// Resolve local name to global ID
let global_id = router.resolve_local_name("Alice").await?;
println!("Alice is: {}", global_id);  // "alice@company.com"

// Get full identity information
let identity = router.get_identity("Alice").await?;
println!("Entity type: {:?}", identity.entity_type);
println!("Capabilities: {:?}", identity.capabilities);
```

#### Enhanced Resolution for Unknown Names

```rust
use message_routing_system::discovery::*;

// Resolve with contextual hints for unknown contacts
let lookup_request = ContactLookupRequest {
    name: "Dr. Sarah Chen".to_string(),
    hints: vec![
        ContactHint::Organization("Stanford AI Lab".to_string()),
        ContactHint::Role("Computer Vision Researcher".to_string()),
        ContactHint::Domain("stanford.edu".to_string()),
    ],
    requester_context: RequesterContext {
        from_entity: "researcher@myuniversity.edu".to_string(),
        purpose: "Collaboration on vision transformer research".to_string(),
        urgency: MessageUrgency::Background,
    },
};

match router.resolve_contact_with_context(lookup_request).await? {
    ResolutionResult::Direct(global_id) => {
        // Direct match found
        router.send_message_smart(&global_id, message, MessageType::Direct,
                                 SecurityLevel::Authenticated, MessageUrgency::Interactive).await?;
    }
    
    ResolutionResult::ContactRequestRequired(candidates) => {
        // Found potential matches, need permission
        for candidate in candidates {
            let request_id = router.send_contact_request(
                &candidate,
                "Hello Dr. Chen, I'm working on similar research and would like to collaborate.",
                vec![Permission::Conversation(Duration::days(7))]
            ).await?;
            
            println!("Contact request sent: {}", request_id);
        }
    }
    
    ResolutionResult::Suggestions(similar_names) => {
        // Show similar existing contacts for clarification
        println!("Did you mean one of these existing contacts?");
        for suggestion in similar_names {
            println!("  - {} ({})", suggestion.local_name, suggestion.global_id);
        }
    }
    
    ResolutionResult::NotFound => {
        println!("Could not find anyone matching that description");
        
        // Optionally suggest manual registration
        println!("You can manually register this contact with:");
        println!("router.register_peer(\"Dr. Chen\", \"email@stanford.edu\").await?;");
    }
}
```

#### Contact Request Management

```rust
// Send contact request to unknown entity
let request_id = router.send_contact_request(
    &discovered_identity,
    "Hello! I'm interested in collaborating on AI research.",
    vec![
        Permission::SingleMessage,
        Permission::Conversation(Duration::hours(24)),
    ]
).await?;

// Handle incoming contact requests
let mut request_receiver = router.get_contact_request_receiver().await?;
while let Some(request) = request_receiver.recv().await {
    println!("Contact request from: {}", request.requester);
    println!("Purpose: {}", request.purpose);
    
    // Respond to request
    let response = ContactResponse::Approved {
        granted_permissions: vec![Permission::Conversation(Duration::hours(24))],
        preferred_contact_method: ContactMethod::Direct,
        introduction_message: Some("Happy to chat about research!".to_string()),
    };
    
    router.respond_to_contact_request(&request.id, response).await?;
}

// Check status of sent requests
let request_status = router.get_contact_request_status(&request_id).await?;
match request_status {
    RequestStatus::Pending => println!("Request still pending"),
    RequestStatus::Approved(permissions) => {
        println!("Request approved with permissions: {:?}", permissions);
        // Can now send messages
    }
    RequestStatus::Declined(reason) => {
        println!("Request declined: {}", reason.unwrap_or_default());
    }
}
```

#### Smart Discovery Configuration

```rust
// Configure how your identity can be discovered
let discovery_config = DiscoveryConfig {
    allow_being_discovered: true,
    enabled_methods: vec![
        DiscoveryMethod::DnsDiscovery { 
            patterns: vec!["${name}@mycompany.com".to_string()] 
        },
        DiscoveryMethod::PeerNetworkQuery { 
            ask_known_peers: true, 
            propagation_limit: 2 
        },
    ],
    discovery_permissions: DiscoveryPermissions {
        discoverable_by_domain: vec!["university.edu".to_string(), "company.com".to_string()],
        discoverable_by_entity_type: vec![EntityType::AiModel, EntityType::Human],
        require_introduction: false,
        public_profile_info: ProfileInfo {
            name: "Research Assistant".to_string(),
            role: Some("AI Research".to_string()),
            organization: Some("My University".to_string()),
            bio: Some("Working on machine learning and NLP".to_string()),
        },
    },
    auto_approval_rules: vec![
        AutoApprovalRule {
            condition: ApprovalCondition::FromDomain("trusted-domain.com".to_string()),
            action: ApprovalAction::AutoApprove(vec![Permission::SingleMessage]),
            priority: 1,
        },
        AutoApprovalRule {
            condition: ApprovalCondition::EmergencyKeyword("urgent".to_string()),
            action: ApprovalAction::AutoApprove(vec![Permission::SingleMessage]),
            priority: 10,
        },
    ],
    max_discovery_requests_per_hour: 20,
    max_pending_requests: 10,
};

router.configure_discovery(discovery_config).await?;
```

### Discovery Operations

```rust
// List all registered peers
let peers = router.list_registered_peers().await?;
for peer in peers {
    println!("{} -> {}", peer.local_name, peer.global_id);
}

// Search for peers
let ai_agents = router.search_peers_by_type(EntityType::AiModel).await?;

// Auto-discover peers on network
router.start_peer_discovery().await?;
```

## 🚛 Transport System

### Transport Types

EMRP automatically selects the best transport based on conditions:

- **TCP**: Direct, reliable connections
- **UDP**: Fast, lightweight messaging
- **mDNS**: Local network discovery
- **Email**: Universal fallback transport
- **NAT Traversal**: Firewall penetration

### Transport Metrics

```rust
let metrics = router.get_transport_metrics().await?;

for (transport_name, metric) in metrics {
    println!("Transport: {}", transport_name);
    println!("  Average latency: {}ms", metric.avg_latency_ms);
    println!("  Success rate: {}%", metric.success_rate);
    println!("  Total messages: {}", metric.message_count);
    println!("  Bandwidth: {} KB/s", metric.bandwidth_kbps);
}
```

### Manual Transport Selection

```rust
// Force specific transport
router.send_via_transport(
    TransportType::TCP,
    "Alice",
    "Direct TCP message",
    SecurityLevel::Basic
).await?;

// Test transport connectivity
let tcp_result = router.test_transport_connectivity(TransportType::TCP, "Alice").await;
match tcp_result {
    Ok(_) => println!("TCP connection successful"),
    Err(e) => println!("TCP failed: {}", e),
}
```

### Error Recovery Mechanisms

Synapse implements several error recovery patterns to ensure reliability even in challenging network conditions:

#### Circuit Breaker Pattern

```rust
use synapse::transport::error_recovery::CircuitBreaker;
use std::time::Duration;

// Create a circuit breaker for a specific service
let breaker = CircuitBreaker::new(
    "email-transport",      // Name of the protected service
    3,                      // Failure threshold before opening circuit
    Duration::from_secs(30) // Reset timeout before allowing retry
);

// Before making a potentially failing call
if breaker.allow_request() {
    match send_operation().await {
        Ok(result) => {
            // Record success to reset failure counter
            breaker.record_success();
            println!("Operation succeeded: {:?}", result);
        },
        Err(e) => {
            // Record failure to potentially trip the circuit
            breaker.record_failure();
            eprintln!("Operation failed: {}", e);
        }
    }
} else {
    // Circuit is open, don't attempt the operation
    eprintln!("Circuit breaker is open, skipping request");
}
```

#### Retry Policy

```rust
use synapse::transport::error_recovery::RetryPolicy;

// Create a retry policy with exponential backoff
let policy = RetryPolicy::new(3, 100)   // 3 attempts starting at 100ms
    .with_max_backoff(30)               // Maximum 30 seconds
    .with_jitter_factor(0.1);           // Add 10% randomization

// Execute an operation with retry
let result = policy.execute(|| {
    // Return a BoxFuture with your operation
    Box::pin(async {
        // Your operation that might fail transiently
        api_call().await
    })
}).await;

match result {
    Ok(data) => println!("Operation succeeded after retries: {:?}", data),
    Err(e) => eprintln!("Operation failed after all retries: {}", e),
}
```

#### Connection Health Monitoring

```rust
use synapse::transport::error_recovery::ConnectionHealthMonitor;

// Create a health monitor
let monitor = ConnectionHealthMonitor::new();

// Record successful and failed operations
match operation().await {
    Ok(_) => monitor.record_success(),
    Err(_) => monitor.record_failure()
}

// Check health status
if monitor.is_healthy() {
    // Connection is healthy, proceed normally
} else {
    // Connection is unhealthy, take remedial action
    let status = monitor.get_status();
    println!("Connection is unhealthy with {} consecutive failures", 
             status.consecutive_failures);
}
```

## ⚙️ Configuration

### Builder Pattern

```rust
let config = Config::builder()
    // Network configuration
    .tcp_port(8080)
    .udp_port(8081)
    .enable_mdns(true)
    
    // Email configuration
    .smtp_server("smtp.gmail.com".to_string())
    .smtp_port(587)
    .email_username("bot@gmail.com".to_string())
    .email_password("app_password".to_string())
    
    // Security configuration
    .enable_encryption(true)
    .require_authentication(true)
    .key_size(4096)
    
    // Performance configuration
    .max_retries(3)
    .connection_timeout(Duration::from_secs(10))
    .worker_threads(4)
    
    .build();
```

### Configuration Validation

```rust
// Validate configuration before use
let validation_result = config.validate();
match validation_result {
    Ok(_) => println!("Configuration is valid"),
    Err(errors) => {
        for error in errors {
            eprintln!("Config error: {}", error);
        }
    }
}
```

### Environment-Based Configuration

```rust
// Load from environment variables
let config = Config::from_env()?;

// Or combine with builder
let config = Config::from_env()?
    .builder()
    .tcp_port(9090)  // Override specific settings
    .build();
```

## ❌ Error Handling

### Error Types

```rust
use synapse::api::errors::{ApiError, ApiErrorType, Result};

// Structured API errors
match api.send_message(recipient_id, content, options).await {
    Ok(message_id) => println!("Message sent successfully: {}", message_id),
    Err(e) => {
        if let Some(api_error) = e.downcast_ref::<ApiError>() {
            match api_error.error_type {
                ApiErrorType::NotFound => eprintln!("Recipient not found: {}", api_error.message),
                ApiErrorType::NetworkError => eprintln!("Network issue: {}", api_error.message),
                ApiErrorType::Unauthorized => eprintln!("Authentication required: {}", api_error.message),
                ApiErrorType::RateLimited => eprintln!("Rate limited: {}", api_error.message),
                ApiErrorType::ServiceUnavailable => eprintln!("Service unavailable: {}", api_error.message),
                _ => eprintln!("API error: {}", api_error),
            }
            
            // Log telemetry for errors
            if api_error.should_report() {
                telemetry::log_error(
                    &api_error.error_id,
                    &api_error.error_type.to_string(),
                    &api_error.message,
                    api_error.context.clone()
                ).await;
            }
        } else {
            eprintln!("Unexpected error: {}", e);
        }
    }
}
```

### ApiError Structure

The `ApiError` is a standardized error structure used throughout the Synapse API:

```rust
pub struct ApiError {
    pub error_id: String,         // Unique identifier for this error instance
    pub error_type: ApiErrorType, // Categorized error type
    pub message: String,          // Human-readable error message
    pub status_code: u16,         // HTTP status code (for API responses)
    pub context: HashMap<String, String>, // Additional error context
    pub source: Option<Box<dyn std::error::Error + Send + Sync>>, // Original error
}

pub enum ApiErrorType {
    NotFound,              // Resource not found
    Unauthorized,          // Authentication required
    Forbidden,             // Not authorized for resource
    InvalidInput,          // Invalid parameters
    ServiceUnavailable,    // Service temporarily unavailable
    NetworkError,          // Transport/network error
    RateLimited,           // Too many requests
    InternalError,         // Unexpected internal error
    ValidationError,       // Failed validation
    TransportFailure,      // Communication transport failure
    CircuitBroken,         // Circuit breaker is open
    Timeout,               // Operation timed out
}
```

### Graceful Error Handling

```rust
// Retry with fallback
async fn send_with_retry(router: &EnhancedEmrpRouter, target: &str, content: &str) -> Result<()> {
    let mut attempts = 0;
    let max_attempts = 3;
    
    while attempts < max_attempts {
        match router.send_message_smart(
            target, content, MessageType::Direct, 
            SecurityLevel::Basic, MessageUrgency::Interactive
        ).await {
            Ok(_) => return Ok(()),
            Err(e) => {
                attempts += 1;
                eprintln!("Attempt {} failed: {}", attempts, e);
                
                if attempts == max_attempts {
                    return Err(e);
                }
                
                // Wait before retrying
                tokio::time::sleep(Duration::from_millis(1000 * attempts as u64)).await;
            }
        }
    }
    
    unreachable!()
}
```

## 🔄 Async Patterns

### Concurrent Message Sending

```rust
use futures::future::join_all;

// Send multiple messages concurrently
let futures = vec![
    router.send_message_smart("Alice", "Hello Alice", MessageType::Direct, 
                             SecurityLevel::Basic, MessageUrgency::Interactive),
    router.send_message_smart("Bob", "Hello Bob", MessageType::Direct, 
                             SecurityLevel::Basic, MessageUrgency::Interactive),
    router.send_message_smart("Charlie", "Hello Charlie", MessageType::Direct, 
                             SecurityLevel::Basic, MessageUrgency::Interactive),
];

let results = join_all(futures).await;
for (i, result) in results.into_iter().enumerate() {
    match result {
        Ok(_) => println!("Message {} sent successfully", i),
        Err(e) => eprintln!("Message {} failed: {}", i, e),
    }
}
```

### Background Message Processing

```rust
// Spawn background task for message processing
let router_clone = router.clone();
let _task = tokio::spawn(async move {
    let mut receiver = router_clone.get_message_receiver().await.unwrap();
    
    while let Some(message) = receiver.recv().await {
        // Process message in background
        tokio::spawn(async move {
            if message.message_type == MessageType::Notification {
                handle_notification(message).await;
            } else if message.from_entity.contains("@ai-company.com") {
                handle_ai_message(message).await;
            } else {
                handle_regular_message(message).await;
            }
        });
    }
});
```

### Timed Operations

```rust
use tokio::time::{timeout, Duration};

// Operation with timeout
let result = timeout(
    Duration::from_secs(30),
    router.send_message_smart("SlowPeer", "Hello", MessageType::Direct, 
                             SecurityLevel::Basic, MessageUrgency::Interactive)
).await;

match result {
    Ok(Ok(_)) => println!("Message sent within timeout"),
    Ok(Err(e)) => eprintln!("Message failed: {}", e),
    Err(_) => eprintln!("Operation timed out"),
}
```

## 🔍 Debugging and Monitoring

### Enable Logging

```rust
// Initialize logging
env_logger::init();

// Or with custom level
env_logger::Builder::from_default_env()
    .filter_level(log::LevelFilter::Debug)
    .init();
```

### Performance Monitoring

```rust
// Get detailed performance metrics
let stats = router.get_performance_statistics().await?;
println!("Messages per second: {}", stats.message_rate);
println!("Average latency: {}ms", stats.avg_latency_ms);
println!("Memory usage: {}MB", stats.memory_usage_mb);

// Monitor specific operations
let start = std::time::Instant::now();
router.send_message_smart("Alice", "Test", MessageType::Direct, 
                         SecurityLevel::Basic, MessageUrgency::Interactive).await?;
let duration = start.elapsed();
println!("Message send took: {:?}", duration);
```

## 🧪 Testing Utilities

### Test Configuration

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_message_sending() {
        let config = Config::test_config().build();
        let router = EnhancedEmrpRouter::new(config, "test@example.com".to_string()).await.unwrap();
        
        router.register_peer("TestPeer", "peer@example.com").await.unwrap();
        
        let result = router.send_message_smart(
            "TestPeer", "Test message", MessageType::Direct,
            SecurityLevel::Basic, MessageUrgency::Interactive
        ).await;
        
        assert!(result.is_ok());
    }
}
```

---

## 📋 Quick Reference

### Essential Imports

```rust
use message_routing_system::*;
use std::collections::HashMap;
use tokio::time::{timeout, Duration, sleep};
```

### Basic Setup Pattern

```rust
let config = Config::builder()
    .tcp_port(8080)
    .enable_encryption(true)
    .build();

let router = EnhancedEmrpRouter::new(config, "mybot@example.com".to_string()).await?;
router.start().await?;
```

### Send and Receive Pattern

```rust
// Send
router.send_message_smart("Alice", "Hello!", MessageType::Direct, 
                         SecurityLevel::Basic, MessageUrgency::Interactive).await?;

// Receive
let mut receiver = router.get_message_receiver().await?;
while let Some(msg) = receiver.recv().await {
    println!("Received: {}", msg.content);
}
```

For complete API documentation, run `cargo doc --open` to view the generated documentation with all available methods, types, and examples!
