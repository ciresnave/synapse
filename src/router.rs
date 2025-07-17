//! Main Synapse router implementation

use crate::{
    types::{SimpleMessage, SecureMessage, SecurityLevel, MessageType},
    identity::IdentityRegistry,
    config::Config,
    error::Result,
    email::SynapseEmailMessage,
    CryptoManager,
    EmailTransport,
    blockchain::serialization::{DateTimeWrapper, UuidWrapper},
};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, debug, warn};
use uuid::Uuid;
use chrono::Utc;

/// Main Synapse router that handles message routing and protocol operations
#[derive(Debug, Clone)]
pub struct SynapseRouter {
    /// Cryptographic manager
    crypto: Arc<RwLock<CryptoManager>>,
    /// Identity registry
    identity: Arc<RwLock<IdentityRegistry>>,
    /// Email transport
    email: Arc<RwLock<EmailTransport>>,
    /// Configuration
    config: Config, // Router configuration settings
    /// Our global identity
    our_global_id: String,
}

impl SynapseRouter {
    /// Create a new Synapse router
    pub async fn new(config: Config, our_global_id: String) -> Result<Self> {
        // Initialize crypto manager
        let crypto = Arc::new(RwLock::new(CryptoManager::new()));
        
        // Initialize identity registry
        let identity = Arc::new(RwLock::new(IdentityRegistry::new()));
        
        // Initialize email transport
        let email_transport = EmailTransport::new(config.email.clone()).await?;
        let email = Arc::new(RwLock::new(email_transport));
        
        Ok(Self {
            crypto,
            identity,
            email,
            config: config,
            our_global_id,
        })
    }

    /// Send a message through the best available transport
    pub async fn send_message(
        &self,
        simple_msg: SimpleMessage,
        destination_global_id: String,
    ) -> Result<()> {
        info!("Sending message to {}: {}", destination_global_id, simple_msg.content);
        
        // Create secure message
        let mut secure_msg = SecureMessage {
            message_id: UuidWrapper::new(uuid::Uuid::new_v4()),
            to_global_id: destination_global_id.clone(),
            from_global_id: self.our_global_id.clone(),
            encrypted_content: Vec::new(),
            signature: Vec::new(),
            timestamp: DateTimeWrapper::new(chrono::Utc::now()),
            security_level: SecurityLevel::Authenticated,
            routing_path: Vec::new(),
            metadata: simple_msg.metadata.clone(),
        };
        
        // Apply cryptographic operations if available
        {
            let crypto = self.crypto.read().await;
            
            // Try to encrypt if we have recipient's key
            if let SecurityLevel::Secure = secure_msg.security_level {
                if let Ok(encrypted) = crypto.encrypt_message(&simple_msg.content, &simple_msg.to) {
                    secure_msg.encrypted_content = encrypted;
                }
            }
        }
        
        // Sign the message
        let signature = {
            let crypto = self.crypto.read().await;
            crypto.sign_message(&simple_msg.content).unwrap_or_default()
        };
        secure_msg.signature = signature;
        
        // Send via email transport
        let email_transport = self.email.read().await;
        let simple_message = SimpleMessage {
            to: destination_global_id.clone(),
            from_entity: self.our_global_id.clone(),
            content: serde_json::to_string(&secure_msg)?,
            message_type: simple_msg.message_type.clone(),
            metadata: simple_msg.metadata.clone(),
        };
        email_transport.send_message(&secure_msg, &self.our_global_id, &destination_global_id, &simple_message).await?;
        
        info!("Message sent successfully to {}", destination_global_id);
        Ok(())
    }

    /// Receive messages from all transports
    pub async fn receive_messages(&self) -> Result<Vec<SimpleMessage>> {
        let mut all_messages = Vec::new();
        
        // Get messages from email transport
        let email_transport = self.email.read().await;
        let email_messages = email_transport.receive_messages().await?;
        
        for email_msg in email_messages {
            match self.process_email_message(email_msg).await {
                Ok(processed_msg) => {
                    all_messages.push(processed_msg);
                }
                Err(e) => {
                    warn!("Failed to process email message: {}", e);
                }
            }
        }
        
        Ok(all_messages)
    }

    /// Process an incoming email message
    async fn process_email_message(&self, email_msg: SynapseEmailMessage) -> Result<SimpleMessage> {
        debug!("Processing email message from {}", email_msg.from_entity);
        
        // Convert SynapseEmailMessage to SimpleMessage
        let simple_msg = SimpleMessage {
            to: email_msg.to_entity,
            from_entity: email_msg.from_entity,
            content: email_msg.content,
            message_type: MessageType::Direct,
            metadata: std::collections::HashMap::new(),
        };
        
        // Try to parse as secure message
        if let Ok(secure_msg) = serde_json::from_str::<SecureMessage>(&simple_msg.content) {
            // Decrypt and return message  
            let crypto_manager = self.crypto.read().await;
            if let Ok(decrypted_content) = crypto_manager.decrypt_message(&secure_msg.encrypted_content) {
                let decrypted_msg = SimpleMessage {
                    to: simple_msg.to,
                    from_entity: simple_msg.from_entity,
                    content: decrypted_content,
                    message_type: simple_msg.message_type,
                    metadata: simple_msg.metadata,
                };
                return Ok(decrypted_msg);
            }
            
            // If decryption failed, return original message
            Ok(simple_msg)
        } else {
            // Return as-is if not a secure message
            Ok(simple_msg)
        }
    }

    /// Register a peer's public key
    pub async fn register_peer_key(&self, global_id: &str, public_key_pem: &str) -> Result<()> {
        let mut crypto_manager = self.crypto.write().await;
        crypto_manager.import_public_key(global_id, public_key_pem).map_err(|e| e.into())
    }

    /// Generate our own keypair
    pub async fn generate_keypair(&self) -> Result<(String, String)> {
        let mut crypto_manager = self.crypto.write().await;
        crypto_manager.generate_keypair().map_err(|e| e.into())
    }

    /// Get our global identity
    pub fn get_our_global_id(&self) -> &str {
        &self.our_global_id
    }

    /// Get router health status
    pub async fn get_health(&self) -> RouterHealth {
        RouterHealth {
            status: "healthy".to_string(),
            crypto_available: true,
            email_available: true,
            known_peers: {
                let identity_registry = self.identity.read().await;
                identity_registry.count()
            },
            known_keys: {
                let crypto_manager = self.crypto.read().await;
                crypto_manager.known_entities().len()
            },
            our_global_id: self.our_global_id.clone(),
        }
    }

    /// Convert a SimpleMessage to SecureMessage (for testing and compatibility)
    pub async fn convert_to_secure_message(&self, simple_msg: &SimpleMessage) -> Result<SecureMessage> {
        let mut secure_msg = SecureMessage {
            message_id: UuidWrapper::new(uuid::Uuid::new_v4()),
            to_global_id: simple_msg.to.clone(),
            from_global_id: self.our_global_id.clone(),
            encrypted_content: Vec::new(),
            signature: Vec::new(),
            timestamp: DateTimeWrapper::new(chrono::Utc::now()),
            security_level: SecurityLevel::Authenticated,
            routing_path: Vec::new(),
            metadata: simple_msg.metadata.clone(),
        };

        // Apply cryptographic operations if available
        {
            let crypto = self.crypto.read().await;
            
            // Try to encrypt content
            if let Ok(encrypted) = crypto.encrypt_message(&simple_msg.content, &simple_msg.to) {
                secure_msg.encrypted_content = encrypted;
                secure_msg.security_level = SecurityLevel::Secure;
            } else {
                secure_msg.encrypted_content = simple_msg.content.as_bytes().to_vec();
            }
        }

        Ok(secure_msg)
    }

    /// Start the router, initializing all transports
    pub async fn start(&self) -> Result<()> {
        info!("Starting Synapse router with global ID: {}", self.our_global_id);
        let email = self.email.read().await;
        email.start().await?;
        Ok(())
    }

    /// Stop the router gracefully
    pub async fn stop(&self) -> Result<()> {
        info!("Stopping Synapse router");
        let email = self.email.read().await;
        email.stop().await?;
        Ok(())
    }

    /// Get router status
    pub async fn status(&self) -> String {
        match self.get_health().await.status.as_str() {
            "healthy" => "Running",
            _ => "Degraded"
        }.to_string()
    }
    
    /// Register a new entity
    pub async fn register_entity(&self, global_id: &str, name: &str, profile: Option<String>) -> Result<()> {
        let identity = self.identity.write().await;
        identity.register_entity(global_id, name, profile)
    }
    
    /// Add an entity's key
    pub async fn add_entity_key(&self, global_id: &str, public_key: &str) -> Result<()> {
        let mut crypto = self.crypto.write().await;
        crypto.import_public_key(global_id, public_key)?;
        Ok(())
    }
}

/// Router health information  
#[derive(Debug, Clone)]
pub struct RouterHealth {
    pub status: String,
    pub crypto_available: bool,
    pub email_available: bool,
    pub known_peers: usize,
    pub known_keys: usize,
    pub our_global_id: String,
}

/// Check if SMTP is configured
pub async fn is_smtp_configured(router: &SynapseRouter) -> bool {
    let email_transport = router.email.read().await;
    email_transport.is_smtp_configured()
}

/// Check if IMAP is configured  
pub async fn is_imap_configured(router: &SynapseRouter) -> bool {
    let email_transport = router.email.read().await;
    email_transport.is_imap_configured()
}

/// Encrypt a message for a specific recipient
#[allow(dead_code)]
async fn encrypt_message_for_recipient(
    simple_msg: &SimpleMessage,
    destination_global_id: &str,
    crypto: &CryptoManager,
) -> Result<SecureMessage> {
    let encrypted_content = if let Ok(encrypted) = crypto.encrypt_message(&simple_msg.content, destination_global_id) {
        encrypted
    } else {
        simple_msg.content.as_bytes().to_vec()
    };
    
    let signature = crypto.sign_message(&simple_msg.content).unwrap_or_default();
    
    Ok(SecureMessage {
        message_id: UuidWrapper::new(Uuid::new_v4()),
        to_global_id: destination_global_id.to_string(),
        from_global_id: simple_msg.from_entity.clone(),
        encrypted_content,
        signature,
        timestamp: DateTimeWrapper::new(Utc::now()),
        security_level: SecurityLevel::Secure,
        routing_path: Vec::new(),
        metadata: simple_msg.metadata.clone(),
    })
}
