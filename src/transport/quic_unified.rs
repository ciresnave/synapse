// SPDX-License-Identifier: MIT OR Apache-2.0
//! QUIC transport conforming to the unified `Transport`/`TransportReceive` traits.
//!
//! # Wire format (Task 1: one connection, one stream, per message)
//!
//! `send_message` connects to the target (a full handshake; no 0-RTT -- spec §5), opens one
//! bidirectional QUIC stream, writes the serialized `SecureMessage` as JSON, and finishes the
//! send side so the receiver reads to a clean stream end. The receiver accepts the stream, reads
//! to its end, and parses it the same way TCP/WebSocket/HTTP do. Connection pooling and stream
//! multiplexing (reusing one connection for many messages) arrive in Task 2; this task proves
//! the plumbing with one connection per message.
//!
//! # Delivery claim
//!
//! `Sent` only, never `Delivered` -- see
//! `docs/superpowers/specs/2026-09-22-quic-transport-design.md` §4. QUIC has no synchronous
//! request/response the way HTTP does; a QUIC-level ACK proves the peer's kernel got the bytes,
//! not that its application processed them. `delivery_ack.rs` remains the only mechanism for
//! that, transport-agnostic, and needs no QUIC-specific code.
//!
//! # Config keys
//!
//! | key | default | meaning |
//! |---|---|---|
//! | [`LOCAL_PORT_KEY`] (`local_port`) | `0` | the port `start` listens on; `0` lets the OS choose |
//! | `bind_scope` | `loopback` | which interfaces the endpoint binds ([`crate::network_scope::BindScope`]) |

use super::abstraction::{
    self, ConnectivityResult, DeliveryConfirmation, DeliveryReceipt, IncomingMessage, RawInbox,
    Transport, TransportCapabilities, TransportEstimate, TransportFactory, TransportMetrics,
    TransportStatus, TransportTarget, TransportType,
};
use crate::error::{Result, SynapseError};
use crate::types::SecureMessage;
use async_trait::async_trait;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::Mutex;

pub const LOCAL_PORT_KEY: &str = "local_port";

pub struct QuicTransportImpl {
    endpoint: quinn::Endpoint,
    #[allow(dead_code)] // Retained for a future capabilities/metrics pass (Task 4).
    local_port: u16,
    #[allow(dead_code)] // Retained for a future capabilities/metrics pass (Task 4).
    bind_scope: crate::network_scope::BindScope,
    received: Arc<Mutex<Vec<IncomingMessage>>>,
    is_running: Arc<Mutex<bool>>,
}

impl QuicTransportImpl {
    pub async fn new(config: &HashMap<String, String>) -> Result<Self> {
        validate_config(config)?;
        let local_port = config
            .get(LOCAL_PORT_KEY)
            .map(|p| p.parse::<u16>())
            .transpose()
            .map_err(|_| SynapseError::Config("Invalid port number".to_string()))?
            .unwrap_or(0);
        let bind_scope = crate::network_scope::BindScope::from_config_map(config)?;

        let (cert, key) = super::quic_tls::generate_self_signed_cert()?;
        let server_tls = super::quic_tls::server_config(cert, key)?; // Arc<rustls::ServerConfig>
        let server_quic_config = quinn::ServerConfig::with_crypto(Arc::new(
            quinn::crypto::rustls::QuicServerConfig::try_from(server_tls).map_err(|e| {
                SynapseError::TransportError(format!("QUIC server TLS setup failed: {e}"))
            })?,
        ));

        let bind_addr = bind_scope.listen_addr(local_port);
        let mut endpoint =
            quinn::Endpoint::server(server_quic_config, bind_addr).map_err(|e| {
                SynapseError::TransportError(format!(
                    "Failed to bind QUIC endpoint to {bind_addr}: {e}"
                ))
            })?;

        let client_tls = super::quic_tls::client_config()?; // Arc<rustls::ClientConfig>
        let client_quic_config = quinn::ClientConfig::new(Arc::new(
            quinn::crypto::rustls::QuicClientConfig::try_from(client_tls).map_err(|e| {
                SynapseError::TransportError(format!("QUIC client TLS setup failed: {e}"))
            })?,
        ));
        endpoint.set_default_client_config(client_quic_config);

        Ok(Self {
            endpoint,
            local_port,
            bind_scope,
            received: Arc::new(Mutex::new(Vec::new())),
            is_running: Arc::new(Mutex::new(false)),
        })
    }

    fn local_addr(&self) -> Result<SocketAddr> {
        self.endpoint.local_addr().map_err(|e| {
            SynapseError::TransportError(format!("QUIC endpoint has no local address: {e}"))
        })
    }
}

pub fn validate_config(config: &HashMap<String, String>) -> Result<()> {
    if let Some(port_str) = config.get(LOCAL_PORT_KEY)
        && port_str.parse::<u16>().is_err()
    {
        return Err(SynapseError::Config("Invalid port number".to_string()));
    }
    Ok(())
}

#[async_trait]
impl Transport for QuicTransportImpl {
    fn transport_type(&self) -> TransportType {
        TransportType::Quic
    }

    fn capabilities(&self) -> TransportCapabilities {
        // Task 4 replaces this with measured figures. Task 1 states what is true today.
        TransportCapabilities {
            max_message_size: 1024 * 1024,
            reliable: true,
            real_time: true,
            broadcast: false,
            bidirectional: true,
            encrypted: true,
            network_spanning: true,
            supported_urgencies: vec![abstraction::MessageUrgency::RealTime],
            features: vec!["one_stream_per_message".to_string()],
        }
    }

    async fn can_reach(&self, target: &TransportTarget) -> bool {
        parse_target(target).is_ok()
    }

    async fn estimate_metrics(&self, target: &TransportTarget) -> Result<TransportEstimate> {
        let _addr = parse_target(target)?;
        // Task 4 replaces this with a real probe.
        Ok(TransportEstimate {
            latency: std::time::Duration::from_millis(20),
            reliability: 0.9,
            bandwidth: 1,
            cost: 1.0,
            available: true,
            confidence: 0.5,
        })
    }

    async fn send_message(
        &self,
        target: &TransportTarget,
        message: &SecureMessage,
    ) -> Result<DeliveryReceipt> {
        let start = std::time::Instant::now();
        let addr = parse_target(target)?;

        let connecting = self
            .endpoint
            .connect(addr, "synapse-quic")
            .map_err(|e| SynapseError::TransportError(format!("QUIC connect setup failed: {e}")))?;
        let connection = connecting
            .await
            .map_err(|e| SynapseError::TransportError(format!("QUIC handshake failed: {e}")))?;

        let (mut send, _recv) = connection
            .open_bi()
            .await
            .map_err(|e| SynapseError::TransportError(format!("QUIC stream open failed: {e}")))?;

        let data = serde_json::to_vec(message).map_err(|e| {
            SynapseError::TransportError(format!("Failed to serialize message: {e}"))
        })?;
        send.write_all(&data)
            .await
            .map_err(|e| SynapseError::TransportError(format!("QUIC stream write failed: {e}")))?;
        send.finish()
            .map_err(|e| SynapseError::TransportError(format!("QUIC stream finish failed: {e}")))?;
        // Wait for the peer to acknowledge receipt of the whole stream before dropping
        // `connection`: dropping the last handle to a `quinn::Connection` that isn't already
        // closing sends an abrupt CONNECTION_CLOSE (see `quinn`'s `ConnectionRef::drop` ->
        // `implicit_close`), which can race the still-in-flight STREAM/FIN frame and reset the
        // stream before the receiver finishes reading it -- an intermittent, silent message loss
        // that "the transport sends" would otherwise never reveal.
        send.stopped().await.map_err(|e| {
            SynapseError::TransportError(format!(
                "QUIC stream was not acknowledged by the peer: {e}"
            ))
        })?;

        Ok(DeliveryReceipt {
            message_id: message.message_id.0.to_string(),
            transport_used: TransportType::Quic,
            delivery_time: start.elapsed(),
            target_reached: target.identifier.clone(),
            confirmation: DeliveryConfirmation::Sent,
            metadata: HashMap::new(),
        })
    }

    async fn test_connectivity(&self, target: &TransportTarget) -> Result<ConnectivityResult> {
        let can_reach = self.can_reach(target).await;
        Ok(ConnectivityResult {
            connected: can_reach,
            rtt: None,
            error: if can_reach {
                None
            } else {
                Some("invalid QUIC target".to_string())
            },
            quality: if can_reach { 0.5 } else { 0.0 },
            details: HashMap::new(),
        })
    }

    async fn start(&self) -> Result<()> {
        let mut running = self.is_running.lock().await;
        if *running {
            return Ok(());
        }
        let local_addr = self.local_addr()?;
        tracing::info!("QUIC transport bound to {}", local_addr);

        let endpoint = self.endpoint.clone();
        let received = Arc::clone(&self.received);
        tokio::spawn(async move {
            while let Some(incoming) = endpoint.accept().await {
                let received = Arc::clone(&received);
                tokio::spawn(async move {
                    let connection = match incoming.await {
                        Ok(c) => c,
                        Err(e) => {
                            tracing::warn!("QUIC handshake failed: {e}");
                            return;
                        }
                    };
                    let source = connection.remote_address().to_string();
                    let (_send, mut recv) = match connection.accept_bi().await {
                        Ok(streams) => streams,
                        Err(e) => {
                            tracing::warn!("QUIC accept_bi failed from {source}: {e}");
                            return;
                        }
                    };
                    let data = match recv.read_to_end(8 * 1024 * 1024).await {
                        Ok(d) => d,
                        Err(e) => {
                            tracing::warn!("QUIC stream read failed from {source}: {e}");
                            return;
                        }
                    };
                    let message: SecureMessage = match serde_json::from_slice(&data) {
                        Ok(m) => m,
                        Err(e) => {
                            tracing::warn!(
                                "Dropped QUIC message from {source}: {} bytes did not parse as a SecureMessage: {e}",
                                data.len()
                            );
                            return;
                        }
                    };
                    let incoming_message =
                        IncomingMessage::new(message, TransportType::Quic, source);
                    received.lock().await.push(incoming_message);
                });
            }
        });

        *running = true;
        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        let mut running = self.is_running.lock().await;
        self.endpoint.close(0u32.into(), b"stopping");
        *running = false;
        Ok(())
    }

    async fn status(&self) -> TransportStatus {
        if *self.is_running.lock().await {
            TransportStatus::Running
        } else {
            TransportStatus::Stopped
        }
    }

    async fn metrics(&self) -> TransportMetrics {
        TransportMetrics {
            transport_type: TransportType::Quic,
            ..Default::default()
        }
    }
}

#[async_trait]
impl abstraction::TransportReceive for QuicTransportImpl {
    async fn receive_raw(&self, inbox: &mut RawInbox) -> Result<()> {
        let mut received = self.received.lock().await;
        inbox.extend(received.drain(..));
        Ok(())
    }
}

fn parse_target(target: &TransportTarget) -> Result<SocketAddr> {
    target
        .address
        .as_ref()
        .ok_or_else(|| SynapseError::TransportError("No target address provided".to_string()))?
        .parse::<SocketAddr>()
        .map_err(|e| SynapseError::TransportError(format!("Invalid QUIC target address: {e}")))
}

pub struct QuicTransportFactory;

#[async_trait]
impl TransportFactory for QuicTransportFactory {
    async fn create_transport(
        &self,
        config: &HashMap<String, String>,
    ) -> Result<Box<dyn Transport>> {
        let transport = QuicTransportImpl::new(config).await?;
        Ok(Box::new(transport))
    }

    fn transport_type(&self) -> TransportType {
        TransportType::Quic
    }

    fn default_config(&self) -> HashMap<String, String> {
        let mut cfg = HashMap::new();
        cfg.insert(LOCAL_PORT_KEY.to_string(), "0".to_string());
        cfg
    }

    fn validate_config(&self, config: &HashMap<String, String>) -> Result<()> {
        validate_config(config)
    }
}
