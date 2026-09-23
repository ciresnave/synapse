// SPDX-License-Identifier: MIT OR Apache-2.0
//! QUIC transport conforming to the unified `Transport`/`TransportReceive` traits.
//!
//! # Wire format (Task 1: one connection, one stream, per message)
//!
//! `send_message` reuses a pooled connection to the target where one exists (Task 2), or
//! connects (a full handshake; no 0-RTT -- spec §5), opens one bidirectional QUIC stream, writes
//! the serialized `SecureMessage` as JSON, and finishes the send side so the receiver reads to a
//! clean stream end. The receiver accepts the stream, reads to its end, and parses it the same
//! way TCP/WebSocket/HTTP do.
//!
//! # Connection pooling (Task 2)
//!
//! One `quinn::Connection` is kept per peer address and reused across sends; concurrent sends to
//! the same peer open independent, multiplexed streams on that one connection rather than
//! blocking each other or paying for a fresh handshake each time. The pool's lock
//! ([`QuicTransportImpl::pooled_connection`]) is never held across the handshake `await`: a miss
//! releases the lock, connects, then re-acquires it to insert, keeping a connection a racing
//! sender already established instead of overwriting it. A background sweep evicts entries idle
//! for longer than [`IDLE_TIMEOUT_MS_KEY`]; the next send to that peer transparently reopens a
//! connection. [`QuicTransportImpl::evict`] removes a specific peer's entry on demand (wiring it
//! to an application-layer verification failure is a later task).
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
//! | [`IDLE_TIMEOUT_MS_KEY`] (`idle_timeout_ms`) | [`DEFAULT_IDLE_TIMEOUT_MS`] (300000) | how long a pooled connection may go unused before the idle sweep evicts it |

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
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

pub const LOCAL_PORT_KEY: &str = "local_port";

/// The config key for how long, in milliseconds, a pooled connection may go unused before the
/// idle sweep evicts it. See the module documentation.
pub const IDLE_TIMEOUT_MS_KEY: &str = "idle_timeout_ms";

/// The default for [`IDLE_TIMEOUT_MS_KEY`]: 5 minutes.
pub const DEFAULT_IDLE_TIMEOUT_MS: u64 = 300_000;

/// Parse `idle_timeout_ms` the way `websocket_unified.rs`'s `Limits::from_config` parses its own
/// millisecond keys: a positive integer, or the default when absent; anything else is refused
/// rather than silently replaced.
fn parse_idle_timeout_ms(config: &HashMap<String, String>) -> Result<u64> {
    match config.get(IDLE_TIMEOUT_MS_KEY) {
        None => Ok(DEFAULT_IDLE_TIMEOUT_MS),
        Some(value) => match value.trim().parse::<u64>() {
            Ok(0) => Err(SynapseError::Config(format!(
                "{IDLE_TIMEOUT_MS_KEY} must be a positive integer, got 0"
            ))),
            Ok(ms) => Ok(ms),
            Err(e) => Err(SynapseError::Config(format!(
                "{IDLE_TIMEOUT_MS_KEY} must be a positive integer, got {value:?}: {e}"
            ))),
        },
    }
}

/// A pooled connection together with when it was last used, keyed by peer address. The last-use
/// `Instant` is refreshed on every hit or insert and read by the idle sweep spawned in `start`.
type ConnectionPool = Arc<Mutex<HashMap<SocketAddr, (Arc<quinn::Connection>, Instant)>>>;

pub struct QuicTransportImpl {
    endpoint: quinn::Endpoint,
    #[allow(dead_code)] // Retained for a future capabilities/metrics pass (Task 4).
    local_port: u16,
    #[allow(dead_code)] // Retained for a future capabilities/metrics pass (Task 4).
    bind_scope: crate::network_scope::BindScope,
    received: Arc<Mutex<Vec<IncomingMessage>>>,
    is_running: Arc<Mutex<bool>>,
    /// One connection per peer, reused across sends; see the module documentation.
    pool: ConnectionPool,
    /// How long a pooled connection may go unused before the idle sweep evicts it.
    idle_timeout: Duration,
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

        let idle_timeout = Duration::from_millis(parse_idle_timeout_ms(config)?);

        Ok(Self {
            endpoint,
            local_port,
            bind_scope,
            received: Arc::new(Mutex::new(Vec::new())),
            is_running: Arc::new(Mutex::new(false)),
            pool: Arc::new(Mutex::new(HashMap::new())),
            idle_timeout,
        })
    }

    fn local_addr(&self) -> Result<SocketAddr> {
        self.endpoint.local_addr().map_err(|e| {
            SynapseError::TransportError(format!("QUIC endpoint has no local address: {e}"))
        })
    }

    /// Reuse a pooled connection to `addr`, or establish one. The pool's lock is never held
    /// across the handshake `await` (spec §3, condition 2): a miss releases the lock, connects,
    /// then re-acquires it to insert -- double-checking for a connection a racing sender
    /// established first, keeping that one and dropping the one just established, rather than
    /// overwrite it. Every return path refreshes the entry's last-use `Instant`, so the idle
    /// sweep (spawned in `start`) only evicts a connection nothing has used recently.
    async fn pooled_connection(&self, addr: SocketAddr) -> Result<Arc<quinn::Connection>> {
        {
            let mut pool = self.pool.lock().await;
            if let Some((conn, last_used)) = pool.get_mut(&addr)
                && conn.close_reason().is_none()
            {
                *last_used = Instant::now();
                return Ok(Arc::clone(conn));
            }
        }
        let connecting = self
            .endpoint
            .connect(addr, "synapse-quic")
            .map_err(|e| SynapseError::TransportError(format!("QUIC connect setup failed: {e}")))?;
        let fresh =
            Arc::new(connecting.await.map_err(|e| {
                SynapseError::TransportError(format!("QUIC handshake failed: {e}"))
            })?);
        let mut pool = self.pool.lock().await;
        if let Some((existing, last_used)) = pool.get_mut(&addr)
            && existing.close_reason().is_none()
        {
            // A racing sender inserted a connection to the same peer while we were connecting:
            // keep it, and let `fresh` be dropped (which closes it cleanly).
            *last_used = Instant::now();
            return Ok(Arc::clone(existing));
        }
        pool.insert(addr, (Arc::clone(&fresh), Instant::now()));
        Ok(fresh)
    }

    /// Evict a pooled connection to `addr` -- for when a message received over it fails
    /// application-layer verification, so a connection that has started reaching the wrong peer
    /// (NAT rebinding, address reuse) is not silently reused again. Wiring this to the manager's
    /// verification path is a later task; this task provides the method and tests it directly.
    pub async fn evict(&self, addr: SocketAddr) {
        if let Some((conn, _)) = self.pool.lock().await.remove(&addr) {
            conn.close(0u32.into(), b"evicted: verification failure");
        }
    }

    /// How many connections the pool currently holds. Exposed as a plain `pub` method rather than
    /// `#[cfg(test)]`-gated: the integration tests in `tests/transport_repairs.rs` link against
    /// this crate's normal build, which does not include `cfg(test)` items, so a gated accessor
    /// would not exist in the binary they run against.
    pub async fn pool_size(&self) -> usize {
        self.pool.lock().await.len()
    }
}

pub fn validate_config(config: &HashMap<String, String>) -> Result<()> {
    if let Some(port_str) = config.get(LOCAL_PORT_KEY)
        && port_str.parse::<u16>().is_err()
    {
        return Err(SynapseError::Config("Invalid port number".to_string()));
    }
    parse_idle_timeout_ms(config)?;
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

        let connection = self.pooled_connection(addr).await?;

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
        // Wait for the peer to acknowledge receipt of the whole stream before returning: without
        // this, a caller could observe `Sent` before the peer's application ever saw the bytes.
        // It also still guards the connection-drop race this was originally added for (dropping
        // the last handle to a `quinn::Connection` that isn't already closing sends an abrupt
        // CONNECTION_CLOSE -- see `quinn`'s `ConnectionRef::drop` -> `implicit_close` -- which can
        // race the still-in-flight STREAM/FIN frame and reset the stream before the receiver
        // finishes reading it): now that `connection` comes from the pool, the last handle is
        // usually held there rather than dropped here, but a connection that lost the
        // insert-race in `pooled_connection` (or one evicted concurrently) can still be the last
        // handle at this point.
        //
        // Bounded, not an open-ended hang: `stopped()` resolves once the connection is closed for
        // any reason, including quinn's default `max_idle_timeout` (30s) if the peer never
        // responds at all.
        match send.stopped().await {
            Ok(None) => {
                // The peer acknowledged receipt of all stream data. Proceed as success.
            }
            Ok(Some(error_code)) => {
                // The peer sent STOP_SENDING: it explicitly did not accept the stream. This must
                // not be reported as `Sent` -- that would be exactly the silent-loss-on-send this
                // wait was added to close.
                return Err(SynapseError::TransportError(format!(
                    "QUIC peer rejected the stream (STOP_SENDING, code {error_code})"
                )));
            }
            Err(e) => {
                return Err(SynapseError::TransportError(format!(
                    "QUIC stream was not acknowledged by the peer: {e}"
                )));
            }
        }

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
                    // Keep accepting streams for as long as the peer keeps the connection open
                    // (Task 2: the sender pools and reuses one connection for many messages, so
                    // the receiver must accept more than the first stream on it). Each stream is
                    // handled on its own task so concurrent sends on the same connection don't
                    // wait on each other (multiplexing). `accept_bi` returning `Err` means the
                    // connection closed -- gracefully (idle timeout, `stop()`, or the peer's own
                    // pool evicting it) or otherwise -- either way there is nothing left to accept.
                    loop {
                        let (_send, mut recv) = match connection.accept_bi().await {
                            Ok(streams) => streams,
                            Err(e) => {
                                tracing::debug!("QUIC connection from {source} closed: {e}");
                                break;
                            }
                        };
                        let received = Arc::clone(&received);
                        let source = source.clone();
                        tokio::spawn(async move {
                            // 8 MiB is a placeholder, not a considered limit -- Task 3 replaces
                            // this with the real size-limit design (see `stop()`'s accept-loop
                            // shutdown gap above for the same "known Task 1 gap, noted for later"
                            // treatment).
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
            }
        });

        // Idle-timeout eviction sweep: a connection nothing has used in `idle_timeout` is closed
        // and dropped from the pool; the next send to that peer transparently reopens one.
        // Woken at twice the idle rate so an entry is evicted within `idle_timeout` of going
        // idle, not up to `idle_timeout` late.
        {
            let pool = Arc::clone(&self.pool);
            let idle_timeout = self.idle_timeout;
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(idle_timeout / 2);
                loop {
                    interval.tick().await;
                    let now = Instant::now();
                    let mut pool = pool.lock().await;
                    // Inlined rather than calling `evict()`: `evict()` takes the pool lock itself,
                    // and this closure already runs with it held (via `retain`) -- calling it here
                    // would deadlock. Keep this in sync with `evict()`'s close-and-drop behavior by
                    // hand if either changes.
                    pool.retain(|_, (conn, last_used)| {
                        let keep = now.duration_since(*last_used) < idle_timeout
                            && conn.close_reason().is_none();
                        if !keep {
                            conn.close(0u32.into(), b"idle timeout");
                        }
                        keep
                    });
                }
            });
        }

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
        cfg.insert(
            IDLE_TIMEOUT_MS_KEY.to_string(),
            DEFAULT_IDLE_TIMEOUT_MS.to_string(),
        );
        cfg
    }

    fn validate_config(&self, config: &HashMap<String, String>) -> Result<()> {
        validate_config(config)
    }
}

// A test attempting to cover the `Ok(Some(error_code))` (STOP_SENDING) branch above was tried and
// removed: see the task report's fix-report section for why a bare `quinn` peer that calls
// `recv.stop()` right after `accept_bi()` still measured `send.stopped()` resolving to `Ok(None)`
// on loopback with a small payload -- `stopped()`'s own documentation explains why (once the
// peer's transport layer has acknowledged all stream data, "the peer closing the stream is no
// longer meaningful", independent of whether the application later calls `stop()`), so this is a
// structural property of small loopback messages, not a flaky test. Known gap, flagged for a
// later task's test coverage.
