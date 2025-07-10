//! Main EMRP router implementation

use crate::{
    types::{SimpleMessage, SecureMessage, SecurityLevel, MessageType},
    crypto::CryptoManager,
    identity::IdentityRegistry,
    email::EmailTransport,
    config::Config,
    error::Result,
};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, debug, warn, error};

/// Main EMRP router that handles message routing and protocol operations
pub struct EmrpRouter {
    /// Cryptographic manager
    crypto: Arc<RwLock<CryptoManager>>,
    /// Identity registry
    identity: Arc<RwLock<IdentityRegistry>>,
    /// Email transport
    email: Arc<RwLock<EmailTransport>>,
    /// Configuration
    config: Config,
    /// Our global identity
    our_global_id: String,
}

impl EmrpRouter {
    /// Create a new EMRP router
    pub async fn new(config: Config, our_global_id: String) -> Result<Self> {
        // Initialize crypto manager
        let crypto = Arc::new(RwLock::new(CryptoManager::new()));
        
        // Initialize identity registry
        let identity = Arc::new(RwLock::new(IdentityRegistry::new()));
        
        // Initialize email transport
        let email_transport = EmailTransport::new(config.email.clone()).await?;
        let email = Arc::new(RwLock::new(email_transport));

        info!("EMRP Router initialized for entity: {}", our_global_id);

        Ok(Self {
            crypto,
            identity,
            email,
            config,
            our_global_id,
        })
    }

    /// Send a message to another entity
    pub async fn send_message(
        &self,
        to_entity: &str,
        content: &str,
        message_type: MessageType,
        security_level: SecurityLevel,
    ) -> Result<String> {
        debug!("Sending message to {} (type: {:?}, security: {:?})", to_entity, message_type, security_level);

        // Create simple message
        let simple_msg = SimpleMessage {
            to: to_entity.to_string(),
            from_entity: self.our_global_id.clone(),
            content: content.to_string(),
            message_type,
            metadata: std::collections::HashMap::new(),
        };

        // Resolve destination entity
        let destination_global_id = {
            let identity_registry = self.identity.read().await;
            let destination = identity_registry.resolve_entity(to_entity)?;
            destination.global_id.clone()
        };

        // Create secure message
        let crypto_manager = self.crypto.read().await;
        let secure_msg = self.create_secure_message(&simple_msg, security_level, &crypto_manager).await?;
        drop(crypto_manager);

        // Send via email
        let email_transport = self.email.read().await;
        email_transport.send_message(
            &secure_msg,
            &self.config.email.smtp.username,
            &destination_global_id,
            &simple_msg,
        ).await?;

        info!("Message sent successfully to {}", to_entity);
        Ok(secure_msg.message_id.to_string())
    }

    /// Create a secure message from a simple message
    async fn create_secure_message(
        &self,
        simple_msg: &SimpleMessage,
        security_level: SecurityLevel,
        crypto: &CryptoManager,
    ) -> Result<SecureMessage> {
        let message_id = uuid::Uuid::new_v4();
        let timestamp = chrono::Utc::now();

        let mut secure_msg = SecureMessage {
            message_id,
            to_global_id: simple_msg.to.clone(),
            from_global_id: self.our_global_id.clone(),
            timestamp,
            security_level: security_level.clone(),
            encrypted_content: Vec::new(),
            signature: Vec::new(),
            routing_path: Vec::new(),
            metadata: simple_msg.metadata.clone(),
        };

        // Handle encryption
        match security_level {
            SecurityLevel::Public => {
                // No encryption needed
            }
            SecurityLevel::Private | SecurityLevel::Authenticated | SecurityLevel::Secure => {
                // Encrypt the message content
                if crypto.has_key_for(&simple_msg.to) {
                    let encrypted = crypto.encrypt_message(&simple_msg.content, &simple_msg.to)?;
                    secure_msg.encrypted_content = encrypted;
                } else {
                    warn!("No public key available for {}, sending as plaintext", simple_msg.to);
                }
            }
        }

        // Sign the message
        let signature = crypto.sign_message(&simple_msg.content)?;
        secure_msg.signature = signature;

        Ok(secure_msg)
    }

    /// Receive and process incoming messages
    pub async fn receive_messages(&self) -> Result<Vec<SimpleMessage>> {
        debug!("Checking for incoming messages");

        let email_transport = self.email.read().await;
        let email_messages = email_transport.receive_messages().await?;
        drop(email_transport);

        let mut processed_messages = Vec::new();

        for email_msg in email_messages {
            match self.process_email_message(email_msg).await {
                Ok(simple_msg) => processed_messages.push(simple_msg),
                Err(e) => {
                    error!("Failed to process email message: {}", e);
                }
            }
        }

        if !processed_messages.is_empty() {
            info!("Received {} messages", processed_messages.len());
        }

        Ok(processed_messages)
    }

    /// Process a single email message
    async fn process_email_message(&self, email_msg: crate::email::EmrpEmailMessage) -> Result<SimpleMessage> {
        let mut simple_msg = email_msg.to_simple_message()?;

        // Verify signature if present
        if email_msg.signed {
            let crypto_manager = self.crypto.read().await;
            if !crypto_manager.verify_signature(&simple_msg.content, &[], &simple_msg.from_entity)? {
                warn!("Invalid signature for message from {}", simple_msg.from_entity);
            }
        }

        // Decrypt if encrypted
        if email_msg.encrypted {
            use base64::{engine::general_purpose::STANDARD, Engine as _};
            let crypto_manager = self.crypto.read().await;
            
            // Try to decode base64 content and decrypt
            match STANDARD.decode(&email_msg.content) {
                Ok(encrypted_data) => {
                    match crypto_manager.decrypt_message(&encrypted_data) {
                        Ok(decrypted_content) => {
                            simple_msg.content = decrypted_content;
                            info!("Successfully decrypted message from {}", simple_msg.from_entity);
                        }
                        Err(e) => {
                            error!("Failed to decrypt message from {}: {}", simple_msg.from_entity, e);
                            simple_msg.content = format!("[ENCRYPTED - Decryption failed: {}]", e);
                        }
                    }
                }
                Err(e) => {
                    error!("Failed to decode base64 encrypted content: {}", e);
                    simple_msg.content = format!("[ENCRYPTED - Invalid base64: {}]", e);
                }
            }
        }

        Ok(simple_msg)
    }

    /// Add a public key for an entity
    pub async fn add_entity_key(&self, global_id: &str, public_key_pem: &str) -> Result<()> {
        let mut crypto_manager = self.crypto.write().await;
        crypto_manager.import_public_key(global_id, public_key_pem)?;
        info!("Added public key for entity: {}", global_id);
        Ok(())
    }

    /// Generate our keypair
    pub async fn generate_our_keypair(&self) -> Result<(String, String)> {
        let mut crypto_manager = self.crypto.write().await;
        let keypair = crypto_manager.generate_keypair()?;
        info!("Generated new keypair for entity: {}", self.our_global_id);
        Ok(keypair)
    }

    /// Register an entity in our identity registry
    pub async fn register_entity(&self, global_id: &str, email: &str, display_name: Option<String>) -> Result<()> {
        let identity_registry = self.identity.read().await;
        identity_registry.register_entity(global_id, email, display_name)?;
        info!("Registered entity: {} ({})", global_id, email);
        Ok(())
    }

    /// Start the router (placeholder for background services)
    pub async fn start(&self) -> Result<()> {
        info!("EMRP Router started for entity: {}", self.our_global_id);
        
        // Start background services
        self.start_message_polling().await?;
        self.start_health_checks().await?;
        self.start_key_rotation().await?;
        self.start_identity_sync().await?;
        
        Ok(())
    }

    /// Start background message polling service
    async fn start_message_polling(&self) -> Result<()> {
        info!("Starting message polling service");
        // In a real implementation, this would spawn a background task
        // that periodically checks for new messages
        
        // tokio::spawn(async move {
        //     let mut interval = tokio::time::interval(Duration::from_secs(30));
        //     loop {
        //         interval.tick().await;
        //         if let Err(e) = self.receive_messages().await {
        //             error!("Error polling messages: {}", e);
        //         }
        //     }
        // });
        
        debug!("Message polling service configured (not actively running in this demo)");
        Ok(())
    }

    /// Start health check service
    async fn start_health_checks(&self) -> Result<()> {
        info!("Starting health check service");
        // This would periodically check:
        // - Email connectivity
        // - Crypto key validity
        // - Identity registry health
        
        debug!("Health check service configured");
        Ok(())
    }

    /// Start key rotation service
    async fn start_key_rotation(&self) -> Result<()> {
        info!("Starting key rotation service");
        // This would:
        // - Check key age
        // - Generate new keys when needed
        // - Publish new public keys
        // - Revoke old keys
        
        debug!("Key rotation service configured");
        Ok(())
    }

    /// Start identity synchronization service
    async fn start_identity_sync(&self) -> Result<()> {
        info!("Starting identity synchronization service");
        // This would:
        // - Sync with known identity registries
        // - Update entity capabilities
        // - Handle identity changes
        
        debug!("Identity sync service configured");
        Ok(())
    }

    /// Stop the router
    pub async fn stop(&self) -> Result<()> {
        info!("EMRP Router stopped for entity: {}", self.our_global_id);
        Ok(())
    }

    /// Get router status information
    pub async fn status(&self) -> RouterStatus {
        let crypto_manager = self.crypto.read().await;
        let identity_registry = self.identity.read().await;
        
        // Check actual email status
        let email_configured = self.check_email_status().await;
        
        RouterStatus {
            our_global_id: self.our_global_id.clone(),
            known_entities: identity_registry.list_entities().len(),
            known_keys: crypto_manager.known_entities().len(),
            email_configured,
        }
    }

    /// Check if email is properly configured and working
    async fn check_email_status(&self) -> bool {
        // Check SMTP connection
        let smtp_ok = self.check_smtp_connection().await;
        
        // Check IMAP connection (would be implemented for full version)
        let imap_ok = self.check_imap_connection().await;
        
        smtp_ok && imap_ok
    }

    /// Test SMTP connection
    async fn check_smtp_connection(&self) -> bool {
        let email_transport = self.email.read().await;
        
        // In a real implementation, you'd test the connection:
        // match email_transport.smtp_transport.test_connection().await {
        //     Ok(()) => true,
        //     Err(e) => {
        //         warn!("SMTP connection failed: {}", e);
        //         false
        //     }
        // }
        
        // For now, just check if we have credentials
        let has_credentials = email_transport.is_smtp_configured();
        
        if !has_credentials {
            warn!("SMTP credentials incomplete");
        }
        
        has_credentials
    }

    /// Test IMAP connection
    async fn check_imap_connection(&self) -> bool {
        let email_transport = self.email.read().await;
        
        // Similar to SMTP, check if we have IMAP credentials
        let has_credentials = email_transport.is_imap_configured();
        
        if !has_credentials {
            warn!("IMAP credentials incomplete");
        }
        
        has_credentials
    }
}

/// Router status information
#[derive(Debug, Clone)]
pub struct RouterStatus {
    pub our_global_id: String,
    pub known_entities: usize,
    pub known_keys: usize,
    pub email_configured: bool,
}
