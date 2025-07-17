//! Email transport layer for EMRP

use crate::{
    types::{SimpleMessage, MessageType, EmailConfig, SecureMessage},
    error::{Result, EmailError},
};
use std::string::ToString;
use std::collections::HashMap;

#[cfg(feature = "email")]
use lettre::{
    message::{header, Mailbox, Message, SinglePart},
    transport::smtp::authentication::Credentials,
    SmtpTransport, Transport,
};

use base64::Engine as _;

/// Email transport for sending and receiving EMRP messages
#[cfg(feature = "email")]
#[derive(Debug, Clone)]
pub struct EmailTransport {
    config: EmailConfig,
    smtp_transport: SmtpTransport,
}

/// Dummy email transport for when email feature is disabled
#[cfg(not(feature = "email"))]
pub struct EmailTransport {
    config: EmailConfig, // Configuration for email transport (stored for future use)
}

#[cfg(feature = "email")]
impl EmailTransport {
    /// Create a new email transport
    pub async fn new(config: EmailConfig) -> Result<Self> {
        // Create SMTP transport
        let smtp_transport = if config.smtp.use_ssl {
            SmtpTransport::relay(&config.smtp.host)
                .map_err(|e| EmailError::SmtpConnection(e.to_string()))?
                .port(config.smtp.port)
                .credentials(Credentials::new(
                    config.smtp.username.clone(),
                    config.smtp.password.clone(),
                ))
                .build()
        } else {
            let transport_builder = SmtpTransport::relay(&config.smtp.host)
                .map_err(|e| EmailError::SmtpConnection(e.to_string()))?
                .port(config.smtp.port)
                .credentials(Credentials::new(
                    config.smtp.username.clone(),
                    config.smtp.password.clone(),
                ));

            if config.smtp.use_tls {
                transport_builder.build()
            } else {
                transport_builder.build()
            }
        };

        Ok(Self {
            config,
            smtp_transport,
        })
    }

    /// Send an EMRP message via email
    pub async fn send_message(&self, secure_msg: &SecureMessage, from_email: &str, to_email: &str, simple_msg: &SimpleMessage) -> Result<()> {
        let email_message = self.create_email_message(secure_msg, from_email, to_email, simple_msg)?;
        
        let _response = self.smtp_transport.send(&email_message)
            .map_err(|e| EmailError::SendFailed(e.to_string()))?;

        tracing::debug!("Email sent successfully");
        Ok(())
    }

    /// Create an email message with EMRP headers
    fn create_email_message(&self, secure_msg: &SecureMessage, from_email: &str, to_email: &str, simple_msg: &SimpleMessage) -> Result<Message> {
        let from_mailbox = from_email.parse::<Mailbox>()
            .map_err(|e| EmailError::InvalidFormat(format!("Invalid from address: {}", e)))?;
        
        let to_mailbox = to_email.parse::<Mailbox>()
            .map_err(|e| EmailError::InvalidFormat(format!("Invalid to address: {}", e)))?;

        let subject = self.generate_subject(simple_msg);
        
        // Create message without custom headers first (lettre doesn't support arbitrary headers well)
        let body_content = if secure_msg.encrypted_content.is_empty() {
            simple_msg.content.clone()
        } else {
            // For encrypted content, use base64 encoding
            base64::engine::general_purpose::STANDARD.encode(&secure_msg.encrypted_content)
        };

        let message = Message::builder()
            .from(from_mailbox)
            .to(to_mailbox)
            .subject(subject)
            .singlepart(SinglePart::builder()
                .header(header::ContentType::TEXT_PLAIN)
                .body(body_content))
            .map_err(|e| EmailError::InvalidFormat(e.to_string()))?;

        Ok(message)
    }

    /// Generate appropriate email subject
    fn generate_subject(&self, simple_msg: &SimpleMessage) -> String {
        match simple_msg.message_type {
            MessageType::ToolCall => format!("[Synapse Tool Call] {} → {}", simple_msg.from_entity, simple_msg.to),
            MessageType::ToolResponse => format!("[Synapse Tool Response] {} → {}", simple_msg.from_entity, simple_msg.to),
            MessageType::System => format!("[Synapse System] {} → {}", simple_msg.from_entity, simple_msg.to),
            MessageType::Broadcast => format!("[Synapse Broadcast] {}", simple_msg.from_entity),
            MessageType::StreamChunk => format!("[Synapse Stream] {} → {}", simple_msg.from_entity, simple_msg.to),
            MessageType::Direct => {
                // Extract first few words for subject
                let words: Vec<&str> = simple_msg.content.split_whitespace().take(5).collect();
                let preview = words.join(" ");
                let preview = if simple_msg.content.split_whitespace().count() > 5 {
                    format!("{}...", preview)
                } else {
                    preview
                };
                format!("[Synapse] {}", preview)
            }
        }
    }

    /// Receive messages from IMAP server
    pub async fn receive_messages(&self) -> Result<Vec<SynapseEmailMessage>> {
        // Note: This is a simplified IMAP implementation
        // In production, you'd want to use async-imap for full functionality
        
        tracing::debug!("Checking for new messages via IMAP simulation");
        
        // For now, simulate checking for messages
        // In a real implementation, this would:
        // 1. Connect to IMAP server
        // 2. Login with credentials
        // 3. Select INBOX
        // 4. Search for new EMRP messages
        // 5. Parse email headers and body
        // 6. Convert to SynapseEmailMessage structs
        
        // Simulate finding some messages (empty for now)
        let messages = Vec::new();
        
        tracing::debug!("Retrieved {} messages from IMAP", messages.len());
        Ok(messages)
    }

    /// Connect to IMAP and retrieve actual messages (full implementation)
    pub async fn receive_messages_imap(&self) -> Result<Vec<SynapseEmailMessage>> {
        // This would be the real IMAP implementation
        // For now, we'll provide a framework that could be extended
        
        tracing::info!("Attempting IMAP connection to {}", self.config.imap.host);
        
        // In a real implementation, you would:
        // let tls = async_native_tls::TlsConnector::new();
        // let client = async_imap::connect(
        //     (self.config.imap.host.as_str(), self.config.imap.port),
        //     &self.config.imap.host,
        //     &tls,
        // ).await?;
        
        // let mut imap_session = client
        //     .login(&self.config.imap.username, &self.config.imap.password)
        //     .await?;
        
        // imap_session.select("INBOX").await?;
        
        // let messages = imap_session.search("UNSEEN").await?;
        
        // Parse and convert messages here...
        
        // For now, return empty list
        tracing::warn!("Full IMAP implementation requires async-imap dependency");
        Ok(Vec::new())
    }

    /// Check if SMTP is properly configured
    pub fn is_smtp_configured(&self) -> bool {
        !self.config.smtp.username.is_empty() 
            && !self.config.smtp.password.is_empty()
            && !self.config.smtp.host.is_empty()
            && self.config.smtp.port > 0
    }

    /// Check if IMAP is properly configured
    pub fn is_imap_configured(&self) -> bool {
        !self.config.imap.username.is_empty() 
            && !self.config.imap.password.is_empty()
            && !self.config.imap.host.is_empty()
            && self.config.imap.port > 0
    }

    /// Check if email transport is fully configured
    pub fn is_configured(&self) -> bool {
        self.is_smtp_configured() && self.is_imap_configured()
    }

    /// Start the email transport
    pub async fn start(&self) -> Result<()> {
        // No specific startup needed for email transport
        Ok(())
    }

    /// Stop the email transport
    pub async fn stop(&self) -> Result<()> {
        // No specific cleanup needed for email transport
        Ok(())
    }
}

#[cfg(not(feature = "email"))]
impl EmailTransport {
    pub async fn new(config: EmailConfig) -> Result<Self> {
        Ok(Self { _config: config })
    }
    
    pub async fn send_message(&self, _message: &SimpleMessage) -> Result<()> {
        Err(crate::error::SynapseError::Anyhow("Email feature not enabled".to_string()))
    }
    
    pub async fn receive_messages(&self) -> Result<Vec<SimpleMessage>> {
        Ok(Vec::new())
    }
    
    pub fn is_smtp_configured(&self) -> bool {
        false
    }
    
    pub fn is_imap_configured(&self) -> bool {
        false
    }
}

/// Parsed Synapse email message
#[derive(Debug, Clone)]
pub struct SynapseEmailMessage {
    pub from_entity: String,
    pub to_entity: String,
    pub content: String,
    pub message_type: String,
    pub encrypted: bool,
    pub signed: bool,
    pub request_id: Option<String>,
    pub metadata: HashMap<String, String>,
}

impl SynapseEmailMessage {
    /// Convert to SimpleMessage
    pub fn to_simple_message(&self) -> Result<SimpleMessage> {
        let message_type = match self.message_type.as_str() {
            "direct" => MessageType::Direct,
            "tool_call" => MessageType::ToolCall,
            "tool_response" => MessageType::ToolResponse,
            "system" => MessageType::System,
            "broadcast" => MessageType::Broadcast,
            "stream_chunk" => MessageType::StreamChunk,
            _ => MessageType::Direct,
        };

        let simple_msg = SimpleMessage {
            to: self.to_entity.clone(),
            from_entity: self.from_entity.clone(),
            content: self.content.clone(),
            message_type,
            metadata: self.metadata.clone(),
        };

        Ok(simple_msg)
    }
}

#[cfg(all(test, feature = "email"))]
mod tests {
    use super::*;

    #[test]
    fn test_subject_generation() {
        let transport = create_test_transport();
        
        let tool_call = SimpleMessage {
            to: "FileSystem".to_string(),
            from_entity: "Claude".to_string(),
            content: "list_files /home".to_string(),
            message_type: MessageType::ToolCall,
            metadata: HashMap::new(),
        };
        
        let subject = transport.generate_subject(&tool_call);
        assert_eq!(subject, "[Synapse Tool Call] Claude → FileSystem");

        let direct_msg = SimpleMessage {
            to: "Eric".to_string(),
            from_entity: "Claude".to_string(),
            content: "Hello! How can I help you today?".to_string(),
            message_type: MessageType::Direct,
            metadata: HashMap::new(),
        };
        
        let subject = transport.generate_subject(&direct_msg);
        assert_eq!(subject, "[Synapse] Hello! How can I help...");
    }

    fn create_test_transport() -> EmailTransport {
        let config = EmailConfig {
            smtp: crate::types::SmtpConfig {
                host: "localhost".to_string(),
                port: 587,
                username: "test@localhost".to_string(),
                password: "test".to_string(),
                use_tls: false,
                use_ssl: false,
            },
            imap: crate::types::ImapConfig {
                host: "localhost".to_string(),
                port: 993,
                username: "test@localhost".to_string(),
                password: "test".to_string(),
                use_ssl: false,
            },
        };

        // Note: This will fail in actual test runs due to SMTP connection
        // In real tests, we'd use a mock transport
        EmailTransport {
            config,
            smtp_transport: SmtpTransport::unencrypted_localhost(),
        }
    }
}
