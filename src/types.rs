//! # Core Types and Message Structures for EMRP
//!
//! This module defines all the fundamental data types used throughout the
//! Email-Based Message Routing Protocol. Understanding these types is essential
//! for working with EMRP messages, identities, and configurations.
//!
//! ## 🏗️ Message Architecture
//!
//! EMRP uses a layered message architecture designed for flexibility and security:
//!
//! ```text
//! ┌─────────────────────────────────────────────────────┐
//! │                SimpleMessage                        │
//! │  Human-readable, easy to work with                  │
//! │  • to: "Alice"                                      │
//! │  • from_entity: "Claude"                            │
//! │  • content: "Hello!"                                │
//! │  • message_type: Direct                             │
//! └─────────────────┬───────────────────────────────────┘
//!                   │ (automatic conversion)
//!                   ▼
//! ┌─────────────────────────────────────────────────────┐
//! │                SecureMessage                        │
//! │  Network-ready with security and routing            │
//! │  • message_id: uuid                                 │
//! │  • to_global_id: "alice@ai-lab.example.com"        │
//! │  • from_global_id: "claude@anthropic.com"          │
//! │  • encrypted_content: [encrypted bytes]            │
//! │  • signature: [digital signature]                  │
//! │  • security_level: Authenticated                   │
//! │  • routing_path: [hop1, hop2, ...]                 │
//! └─────────────────────────────────────────────────────┘
//! ```
//!
//! ## 🎯 Entity Types - Who's Who in EMRP
//!
//! EMRP supports different types of communicating entities:
//!
//! ### Human
//! - **Purpose**: Represents actual human users
//! - **Examples**: Researchers, developers, end users
//! - **Capabilities**: Typically uses client applications, email interfaces
//! - **Security**: Usually requires authentication, may have elevated privileges
//!
//! ### AiModel  
//! - **Purpose**: AI systems, language models, intelligent agents
//! - **Examples**: Claude, GPT-4, local AI assistants, specialized ML models
//! - **Capabilities**: Automated responses, real-time communication, batch processing
//! - **Security**: May have special encryption requirements, rate limiting
//!
//! ### Tool
//! - **Purpose**: Utility services and specialized tools
//! - **Examples**: Image generators, code analyzers, data processors
//! - **Capabilities**: Function-specific, often stateless, API-driven
//! - **Security**: Often public or semi-public access
//!
//! ### Service
//! - **Purpose**: Infrastructure and platform services
//! - **Examples**: Databases, authentication servers, load balancers
//! - **Capabilities**: High reliability, scalability, enterprise features
//! - **Security**: Strong authentication, audit logging, access controls
//!
//! ### Router
//! - **Purpose**: EMRP routing infrastructure
//! - **Examples**: Email servers, message relays, protocol gateways
//! - **Capabilities**: Message forwarding, protocol translation, caching
//! - **Security**: Trusted infrastructure, certificate-based authentication
//!
//! ## 📝 Message Types - Communication Patterns
//!
//! Different message types enable different communication patterns:
//!
//! ### Direct
//! ```rust
//! // One-to-one private communication
//! SimpleMessage {
//!     to: "Alice".to_string(),
//!     from_entity: "Claude".to_string(), 
//!     content: "Can you help me with this analysis?".to_string(),
//!     message_type: MessageType::Direct,
//!     metadata: HashMap::new(),
//! }
//! ```
//! - **Use Case**: Private conversations, specific requests
//! - **Routing**: Point-to-point, highest priority
//! - **Security**: End-to-end encryption by default
//!
//! ### Broadcast
//! ```rust
//! // One-to-many public announcements
//! SimpleMessage {
//!     to: "AllTeamMembers".to_string(),
//!     from_entity: "ProjectManager".to_string(),
//!     content: "Weekly meeting at 3pm today".to_string(), 
//!     message_type: MessageType::Broadcast,
//!     metadata: [("priority", "high")].into(),
//! }
//! ```
//! - **Use Case**: Announcements, status updates, alerts
//! - **Routing**: Fan-out to all subscribers
//! - **Security**: Usually public or group-encrypted
//!
//! ### Conversation
//! ```rust
//! // Multi-party ongoing discussion
//! SimpleMessage {
//!     to: "ResearchGroup".to_string(),
//!     from_entity: "Alice".to_string(),
//!     content: "I think we should try a different approach".to_string(),
//!     message_type: MessageType::Conversation,
//!     metadata: [("thread_id", "quantum-research-2024")].into(),
//! }
//! ```
//! - **Use Case**: Group discussions, collaborative work
//! - **Routing**: All participants receive message
//! - **Security**: Group-encrypted, shared access
//!
//! ### Notification
//! ```rust
//! // System-generated alerts and updates
//! SimpleMessage {
//!     to: "DevOpsTeam".to_string(),
//!     from_entity: "MonitoringSystem".to_string(),
//!     content: "Server CPU usage exceeded 90% threshold".to_string(),
//!     message_type: MessageType::Notification,
//!     metadata: [
//!         ("severity", "warning"),
//!         ("server", "web-01.prod"), 
//!         ("metric", "cpu_usage"),
//!         ("value", "92.3")
//!     ].into(),
//! }
//! ```
//! - **Use Case**: Automated alerts, system status, monitoring
//! - **Routing**: Priority-based, may have special handling
//! - **Security**: Often authenticated but not necessarily encrypted
//!
//! ## 🔒 Security Levels - Protection Gradients
//!
//! EMRP provides multiple security levels to balance protection with performance:
//!
//! ### Public
//! - **Protection**: Minimal - no encryption, optional signatures
//! - **Use Case**: Public announcements, status updates, discovery messages
//! - **Performance**: Fastest - no crypto overhead
//! - **Example**: "Bot online and ready for requests"
//!
//! ### Authenticated  
//! - **Protection**: Sender verification via digital signatures
//! - **Use Case**: Trusted communications where identity matters
//! - **Performance**: Fast - only signature overhead
//! - **Example**: "Command acknowledged, processing request #1234"
//!
//! ### Encrypted
//! - **Protection**: Content encrypted with recipient's public key
//! - **Use Case**: Sensitive information, private conversations
//! - **Performance**: Moderate - encryption overhead
//! - **Example**: "API key for service X is: sk_abc123..."
//!
//! ### Confidential
//! - **Protection**: Encryption + signature + additional metadata protection
//! - **Use Case**: Highly sensitive data, regulatory compliance
//! - **Performance**: Slower - maximum security overhead
//! - **Example**: Personal data, financial information, trade secrets
//!
//! ## 🌐 Global Identity Structure
//!
//! Every entity in EMRP has a structured global identity:
//!
//! ```rust
//! GlobalIdentity {
//!     local_name: "Alice".to_string(),           // Human-friendly name
//!     global_id: "alice@ai-lab.example.com".to_string(), // Globally unique
//!     entity_type: EntityType::AiModel,         // What kind of entity
//!     capabilities: vec![                       // What it can do
//!         "real-time-messaging".to_string(),
//!         "file-transfer".to_string(),
//!         "voice-calls".to_string(),
//!     ],
//!     public_key: Some(public_key_bytes),       // For encryption
//!     display_name: Some("Alice AI Researcher".to_string()), // Pretty name
//!     created_at: Utc::now(),                   // When registered
//! }
//! ```
//!
//! ### Identity Resolution Chain
//! ```text
//! "Alice" → alice@ai-lab.example.com → 192.168.1.100:8080 → [TCP, UDP, Email]
//!   ↑            ↑                        ↑                      ↑
//! Local      Global ID              Network Address        Capabilities
//! Name      (DNS-based)            (Dynamic Discovery)    (Feature Detection)
//! ```
//!
//! ## 📧 Email Configuration
//!
//! EMRP seamlessly integrates with email infrastructure:
//!
//! ```rust
//! EmailConfig {
//!     smtp: SmtpConfig {
//!         host: "smtp.gmail.com".to_string(),
//!         port: 587,
//!         username: "mybot@gmail.com".to_string(),
//!         password: "app_password".to_string(),
//!         use_tls: true,    // Modern security
//!         use_ssl: false,   // Legacy security
//!     },
//!     imap: ImapConfig {
//!         host: "imap.gmail.com".to_string(),
//!         port: 993, 
//!         username: "mybot@gmail.com".to_string(),
//!         password: "app_password".to_string(),
//!         use_ssl: true,    // Secure IMAP
//!     },
//! }
//! ```
//!
//! ## 🎛️ Message Metadata - Extensible Information
//!
//! All messages can carry arbitrary metadata for extended functionality:
//!
//! ```rust
//! // Example: AI collaboration metadata
//! let mut metadata = HashMap::new();
//! metadata.insert("conversation_id".to_string(), "research-session-001".to_string());
//! metadata.insert("model_version".to_string(), "claude-3.5".to_string());
//! metadata.insert("temperature".to_string(), "0.7".to_string());
//! metadata.insert("context_window".to_string(), "200k".to_string());
//!
//! // Example: File transfer metadata
//! metadata.insert("file_name".to_string(), "research_data.zip".to_string());
//! metadata.insert("file_size".to_string(), "15728640".to_string()); // 15MB
//! metadata.insert("file_hash".to_string(), "sha256:abc123...".to_string());
//! metadata.insert("compression".to_string(), "gzip".to_string());
//!
//! // Example: Real-time communication metadata
//! metadata.insert("urgency".to_string(), "real-time".to_string());
//! metadata.insert("timeout_ms".to_string(), "5000".to_string());
//! metadata.insert("retry_count".to_string(), "3".to_string());
//! metadata.insert("preferred_transport".to_string(), "tcp".to_string());
//! ```
//!
//! ## 🔄 Message Lifecycle
//!
//! Understanding how messages flow through EMRP:
//!
//! ```text
//! 1. Creation
//!    SimpleMessage → user creates with simple fields
//!    
//! 2. Identity Resolution  
//!    "Alice" → alice@ai-lab.example.com (local name to global ID)
//!    
//! 3. Security Processing
//!    SimpleMessage → SecureMessage (encryption, signing)
//!    
//! 4. Transport Selection
//!    Network discovery → choose TCP/UDP/Email based on urgency
//!    
//! 5. Delivery
//!    Send via chosen transport with automatic retry/fallback
//!    
//! 6. Receipt Processing
//!    Decrypt, verify, route to application handler
//! ```
//!
//! This type system provides the foundation for EMRP's flexibility while
//! maintaining strong typing and security throughout the communication process.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use chrono::{DateTime, Utc};
use uuid::Uuid;

/// Types of entities in the global network
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityType {
    /// Human user
    Human,
    /// AI model (LLM, assistant, etc.)
    AiModel,
    /// Tool or service
    Tool,
    /// System service
    Service,
    /// Message router
    Router,
}

impl std::fmt::Display for EntityType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EntityType::Human => write!(f, "human"),
            EntityType::AiModel => write!(f, "ai_model"),
            EntityType::Tool => write!(f, "tool"),
            EntityType::Service => write!(f, "service"),
            EntityType::Router => write!(f, "router"),
        }
    }
}

/// Types of messages in the protocol
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageType {
    /// Direct communication between entities
    Direct,
    /// Tool invocation request
    ToolCall,
    /// Tool response/result
    ToolResponse,
    /// System/routing message
    System,
    /// Broadcast to multiple recipients
    Broadcast,
    /// Streaming data chunk
    StreamChunk,
}

impl std::fmt::Display for MessageType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MessageType::Direct => write!(f, "direct"),
            MessageType::ToolCall => write!(f, "tool_call"),
            MessageType::ToolResponse => write!(f, "tool_response"),
            MessageType::System => write!(f, "system"),
            MessageType::Broadcast => write!(f, "broadcast"),
            MessageType::StreamChunk => write!(f, "stream_chunk"),
        }
    }
}

/// Security levels for different message types
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecurityLevel {
    /// No encryption needed
    Public,
    /// End-to-end encrypted
    Private,
    /// Signed but not encrypted
    Authenticated,
    /// Both encrypted and signed
    Secure,
}

impl std::fmt::Display for SecurityLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SecurityLevel::Public => write!(f, "public"),
            SecurityLevel::Private => write!(f, "private"),
            SecurityLevel::Authenticated => write!(f, "authenticated"),
            SecurityLevel::Secure => write!(f, "secure"),
        }
    }
}

/// The simple message format that users interact with
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimpleMessage {
    /// Recipient's local name (e.g., "Eric", "Claude", "FileSystem")
    pub to: String,
    /// Sender's local name
    pub from_entity: String,
    /// Message content
    pub content: String,
    /// Type of message
    pub message_type: MessageType,
    /// Optional metadata
    #[serde(default)]
    pub metadata: HashMap<String, String>,
}

impl SimpleMessage {
    /// Create a new simple message
    pub fn new(
        to: impl Into<String>,
        from_entity: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self {
            to: to.into(),
            from_entity: from_entity.into(),
            content: content.into(),
            message_type: MessageType::Direct,
            metadata: HashMap::new(),
        }
    }

    /// Create a tool call message
    pub fn tool_call(
        to: impl Into<String>,
        from_entity: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self {
            to: to.into(),
            from_entity: from_entity.into(),
            content: content.into(),
            message_type: MessageType::ToolCall,
            metadata: HashMap::new(),
        }
    }

    /// Create a tool response message
    pub fn tool_response(
        to: impl Into<String>,
        from_entity: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self {
            to: to.into(),
            from_entity: from_entity.into(),
            content: content.into(),
            message_type: MessageType::ToolResponse,
            metadata: HashMap::new(),
        }
    }

    /// Add metadata to the message
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }
}

/// Global identity for an entity in the network
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalIdentity {
    /// Local name (e.g., "Eric", "Claude")
    pub local_name: String,
    /// Global email identifier
    pub global_id: String,
    /// Type of entity
    pub entity_type: EntityType,
    /// Public key for encryption (PEM format)
    pub public_key: String,
    /// Entity capabilities
    pub capabilities: Vec<String>,
    /// Trust level (0-100)
    pub trust_level: u8,
    /// Last seen timestamp
    pub last_seen: DateTime<Utc>,
    /// Routing preferences
    #[serde(default)]
    pub routing_preferences: HashMap<String, String>,
}

impl Default for GlobalIdentity {
    fn default() -> Self {
        Self {
            local_name: String::new(),
            global_id: String::new(),
            entity_type: EntityType::AiModel,
            public_key: String::new(),
            capabilities: Vec::new(),
            trust_level: 0,
            last_seen: Utc::now(),
            routing_preferences: HashMap::new(),
        }
    }
}

impl GlobalIdentity {
    /// Create a new global identity
    pub fn new(
        local_name: impl Into<String>,
        global_id: impl Into<String>,
        entity_type: EntityType,
        public_key: impl Into<String>,
    ) -> Self {
        Self {
            local_name: local_name.into(),
            global_id: global_id.into(),
            entity_type,
            public_key: public_key.into(),
            capabilities: Vec::new(),
            trust_level: 50,
            last_seen: Utc::now(),
            routing_preferences: HashMap::new(),
        }
    }

    /// Add a capability to this identity
    pub fn add_capability(&mut self, capability: impl Into<String>) {
        self.capabilities.push(capability.into());
    }

    /// Check if this identity has a specific capability
    pub fn has_capability(&self, capability: &str) -> bool {
        self.capabilities.iter().any(|c| c == capability)
    }

    /// Update the last seen timestamp
    pub fn update_last_seen(&mut self) {
        self.last_seen = Utc::now();
    }
}

/// Secure message for network transport
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecureMessage {
    /// Unique message identifier
    pub message_id: Uuid,
    /// Recipient's global ID
    pub to_global_id: String,
    /// Sender's global ID
    pub from_global_id: String,
    /// Encrypted message content
    pub encrypted_content: Vec<u8>,
    /// Digital signature
    pub signature: Vec<u8>,
    /// Message timestamp
    pub timestamp: DateTime<Utc>,
    /// Security level applied
    pub security_level: SecurityLevel,
    /// Routing path taken
    #[serde(default)]
    pub routing_path: Vec<String>,
    /// Additional metadata
    #[serde(default)]
    pub metadata: HashMap<String, String>,
}

impl SecureMessage {
    /// Create a new secure message
    pub fn new(
        to_global_id: impl Into<String>,
        from_global_id: impl Into<String>,
        encrypted_content: Vec<u8>,
        signature: Vec<u8>,
        security_level: SecurityLevel,
    ) -> Self {
        Self {
            message_id: Uuid::new_v4(),
            to_global_id: to_global_id.into(),
            from_global_id: from_global_id.into(),
            encrypted_content,
            signature,
            timestamp: Utc::now(),
            security_level,
            routing_path: Vec::new(),
            metadata: HashMap::new(),
        }
    }

    /// Add a routing hop to the path
    pub fn add_routing_hop(&mut self, hop: impl Into<String>) {
        self.routing_path.push(hop.into());
    }

    /// Add metadata to the message
    pub fn add_metadata(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.metadata.insert(key.into(), value.into());
    }
}

/// Configuration for email providers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailConfig {
    /// SMTP configuration
    pub smtp: SmtpConfig,
    /// IMAP configuration
    pub imap: ImapConfig,
}

/// SMTP server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmtpConfig {
    /// SMTP server hostname
    pub host: String,
    /// SMTP server port
    pub port: u16,
    /// Username for authentication
    pub username: String,
    /// Password for authentication
    pub password: String,
    /// Use TLS encryption
    pub use_tls: bool,
    /// Use SSL encryption
    pub use_ssl: bool,
}

/// IMAP server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImapConfig {
    /// IMAP server hostname
    pub host: String,
    /// IMAP server port
    pub port: u16,
    /// Username for authentication
    pub username: String,
    /// Password for authentication
    pub password: String,
    /// Use SSL encryption
    pub use_ssl: bool,
}

/// Stream priority levels
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamPriority {
    /// < 100ms latency required
    RealTime,
    /// < 1s latency acceptable
    NearRealTime,
    /// > 1s latency acceptable
    Background,
    /// Collected and sent periodically
    Batch,
}

/// Types of streaming scenarios
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamType {
    /// Tool streaming results back
    ToolOutput,
    /// LLM streaming to tool
    LlmToTool,
    /// User streaming to tool
    UserToTool,
    /// Continuous log streaming
    LogStream,
    /// Real-time data feed
    DataFeed,
    /// Task progress streaming
    Progress,
    /// Back-and-forth streaming
    Interactive,
}

/// A chunk of streaming data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamChunk {
    /// Stream identifier
    pub stream_id: Uuid,
    /// Sequence number in stream
    pub sequence_number: u64,
    /// Type of chunk (data, metadata, control, end)
    pub chunk_type: String,
    /// Base64 encoded payload
    pub data: String,
    /// Timestamp
    pub timestamp: DateTime<Utc>,
    /// Priority level
    pub priority: StreamPriority,
    /// Is this the final chunk?
    pub is_final: bool,
    /// Compression algorithm used
    pub compression: String,
}

impl StreamChunk {
    /// Create a new data chunk
    pub fn new_data(
        stream_id: Uuid,
        sequence_number: u64,
        data: impl Into<String>,
        priority: StreamPriority,
    ) -> Self {
        Self {
            stream_id,
            sequence_number,
            chunk_type: "data".to_string(),
            data: data.into(),
            timestamp: Utc::now(),
            priority,
            is_final: false,
            compression: "none".to_string(),
        }
    }

    /// Create a final chunk to end the stream
    pub fn new_final(stream_id: Uuid, sequence_number: u64) -> Self {
        Self {
            stream_id,
            sequence_number,
            chunk_type: "end".to_string(),
            data: String::new(),
            timestamp: Utc::now(),
            priority: StreamPriority::Background,
            is_final: true,
            compression: "none".to_string(),
        }
    }
}

/// Metadata for a streaming session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamMetadata {
    /// Stream identifier
    pub stream_id: Uuid,
    /// Type of stream
    pub stream_type: StreamType,
    /// Source email address
    pub source: String,
    /// Destination email address
    pub destination: String,
    /// When stream started
    pub started_at: DateTime<Utc>,
    /// Expected duration in seconds
    pub expected_duration: Option<u64>,
    /// Total size estimate in bytes
    pub total_size_estimate: Option<u64>,
    /// Chunk size in bytes
    pub chunk_size: usize,
    /// Compression algorithm
    pub compression: String,
    /// Encryption algorithm
    pub encryption: String,
}

impl StreamMetadata {
    /// Create new stream metadata
    pub fn new(
        stream_type: StreamType,
        source: impl Into<String>,
        destination: impl Into<String>,
    ) -> Self {
        Self {
            stream_id: Uuid::new_v4(),
            stream_type,
            source: source.into(),
            destination: destination.into(),
            started_at: Utc::now(),
            expected_duration: None,
            total_size_estimate: None,
            chunk_size: 32 * 1024, // 32KB default
            compression: "gzip".to_string(),
            encryption: "none".to_string(),
        }
    }
}
