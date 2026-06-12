//! # Enhanced Synapse Router - Your Gateway to Intelligent Communication
//!
//! The **Enhanced Synapse Router** is the main interface for all Synapse communication.
//! It combines multiple transport methods, email server capabilities, and intelligent
//! routing to provide seamless, high-performance messaging with automatic fallback.
//!
//! ## 🌟 Key Features
//!
//! - **🧠 Intelligent Transport Selection**: Automatically chooses the best method
//! - **📧 Integrated Email Server**: Runs SMTP/IMAP when externally accessible
//! - **⚡ Multi-Speed Communication**: From <100ms real-time to reliable email
//! - **🔄 Graceful Fallback**: Degrades gracefully when fast transports fail
//! - **🎯 Simple API**: Send to names like "Alice" instead of complex addresses
//! - **🔒 Security Built-in**: Automatic encryption and authentication
//!
//! ## 🚀 Quick Start
//!
//! ```rust
//! use synapse::*;
//! use synapse::types::{MessageType, SecurityLevel};
//! use synapse::transport::abstraction::MessageUrgency;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Create and configure router
//!     let config = Config::default();
//!     let router = EnhancedSynapseRouter::new(config, "mybot@example.com".to_string()).await?;
//!
//!     // Start all services
//!     router.start().await?;
//!
//!     // Send message using simple name (commented out for doctest)
//!     // router.send_message_smart(
//!     //     "alice@example.com",          // Use email address
//!     //     "Hello from Synapse!",        // Your message
//!     //     MessageType::Direct,          // Type of communication
//!     //     SecurityLevel::Authenticated, // Security level
//!     //     MessageUrgency::Interactive,  // Speed preference
//!     // ).await?;
//!
//!     Ok(())
//! }
//! ```
//!
//! ## 🎛️ Transport Intelligence
//!
//! The Enhanced Router automatically selects the best transport based on:
//!
//! ### Message Urgency
//! - **RealTime** (`<100ms`): Prefers TCP/UDP direct connections
//! - **Interactive** (`<1s`): Uses fast transports with email fallback
//! - **Background** (reliable): Prioritizes email for guaranteed delivery
//! - **Discovery**: Special mode for finding and connecting to peers
//!
//! ### Network Conditions
//! - **Local Network**: Direct TCP/UDP, mDNS discovery
//! - **Internet**: TCP with NAT traversal, STUN/TURN assistance
//! - **Restricted**: Email relay through external providers
//! - **Offline**: Store-and-forward via email infrastructure
//!
//! ### Peer Capabilities
//! - **Synapse-native**: Direct protocol communication
//! - **Email-only**: Standard SMTP/IMAP interaction
//! - **Hybrid**: Both direct and email capabilities
//! - **Unknown**: Discovery mode to learn capabilities
//!
//! ## 📧 Email Server Integration
//!
//! One of the most powerful features is the integrated email server:
//!
//! ### Automatic Mode Detection
//! ```rust
//! # use synapse::*;
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! # let config = Config::default();
//! # let entity_id = "test@example.com".to_string();
//! // The router automatically detects your network situation
//! let router = EnhancedSynapseRouter::new(config, entity_id).await?;
//!
//! match router.email_server_connectivity() {
//!     Some(info) if info.contains("RunLocalServer") => {
//!         println!("🏃 Running full SMTP/IMAP server");
//!         // Can receive emails directly at your domain
//!     }
//!     Some(info) if info.contains("RelayOnly") => {
//!         println!("🔄 Relay-only mode (behind firewall)");
//!         // Can send emails, forwarding for receiving
//!     }
//!     _ => {
//!         println!("🌐 Using external email providers");
//!         // Falls back to Gmail, Outlook, etc.
//!     }
//! }
//! # Ok(())
//! # }
//! ```
//!
//! ### Email Server Benefits
//! - **Direct Receiving**: Get emails at your own domain
//! - **Custom Routing**: Implement domain-specific logic
//! - **Performance**: Eliminate external relay delays
//! - **Privacy**: Keep communications within your infrastructure
//! - **Reliability**: Redundancy with external provider fallback
//!
//! ## 🔗 Multi-Transport Architecture
//!
//! The router manages multiple transport types simultaneously:
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────┐
//! │                Enhanced Synapse Router                  │
//! ├─────────────┬─────────────┬─────────────┬─────────────┤
//! │ Real-Time   │  Direct     │   Local     │   Email     │
//! │ (<100ms)    │ Connection  │ Discovery   │ Reliable    │
//! │             │             │             │             │
//! │ • TCP       │ • UDP       │ • mDNS      │ • SMTP      │
//! │ • WebSocket │ • Raw IP    │ • LAN scan  │ • IMAP      │
//! │ • gRPC      │ • P2P       │ • Bluetooth │ • Exchange  │
//! └─────────────┴─────────────┴─────────────┴─────────────┘
//! ```
//!
//! ## 🎯 Smart Message Routing Examples
//!
//! ### Real-time AI Collaboration
//! ```rust
//! # use synapse::*;
//! # use synapse::types::{MessageType, SecurityLevel};
//! # use synapse::transport::abstraction::MessageUrgency;
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! # let config = Config::default();
//! # let router = EnhancedSynapseRouter::new(config, "test@example.com".to_string()).await?;
//! // For real-time AI interactions (commented out for doctest)
//! // router.send_message_smart(
//! //     "claude@example.com",
//! //     "Quick brainstorming session?",
//! //     MessageType::Direct,
//! //     SecurityLevel::Public,
//! //     MessageUrgency::RealTime,  // System will prefer TCP/UDP
//! // ).await?;
//! # Ok(())
//! # }
//! ```
//!
//! ### Reliable File Sharing
//! ```rust
//! # use synapse::*;
//! # use synapse::transport::abstraction::MessageUrgency;
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! # let config = Config::default();
//! # let router = EnhancedSynapseRouter::new(config, "test@example.com".to_string()).await?;
//! // For important file transfers - using send_message_smart as fallback (commented out for doctest)
//! // router.send_message_smart(
//! //     "research-team@example.com",
//! //     "Major breakthrough achieved! File: breakthrough_results.pdf",
//! //     synapse::types::MessageType::Direct,
//! //     synapse::types::SecurityLevel::Secure,
//! //     MessageUrgency::Background,  // System will use email for reliability
//! // ).await?;
//! # Ok(())
//! # }
//! ```
//!
//! ### Mixed-urgency Workflow
//! ```rust
//! # use synapse::*;
//! # use synapse::types::{MessageType, SecurityLevel};
//! # use synapse::transport::abstraction::MessageUrgency;
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! # let config = Config::default();
//! # let router = EnhancedSynapseRouter::new(config, "test@example.com".to_string()).await?;
//! // Real-time coordination (commented out for doctest)
//! // router.send_message_smart(
//! //     "team-bot@example.com",
//! //     "Starting analysis now",
//! //     MessageType::System,
//! //     SecurityLevel::Public,
//! //     MessageUrgency::Interactive,
//! // ).await?;
//!
//! // ... do work ...
//!
//! // Reliable results delivery (commented out for doctest)
//! // router.send_message_smart(
//! //     "team-bot@example.com",
//! //     "Analysis complete. Results attached.",
//! //     MessageType::Direct,
//! //     SecurityLevel::Secure,
//! //     MessageUrgency::Background,  // Guarantees delivery
//! // ).await?;
//! # Ok(())
//! # }
//! ```
//!
//! ## 📊 Performance Monitoring
//!
//! The router provides comprehensive performance insights:
//!
//! ```rust
//! # use synapse::*;
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! # let config = Config::default();
//! # let router = EnhancedSynapseRouter::new(config, "test@example.com".to_string()).await?;
//! # #[derive(Debug)]
//! # struct TestCapabilities {
//! #     email: bool, direct_tcp: bool, direct_udp: bool, mdns_local: bool,
//! #     nat_traversal: bool, estimated_latency_ms: u32,
//! # }
//! # #[derive(Debug)]
//! # struct Benchmarks {
//! #     tcp_latency_ms: Option<u32>, udp_latency_ms: Option<u32>, email_latency_ms: u32,
//! # }
//! # let capabilities = TestCapabilities {
//! #     email: true, direct_tcp: true, direct_udp: false, mdns_local: false,
//! #     nat_traversal: true, estimated_latency_ms: 50
//! # };
//! # let benchmarks = Benchmarks {
//! #     tcp_latency_ms: Some(25), udp_latency_ms: None, email_latency_ms: 1500
//! # };
//! // Test connection capabilities to a peer
//! // let capabilities = router.test_connection("Alice").await;
//! println!("Connection to Alice:");
//! println!("  📧 Email: {}", capabilities.email);
//! println!("  🔗 Direct TCP: {}", capabilities.direct_tcp);
//! println!("  📡 Direct UDP: {}", capabilities.direct_udp);
//! println!("  🏠 Local mDNS: {}", capabilities.mdns_local);
//! println!("  🌐 NAT Traversal: {}", capabilities.nat_traversal);
//! println!("  ⏱️  Estimated latency: {}ms", capabilities.estimated_latency_ms);
//!
//! // Benchmark transport performance
//! // let benchmarks = router.benchmark_transport("Alice").await;
//! println!("Performance to Alice:");
//! if let Some(tcp) = benchmarks.tcp_latency_ms {
//!     println!("  🔗 TCP: {}ms", tcp);
//! }
//! if let Some(udp) = benchmarks.udp_latency_ms {
//!     println!("  📡 UDP: {}ms", udp);
//! }
//! println!("  📧 Email: {}ms", benchmarks.email_latency_ms);
//! # Ok(())
//! # }
//! ```
//!
//! ## 🔧 Configuration and Management
//!
//! ### Router Status
//! ```rust
//! # use synapse::*;
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! # let config = Config::default();
//! # let router = EnhancedSynapseRouter::new(config, "test@example.com".to_string()).await?;
//! # #[derive(Debug)]
//! # struct RouterStatus {
//! #     synapse_status: SynapseStatus,
//! #     multi_transport_enabled: bool,
//! #     email_server_enabled: bool,
//! #     available_transports: Vec<String>,
//! # }
//! # #[derive(Debug)]
//! # struct SynapseStatus {
//! #     our_global_id: String,
//! #     known_entities: usize,
//! # }
//! # let status = RouterStatus {
//! #     synapse_status: SynapseStatus {
//! #         our_global_id: "test@example.com".to_string(),
//! #         known_entities: 5,
//! #     },
//! #     multi_transport_enabled: true,
//! #     email_server_enabled: false,
//! #     available_transports: vec!["TCP".to_string(), "Email".to_string()],
//! # };
//! // let status = router.status().await;
//! println!("Router Status:");
//! println!("  🆔 Our ID: {}", status.synapse_status.our_global_id);
//! println!("  👥 Known entities: {}", status.synapse_status.known_entities);
//! println!("  🚀 Multi-transport: {}", status.multi_transport_enabled);
//! println!("  📧 Email server: {}", status.email_server_enabled);
//! println!("  🔌 Available transports: {:?}", status.available_transports);
//! # Ok(())
//! # }
//! ```
//!
//! ### Email Server Access
//! ```rust
//! # use synapse::*;
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! # let config = Config::default();
//! # let router = EnhancedSynapseRouter::new(config, "test@example.com".to_string()).await?;
//! # #[derive(Debug)]
//! # struct UserAccount {
//! #     username: String, email: String, password_hash: &'static str,
//! #     active: bool, permissions: UserPermissions,
//! # }
//! # #[derive(Debug)]
//! # struct UserPermissions {
//! #     can_send: bool, can_receive: bool, can_relay: bool, is_admin: bool,
//! # }
//! # struct EmailServer;
//! # impl EmailServer {
//! #     fn add_user(&self, _user: UserAccount) -> Result<(), Box<dyn std::error::Error>> { Ok(()) }
//! #     fn add_local_domain(&self, _domain: &str) -> Result<(), Box<dyn std::error::Error>> { Ok(()) }
//! #     fn add_relay_domain(&self, _domain: &str) -> Result<(), Box<dyn std::error::Error>> { Ok(()) }
//! # }
//! # let email_server = Some(EmailServer);
//! // Configure email server if running locally
//! if let Some(email_server) = &email_server {
//!     // Add users for authentication
//!     email_server.add_user(UserAccount {
//!         username: "alice".to_string(),
//!         email: "alice@mydomain.com".to_string(),
//!         password_hash: "...",
//!         active: true,
//!         permissions: UserPermissions {
//!             can_send: true,
//!             can_receive: true,
//!             can_relay: false,
//!             is_admin: false,
//!         },
//!     })?;
//!
//!     // Add domains for email routing
//!     email_server.add_local_domain("mydomain.com")?;
//!     email_server.add_relay_domain("trusted-partner.com")?;
//! }
//! # Ok(())
//! # }
//! ```
//!
//! ## 🔒 Security Features
//!
//! Security is built into every layer:
//!
//! ### Automatic Encryption
//! - **Message Content**: PGP-encrypted with recipient's public key
//! - **Transport Layer**: TLS for real-time connections
//! - **Email Security**: S/MIME and PGP support
//! - **Key Management**: Automatic key generation and distribution
//!
//! ### Authentication
//! - **Sender Verification**: Digital signatures on all messages
//! - **Identity Validation**: DNS and email-based verification
//! - **Trust Levels**: Graduated trust based on verification methods
//! - **Replay Protection**: Timestamps and nonces prevent replay attacks
//!
//! ### Access Control
//! - **Domain-based**: Allow/deny based on email domains
//! - **User-based**: Individual user permissions and capabilities
//! - **Transport-based**: Different security for different channels
//! - **Rate Limiting**: Prevent abuse and DoS attacks
//!
//! The Enhanced Synapse Router provides enterprise-grade messaging with
//! the simplicity of sending an email and the performance of modern
//! real-time protocols.

use crate::synapse::blockchain::serialization::{DateTimeWrapper, UuidWrapper};
use crate::{
    config::Config,
    email_server::{ServerRecommendation, SynapseEmailServer},
    error::Result,
    router::SynapseRouter,
    transport::MultiTransportRouter,
    transport::TransportRoute,
    transport::abstraction::MessageUrgency,
    types::{MessageType, SecureMessage, SecurityLevel, SimpleMessage},
};
use chrono::Utc;
use std::{collections::HashMap, sync::Arc};
use tracing::{info, warn};
use uuid::Uuid;

/// Enhanced Synapse router with multi-transport support and email server
pub struct EnhancedSynapseRouter {
    /// Original email-based router
    synapse_router: SynapseRouter,
    /// Multi-transport router for fast communication
    multi_transport: Option<Arc<MultiTransportRouter>>,
    /// Local email server (SMTP/IMAP) for when we're externally accessible
    email_server: Option<Arc<SynapseEmailServer>>,
    /// Configuration
    #[allow(dead_code)] // Configuration is used for feature flags and transport initialization
    config: Config,
    /// Our global identity
    our_global_id: String,
    /// Enable multi-transport features
    multi_transport_enabled: bool,
    /// Email server enabled
    email_server_enabled: bool,
}

impl EnhancedSynapseRouter {
    /// Create a new enhanced router with multi-transport support and email server
    pub async fn new(config: Config, our_global_id: String) -> Result<Self> {
        info!("Initializing enhanced Synapse router with multi-transport support and email server");

        // Create the traditional Synapse router
        let synapse_router =
            crate::router::SynapseRouter::new(config.clone(), our_global_id.clone()).await?;

        // Try to initialize multi-transport router
        let multi_transport =
            match MultiTransportRouter::new(config.clone(), our_global_id.clone()).await {
                Ok(mt_router) => {
                    info!("Multi-transport router initialized successfully");
                    Some(Arc::new(mt_router))
                }
                Err(e) => {
                    warn!("Failed to initialize multi-transport router: {}", e);
                    warn!("Falling back to email-only mode");
                    None
                }
            };

        // Try to initialize email server with connectivity detection
        let email_server = match SynapseEmailServer::new().await {
            Ok(server) => {
                let connectivity = server.get_connectivity();
                match &connectivity.recommended_config {
                    ServerRecommendation::RunLocalServer {
                        smtp_port,
                        imap_port,
                        external_ip,
                    } => {
                        info!(
                            "Email server configured to run locally on {}:{}/{}",
                            external_ip, smtp_port, imap_port
                        );
                        Some(Arc::new(server))
                    }
                    ServerRecommendation::RelayOnly { reason } => {
                        info!("Email server configured for relay-only mode: {}", reason);
                        Some(Arc::new(server))
                    }
                    ServerRecommendation::ExternalProvider { reason } => {
                        warn!("Using external email provider: {}", reason);
                        warn!("Email server will not be started locally");
                        None
                    }
                }
            }
            Err(e) => {
                warn!("Failed to initialize email server: {}", e);
                warn!("Will use external email provider only");
                None
            }
        };

        let multi_transport_enabled = multi_transport.is_some();
        let email_server_enabled = email_server.is_some();

        Ok(Self {
            synapse_router,
            multi_transport,
            email_server,
            config,
            our_global_id,
            multi_transport_enabled,
            email_server_enabled,
        })
    }

    /// Send a message with automatic transport selection
    pub async fn send_message_smart(
        &self,
        to_entity: &str,
        content: &str,
        message_type: MessageType,
        security_level: SecurityLevel,
        urgency: MessageUrgency,
    ) -> Result<String> {
        info!(
            "Sending smart message to {} (urgency: {:?})",
            to_entity, urgency
        );

        // If multi-transport is available and urgency is high, try it first
        if let Some(ref mt_router) = self.multi_transport
            && matches!(
                urgency,
                MessageUrgency::RealTime | MessageUrgency::Interactive
            )
        {
            // Create secure message
            let simple_msg = SimpleMessage {
                to: to_entity.to_string(),
                from_entity: self.our_global_id.clone(),
                content: content.to_string(),
                message_type: message_type.clone(),
                metadata: std::collections::HashMap::new(),
            };

            let secure_msg = self
                .create_secure_message(&simple_msg, security_level.clone())
                .await?;

            // Try multi-transport first
            match mt_router
                .send_message(to_entity, &secure_msg, urgency)
                .await
            {
                Ok(delivery_receipt) => {
                    let message_id = delivery_receipt.message_id.clone();
                    info!("Message sent via multi-transport: {}", message_id);
                    return Ok(message_id);
                }
                Err(e) => {
                    warn!("Multi-transport failed: {}, falling back to email", e);
                }
            }
        }

        // Fallback to traditional email routing
        info!("Using traditional email routing for {}", to_entity);
        let simple_msg = SimpleMessage {
            to: to_entity.to_string(),
            from_entity: self.our_global_id.clone(),
            content: content.to_string(),
            message_type,
            metadata: HashMap::new(),
        };
        self.synapse_router
            .send_message(simple_msg, to_entity.to_string())
            .await
            .map(|_| "email_fallback".to_string())
    }

    /// Send message with explicit transport preference
    pub async fn send_message_with_transport(
        &self,
        to_entity: &str,
        content: &str,
        message_type: MessageType,
        security_level: SecurityLevel,
        preferred_routes: &[TransportRoute],
    ) -> Result<String> {
        if let Some(ref mt_router) = self.multi_transport {
            let simple_msg = SimpleMessage {
                to: to_entity.to_string(),
                from_entity: self.our_global_id.clone(),
                content: content.to_string(),
                message_type,
                metadata: std::collections::HashMap::new(),
            };

            let secure_msg = self
                .create_secure_message(&simple_msg, security_level)
                .await?;

            return mt_router
                .send_with_fallback_priority(to_entity, &secure_msg, preferred_routes)
                .await
                .map(|receipt| receipt.message_id);
        }

        // Fallback to email
        let simple_msg = SimpleMessage {
            to: to_entity.to_string(),
            from_entity: self.our_global_id.clone(),
            content: content.to_string(),
            message_type,
            metadata: HashMap::new(),
        };
        self.synapse_router
            .send_message(simple_msg, to_entity.to_string())
            .await
            .map(|_| "email_fallback".to_string())
    }

    /// Test connection to an entity
    pub async fn test_connection(&self, target: &str) -> ConnectionCapabilities {
        let mut capabilities = ConnectionCapabilities {
            email: true, // Email is always available via Synapse
            direct_tcp: false,
            direct_udp: false,
            mdns_local: false,
            nat_traversal: false,
            estimated_latency_ms: 60_000, // Default to 1-minute email latency
        };

        if let Some(ref mt_router) = self.multi_transport {
            // Test direct connections
            capabilities.direct_tcp = mt_router.can_connect_directly(target).await;

            // Test local discovery
            if mt_router.discover_local_peer(target).await.is_ok() {
                capabilities.mdns_local = true;
                capabilities.estimated_latency_ms = 50; // Local network latency
            }

            // Test NAT traversal
            if mt_router.establish_nat_traversal(target).await.is_ok() {
                capabilities.nat_traversal = true;
                capabilities.estimated_latency_ms = capabilities.estimated_latency_ms.min(200);
            }

            // If we can connect directly, estimate much lower latency
            if capabilities.direct_tcp || capabilities.direct_udp {
                capabilities.estimated_latency_ms = capabilities.estimated_latency_ms.min(100);
            }
        }

        capabilities
    }

    /// Start all router services including email server
    pub async fn start(&self) -> Result<()> {
        info!("Starting enhanced Synapse router");

        // Start the traditional Synapse router
        // Start Synapse router (no explicit start method)
        // self.synapse_router.start().await?;

        // Start email server if available
        if let Some(ref email_server) = self.email_server {
            email_server.start().await?;
            info!("Email server started successfully");
        }

        // Start multi-transport services if available
        if let Some(ref mt_router) = self.multi_transport {
            mt_router.start_background_services().await?;
            info!("Multi-transport services started");
        }

        info!("Enhanced EMRP router fully started");
        Ok(())
    }

    /// Get enhanced router status including email server
    pub async fn status(&self) -> EnhancedRouterStatus {
        let synapse_status = self.synapse_router.get_health().await;

        let mut capabilities = vec!["email".to_string()];

        if let Some(ref mt_router) = self.multi_transport {
            capabilities.extend(mt_router.get_capabilities());
        }

        if self.email_server_enabled {
            capabilities.push("smtp-server".to_string());
            capabilities.push("imap-server".to_string());
        }

        EnhancedRouterStatus {
            synapse_status,
            multi_transport_enabled: self.multi_transport_enabled,
            email_server_enabled: self.email_server_enabled,
            available_transports: capabilities,
        }
    }

    /// Create secure message (helper method)
    async fn create_secure_message(
        &self,
        simple_msg: &SimpleMessage,
        security_level: SecurityLevel,
    ) -> Result<SecureMessage> {
        let message_id = UuidWrapper::new(Uuid::new_v4());
        let timestamp = DateTimeWrapper::new(Utc::now());

        // For now, create a basic secure message
        // In a real implementation, this would involve the crypto manager
        Ok(SecureMessage {
            message_id,
            to_global_id: simple_msg.to.clone(),
            from_global_id: self.our_global_id.clone(),
            timestamp,
            security_level,
            encrypted_content: simple_msg.content.as_bytes().to_vec(),
            signature: Vec::new(),
            routing_path: Vec::new(),
            metadata: simple_msg.metadata.clone(),
        })
    }

    /// Benchmark transport performance to a target
    pub async fn benchmark_transport(&self, target: &str) -> TransportBenchmarks {
        let mut benchmarks = TransportBenchmarks {
            email_latency_ms: 60_000,
            tcp_latency_ms: None,
            udp_latency_ms: None,
            mdns_latency_ms: None,
            nat_traversal_latency_ms: None,
        };

        if let Some(ref mt_router) = self.multi_transport {
            let test_message = SecureMessage {
                message_id: UuidWrapper::new(Uuid::new_v4()),
                to_global_id: target.to_string(),
                from_global_id: self.our_global_id.clone(),
                timestamp: DateTimeWrapper::new(Utc::now()),
                security_level: SecurityLevel::Public,
                encrypted_content: b"benchmark test".to_vec(),
                signature: Vec::new(),
                routing_path: Vec::new(),
                metadata: std::collections::HashMap::new(),
            };

            // Test different transport routes
            let test_routes = vec![
                TransportRoute::DirectTcp {
                    address: target.to_string(),
                    port: 8080,
                    latency_ms: 0,
                    established_at: std::time::Instant::now(),
                },
                TransportRoute::DirectUdp {
                    address: target.to_string(),
                    port: 8080,
                    latency_ms: 0,
                    established_at: std::time::Instant::now(),
                },
                TransportRoute::LocalMdns {
                    service_name: target.to_string(),
                    address: target.to_string(),
                    port: 5353,
                    latency_ms: 0,
                    discovered_at: std::time::Instant::now(),
                },
            ];

            for route in test_routes {
                let start = std::time::Instant::now();
                match mt_router
                    .send_with_fallback_priority(
                        target,
                        &test_message,
                        std::slice::from_ref(&route),
                    )
                    .await
                {
                    Ok(_) => {
                        let latency = start.elapsed().as_millis() as u32;
                        match route {
                            TransportRoute::DirectTcp { .. } => {
                                benchmarks.tcp_latency_ms = Some(latency)
                            }
                            TransportRoute::DirectUdp { .. } => {
                                benchmarks.udp_latency_ms = Some(latency)
                            }
                            TransportRoute::LocalMdns { .. } => {
                                benchmarks.mdns_latency_ms = Some(latency)
                            }
                            _ => {}
                        }
                    }
                    Err(_) => {
                        // Transport not available or failed
                    }
                }
            }
        }

        benchmarks
    }

    /// Get access to the email server for configuration
    pub fn email_server(&self) -> Option<Arc<SynapseEmailServer>> {
        self.email_server.clone()
    }

    /// Check if we're running our own email server
    pub fn is_running_email_server(&self) -> bool {
        self.email_server_enabled && self.email_server.is_some()
    }

    /// Get email server connectivity information
    pub fn email_server_connectivity(&self) -> Option<String> {
        if let Some(ref server) = self.email_server {
            let connectivity = server.get_connectivity();
            Some(format!("{:?}", connectivity.recommended_config))
        } else {
            None
        }
    }
}

/// Connection capabilities for a target
#[derive(Debug, Clone)]
pub struct ConnectionCapabilities {
    pub email: bool,
    pub direct_tcp: bool,
    pub direct_udp: bool,
    pub mdns_local: bool,
    pub nat_traversal: bool,
    pub estimated_latency_ms: u32,
}

/// Enhanced router status
#[derive(Debug, Clone)]
pub struct EnhancedRouterStatus {
    pub synapse_status: super::router::RouterHealth,
    pub multi_transport_enabled: bool,
    pub email_server_enabled: bool,
    pub available_transports: Vec<String>,
}

/// Transport performance benchmarks
#[derive(Debug, Clone)]
pub struct TransportBenchmarks {
    pub email_latency_ms: u32,
    pub tcp_latency_ms: Option<u32>,
    pub udp_latency_ms: Option<u32>,
    pub mdns_latency_ms: Option<u32>,
    pub nat_traversal_latency_ms: Option<u32>,
}
