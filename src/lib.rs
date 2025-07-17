//! # Synapse: Neural Communi/// This means you can send messages using simple, human-readable names, and Synapseation Network
//! 
//! **A revolutionary neural communication network for AI entities, distributed systems, and modern applications
//! with federated identity management, dual trust systems, and privacy-respecting discovery.**
//!
//! ## 🌟 What Makes Synapse Special?
//!
//! Synapse transforms how entities communicate by providing:
//!
//! - **🧠 Neural Identity**: Contextual participant discovery with natural language
//! - **🌍 Federated Network**: Cross-organizational communication and trust
//! - **⚡ Intelligent Routing**: Multi-speed communication with smart transport selection
//! - **🔒 Privacy First**: Advanced privacy controls with stealth and unlisted modes
//! - **🤖 AI-Native**: Designed for AI-to-AI and human-to-AI interaction
//! - **🏛️ Dual Trust**: Entity-to-entity and blockchain-verified network trust
//! - **⛓️ Blockchain Trust**: Staking, verification, and decay mechanisms
//!
//! ## 🎯 The Neural Identity System
//!
//! One of Synapse's most powerful features is its **contextual identity system**:
//!
//! ```text
//! Simple Name → Global ID → Network Discovery → Smart Transport Selection
//! 
//! "Alice" → alice@ai-lab.example.com → 192.168.1.100:8080 → TCP (real-time)
//! "Claude" → claude@anthropic.com → [external] → Email (reliable)
//! "LocalBot" → bot@localhost → 127.0.0.1:9090 → UDP (fast local)
//! ```
//!
//! This means you can send messages using simple, human-readable names, and Synapse
//! automatically figures out:
//! - Where the recipient is located (email address, IP, domain)
//! - How to reach them (direct connection, relay, email)
//! - What transport to use (TCP for speed, email for reliability)
//! - What security to apply (encryption level, authentication)
//!
//! ## 🏗️ Architecture Overview
//!
//! Synapse operates on multiple intelligent layers:
//!
//! ### Layer 1: Message Layer
//! - Simple message types: Direct, Broadcast, Conversation, Notification
//! - Automatic serialization and security wrapping
//! - Metadata handling and routing information
//!
//! ### Layer 2: Identity Resolution
//! - **Local Names**: `"Alice"`, `"Bob"`, `"MyAI"` (human-friendly)
//! - **Global IDs**: `"alice@company.com"` (globally unique)
//! - **Network Addresses**: IP addresses, ports, service endpoints
//! - **Capability Discovery**: What transports and features each peer supports
//!
//! ### Layer 3: Multi-Transport Intelligence
//! - **Real-time Transports**: TCP, UDP, WebSocket for <100ms messaging
//! - **Local Discovery**: mDNS, LAN scanning, Bluetooth for peer finding
//! - **Email Backbone**: SMTP/IMAP for reliable, federated communication
//! - **NAT Traversal**: STUN/TURN/UPnP for firewall penetration
//!
//! ### Layer 4: Email Server Integration
//! - **Full Server Mode**: Run your own SMTP/IMAP when externally accessible
//! - **Relay Mode**: Outgoing only when behind firewall/NAT
//! - **External Provider**: Fall back to Gmail, Outlook, etc. when restricted
//!
//! ## 🚀 Quick Start Example
//!
//! ```rust
//! use message_routing_system::*;
//! 
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!     // Initialize with your identity
//!     let config = Config::default();
//!     let router = EnhancedSynapseRouter::new(config, "MyBot@example.com".to_string()).await?;
//!     
//!     // Register some contacts (or use auto-discovery)
//!     router.register_peer("Alice", "alice@ai-lab.example.com").await?;
//!     
//!     // Start all services
//!     router.start().await?;
//!     
//!     // Send messages using simple names!
//!     router.send_message_smart(
//!         "Alice",                          // Just the name
//!         "Hello from Synapse!",           // Your message  
//!         MessageType::Direct,             // Type of communication
//!         SecurityLevel::Authenticated,    // Security level
//!         MessageUrgency::Interactive,     // Speed vs reliability preference
//!     ).await?;
//!     
//!     Ok(())
//! }
//! ```
//!
//! ## 🎛️ Advanced Features
//!
//! ### Automatic Transport Selection
//! 
//! Synapse intelligently chooses the best transport based on:
//! - **Message urgency**: Real-time vs. reliable delivery
//! - **Network conditions**: Latency, bandwidth, connectivity  
//! - **Security requirements**: Encryption levels, authentication
//! - **Peer capabilities**: What transports the recipient supports
//!
//! ```rust
//! // Real-time collaboration (prefers TCP/UDP)
//! router.send_message_smart(
//!     "AI-Partner", 
//!     "Quick question about the algorithm...",
//!     MessageType::Conversation,
//!     SecurityLevel::Public,
//!     MessageUrgency::RealTime,  // <100ms preferred
//! ).await?;
//!
//! // Reliable file sharing (uses email for guaranteed delivery)
//! router.send_file_with_urgency(
//!     "ResearchTeam",
//!     "important_results.pdf", 
//!     MessageUrgency::Background,  // Reliability over speed
//! ).await?;
//! ```
//!
//! ### Email Server Capabilities
//!
//! When your system is externally accessible, EMRP can run a full email server:
//!
//! ```rust
//! let router = EnhancedSynapseRouter::new(config, "bot@mydomain.com".to_string()).await?;
//! 
//! if router.is_running_email_server() {
//!     // You can receive emails directly at bot@mydomain.com
//!     // Other EMRP systems can send messages to your domain
//!     // Supports both EMRP messages and standard emails
//!     println!("🏃 Running full email infrastructure");
//! } else {
//!     // Falls back to external providers (Gmail, etc.)
//!     println!("🌐 Using external email services");
//! }
//! ```
//!
//! ### Security by Design
//!
//! All messages are automatically secured:
//! - **🔐 PGP Encryption**: Messages encrypted with recipient's public key
//! - **✍️ Digital Signatures**: Verify sender authenticity  
//! - **🛡️ TLS Transport**: Encrypted connections for real-time transports
//! - **🔑 Automatic Key Management**: Keys generated and distributed automatically
//!
//! ## 📚 Module Overview
//!
//! - [`router_enhanced`]: Main interface - start here for most use cases
//! - [`identity`]: Name resolution and identity management  
//! - [`transport`]: Multi-transport layer and intelligent routing
//! - [`email_server`]: SMTP/IMAP server implementation
//! - [`types`]: Core message types and data structures
//! - [`crypto`]: Encryption, signatures, and key management
//! - [`config`]: Configuration and setup
//!
//! ## 🎯 Use Cases
//!
//! ### AI & Machine Learning
//! - Multi-agent AI systems coordinating in real-time
//! - AI-human collaboration with natural addressing
//! - Federated learning with secure model sharing
//! - Research collaboration between AI entities worldwide
//!
//! ### Enterprise & Distributed Systems  
//! - Microservice communication with email-based service discovery
//! - Cross-organization messaging leveraging existing email infrastructure
//! - Reliable async processing with email-based job queuing
//! - Legacy system integration through email gateways
//!
//! ### IoT & Edge Computing
//! - Device-to-cloud communication using email when internet is limited
//! - Peer-to-peer IoT networks with automatic discovery
//! - Edge AI coordination across distributed deployments
//! - Resilient communication in unstable network conditions
//!
//! ## 🔧 Getting Started
//!
//! 1. **Add to Cargo.toml**: `message_routing_system = "0.1.0"`
//! 2. **See examples**: `cargo run --example email_integration_test`
//! 3. **Read the docs**: Full API documentation and guides
//! 4. **Join the community**: Contribute and get support
//!
//! The combination of email's universal reach with modern real-time transports
//! creates a communication system that's both globally federated and 
//! performance-optimized - perfect for the AI-driven future.

// Core modules
pub mod config;
pub mod error;
pub mod types;

// Platform-specific modules - not available in WASM
#[cfg(not(target_arch = "wasm32"))]
pub mod crypto;
#[cfg(not(target_arch = "wasm32"))]
pub mod router;
#[cfg(not(target_arch = "wasm32"))]
pub mod router_enhanced;
#[cfg(not(target_arch = "wasm32"))]
pub mod identity;
#[cfg(not(target_arch = "wasm32"))]
pub mod email;
#[cfg(not(target_arch = "wasm32"))]
pub mod streaming;
#[cfg(not(target_arch = "wasm32"))]
pub mod connectivity;
#[cfg(not(target_arch = "wasm32"))]
pub mod email_server;
#[cfg(not(target_arch = "wasm32"))]
pub mod circuit_breaker;
#[cfg(not(target_arch = "wasm32"))]
pub mod monitoring;

pub mod transport;

// Re-export commonly used types
pub use crypto::CryptoManager;
pub use email::EmailTransport;

// Re-export transport types needed for tests
pub use transport::{Transport, TransportSelector, HybridConnection, TransportMetrics, NatMethod};
pub use transport::providers::{TransportProvider, ProductionTransportProvider, TestTransportProvider, MockTransport};
pub use config::Config;

// Synapse Neural Communication Network
#[cfg(not(target_arch = "wasm32"))]
pub mod synapse;

// WebAssembly support
#[cfg(target_arch = "wasm32")]
pub mod wasm;

// Re-export key types for convenience
pub use error::SynapseError;
pub use types::*;

// Re-export transport types for tests and external usage
pub use transport::{
    TransportRoute,
};
pub use transport::abstraction::MessageUrgency;

// Re-export router types for tests and external usage
pub use router::SynapseRouter;
pub use router_enhanced::EnhancedSynapseRouter;
pub use transport::router::MultiTransportRouter;

// Re-export Synapse key types (only on non-WASM platforms)
#[cfg(not(target_arch = "wasm32"))]
pub use synapse::{SynapseNode, SynapseConfig};
#[cfg(not(target_arch = "wasm32"))]
pub use synapse::models::{ParticipantProfile, EntityType, DiscoverabilityLevel};

// Re-export synapse submodules
pub use synapse::api;
pub use synapse::blockchain;
pub use synapse::models;
pub use synapse::services;
pub use synapse::storage;

// Auth integration module (conditional based on auth feature)
#[cfg(feature = "auth")]
pub mod auth_integration;

#[cfg(not(target_arch = "wasm32"))]
use tracing_subscriber;

/// Initialize the Synapse system with logging (not available on WASM)
#[cfg(not(target_arch = "wasm32"))]
pub fn init_logging() {
    tracing_subscriber::fmt::init();
}

/// Current protocol version
pub const PROTOCOL_VERSION: &str = "1.0.0";

/// Standard email headers for Synapse
pub mod headers {
    pub const VERSION: &str = "X-Synapse-Version";
    pub const MESSAGE_TYPE: &str = "X-Synapse-Message-Type";
    pub const FROM_ENTITY: &str = "X-Synapse-From-Entity";
    pub const TO_ENTITY: &str = "X-Synapse-To-Entity";
    pub const ENTITY_TYPE: &str = "X-Synapse-Entity-Type";
    pub const CAPABILITIES: &str = "X-Synapse-Capabilities";
    pub const ENCRYPTED: &str = "X-Synapse-Encrypted";
    pub const SIGNED: &str = "X-Synapse-Signed";
    pub const REQUEST_ID: &str = "X-Synapse-Request-ID";
    pub const TIMESTAMP: &str = "X-Synapse-Timestamp";
}
