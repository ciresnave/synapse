// SPDX-License-Identifier: MIT OR Apache-2.0
use crate::error::EmailError;
use crate::types::MessageType;
/// Email transport layer for EMRP
use crate::{
    error::Result,
    types::{EmailConfig, SimpleMessage},
};
use std::collections::HashMap;
use std::string::ToString;

use lettre::{SmtpTransport, transport::smtp::authentication::Credentials};

/// Email transport for sending and receiving EMRP messages
#[derive(Debug, Clone)]
pub struct EmailTransport {
    config: EmailConfig,
    // No longer read: `send_message` now refuses unconditionally rather than ever sending an
    // unsigned message (see its doc comment), so this crate has no code path left that hands a
    // message to it. Kept on the struct rather than removed -- dropping it is the kind of
    // larger, unrelated restructuring this fix explicitly stays out of.
    #[allow(dead_code)]
    smtp_transport: SmtpTransport,
}

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
                    config.smtp.password.expose().to_string(),
                ))
                .build()
        } else {
            let transport_builder = SmtpTransport::relay(&config.smtp.host)
                .map_err(|e| EmailError::SmtpConnection(e.to_string()))?
                .port(config.smtp.port)
                .credentials(Credentials::new(
                    config.smtp.username.clone(),
                    config.smtp.password.expose().to_string(),
                ));

            transport_builder.build()
        };

        Ok(Self {
            config,
            smtp_transport,
        })
    }

    /// Send an EMRP message via email
    ///
    /// `EmailTransport` holds no `CryptoManager` or keypair, so it has no way to produce a real
    /// signature. The project owner's ruling is absolute -- "no messages should ever be sent
    /// unsigned" / "messages must never be sent unsigned" -- and explicitly rejects "it has no
    /// live caller" as a defense. Per the owner's own fallback instruction for exactly this case
    /// (signing genuinely out of reach for this type), this function refuses to send rather than
    /// ever construct or transmit an unsigned `SecureMessage`. Callers that need to send email
    /// should go through `SynapseRouter` (`src/router_merged.rs`), which routes sending through
    /// `EmailTransportImpl` and signs via `sign_new_message` before any transport is touched.
    pub async fn send_message(&self, _simple_msg: &SimpleMessage) -> Result<()> {
        Err(EmailError::SendFailed(
            "EmailTransport::send_message cannot produce a signed message -- this type holds no \
             CryptoManager or keypair, and this crate's policy is that no message may ever be \
             sent unsigned. Use SynapseRouter (src/router_merged.rs), which routes all sending \
             through EmailTransportImpl and signs via sign_new_message before any transport is \
             touched."
                .to_string(),
        ))
    }

    /// Generate appropriate email subject
    ///
    /// No longer called in production code: its only caller, `create_email_message`, was removed
    /// along with the unsigned-send path in `send_message` (see that function's doc comment).
    /// Kept and still covered by `test_subject_generation` since this crate's future signed email
    /// path (via `SynapseRouter`) is expected to want the same subject formatting.
    #[allow(dead_code)]
    fn generate_subject(&self, simple_msg: &SimpleMessage) -> String {
        match simple_msg.message_type {
            MessageType::ToolCall => format!(
                "[Synapse Tool Call] {} → {}",
                simple_msg.from_entity, simple_msg.to
            ),
            MessageType::ToolResponse => format!(
                "[Synapse Tool Response] {} → {}",
                simple_msg.from_entity, simple_msg.to
            ),
            MessageType::System => format!(
                "[Synapse System] {} → {}",
                simple_msg.from_entity, simple_msg.to
            ),
            MessageType::Broadcast => format!("[Synapse Broadcast] {}", simple_msg.from_entity),
            MessageType::StreamChunk => format!(
                "[Synapse Stream] {} → {}",
                simple_msg.from_entity, simple_msg.to
            ),
            MessageType::Direct => {
                // Extract first few words for subject
                let words: Vec<&str> = simple_msg.content.split_whitespace().take(5).collect();
                let preview = words.join(" ");
                let preview = if simple_msg.content.split_whitespace().count() > 5 {
                    format!("{preview}...")
                } else {
                    preview
                };
                format!("[Synapse] {preview}")
            }
        }
    }

    /// Receive messages from IMAP server
    ///
    /// Out of scope for the transport-contract sealing (Task 4): this is the email path, which a
    /// later slice replaces.
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
    ///
    /// Out of scope for the transport-contract sealing (Task 4): this is the email path, which a
    /// later slice replaces.
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
            && !self.config.smtp.password.expose().is_empty()
            && !self.config.smtp.host.is_empty()
            && self.config.smtp.port > 0
    }

    /// Check if IMAP is properly configured
    pub fn is_imap_configured(&self) -> bool {
        !self.config.imap.username.is_empty()
            && !self.config.imap.password.expose().is_empty()
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

#[cfg(test)]
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

    #[tokio::test]
    async fn send_message_refuses_rather_than_send_unsigned() {
        // Per the project owner's ruling ("no messages should ever be sent unsigned" /
        // "messages must never be sent unsigned"), EmailTransport::send_message must never
        // construct or transmit a SecureMessage carrying SenderProof::unsigned(). This type has
        // no CryptoManager/keypair, so it cannot sign -- it must refuse instead.
        //
        // The old (pre-fix) body built the unsigned SecureMessage regardless, then tried to
        // hand it to lettre's SmtpTransport, which -- pointed at an address nothing is
        // listening on -- fails with a *connection* error whose text never mentions signing.
        // That's the born-red signal: it's an Err either way, but for the wrong reason. The
        // fixed body must fail for the *right* reason, before ever touching the network or
        // building a SecureMessage.
        let transport = create_test_transport();

        let msg = SimpleMessage {
            to: "bob@example.com".to_string(),
            from_entity: "alice@example.com".to_string(),
            content: "hello".to_string(),
            message_type: MessageType::Direct,
            metadata: HashMap::new(),
        };

        let err = transport
            .send_message(&msg)
            .await
            .expect_err("send_message must refuse rather than ever send unsigned");

        let text = err.to_string();
        assert!(
            text.contains("cannot produce a signed message"),
            "expected the refusal reason (no signing capability), got: {text}"
        );
        assert!(
            !text.contains("Send failed") || text.contains("cannot produce a signed message"),
            "must not be the generic SMTP send-failure path: {text}"
        );
    }

    fn create_test_transport() -> EmailTransport {
        let config = EmailConfig {
            smtp: crate::types::SmtpConfig {
                host: "localhost".to_string(),
                port: 587,
                username: "test@localhost".to_string(),
                password: crate::types::SecretString::new("test"),
                use_tls: false,
                use_ssl: false,
            },
            imap: crate::types::ImapConfig {
                host: "localhost".to_string(),
                port: 993,
                username: "test@localhost".to_string(),
                password: crate::types::SecretString::new("test"),
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
