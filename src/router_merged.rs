// SPDX-License-Identifier: MIT OR Apache-2.0
//! The merged Synapse router: `SynapseRouter` + `EnhancedSynapseRouter`'s combined API, per
//! docs/superpowers/specs/2026-09-25-router-merge-design.md. No compatibility shim for the two
//! old names -- CireSnave's ruling was that keeping them separate was itself the wrong call.
//!
//! Task 1 scope: get this struct compiling with both old APIs' state present, with `start()` and
//! `status()` resolving the two types' name collisions. The `email` field is a placeholder
//! (`Arc<RwLock<Option<Arc<dyn Transport>>>>`) until Task 2 wires in a real `EmailTransportImpl` --
//! this task does not attempt to make email sending/receiving work, to avoid duplicating Task 2's
//! work and creating a merge conflict with it. `multi_transport` and `email_server` are likewise
//! left unpopulated (`None`/`false`) in `new()`: their real initialization involves network I/O
//! (`MultiTransportRouter::new`, `SynapseEmailServer::new_with_scope`) that later tasks are
//! expected to wire in, matching this same plan's own Step 4 code sample, which hard-codes
//! `multi_transport: None` in `new()` even though the field is declared.

use crate::{
    CryptoManager,
    config::Config,
    email_server::SynapseEmailServer,
    error::Result,
    identity::IdentityRegistry,
    router::RouterHealth,
    sender_auth::TrustStore,
    transport::{abstraction::MessageUrgency, router::MultiTransportRouter},
    types::{MessageType, SecureMessage, SecurityLevel, SimpleMessage},
};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

pub struct SynapseRouter {
    /// Cryptographic manager
    crypto: Arc<RwLock<CryptoManager>>,
    /// Identity registry
    identity: Arc<RwLock<IdentityRegistry>>,
    /// Email transport, lazily constructed by `ensure_email_transport` on first use. This is NOT
    /// the old `src/email.rs::EmailTransport` type -- it is a real `EmailTransportImpl`
    /// (`src/transport/email_unified.rs`), reached through the same
    /// `TransportProvider::create_email_transport` bridge `MultiTransportRouter` uses.
    email: Arc<RwLock<Option<Arc<dyn crate::transport::abstraction::Transport>>>>,
    /// Pinned sender keys for receive-side verification (`receive_messages`, Task 4). The
    /// authenticity check itself is the SAME mechanism `TransportManager` uses for every other
    /// transport in this crate (`transport::manager::TransportManager::receive_messages` gates on
    /// `TrustStore::verify_at`), not a second, weaker, router-specific re-implementation of that
    /// check -- see `receive_messages`'s doc comment for why `IdentityRegistry` (below) was not
    /// used for this. That said, `verify_at` is only one of the two gates
    /// `TransportManager::receive_messages` runs a message through; this router's overall receive
    /// guarantee is narrower than `TransportManager`'s -- see `receive_messages`'s doc comment for
    /// the gap (no anti-replay check, no unsealing).
    trust_store: Arc<RwLock<TrustStore>>,
    /// Multi-transport router for fast communication
    multi_transport: Option<Arc<MultiTransportRouter>>,
    /// Local email server (SMTP/IMAP) for when we're externally accessible
    email_server: Option<Arc<SynapseEmailServer>>,
    /// Configuration, used by `ensure_email_transport` to build the real
    /// `EmailTransportImpl` via `ProductionTransportProvider::create_email_transport`.
    config: Config,
    /// Our global identity
    our_global_id: String,
    /// Enable multi-transport features
    multi_transport_enabled: bool,
    /// Email server enabled
    email_server_enabled: bool,
}

impl SynapseRouter {
    /// Create a new Synapse router with both the original and enhanced APIs available.
    pub async fn new(config: Config, our_global_id: String) -> Result<Self> {
        // Initialize crypto manager
        let crypto = Arc::new(RwLock::new(CryptoManager::new()));
        // Initialize identity registry
        let identity = Arc::new(RwLock::new(IdentityRegistry::new()));

        Ok(Self {
            crypto,
            identity,
            email: Arc::new(RwLock::new(None)),
            trust_store: Arc::new(RwLock::new(TrustStore::new())),
            multi_transport: None,
            email_server: None,
            config,
            our_global_id,
            multi_transport_enabled: false,
            email_server_enabled: false,
        })
    }

    /// Start all router services.
    ///
    /// Old `SynapseRouter::start()`'s body was only ever reachable through a commented-out call
    /// in old `EnhancedSynapseRouter::start()` (`// self.synapse_router.start().await?;`,
    /// `router_enhanced.rs:619` at the time this plan was written) -- that call is now live and
    /// inlined below, since both bodies belong to the same struct.
    pub async fn start(&self) -> Result<()> {
        info!("Starting enhanced Synapse router");

        // Old `SynapseRouter::start()`'s body, now live.
        info!(
            "Starting Synapse router with global ID: {}",
            self.our_global_id
        );

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

    /// Get our global identity
    pub fn get_our_global_id(&self) -> &str {
        &self.our_global_id
    }

    /// Get enhanced router status including email server.
    pub async fn status(&self) -> EnhancedRouterStatus {
        // Mirrors old `SynapseRouter::get_health()`'s body.
        let synapse_status = RouterHealth {
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
        };

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

    /// The ONE place a `SecureMessage` is built and signed for sending. Every send path in this
    /// router -- `send_message`, `send_message_smart`'s fast branch (Task 3),
    /// `send_message_with_transport` -- calls this, so no path can reach a transport with an
    /// unsigned message by forgetting to call `sign_secure_message` itself. This is the
    /// structural fix for board item 65 (the old `EnhancedSynapseRouter::create_secure_message`
    /// hardcoded `SenderProof::unsigned()` and never signed at all).
    async fn sign_new_message(
        &self,
        to_global_id: &str,
        content: &[u8],
        security_level: SecurityLevel,
    ) -> Result<SecureMessage> {
        let mut message = SecureMessage::new(
            to_global_id.to_string(),
            self.our_global_id.clone(),
            content.to_vec(),
            security_level,
        );
        let crypto = self.crypto.read().await;
        crypto.sign_secure_message(&mut message)?;
        Ok(message)
    }

    /// Constructs this router's `EmailTransportImpl` on first use, via the same
    /// `TransportProvider::create_email_transport` bridge `MultiTransportRouter` already uses
    /// (`src/transport/providers.rs:114`) -- not a second, separate construction path.
    async fn ensure_email_transport(
        &self,
    ) -> Result<Arc<dyn crate::transport::abstraction::Transport>> {
        {
            let existing = self.email.read().await;
            if let Some(t) = existing.as_ref() {
                return Ok(Arc::clone(t));
            }
        }
        use crate::transport::providers::TransportProvider as _;
        let provider = crate::transport::providers::ProductionTransportProvider;
        let transport = provider
            .create_email_transport(&self.config)
            .await?
            .ok_or_else(|| {
                crate::error::SynapseError::TransportError(
                    "email transport construction returned None".to_string(),
                )
            })?;
        transport.start().await?;
        let mut slot = self.email.write().await;
        *slot = Some(Arc::clone(&transport));
        Ok(transport)
    }

    /// Email-only send, no transport selection -- the explicit low-level path old `SynapseRouter`
    /// callers reached for. Routes through `EmailTransportImpl` (PR #55), never `src/email.rs`'s
    /// `EmailTransport` and never a `SimpleMessage`-round-tripped-through-JSON-string.
    pub async fn send_message(
        &self,
        simple_msg: SimpleMessage,
        destination_global_id: String,
    ) -> Result<()> {
        let message = self
            .sign_new_message(
                &destination_global_id,
                simple_msg.content.as_bytes(),
                SecurityLevel::Authenticated,
            )
            .await?;
        let transport = self.ensure_email_transport().await?;
        let target = crate::transport::abstraction::TransportTarget::new(destination_global_id);
        transport.send_message(&target, &message).await?;
        Ok(())
    }

    /// Smart send: prefers the fast `MultiTransportRouter` path for real-time/interactive
    /// urgency, falling back to email otherwise or on failure. Old `EnhancedSynapseRouter`'s
    /// name and signature, ported here.
    ///
    /// Fixed (board item 65): the fast branch used to call the old
    /// `EnhancedSynapseRouter::create_secure_message`, which hardcoded `SenderProof::unsigned()`
    /// and never touched the crypto manager -- the opposite of the email path, which signs. That
    /// helper is not ported at all; this now calls the same `sign_new_message` every other send
    /// path uses, so no path can reach a transport unsigned.
    pub async fn send_message_smart(
        &self,
        to_entity: &str,
        content: &str,
        message_type: MessageType,
        security_level: SecurityLevel,
        urgency: MessageUrgency,
    ) -> Result<String> {
        if let Some(ref mt_router) = self.multi_transport
            && matches!(
                urgency,
                MessageUrgency::RealTime | MessageUrgency::Interactive
            )
        {
            let secure_msg = self
                .sign_new_message(to_entity, content.as_bytes(), security_level)
                .await?;
            match mt_router
                .send_message(to_entity, &secure_msg, urgency)
                .await
            {
                Ok(receipt) => return Ok(receipt.message_id),
                Err(e) => {
                    tracing::warn!("Multi-transport failed: {e}, falling back to email");
                }
            }
        }
        let simple_msg = SimpleMessage {
            to: to_entity.to_string(),
            from_entity: self.our_global_id.clone(),
            content: content.to_string(),
            message_type,
            metadata: Default::default(),
        };
        self.send_message(simple_msg, to_entity.to_string())
            .await
            .map(|_| "email_fallback".to_string())
    }

    /// Pin `global_id`'s Ed25519 public key for `receive_messages`'s sender verification. Without
    /// at least one pinned key, `receive_messages` verifies nothing as `Verified` and drops every
    /// message it reads -- this is the router's minimal surface for populating the trust store
    /// that guards it, mirroring `TransportManager::trust_store`/`set_trust_store`'s pinning role
    /// at the single-key scale this router needs. Pinning a key only enables the authenticity
    /// check -- it does not add anti-replay protection or unsealing; see `receive_messages`'s doc
    /// comment.
    pub async fn pin_sender_key(&self, global_id: impl Into<String>, key: [u8; 32]) {
        self.trust_store.write().await.pin(global_id, key);
    }

    /// Receive messages from the email transport, verifying every sender before it is delivered.
    ///
    /// This REPLACES old `SynapseRouter::receive_messages`/`process_email_message`
    /// (`src/router.rs`, deleted in Task 6) entirely -- it is not a fix-in-place of that code, and
    /// none of its body is ported here (design note §4: "Do not let it survive the merge by simply
    /// relocating it onto the merged type's namespace"). The old path parsed a `SecureMessage` out
    /// of a JSON-wrapped `SimpleMessage.content` and handed it to the caller regardless of
    /// `sender_proof` -- self-documented in its own doc comment as "⚠️ Unverified: senders are not
    /// authenticated here" and "this path has no trust store, so it remains unauthenticated." A
    /// message that fails verification here is dropped, never returned -- the old code returned
    /// everything unconditionally, which is exactly the defect this replaces.
    ///
    /// Identity-model decision (Task 4 brief, Step 1): this router's receive-side verification uses
    /// [`TrustStore::verify_at`] -- the SAME mechanism `TransportManager::receive_messages` already
    /// uses for every other transport in this crate (TCP, UDP, WebSocket, QUIC, NAT traversal) --
    /// not `IdentityRegistry::get_public_key`. `IdentityRegistry` (`src/identity.rs`) stores a
    /// public key as an opaque, undefined-format `String` (`GlobalIdentity::public_key`), performs
    /// no signature check anywhere in this crate, and `register_entity` -- its own normal
    /// registration path -- sets that field to `""` by default. There is no existing convention
    /// for what encoding that string would even be in for an Ed25519 key. Verifying against it
    /// would mean inventing a brand-new key encoding and a brand-new, hand-rolled signature-check
    /// routine that duplicates `TrustStore::verify_against_pinned_key`/`canonical_input` without
    /// their review history or test coverage -- itself a second, divergent verification path, the
    /// same defect class board item 65 was on the send side (`sign_new_message`'s doc comment).
    /// `SenderProof`/`canonical_input` (`sender_auth.rs`) are already `TrustStore`'s vocabulary, and
    /// `sign_new_message` (this file) already produces proofs meant to be checked against a
    /// `TrustStore`, so this uses the store that already speaks the signer's language rather than
    /// building a second one for the receiver.
    ///
    /// Required finding: the AUTHENTICITY check here is the identical `TrustStore::verify_at`
    /// call `TransportManager::receive_messages` performs for a directly pinned sender --
    /// byte-for-byte the same check, not a re-implementation. The narrower surface actually used
    /// here (`self.trust_store`, populated only by [`Self::pin_sender_key`]) omits the
    /// certificate-chain/account-key route and revocation -- this router has no equivalent of
    /// `TransportManager::add_revocations` or `pin_account_key`/certificate ingestion yet -- but
    /// that gap alone would still leave this router at parity for the direct-pin case.
    ///
    /// The OVERALL guarantee is narrower than `TransportManager`'s, not the same, because of two
    /// gaps `verify_at` alone does not close:
    ///
    /// 1. **No anti-replay protection.** `TransportManager::receive_messages` also runs every
    ///    message through `src/replay.rs`'s `inbound.admit(...)`/`.check(...)` gate, keyed on
    ///    `(key_id, message_id, timestamp)`, which rejects a message it has already seen. This
    ///    method has no equivalent call. A captured, genuinely-signed message can be replayed
    ///    against this method indefinitely and it will be delivered every time, where
    ///    `TransportManager` would reject it after the first delivery.
    /// 2. **No unsealing of `Private`/`Secure`-level content.** This method never calls
    ///    `sealing::open` (`src/sealing.rs`). If a message was sent at `SecurityLevel::Private` or
    ///    `SecurityLevel::Secure` (reachable via `send_message_smart`'s caller-supplied
    ///    `security_level`), its content is still sealed/encrypted ciphertext on receipt, but this
    ///    method hands it back as `SimpleMessage.content` via
    ///    `String::from_utf8_lossy(&message.encrypted_content)` -- i.e. it silently returns raw
    ///    ciphertext as if it were plaintext, with no error and no indication anything is wrong.
    ///    This gap predates Task 4 (the send side doesn't seal either, going back to Task 2/3), but
    ///    it directly bears on this method's guarantee and must be disclosed here.
    ///
    /// In short: identical authenticity check, but missing anti-replay and missing unsealing --
    /// callers should not treat this method as a drop-in equivalent of
    /// `TransportManager::receive_messages`.
    pub async fn receive_messages(&self) -> Result<Vec<SimpleMessage>> {
        let transport = self.ensure_email_transport().await?;
        let mut inbox = crate::transport::abstraction::RawInbox::new();
        transport.receive_raw(&mut inbox).await?;
        let incoming = inbox.drain();

        let store = self.trust_store.read().await;
        let now = chrono::Utc::now();
        let mut delivered = Vec::with_capacity(incoming.len());
        for item in incoming {
            let message = item.message;
            let verdict = store.verify_at(&message, now);
            if !verdict.is_verified() {
                tracing::debug!(
                    from = %message.from_global_id,
                    verdict = ?verdict,
                    "dropping a received message whose sender did not verify"
                );
                continue;
            }
            delivered.push(SimpleMessage {
                to: message.to_global_id,
                from_entity: message.from_global_id,
                content: String::from_utf8_lossy(&message.encrypted_content).into_owned(),
                message_type: MessageType::Direct,
                metadata: message.metadata,
            });
        }
        Ok(delivered)
    }
}

/// Enhanced router status
#[derive(Debug, Clone)]
pub struct EnhancedRouterStatus {
    pub synapse_status: RouterHealth,
    pub multi_transport_enabled: bool,
    pub email_server_enabled: bool,
    pub available_transports: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[tokio::test]
    async fn new_builds_a_router_with_both_apis_available() {
        let config = Config::default_for_entity("Test", "tool");
        let router = SynapseRouter::new(config, "test@synapse.local".to_string())
            .await
            .expect("construct");
        // From the old SynapseRouter side:
        assert_eq!(router.get_our_global_id(), "test@synapse.local");
        // From the old EnhancedSynapseRouter side -- proves both halves are actually present,
        // not just one renamed:
        let status = router.status().await;
        assert!(!status.multi_transport_enabled || status.available_transports.is_empty());
    }

    #[tokio::test]
    async fn send_message_signs_before_handing_to_the_email_transport() {
        let config = Config::default_for_entity("Test", "tool");
        let router = SynapseRouter::new(config, "alice@synapse.local".to_string())
            .await
            .expect("construct");
        router
            .crypto
            .write()
            .await
            .generate_keypair()
            .expect("keypair");
        router
            .ensure_email_transport()
            .await
            .expect("email transport");

        let _simple_msg = SimpleMessage {
            to: "bob@synapse.local".to_string(),
            from_entity: "alice@synapse.local".to_string(),
            content: "hello".to_string(),
            message_type: MessageType::Direct,
            metadata: Default::default(),
        };
        // This will fail to actually deliver (no real SMTP server at bob@synapse.local in a unit
        // test), but the point of this test is what happens BEFORE delivery is attempted: the
        // signing. Capture the SecureMessage sign_new_message produces directly instead of only
        // asserting on send_message's overall Ok/Err, since a network failure and a signing
        // failure would otherwise look the same from outside.
        let signed = router
            .sign_new_message("bob@synapse.local", b"hello", SecurityLevel::Authenticated)
            .await
            .expect("sign");
        assert!(
            !matches!(signed.sender_proof.alg, crate::sender_auth::ProofAlg::None),
            "a message built for sending must be signed, not left as alg \"none\""
        );
    }

    /// A stub `Transport` that records the `SecureMessage` it is handed instead of sending it
    /// anywhere, so a test can inspect exactly what `send_message_smart`'s fast branch built.
    /// `MockTransport` in `transport::providers` already exists but its `send_message` ignores
    /// the message entirely -- it cannot be reused here for that reason.
    #[derive(Debug, Default)]
    struct RecordingTransport {
        sent: Arc<std::sync::Mutex<Option<SecureMessage>>>,
    }

    #[async_trait::async_trait]
    impl crate::transport::abstraction::TransportReceive for RecordingTransport {
        async fn receive_raw(
            &self,
            _inbox: &mut crate::transport::abstraction::RawInbox,
        ) -> Result<()> {
            Ok(())
        }
    }

    #[async_trait::async_trait]
    impl crate::transport::abstraction::Transport for RecordingTransport {
        fn transport_type(&self) -> crate::transport::abstraction::TransportType {
            crate::transport::abstraction::TransportType::Email
        }

        fn capabilities(&self) -> crate::transport::abstraction::TransportCapabilities {
            crate::transport::abstraction::TransportCapabilities {
                max_message_size: 1024 * 1024,
                reliable: true,
                real_time: false,
                broadcast: false,
                bidirectional: true,
                encrypted: false,
                network_spanning: true,
                supported_urgencies: vec![
                    crate::transport::abstraction::MessageUrgency::Interactive,
                ],
                features: vec![],
                unmeasured_metrics: vec![
                    crate::transport::abstraction::UnmeasuredMetric::AverageLatency,
                    crate::transport::abstraction::UnmeasuredMetric::ReliabilityScore,
                ],
            }
        }

        async fn can_reach(
            &self,
            _target: &crate::transport::abstraction::TransportTarget,
        ) -> bool {
            true
        }

        async fn estimate_metrics(
            &self,
            _target: &crate::transport::abstraction::TransportTarget,
        ) -> Result<crate::transport::abstraction::TransportEstimate> {
            Ok(crate::transport::abstraction::TransportEstimate {
                latency: std::time::Duration::from_millis(1),
                reliability: 0.99,
                bandwidth: 1_000_000,
                cost: 0.0,
                available: true,
                confidence: 1.0,
            })
        }

        async fn send_message(
            &self,
            target: &crate::transport::abstraction::TransportTarget,
            message: &SecureMessage,
        ) -> Result<crate::transport::abstraction::DeliveryReceipt> {
            *self.sent.lock().expect("lock") = Some(message.clone());
            Ok(crate::transport::abstraction::DeliveryReceipt {
                message_id: format!("recorded-for-{}", target.identifier),
                transport_used: crate::transport::abstraction::TransportType::Email,
                delivery_time: std::time::Duration::from_millis(1),
                target_reached: target.identifier.clone(),
                confirmation: crate::transport::abstraction::DeliveryConfirmation::Delivered,
                metadata: Default::default(),
            })
        }

        async fn test_connectivity(
            &self,
            _target: &crate::transport::abstraction::TransportTarget,
        ) -> Result<crate::transport::abstraction::ConnectivityResult> {
            Ok(crate::transport::abstraction::ConnectivityResult {
                connected: true,
                rtt: Some(std::time::Duration::from_millis(1)),
                error: None,
                quality: 1.0,
                details: Default::default(),
            })
        }

        async fn start(&self) -> Result<()> {
            Ok(())
        }

        async fn stop(&self) -> Result<()> {
            Ok(())
        }

        async fn status(&self) -> crate::transport::abstraction::TransportStatus {
            crate::transport::abstraction::TransportStatus::Running
        }

        async fn metrics(&self) -> crate::transport::abstraction::TransportMetrics {
            crate::transport::abstraction::TransportMetrics::default()
        }
    }

    /// A `transport::providers::TransportProvider` stub used only to hand `MultiTransportRouter` a
    /// `RecordingTransport` as its email transport, via the same `new_with_provider` dependency
    /// injection seam `ProductionTransportProvider` uses in production. `router.rs` used to define
    /// its own, separate `TransportProvider` trait/`ProductionTransportProvider` stub with the same
    /// names but a silently-stubbed email transport; that duplicate was deleted in the router-merge
    /// work, and `MultiTransportRouter::new_with_provider` now resolves `TransportProvider` to the
    /// real trait in `transport::providers`, which this impl targets.
    struct StubTransportProvider {
        email_transport: Arc<dyn crate::transport::abstraction::Transport>,
    }

    #[async_trait::async_trait]
    impl crate::transport::providers::TransportProvider for StubTransportProvider {
        async fn create_tcp_transport(
            &self,
            _config: &Config,
        ) -> Result<Option<Arc<dyn crate::transport::abstraction::Transport>>> {
            Ok(None)
        }

        async fn create_mdns_transport(
            &self,
            _config: &Config,
        ) -> Result<Option<Arc<dyn crate::transport::abstraction::Transport>>> {
            Ok(None)
        }

        async fn create_nat_transport(
            &self,
            _config: &Config,
        ) -> Result<Option<Arc<dyn crate::transport::abstraction::Transport>>> {
            Ok(None)
        }

        async fn create_email_transport(
            &self,
            _config: &Config,
        ) -> Result<Option<Arc<dyn crate::transport::abstraction::Transport>>> {
            Ok(Some(Arc::clone(&self.email_transport)))
        }

        fn create_transport_selector(
            &self,
        ) -> Arc<tokio::sync::RwLock<crate::transport::TransportSelector>> {
            Arc::new(tokio::sync::RwLock::new(
                crate::transport::TransportSelector::new(),
            ))
        }
    }

    /// The PM's required born-red test (board item 65): every send path this router hands a
    /// message to a transport through must have signed it first. `send_message` (Task 2) already
    /// provably signs; this proves `send_message_smart`'s fast (`MultiTransportRouter`) branch
    /// does too, by inspecting the actual `SecureMessage` a stub transport received -- not just a
    /// boolean "is it signed", but the signature verifying against the router's own pinned public
    /// key, the same mechanism `TrustStore::verify_at` uses elsewhere in this crate.
    ///
    /// `to_entity` is deliberately not resolvable (no real network/DNS target): with no
    /// TCP/UDP/mDNS/NAT route discoverable, `MultiTransportRouter`'s real transport-selection
    /// logic (`TransportSelector::choose_optimal_transport`) fails to find a real-time-suitable
    /// route and falls back, inside `MultiTransportRouter::send_message` itself, to its email
    /// transport -- which is this test's `RecordingTransport`. This exercises the real selection
    /// code, not a bypass of it.
    #[tokio::test]
    async fn every_send_path_signs_before_it_reaches_a_transport() {
        let config = Config::default_for_entity("Test", "tool");
        let mut router = SynapseRouter::new(config.clone(), "alice@synapse.local".to_string())
            .await
            .expect("construct");
        router
            .crypto
            .write()
            .await
            .generate_keypair()
            .expect("keypair");

        // send_message (Task 2) already provably signs -- assert it again here for completeness.
        let via_send_message = router
            .sign_new_message("bob@synapse.local", b"one", SecurityLevel::Authenticated)
            .await
            .expect("sign");
        assert!(!matches!(
            via_send_message.sender_proof.alg,
            crate::sender_auth::ProofAlg::None
        ));

        let sent: Arc<std::sync::Mutex<Option<SecureMessage>>> =
            Arc::new(std::sync::Mutex::new(None));
        let recording_transport = Arc::new(RecordingTransport {
            sent: Arc::clone(&sent),
        });
        let provider = StubTransportProvider {
            email_transport: recording_transport,
        };
        let mt_router = MultiTransportRouter::new_with_provider(
            config,
            "alice@synapse.local".to_string(),
            Box::new(provider),
        )
        .await
        .expect("multi-transport router");
        router.multi_transport = Some(Arc::new(mt_router));

        let message_id = router
            .send_message_smart(
                "bob@synapse.local",
                "hello fast path",
                MessageType::Direct,
                SecurityLevel::Authenticated,
                MessageUrgency::RealTime,
            )
            .await
            .expect("send_message_smart");
        assert!(
            message_id.starts_with("recorded-for-"),
            "expected the fast path to reach RecordingTransport via MultiTransportRouter's email \
             fallback, got message id {message_id:?} instead -- check that discovery genuinely \
             found no real-time route for the unresolvable test target"
        );

        let recorded = sent.lock().expect("lock").take().expect(
            "RecordingTransport.send_message was never called -- the fast branch didn't reach a transport",
        );
        assert_eq!(recorded.from_global_id, "alice@synapse.local");

        // The load-bearing assertion: the recorded message's signature actually verifies against
        // the router's own public key, not just "alg is not None". A signature-shaped field
        // nobody verifies is exactly board item 65's class of defect.
        let mut trust_store = crate::sender_auth::TrustStore::new();
        let public_key = router
            .crypto
            .read()
            .await
            .public_key_bytes()
            .expect("public key");
        trust_store.pin("alice@synapse.local", public_key);
        let verdict = trust_store.verify(&recorded);
        assert!(
            verdict.is_verified(),
            "recorded message's signature did not verify against the router's own pinned public \
             key: {verdict:?}"
        );
    }

    /// A free loopback port, bound and released immediately -- another process could take it
    /// before the transport binds it, in which case the transport's own bind fails with a clear
    /// error rather than this test silently colliding with something else.
    fn free_port() -> u16 {
        std::net::TcpListener::bind("127.0.0.1:0")
            .expect("bind ephemeral")
            .local_addr()
            .expect("local_addr")
            .port()
    }

    /// A Direct-mode `EmailTransportImpl` on loopback at `port`, started (its SMTP accept loop
    /// running), wrapped for injection into a router's private `email` field. Bypasses
    /// `ensure_email_transport`'s `ProductionTransportProvider` bridge entirely, which hardcodes
    /// `local_port = "2525"` (`transport::providers::ProductionTransportProvider::create_email_transport`)
    /// -- two routers in one test process cannot both bind that port, so each gets its own real
    /// `EmailTransportImpl` on a distinct free port instead, exactly the same transport type
    /// `ensure_email_transport` would have built.
    async fn direct_email_transport_on(
        port: u16,
    ) -> Arc<dyn crate::transport::abstraction::Transport> {
        let mut config = std::collections::HashMap::new();
        config.insert("local_port".to_string(), port.to_string());
        config.insert(
            crate::network_scope::BIND_SCOPE_KEY.to_string(),
            crate::network_scope::BindScope::Loopback
                .config_value()
                .to_string(),
        );
        use crate::transport::abstraction::Transport as _;
        let transport = crate::transport::email_unified::EmailTransportImpl::new(&config)
            .await
            .expect("construct email transport");
        transport.start().await.expect("start email transport");
        Arc::new(transport)
    }

    /// The PM's required born-red test (Task 4): a message claiming to be from a sender whose real
    /// key IS pinned in the receiver's trust store, but signed by a DIFFERENT, unregistered
    /// keypair, must be dropped by `receive_messages` -- not merely unsigned (the old
    /// `SenderProof::unsigned()`/board-item-65 case) but an active impersonation attempt with a
    /// real, well-formed, verifiable-looking signature that simply does not match the pinned key.
    ///
    /// The old `SynapseRouter::receive_messages`/`process_email_message` (`src/router.rs`) had no
    /// trust store at all and returned every parsed message unconditionally, so this exact scenario
    /// would have been delivered under the old code -- a happy-path-only test could not distinguish
    /// the two. This asserts the refusal directly: the impostor's message must NOT appear in
    /// `receive_messages`'s result, while a genuinely-signed message from the same claimed sender
    /// (the positive control) DOES arrive -- proving the drop is really about the bad signature, not
    /// about the whole pipeline being broken or the message never having reached the transport at
    /// all.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn receive_messages_rejects_an_unverifiable_sender() {
        const ALICE: &str = "alice@synapse.local";
        const BOB: &str = "bob@synapse.local";

        let bob_config = Config::default_for_entity("Bob", "tool");
        let bob_router = SynapseRouter::new(bob_config, BOB.to_string())
            .await
            .expect("construct bob");

        // Alice's real keypair -- this is the one Bob pins, i.e. "the one registered".
        let alice_crypto = {
            let mut c = CryptoManager::new();
            c.generate_keypair().expect("alice keypair");
            c
        };
        let alice_public_key = alice_crypto.public_key_bytes().expect("alice public key");
        bob_router.pin_sender_key(ALICE, alice_public_key).await;

        // An impostor's keypair -- deliberately NOT the one Bob pinned for ALICE.
        let mut impostor_crypto = CryptoManager::new();
        impostor_crypto
            .generate_keypair()
            .expect("impostor keypair");

        let bob_port = free_port();
        let alice_port = free_port();
        let bob_transport = direct_email_transport_on(bob_port).await;
        let alice_transport = direct_email_transport_on(alice_port).await;
        *bob_router.email.write().await = Some(Arc::clone(&bob_transport));

        let bob_target = crate::transport::abstraction::TransportTarget::new(BOB.to_string())
            .with_address(format!("127.0.0.1:{bob_port}"));

        // Positive control: a message genuinely signed by Alice's real (pinned) key.
        let mut genuine = SecureMessage::new(
            BOB.to_string(),
            ALICE.to_string(),
            b"hello from the real alice".to_vec(),
            SecurityLevel::Authenticated,
        );
        alice_crypto
            .sign_secure_message(&mut genuine)
            .expect("sign genuine");
        alice_transport
            .send_message(&bob_target, &genuine)
            .await
            .expect("send genuine");

        // The attack: claims to be ALICE in `from_global_id`, but signed by the impostor's key,
        // which Bob never pinned for ALICE (or for anyone).
        let mut forged = SecureMessage::new(
            BOB.to_string(),
            ALICE.to_string(),
            b"hello from an impostor".to_vec(),
            SecurityLevel::Authenticated,
        );
        impostor_crypto
            .sign_secure_message(&mut forged)
            .expect("sign forged");
        alice_transport
            .send_message(&bob_target, &forged)
            .await
            .expect("send forged");

        // Poll until both have had a chance to arrive at the transport layer.
        let mut delivered: Vec<SimpleMessage> = Vec::new();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while delivered.is_empty() && std::time::Instant::now() < deadline {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            delivered.extend(bob_router.receive_messages().await.expect("receive"));
        }
        // One more poll, generous, to give the forged message every chance to show up too, if the
        // verification gate were not actually dropping it.
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        delivered.extend(bob_router.receive_messages().await.expect("receive"));

        assert!(
            delivered
                .iter()
                .any(|m| m.content == "hello from the real alice"),
            "the genuinely-signed control message never arrived: {delivered:?}"
        );
        assert!(
            !delivered
                .iter()
                .any(|m| m.content == "hello from an impostor"),
            "a message signed by an unpinned, unrelated key was delivered anyway -- \
             receive_messages must drop an unverifiable sender, not just an unsigned one: \
             {delivered:?}"
        );
    }
}
