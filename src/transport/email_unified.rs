// SPDX-License-Identifier: MIT OR Apache-2.0
//! Email transport: SMTP send, SMTP-server receive (Direct mode); relay/smart-host SMTP send and
//! IMAP-polling receive (RelayOut/External modes).
//!
//! # Three modes, chosen at construction
//!
//! [`EmailMode`] is decided once, in [`EmailTransportImpl::new`], from
//! [`ConnectivityDetector::assess_connectivity`] -- unless the config map's `email_mode` key names
//! one explicitly (`"direct"`/`"relay_out"`/`"external"`), which skips the detector entirely. That
//! override exists for two reasons: an operator who already knows their network shape (e.g. "this
//! box is firewalled, don't bother probing") can say so directly, and it is the only way to test
//! RelayOut/External deterministically -- `ConnectivityDetector::assess_connectivity` probes fixed
//! ports (25/587/2525, 143/993/1143) that other loopback tests in the same `cargo test` run bind
//! and release, so a real assessment can flap between `RelayOnly`/`ExternalProvider` and
//! `RunLocalServer` from one run to the next.
//!
//! ## Direct mode
//!
//! Direct mode is literal direct-to-recipient SMTP: this transport connects straight to the
//! target's own mail server (its address, `host:port`, comes from [`TransportTarget::address`] --
//! in production that would be resolved via the target's MX record; in this crate's loopback
//! tests it is the peer's own `SynapseSmtpServer` port) rather than relaying through a
//! statically-configured smart host. That is why `send_message` builds a fresh, unauthenticated
//! `lettre` client per send targeting `target.address` in this mode, instead of the configured
//! `smtp_host`. This is the only mode that binds an inbound `SynapseSmtpServer`, in
//! [`EmailTransportImpl::new`] (see [`Transport::start`]'s doc on binding-in-constructor).
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
//! Because there is no configured local recipient address to filter by (see above),
//! `receive_raw` drains every message the whole store holds
//! ([`SynapseSmtpServer::drain_all_messages`]), not a single recipient's queue -- matching every
//! other transport's shape, where `receive_raw` empties this instance's own inbound queue
//! wholesale rather than filtering by identity (identity is `TransportManager`'s job, from the
//! `SecureMessage`'s signature).
//!
//! ## RelayOut and External modes
//!
//! Per spec §1a(4) (CireSnave verbatim, design doc): IMAP here exists so one server can send and
//! receive on behalf of another as a proxy, when direct SMTP isn't reachable (firewalled network,
//! corporate policy) -- never for a human to read agent traffic. Both modes share one
//! implementation, because from this transport's point of view they are the same shape: it cannot
//! run an inbound listener (`start` is a no-op; no `SynapseSmtpServer` is ever constructed), so
//! `send_message` relays through the configured `smtp_host`/`smtp_port` smart host instead of
//! dialing `target.address` directly, and `receive_raw` polls the configured
//! `imap_host`/`imap_port` mailbox over `async-imap` instead of draining an owned store. They
//! differ only in *why* a direct listener isn't available (`RelayOnly` vs `ExternalProvider`,
//! from [`ServerRecommendation`]), which changes nothing about how this transport behaves.
//!
//! `SynapseImapServer` (the only IMAP peer any test here is allowed to use -- spec §7 forbids a
//! live external provider in any test) implements no `SEARCH` command and tracks no `\Seen` flag,
//! so "fetch only unseen" is not literally available against it. `receive_raw` fetches every
//! message in the mailbox on every poll and de-duplicates client-side by `message_id`
//! ([`EmailTransportImpl::imap_seen_ids`]): nothing is ever removed from the mailbox, but nothing
//! already delivered to a caller's inbox is delivered again. A message whose fetched body does not
//! parse as a `SecureMessage` increments `receive_failures` and is skipped, not fatal to the poll
//! -- mirroring `nat_traversal.rs`'s `receive_raw` handling of an unparseable UDP datagram.

use super::abstraction::*;
use crate::{
    email_server::{
        AuthHandler, ConnectivityDetector, ServerRecommendation, SmtpServerConfig,
        SynapseSmtpServer,
    },
    error::{Result, SynapseError},
    types::{EmailConfig, ImapConfig, SecureMessage, SmtpConfig},
};
use async_trait::async_trait;
use futures::StreamExt;
use lettre::{
    SmtpTransport, Transport as LettreTransport,
    message::{Mailbox, Message, SinglePart, header},
    transport::smtp::authentication::Credentials,
};
use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc, Mutex as StdMutex,
        atomic::{AtomicU64, Ordering},
    },
};
use tokio::net::TcpListener;

/// Which of the three shapes (module docs) this transport instance is operating in, decided once
/// at construction. `RelayOut` and `External` are kept as separate variants (rather than one
/// `Proxied` variant) to preserve *why* -- useful in logs/metrics -- even though `send_message`
/// and `receive_raw` dispatch on them identically.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmailMode {
    /// Owns an inbound `SynapseSmtpServer`; sends straight to `target.address`.
    Direct,
    /// Can bind locally but not reach the internet directly (behind NAT/firewall): send relays
    /// through the configured smart host, receive polls the configured IMAP mailbox.
    RelayOut,
    /// Cannot bind an inbound listener at all: same send/receive shape as `RelayOut`.
    External,
}

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

/// Email transport, dispatching `send_message`/`start`/`receive_raw` on [`EmailMode`] (module
/// docs). Direct mode's listener is bound synchronously in [`new`](Self::new), not in
/// [`start`](Self::start): every other transport in this contract (`tcp_unified.rs`'s
/// `start_server` comment documents the bug this avoids) binds its socket during construction so
/// that, by the time `TransportManager` reports this transport `Running`, the port is actually
/// held. `start` only spawns the accept loop over the listener `new` already bound. RelayOut and
/// External modes bind no listener at all (module docs), so `smtp_server`/`listener` stay `None`.
pub struct EmailTransportImpl {
    config: EmailConfig,
    mode: EmailMode,
    smtp_server: Option<Arc<SynapseSmtpServer>>,
    listener: StdMutex<Option<TcpListener>>,
    /// `message_id`s already delivered by a RelayOut/External poll (module docs) -- unused in
    /// Direct mode, which drains its own store instead.
    imap_seen_ids: StdMutex<HashSet<uuid::Uuid>>,
    /// Real counter: incremented only where an IMAP-fetched body actually failed to parse as a
    /// `SecureMessage` (module docs). `metrics()` (Task 5) is where every transport's counters are
    /// finalized; this one exists now because Task 3's own contract requires the signal to exist.
    receive_failures: AtomicU64,
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
        let mode = Self::determine_mode(config, bind_scope).await?;

        let (smtp_server, listener) = if mode == EmailMode::Direct {
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
                    // AUTH from them. SMTP AUTH governs *relay-out submission* (RelayOut/External
                    // modes), never inbound delivery, and per the Global Constraints must never
                    // stand in for sender identity either way.
                    require_auth: false,
                    ..Default::default()
                },
                auth_handler,
                message_store,
            ));
            (Some(smtp_server), Some(listener))
        } else {
            // RelayOut/External: no inbound listener (module docs) -- send relays through
            // `email_config.smtp`, receive polls `email_config.imap`.
            (None, None)
        };

        Ok(Self {
            config: email_config,
            mode,
            smtp_server,
            listener: StdMutex::new(listener),
            imap_seen_ids: StdMutex::new(HashSet::new()),
            receive_failures: AtomicU64::new(0),
        })
    }

    /// `config`'s `email_mode` key, if present, names the mode directly (`"direct"`/
    /// `"relay_out"`/`"external"`) and skips connectivity detection entirely -- see the module
    /// docs for why (determinism in tests; an operator who already knows their network shape).
    ///
    /// Absent that override, `bind_scope` decides whether detection can say anything useful at
    /// all: `ConnectivityDetector::detect_external_ip` (spec: `network_scope.rs` module docs) is
    /// switched off entirely under `BindScope::Loopback`, so a real assessment there can *never*
    /// recommend `RunLocalServer` (`connectivity.rs`'s `determine_recommendation` has no arm that
    /// reaches it without `external_ip: Some`) -- it would always force RelayOut/External, which
    /// would silently break every loopback deployment, including Task 1's own established
    /// contract (`docs/superpowers/specs/2026-09-24-email-transport-design.md`'s Direct-mode
    /// loopback tests, e.g. `tests/transport_repairs.rs::email_direct_mode_carries_a_verified_message_end_to_end`).
    /// So `BindScope::Loopback` keeps defaulting to `Direct` without invoking the detector; only
    /// `BindScope::AllInterfaces` (a real, off-box deployment, where external-IP detection
    /// actually runs) calls [`ConnectivityDetector::assess_connectivity`] and maps its
    /// [`ServerRecommendation`] onto [`EmailMode`].
    async fn determine_mode(
        config: &HashMap<String, String>,
        bind_scope: crate::network_scope::BindScope,
    ) -> Result<EmailMode> {
        if let Some(forced) = config.get("email_mode") {
            return match forced.as_str() {
                "direct" => Ok(EmailMode::Direct),
                "relay_out" => Ok(EmailMode::RelayOut),
                "external" => Ok(EmailMode::External),
                other => Err(SynapseError::Config(format!(
                    "email_mode must be \"direct\", \"relay_out\" or \"external\", not {other:?}"
                ))),
            };
        }

        if !bind_scope.is_all_interfaces() {
            return Ok(EmailMode::Direct);
        }

        let assessment = ConnectivityDetector::default()
            .with_bind_scope(bind_scope)
            .assess_connectivity()
            .await?;
        Ok(match assessment.recommended_config {
            ServerRecommendation::RunLocalServer { .. } => EmailMode::Direct,
            ServerRecommendation::RelayOnly { .. } => EmailMode::RelayOut,
            ServerRecommendation::ExternalProvider { .. } => EmailMode::External,
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
        // Metrics finalization (messages_sent/bytes_sent/etc.) is a later task; `features` here
        // is real -- it names the mode actually in effect, not a placeholder.
        let features = match self.mode {
            EmailMode::Direct => vec!["direct_smtp".to_string()],
            EmailMode::RelayOut => vec!["relay_smtp".to_string(), "imap_poll".to_string()],
            EmailMode::External => vec!["external_smtp".to_string(), "imap_poll".to_string()],
        };
        TransportCapabilities {
            max_message_size: 10 * 1024 * 1024,
            reliable: true,
            real_time: false,
            broadcast: false,
            bidirectional: true,
            encrypted: self.config.smtp.use_tls || self.config.smtp.use_ssl,
            network_spanning: true,
            supported_urgencies: vec![MessageUrgency::Background, MessageUrgency::Batch],
            features,
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

        let smtp_client = match self.mode {
            EmailMode::Direct => {
                // Connect straight to the target's own SMTP server (its address, not a configured
                // smart host -- see module docs), unauthenticated, no TLS. `builder_dangerous` is
                // lettre's name for "plain SMTP, no implicit TLS wrapping" -- appropriate here
                // since our own `SynapseSmtpServer` speaks plain SMTP with no STARTTLS support.
                let (host, port) = Self::target_smtp_addr(target)?;
                SmtpTransport::builder_dangerous(&host).port(port).build()
            }
            EmailMode::RelayOut | EmailMode::External => {
                // No direct route to the target (module docs): relay through the configured smart
                // host/external provider instead. SMTP AUTH here governs who may submit to that
                // relay -- never a claim about the sender's identity, which is the
                // `SecureMessage` signature alone (Global Constraints, spec §3).
                if self.config.smtp.host.is_empty() {
                    return Err(SynapseError::TransportError(
                        "email RelayOut/External mode needs a configured smtp_host".to_string(),
                    ));
                }
                let mut builder = SmtpTransport::builder_dangerous(&self.config.smtp.host)
                    .port(self.config.smtp.port);
                if !self.config.smtp.username.is_empty() {
                    builder = builder.credentials(Credentials::new(
                        self.config.smtp.username.clone(),
                        self.config.smtp.password.expose().to_string(),
                    ));
                }
                builder.build()
            }
        };

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
        match self.mode {
            EmailMode::Direct => {
                let listener = self.listener.lock().unwrap().take().ok_or_else(|| {
                    SynapseError::TransportError("email transport already started".to_string())
                })?;
                let server = Arc::clone(self.smtp_server.as_ref().ok_or_else(|| {
                    SynapseError::TransportError(
                        "email transport in Direct mode has no SynapseSmtpServer (bug)".to_string(),
                    )
                })?);
                tokio::spawn(async move {
                    if let Err(e) = server.serve(listener).await {
                        tracing::warn!("email SMTP server error: {e}");
                    }
                });
            }
            EmailMode::RelayOut | EmailMode::External => {
                // No inbound listener to run (module docs): sending relays out per-call, and
                // receiving polls IMAP per-call from `receive_raw`. Nothing to spawn here.
            }
        }
        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        Ok(())
    }

    async fn status(&self) -> TransportStatus {
        TransportStatus::Running
    }

    async fn metrics(&self) -> TransportMetrics {
        // Send-side counters (messages_sent/bytes_sent/send_failures) are a later task; the
        // receive-failure counter this task adds (module docs) is real.
        TransportMetrics {
            transport_type: TransportType::Email,
            receive_failures: self.receive_failures.load(Ordering::Relaxed),
            ..Default::default()
        }
    }
}

#[async_trait]
impl TransportReceive for EmailTransportImpl {
    async fn receive_raw(&self, inbox: &mut RawInbox) -> Result<()> {
        match self.mode {
            EmailMode::Direct => {
                // Drain our own SMTP server's whole message store. There is no configured local
                // recipient address to filter by (module docs); this instance is single-tenant,
                // so everything that landed here is ours.
                let smtp_server = self.smtp_server.as_ref().ok_or_else(|| {
                    SynapseError::TransportError(
                        "email transport in Direct mode has no SynapseSmtpServer (bug)".to_string(),
                    )
                })?;
                let messages = smtp_server.drain_all_messages()?;
                for secure_message in messages {
                    inbox.push(IncomingMessage::new(
                        secure_message,
                        TransportType::Email,
                        "smtp".to_string(),
                    ));
                }
                Ok(())
            }
            EmailMode::RelayOut | EmailMode::External => self.poll_imap_inbox(inbox).await,
        }
    }
}

impl EmailTransportImpl {
    /// RelayOut/External receive (module docs): log in to the configured IMAP mailbox, select
    /// `INBOX`, fetch every message and parse each one as a `SecureMessage` -- same wire format as
    /// the SMTP path (`serde_json`, after stripping the RFC 5322 envelope with `mail_parser` the
    /// same way `smtp_server.rs`'s `handle_connection` does for its DATA body). A body that does
    /// not parse increments `receive_failures` and is skipped, not fatal to the poll (matches
    /// `nat_traversal.rs`'s `receive_raw` handling of an unparseable UDP datagram). A message
    /// already delivered on an earlier poll (tracked by `imap_seen_ids`) is skipped silently, not
    /// counted as a failure.
    async fn poll_imap_inbox(&self, inbox: &mut RawInbox) -> Result<()> {
        let imap = &self.config.imap;
        let addr = (imap.host.as_str(), imap.port);
        let stream = tokio::net::TcpStream::connect(addr).await.map_err(|e| {
            SynapseError::TransportError(format!(
                "IMAP connect to {}:{} failed: {e}",
                imap.host, imap.port
            ))
        })?;

        let client = async_imap::Client::new(stream);
        let mut session = client
            .login(&imap.username, imap.password.expose())
            .await
            .map_err(|(e, _client)| {
                SynapseError::TransportError(format!("IMAP login failed: {e}"))
            })?;

        session
            .select("INBOX")
            .await
            .map_err(|e| SynapseError::TransportError(format!("IMAP SELECT INBOX failed: {e}")))?;

        // No SEARCH support to ask for UNSEEN (module docs): fetch everything, de-dupe below.
        // Scoped in its own block so the mutable borrow `fetch_stream` holds on `session` (via
        // `std::pin::pin!`'s hidden local, which otherwise lives to the end of this function) ends
        // before `session.logout()` below needs its own `&mut session`.
        {
            let fetch_stream = session
                .fetch("1:*", "RFC822")
                .await
                .map_err(|e| SynapseError::TransportError(format!("IMAP FETCH failed: {e}")))?;
            let mut fetch_stream = std::pin::pin!(fetch_stream);

            while let Some(fetch_result) = fetch_stream.next().await {
                let fetch = match fetch_result {
                    Ok(fetch) => fetch,
                    Err(e) => {
                        self.receive_failures.fetch_add(1, Ordering::Relaxed);
                        tracing::warn!("IMAP FETCH response did not parse: {e}");
                        continue;
                    }
                };
                let Some(raw) = fetch.body() else {
                    continue;
                };

                // Same extraction as `smtp_server.rs`'s DATA handler: strip the RFC 5322 envelope
                // and reverse whatever transfer encoding `lettre` applied, rather than assuming
                // the body starts unencoded right after the first blank line.
                let body = mail_parser::MessageParser::default()
                    .parse(raw)
                    .and_then(|parsed| {
                        parsed
                            .body_text(0)
                            .map(|text| text.into_owned().into_bytes())
                    })
                    .unwrap_or_else(|| raw.to_vec());

                let secure_message: SecureMessage = match serde_json::from_slice(&body) {
                    Ok(message) => message,
                    Err(e) => {
                        self.receive_failures.fetch_add(1, Ordering::Relaxed);
                        tracing::warn!(
                            "IMAP message did not parse as a SecureMessage: {} bytes, {e}",
                            body.len()
                        );
                        continue;
                    }
                };

                let already_delivered = {
                    let mut seen = self.imap_seen_ids.lock().unwrap();
                    !seen.insert(secure_message.message_id.0)
                };
                if already_delivered {
                    continue;
                }

                inbox.push(IncomingMessage::new(
                    secure_message,
                    TransportType::Email,
                    "imap".to_string(),
                ));
            }
        }

        if let Err(e) = session.logout().await {
            tracing::debug!("IMAP LOGOUT failed (non-fatal): {e}");
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
