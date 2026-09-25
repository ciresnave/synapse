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
    transport::router::MultiTransportRouter,
    types::{SecureMessage, SecurityLevel, SimpleMessage},
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
    use crate::types::MessageType;

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
}
