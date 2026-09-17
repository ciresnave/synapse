// SPDX-License-Identifier: MIT OR Apache-2.0
//! MCP stdio surface (P2 slice c): lets any MCP client send, poll, list and ack through synapse.
//!
//! Design: `docs/superpowers/specs/2026-09-17-mcp-surface-design.md`.
//!
//! Secret hygiene (spec §6): nothing here may put the private key's path or bytes into a tool result,
//! a tool error or a log line. `McpConfig` deliberately does not implement `Debug`.

use crate::crypto::CryptoManager;
use crate::error::{Result, SynapseError};
use crate::sender_auth::TrustStore;
use crate::transport::{
    ReceivedMessage, TransportManager, TransportManagerBuilder, TransportStatus, TransportType,
    UdpTransportFactory,
};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

/// The server's configuration. Keys and peers can only be changed here (spec §3).
#[derive(Clone, Deserialize)]
pub struct McpConfig {
    pub global_id: String,
    pub private_key_pem_path: PathBuf,
    pub udp_bind_port: u16,
    /// Where peers send acks; defaults to `127.0.0.1:<udp_bind_port>`.
    #[serde(default)]
    pub reply_address: Option<String>,
    #[serde(default)]
    pub peers: Vec<PeerConfig>,
}

#[derive(Clone, Deserialize)]
pub struct PeerConfig {
    pub global_id: String,
    pub public_key_pem: String,
    pub address: String,
}

impl McpConfig {
    pub fn from_toml(text: &str) -> Result<Self> {
        // toml's error text can quote the offending line, which may be the key path: drop it.
        toml::from_str(text)
            .map_err(|_| config_error("the config file is not valid TOML for synapse-mcp"))
    }
}

fn config_error(message: impl Into<String>) -> SynapseError {
    SynapseError::ConfigurationError(message.into())
}

/// A configured peer, as `list` shows it.
#[derive(Clone)]
pub(crate) struct PeerView {
    pub(crate) global_id: String,
    pub(crate) address: String,
    pub(crate) key_id: String,
}

pub(crate) struct Inner {
    pub(crate) global_id: String,
    pub(crate) key_id: String,
    pub(crate) crypto: CryptoManager,
    pub(crate) manager: TransportManager,
    pub(crate) peers: Vec<PeerView>,
    pub(crate) reply_address: String,
    /// Messages `poll` returned, by id, so `ack` can find them. Unbounded until slice e.
    pub(crate) kept: Mutex<HashMap<String, ReceivedMessage>>,
    /// Ids this server sent with `request_ack`. Unbounded until slice e.
    pub(crate) sent_with_ack: Mutex<Vec<String>>,
}

/// The MCP server. Cheap to clone; all state is shared.
#[derive(Clone)]
pub struct SynapseMcpServer {
    pub(crate) inner: Arc<Inner>,
}

impl SynapseMcpServer {
    /// Load the key, pin the peers, and start the UDP transport (spec §3). Every error message is
    /// fixed text: none names the key path or contains key bytes.
    pub async fn start(config: McpConfig) -> Result<Self> {
        let pem = std::fs::read_to_string(&config.private_key_pem_path)
            .map_err(|_| config_error("cannot read the private key file"))?;
        let mut crypto = CryptoManager::new();
        crypto.load_private_key(&pem).map_err(|_| {
            config_error("the private key file does not hold a usable Ed25519 PKCS#8 key")
        })?;
        let own_key = crypto
            .public_key_bytes()
            .map_err(|_| config_error("the private key could not be loaded"))?;

        let mut store = TrustStore::new();
        let mut peers = Vec::with_capacity(config.peers.len());
        for peer in &config.peers {
            store
                .pin_pem(&peer.global_id, &peer.public_key_pem)
                .map_err(|_| {
                    config_error(format!(
                        "peer {}: public_key_pem is not an Ed25519 public key",
                        peer.global_id
                    ))
                })?;
            peers.push(PeerView {
                global_id: peer.global_id.clone(),
                address: peer.address.clone(),
                key_id: store
                    .pinned_key_id(&peer.global_id)
                    .expect("pinned on the line above"),
            });
        }

        let mut udp = HashMap::new();
        udp.insert("bind_port".to_string(), config.udp_bind_port.to_string());
        let manager = TransportManagerBuilder::new()
            .disable_transport(TransportType::Tcp)
            .disable_transport(TransportType::Http)
            .disable_transport(TransportType::Email)
            .disable_transport(TransportType::AutoDiscovery)
            .transport_config(TransportType::Udp, udp)
            .trust_store(store)
            .build();
        manager
            .register_factory(Box::new(UdpTransportFactory))
            .await?;
        manager.start().await?;
        // start() only warns when a transport fails, so confirm UDP is really up.
        match manager
            .get_transport_status()
            .await
            .get(&TransportType::Udp)
        {
            Some(TransportStatus::Running) => {}
            _ => {
                return Err(config_error(format!(
                    "the UDP transport did not start on port {}",
                    config.udp_bind_port
                )));
            }
        }

        let reply_address = config
            .reply_address
            .clone()
            .unwrap_or_else(|| format!("127.0.0.1:{}", config.udp_bind_port));
        Ok(Self {
            inner: Arc::new(Inner {
                global_id: config.global_id.clone(),
                key_id: crate::sender_auth::key_id(&own_key),
                crypto,
                manager,
                peers,
                reply_address,
                kept: Mutex::new(HashMap::new()),
                sent_with_ack: Mutex::new(Vec::new()),
            }),
        })
    }
}
