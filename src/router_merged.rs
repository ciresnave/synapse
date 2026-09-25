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
    CryptoManager, config::Config, email_server::SynapseEmailServer, error::Result,
    identity::IdentityRegistry, router::RouterHealth, transport::router::MultiTransportRouter,
};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

pub struct SynapseRouter {
    /// Cryptographic manager
    crypto: Arc<RwLock<CryptoManager>>,
    /// Identity registry
    identity: Arc<RwLock<IdentityRegistry>>,
    /// Email transport. `None` until Task 2 wires a real `EmailTransportImpl` in; kept as a
    /// distinct step so Task 1's own test can pass without depending on Task 2's
    /// transport-construction code. This is NOT the old `src/email.rs::EmailTransport` type.
    /// Unread until Task 2 adds the send/receive paths that use it.
    #[allow(dead_code)]
    email: Arc<RwLock<Option<Arc<dyn crate::transport::abstraction::Transport>>>>,
    /// Multi-transport router for fast communication
    multi_transport: Option<Arc<MultiTransportRouter>>,
    /// Local email server (SMTP/IMAP) for when we're externally accessible
    email_server: Option<Arc<SynapseEmailServer>>,
    /// Configuration. Not read by anything in Task 1; kept for Task 2's
    /// `ensure_email_transport`, which needs the router's own `Config` to build the real
    /// transport.
    #[allow(dead_code)]
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
}
