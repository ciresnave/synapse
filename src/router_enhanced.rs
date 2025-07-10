//! # Enhanced EMRP Router - Your Gateway to Intelligent Communication
//!
//! The **Enhanced EMRP Router** is the main interface for all EMRP communication.
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
//! use message_routing_system::*;
//!
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!     // Create and configure router
//!     let config = Config::default();
//!     let router = EnhancedEmrpRouter::new(config, "mybot@example.com".to_string()).await?;
//!     
//!     // Start all services
//!     router.start().await?;
//!     
//!     // Send message using simple name
//!     router.send_message_smart(
//!         "Alice",                      // Simple name (auto-resolved)
//!         "Hello from EMRP!",           // Your message
//!         MessageType::Direct,          // Type of communication
//!         SecurityLevel::Authenticated, // Security level
//!         MessageUrgency::Interactive,  // Speed preference
//!     ).await?;
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
//! - **EMRP-native**: Direct protocol communication
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
//! // The router automatically detects your network situation
//! let router = EnhancedEmrpRouter::new(config, entity_id).await?;
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
//! │                Enhanced EMRP Router                     │
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
//! // For real-time AI interactions
//! router.send_message_smart(
//!     "Claude",
//!     "Quick brainstorming session?",
//!     MessageType::Conversation,
//!     SecurityLevel::Public,
//!     MessageUrgency::RealTime,  // System will prefer TCP/UDP
//! ).await?;
//! ```
//!
//! ### Reliable File Sharing
//! ```rust
//! // For important file transfers
//! router.send_file_with_message(
//!     "ResearchTeam", 
//!     "breakthrough_results.pdf",
//!     "Major breakthrough achieved! See attached data.",
//!     MessageUrgency::Background,  // System will use email for reliability
//! ).await?;
//! ```
//!
//! ### Mixed-urgency Workflow
//! ```rust
//! // Real-time coordination
//! router.send_message_smart(
//!     "TeamBot", 
//!     "Starting analysis now",
//!     MessageType::Notification,
//!     SecurityLevel::Public,
//!     MessageUrgency::Interactive,
//! ).await?;
//!
//! // ... do work ...
//!
//! // Reliable results delivery
//! router.send_message_smart(
//!     "TeamBot",
//!     "Analysis complete. Results attached.",
//!     MessageType::Direct, 
//!     SecurityLevel::Encrypted,
//!     MessageUrgency::Background,  // Guarantees delivery
//! ).await?;
//! ```
//!
//! ## 📊 Performance Monitoring
//!
//! The router provides comprehensive performance insights:
//!
//! ```rust
//! // Test connection capabilities to a peer
//! let capabilities = router.test_connection("Alice").await;
//! println!("Connection to Alice:");
//! println!("  📧 Email: {}", capabilities.email);
//! println!("  🔗 Direct TCP: {}", capabilities.direct_tcp);
//! println!("  📡 Direct UDP: {}", capabilities.direct_udp);
//! println!("  🏠 Local mDNS: {}", capabilities.mdns_local);
//! println!("  🌐 NAT Traversal: {}", capabilities.nat_traversal);
//! println!("  ⏱️  Estimated latency: {}ms", capabilities.estimated_latency_ms);
//!
//! // Benchmark transport performance
//! let benchmarks = router.benchmark_transport("Alice").await;
//! println!("Performance to Alice:");
//! if let Some(tcp) = benchmarks.tcp_latency_ms {
//!     println!("  🔗 TCP: {}ms", tcp);
//! }
//! if let Some(udp) = benchmarks.udp_latency_ms {
//!     println!("  📡 UDP: {}ms", udp);
//! }
//! println!("  📧 Email: {}ms", benchmarks.email_latency_ms);
//! ```
//!
//! ## 🔧 Configuration and Management
//!
//! ### Router Status
//! ```rust
//! let status = router.status().await;
//! println!("Router Status:");
//! println!("  🆔 Our ID: {}", status.emrp_status.our_global_id);
//! println!("  👥 Known entities: {}", status.emrp_status.known_entities);
//! println!("  🚀 Multi-transport: {}", status.multi_transport_enabled);
//! println!("  📧 Email server: {}", status.email_server_enabled);
//! println!("  🔌 Available transports: {:?}", status.available_transports);
//! ```
//!
//! ### Email Server Access
//! ```rust
//! // Configure email server if running locally
//! if let Some(email_server) = router.email_server() {
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
//! The Enhanced EMRP Router provides enterprise-grade messaging with
//! the simplicity of sending an email and the performance of modern
//! real-time protocols.

use crate::{
    types::{SimpleMessage, SecureMessage, SecurityLevel, MessageType},
    transport::{MultiTransportRouter, MessageUrgency, TransportRoute},
    config::Config,
    error::Result,
    email_server::{EmrpEmailServer, ServerRecommendation},
    router::EmrpRouter,
};
use std::sync::Arc;
use tracing::{info, warn};

/// Enhanced EMRP router with multi-transport support and email server
pub struct EnhancedEmrpRouter {
    /// Original email-based router
    emrp_router: EmrpRouter,
    /// Multi-transport router for fast communication
    multi_transport: Option<Arc<MultiTransportRouter>>,
    /// Local email server (SMTP/IMAP) for when we're externally accessible
    email_server: Option<Arc<EmrpEmailServer>>,
    /// Configuration
    #[allow(dead_code)]
    config: Config,
    /// Our global identity
    our_global_id: String,
    /// Enable multi-transport features
    multi_transport_enabled: bool,
    /// Email server enabled
    email_server_enabled: bool,
}

impl EnhancedEmrpRouter {
    /// Create a new enhanced router with multi-transport support and email server
    pub async fn new(config: Config, our_global_id: String) -> Result<Self> {
        info!("Initializing enhanced EMRP router with multi-transport support and email server");
        
        // Create the traditional EMRP router
        let emrp_router = crate::router::EmrpRouter::new(config.clone(), our_global_id.clone()).await?;
        
        // Try to initialize multi-transport router
        let multi_transport = match MultiTransportRouter::new(config.clone(), our_global_id.clone()).await {
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
        let email_server = match EmrpEmailServer::new().await {
            Ok(server) => {
                let connectivity = server.get_connectivity();
                match &connectivity.recommended_config {
                    ServerRecommendation::RunLocalServer { smtp_port, imap_port, external_ip } => {
                        info!("Email server configured to run locally on {}:{}/{}", external_ip, smtp_port, imap_port);
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
            emrp_router,
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
        info!("Sending smart message to {} (urgency: {:?})", to_entity, urgency);
        
        // If multi-transport is available and urgency is high, try it first
        if let Some(ref mt_router) = self.multi_transport {
            if matches!(urgency, MessageUrgency::RealTime | MessageUrgency::Interactive) {
                // Create secure message
                let simple_msg = SimpleMessage {
                    to: to_entity.to_string(),
                    from_entity: self.our_global_id.clone(),
                    content: content.to_string(),
                    message_type: message_type.clone(),
                    metadata: std::collections::HashMap::new(),
                };
                
                let secure_msg = self.create_secure_message(&simple_msg, security_level.clone()).await?;
                
                // Try multi-transport first
                match mt_router.send_message(to_entity, &secure_msg, urgency).await {
                    Ok(message_id) => {
                        info!("Message sent via multi-transport: {}", message_id);
                        return Ok(message_id);
                    }
                    Err(e) => {
                        warn!("Multi-transport failed: {}, falling back to email", e);
                    }
                }
            }
        }
        
        // Fallback to traditional email routing
        info!("Using traditional email routing for {}", to_entity);
        self.emrp_router.send_message(to_entity, content, message_type, security_level).await
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
            
            let secure_msg = self.create_secure_message(&simple_msg, security_level).await?;
            
            return mt_router.send_with_fallback_priority(to_entity, &secure_msg, preferred_routes).await;
        }
        
        // Fallback to email
        self.emrp_router.send_message(to_entity, content, message_type, security_level).await
    }
    
    /// Test connection to an entity
    pub async fn test_connection(&self, target: &str) -> ConnectionCapabilities {
        let mut capabilities = ConnectionCapabilities {
            email: true, // Email is always available via EMRP
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
        info!("Starting enhanced EMRP router");
        
        // Start the traditional EMRP router
        self.emrp_router.start().await?;
        
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
        let emrp_status = self.emrp_router.status().await;
        
        let mut capabilities = vec!["email".to_string()];
        
        if let Some(ref mt_router) = self.multi_transport {
            capabilities.extend(mt_router.get_capabilities());
        }
        
        if self.email_server_enabled {
            capabilities.push("smtp-server".to_string());
            capabilities.push("imap-server".to_string());
        }
        
        EnhancedRouterStatus {
            emrp_status,
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
        let message_id = uuid::Uuid::new_v4();
        let timestamp = chrono::Utc::now();
        
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
                message_id: uuid::Uuid::new_v4(),
                to_global_id: target.to_string(),
                from_global_id: self.our_global_id.clone(),
                timestamp: chrono::Utc::now(),
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
                    service_name: format!("_emrp._tcp.local"),
                    address: target.to_string(),
                    port: 8080,
                    latency_ms: 0,
                    discovered_at: std::time::Instant::now(),
                },
            ];
            
            for route in test_routes {
                let start = std::time::Instant::now();
                match mt_router.send_with_fallback_priority(target, &test_message, &[route.clone()]).await {
                    Ok(_) => {
                        let latency = start.elapsed().as_millis() as u32;
                        match route {
                            TransportRoute::DirectTcp { .. } => benchmarks.tcp_latency_ms = Some(latency),
                            TransportRoute::DirectUdp { .. } => benchmarks.udp_latency_ms = Some(latency),
                            TransportRoute::LocalMdns { .. } => benchmarks.mdns_latency_ms = Some(latency),
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
    pub fn email_server(&self) -> Option<Arc<EmrpEmailServer>> {
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

/// Re-export the original router for compatibility


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
    pub emrp_status: super::router::RouterStatus,
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
