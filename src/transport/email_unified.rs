// SPDX-License-Identifier: MIT OR Apache-2.0
//! Email transport: SMTP send, SMTP-server receive (Direct mode). Relay-out/External modes and
//! IMAP polling arrive in a later task.
//!
//! # Direct mode
//!
//! Direct mode is literal direct-to-recipient SMTP: this transport connects straight to the
//! target's own mail server (its address, `host:port`, comes from [`TransportTarget::address`] --
//! in production that would be resolved via the target's MX record; in this crate's loopback
//! tests it is the peer's own `SynapseSmtpServer` port) rather than relaying through a
//! statically-configured smart host. That is why `send_message` builds a fresh, unauthenticated
//! `lettre` client per send targeting `target.address`, instead of reusing one client built once
//! in [`EmailTransportImpl::new`] against a configured `smtp_host` -- there is no single
//! "our-account" SMTP host to relay through in Direct mode; `smtp_host`/`smtp_port`/
//! `smtp_username`/`smtp_password` in [`EmailConfig`] are for the smart-host relay a later task
//! adds, not for Direct mode.
//!
//! Per the Global Constraints (spec §3): this transport never bases any trust or authorization
//! decision on `MAIL FROM`, `From:`, or the connecting client's address. Sender authentication is
//! the `SecureMessage` signature alone, verified downstream by `TransportManager`. The SMTP
//! envelope's `MAIL FROM`/`RCPT TO` are accepted unconditionally by this transport's own
//! `SynapseSmtpServer` instance -- see [`AcceptAllAuthHandler`] -- because this is a private,
//! single-tenant server (one `EmailTransportImpl` per bound port, not a shared multi-user mail
//! server), so there is no local recipient identity to check against, and even if there were,
//! checking it would not be a security control (spec §3 forbids treating it as one).
//!
//! # Receiving
//!
//! Because there is no configured local recipient address to filter by (see above),
//! `receive_raw` drains every message the whole store holds
//! ([`SynapseSmtpServer::drain_all_messages`]), not a single recipient's queue -- matching every
//! other transport's shape, where `receive_raw` empties this instance's own inbound queue
//! wholesale rather than filtering by identity (identity is `TransportManager`'s job, from the
//! `SecureMessage`'s signature).

use super::abstraction::*;
use crate::{
    email_server::{AuthHandler, SmtpServerConfig, SynapseSmtpServer},
    error::{Result, SynapseError},
    types::{EmailConfig, ImapConfig, SecureMessage, SmtpConfig},
};
use async_trait::async_trait;
use lettre::{
    SmtpTransport, Transport as LettreTransport,
    message::{Mailbox, Message, SinglePart, header},
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex as StdMutex},
};
use tokio::net::TcpListener;

/// The one address validator `can_reach`/`test_connectivity` use: exactly one `@`, a non-empty
/// local part, and a domain containing a `.` with non-empty labels on each side of it. Ported from
/// `email_simple.rs`'s validator of the same name (word for word) rather than reinvented, since a
/// looser check here (e.g. "just contains `@`") would accept malformed addresses like `"invalid@"`
/// that the old `SimpleEmailTransport` correctly refused --
/// `tests/transport_error_handling_test.rs::test_transport_error_handling` exercises exactly this.
fn valid_address(address: &str) -> bool {
    let mut parts = address.split('@');
    let Some(local) = parts.next() else {
        return false;
    };
    let Some(domain) = parts.next() else {
        return false;
    };
    if parts.next().is_some() {
        // More than one '@'.
        return false;
    }
    if local.is_empty() || domain.is_empty() {
        return false;
    }
    domain.contains('.') && domain.split('.').all(|label| !label.is_empty())
}

/// Accepts every SMTP envelope unconditionally. This transport's `SynapseSmtpServer` is a
/// private, single-tenant listener (see module docs): nothing about `MAIL FROM`/`RCPT TO` is ever
/// used for a security or routing decision here, per the Global Constraints (spec §3) that forbid
/// treating SMTP-level identity as meaningful for anything security-relevant. `authenticate` is
/// never reached because `SmtpServerConfig::require_auth` is `false` for Direct mode: real
/// senders on the internet are not expected to authenticate to deliver mail directly, the same as
/// any ordinary receiving mail server.
struct AcceptAllAuthHandler;

impl AuthHandler for AcceptAllAuthHandler {
    fn authenticate(&self, _username: &str, _password: &str) -> Result<bool> {
        Ok(true)
    }

    fn is_authorized_sender(&self, _email: &str) -> Result<bool> {
        Ok(true)
    }

    fn is_authorized_recipient(&self, _email: &str) -> Result<bool> {
        Ok(true)
    }
}

/// Direct-mode email transport: sends via `lettre` straight to the target's own SMTP server
/// (`target.address`), receives via its own `SynapseSmtpServer` listening on `local_port`.
///
/// The listener is bound synchronously in [`new`](Self::new), not in [`start`](Self::start): every
/// other transport in this contract (`tcp_unified.rs`'s `start_server` comment documents the bug
/// this avoids) binds its socket during construction so that, by the time `TransportManager`
/// reports this transport `Running`, the port is actually held. `start` only spawns the accept
/// loop over the listener `new` already bound.
pub struct EmailTransportImpl {
    config: EmailConfig,
    smtp_server: Arc<SynapseSmtpServer>,
    listener: StdMutex<Option<TcpListener>>,
}

impl EmailTransportImpl {
    pub async fn new(config: &HashMap<String, String>) -> Result<Self> {
        let local_port: u16 = config
            .get("local_port")
            .map(|p| p.parse())
            .transpose()
            .map_err(|_| SynapseError::Config("invalid local_port: must be a u16".to_string()))?
            .unwrap_or(0);
        let bind_scope = crate::network_scope::BindScope::from_config_map(config)?;

        let email_config = Self::email_config_from_map(config)?;

        let bind_addr = bind_scope.listen_addr(local_port);
        let listener = TcpListener::bind(bind_addr).await.map_err(|e| {
            SynapseError::NetworkError(format!(
                "failed to bind email transport's SMTP server to {bind_addr}: {e}"
            ))
        })?;

        let auth_handler: Arc<dyn AuthHandler + Send + Sync> = Arc::new(AcceptAllAuthHandler);
        let message_store = Arc::new(StdMutex::new(HashMap::new()));
        let smtp_server = Arc::new(SynapseSmtpServer::new(
            SmtpServerConfig {
                port: local_port,
                bind_scope,
                // Direct mode receives mail addressed to us from arbitrary senders on the
                // internet, the same as any ordinary mail server -- it does not require SMTP
                // AUTH from them. SMTP AUTH governs *relay-out submission* (a later task), never
                // inbound delivery, and per the Global Constraints must never stand in for
                // sender identity either way.
                require_auth: false,
                ..Default::default()
            },
            auth_handler,
            message_store,
        ));

        Ok(Self {
            config: email_config,
            smtp_server,
            listener: StdMutex::new(Some(listener)),
        })
    }

    /// Build `EmailConfig` from the factory's `HashMap<String, String>` config, the same shape
    /// every other `_unified.rs` factory takes. All keys are optional in Task 1: Direct mode sends
    /// straight to `target.address`, needing no configured SMTP account, and does not use IMAP at
    /// all, so nothing here is load-bearing yet. They exist so a later task's relay/proxy modes
    /// can populate the same `EmailConfig` without a second, separate parsing path. Config keys:
    /// `smtp_host`, `smtp_port` (default 25), `smtp_username`, `smtp_password`, `smtp_use_tls`,
    /// `smtp_use_ssl`, `imap_host`, `imap_port` (default 143), `imap_username`, `imap_password`,
    /// `imap_use_ssl`.
    fn email_config_from_map(config: &HashMap<String, String>) -> Result<EmailConfig> {
        fn optional(config: &HashMap<String, String>, key: &str) -> String {
            config.get(key).cloned().unwrap_or_default()
        }
        fn port(config: &HashMap<String, String>, key: &str, default: u16) -> Result<u16> {
            match config.get(key) {
                None => Ok(default),
                Some(v) if v.is_empty() => Ok(default),
                Some(v) => v.parse().map_err(|_| {
                    SynapseError::Config(format!("{key} must be a valid port number"))
                }),
            }
        }
        fn flag(config: &HashMap<String, String>, key: &str) -> bool {
            config.get(key).map(|v| v == "true").unwrap_or(false)
        }

        Ok(EmailConfig {
            smtp: SmtpConfig {
                host: optional(config, "smtp_host"),
                port: port(config, "smtp_port", 25)?,
                username: optional(config, "smtp_username"),
                password: crate::types::SecretString::new(optional(config, "smtp_password")),
                use_tls: flag(config, "smtp_use_tls"),
                use_ssl: flag(config, "smtp_use_ssl"),
            },
            imap: ImapConfig {
                host: optional(config, "imap_host"),
                port: port(config, "imap_port", 143)?,
                username: optional(config, "imap_username"),
                password: crate::types::SecretString::new(optional(config, "imap_password")),
                use_ssl: flag(config, "imap_use_ssl"),
            },
        })
    }

    /// Split `host:port` out of a [`TransportTarget`]'s address, the peer's SMTP server for
    /// Direct-mode delivery. Direct mode has no other source for this: there is no configured
    /// smart host to fall back to (see module docs), so a target with no address, or a malformed
    /// one, is refused rather than silently guessing a host.
    fn target_smtp_addr(target: &TransportTarget) -> Result<(String, u16)> {
        let addr = target.address.as_ref().ok_or_else(|| {
            SynapseError::TransportError(format!(
                "email Direct mode needs target.address (host:port of the recipient's SMTP \
                 server); {} has none",
                target.identifier
            ))
        })?;
        let (host, port_str) = addr.rsplit_once(':').ok_or_else(|| {
            SynapseError::TransportError(format!("email target address {addr:?} is not host:port"))
        })?;
        let port: u16 = port_str.parse().map_err(|_| {
            SynapseError::TransportError(format!(
                "email target address {addr:?} has an invalid port"
            ))
        })?;
        Ok((host.to_string(), port))
    }
}

#[async_trait]
impl Transport for EmailTransportImpl {
    fn transport_type(&self) -> TransportType {
        TransportType::Email
    }

    fn capabilities(&self) -> TransportCapabilities {
        // A later task finalizes this; this skeleton only needs enough to pass its own test.
        TransportCapabilities {
            max_message_size: 10 * 1024 * 1024,
            reliable: true,
            real_time: false,
            broadcast: false,
            bidirectional: true,
            encrypted: self.config.smtp.use_tls || self.config.smtp.use_ssl,
            network_spanning: true,
            supported_urgencies: vec![MessageUrgency::Background, MessageUrgency::Batch],
            features: vec!["direct_smtp".to_string()],
            unmeasured_metrics: vec![
                UnmeasuredMetric::AverageLatency,
                UnmeasuredMetric::ReliabilityScore,
            ],
        }
    }

    async fn can_reach(&self, target: &TransportTarget) -> bool {
        valid_address(&target.identifier)
    }

    async fn estimate_metrics(&self, target: &TransportTarget) -> Result<TransportEstimate> {
        let available = self.can_reach(target).await;
        Ok(TransportEstimate {
            latency: std::time::Duration::from_secs(5),
            reliability: if available { 0.5 } else { 0.0 },
            bandwidth: 0,
            cost: 0.1,
            available,
            confidence: 0.3,
        })
    }

    async fn send_message(
        &self,
        target: &TransportTarget,
        message: &SecureMessage,
    ) -> Result<DeliveryReceipt> {
        let start = std::time::Instant::now();
        let to_email = &target.identifier;

        // Whole SecureMessage as JSON -- never hand-picked fields (Global Constraints, spec §4).
        let body = serde_json::to_vec(message)
            .map_err(|e| SynapseError::TransportError(format!("serialize failed: {e}")))?;

        let from_mailbox: Mailbox = "synapse-agent@localhost"
            .parse()
            .map_err(|e| SynapseError::TransportError(format!("invalid from address: {e}")))?;
        let to_mailbox: Mailbox = to_email
            .parse()
            .map_err(|e| SynapseError::TransportError(format!("invalid to address: {e}")))?;

        let email = Message::builder()
            .from(from_mailbox)
            .to(to_mailbox)
            .subject("Synapse")
            .singlepart(
                SinglePart::builder()
                    .header(header::ContentType::TEXT_PLAIN)
                    .body(String::from_utf8_lossy(&body).into_owned()),
            )
            .map_err(|e| SynapseError::TransportError(format!("build failed: {e}")))?;

        // Direct mode: connect straight to the target's own SMTP server (its address, not a
        // configured smart host -- see module docs), unauthenticated, no TLS. `builder_dangerous`
        // is lettre's name for "plain SMTP, no implicit TLS wrapping" -- appropriate here since
        // our own `SynapseSmtpServer` speaks plain SMTP with no STARTTLS support.
        let (host, port) = Self::target_smtp_addr(target)?;
        let smtp_client = SmtpTransport::builder_dangerous(&host).port(port).build();

        LettreTransport::send(&smtp_client, &email)
            .map_err(|e| SynapseError::TransportError(format!("SMTP send failed: {e}")))?;

        Ok(DeliveryReceipt {
            message_id: message.message_id.0.to_string(),
            transport_used: TransportType::Email,
            delivery_time: start.elapsed(),
            target_reached: to_email.clone(),
            confirmation: DeliveryConfirmation::Sent,
            metadata: HashMap::new(),
        })
    }

    async fn test_connectivity(&self, target: &TransportTarget) -> Result<ConnectivityResult> {
        Ok(ConnectivityResult {
            connected: self.can_reach(target).await,
            rtt: None,
            error: None,
            quality: 0.5,
            details: HashMap::new(),
        })
    }

    async fn start(&self) -> Result<()> {
        let listener = self.listener.lock().unwrap().take().ok_or_else(|| {
            SynapseError::TransportError("email transport already started".to_string())
        })?;
        let server = Arc::clone(&self.smtp_server);
        tokio::spawn(async move {
            if let Err(e) = server.serve(listener).await {
                tracing::warn!("email SMTP server error: {e}");
            }
        });
        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        Ok(())
    }

    async fn status(&self) -> TransportStatus {
        TransportStatus::Running
    }

    async fn metrics(&self) -> TransportMetrics {
        // A later task finalizes this with real counters.
        TransportMetrics {
            transport_type: TransportType::Email,
            ..Default::default()
        }
    }
}

#[async_trait]
impl TransportReceive for EmailTransportImpl {
    async fn receive_raw(&self, inbox: &mut RawInbox) -> Result<()> {
        // Direct mode: drain our own SMTP server's whole message store. There is no configured
        // local recipient address to filter by (module docs); this instance is single-tenant, so
        // everything that landed here is ours.
        let messages = self.smtp_server.drain_all_messages()?;
        for secure_message in messages {
            inbox.push(IncomingMessage::new(
                secure_message,
                TransportType::Email,
                "smtp".to_string(),
            ));
        }
        Ok(())
    }
}

pub struct EmailTransportFactory;

#[async_trait]
impl TransportFactory for EmailTransportFactory {
    async fn create_transport(
        &self,
        config: &HashMap<String, String>,
    ) -> Result<Box<dyn Transport>> {
        Ok(Box::new(EmailTransportImpl::new(config).await?))
    }

    fn transport_type(&self) -> TransportType {
        TransportType::Email
    }

    fn default_config(&self) -> HashMap<String, String> {
        HashMap::new()
    }

    fn validate_config(&self, _config: &HashMap<String, String>) -> Result<()> {
        Ok(())
    }
}
