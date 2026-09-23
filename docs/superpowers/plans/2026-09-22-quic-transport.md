# QUIC Transport Slice Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement a real, `quinn`-backed QUIC transport (`TransportType::Quic`) conforming to the same `Transport`/`TransportReceive` contract TCP, WebSocket, HTTP, and NAT traversal already follow, with a persistent multiplexed connection pool, honest capabilities, and no new application-level ack mechanism.

**Architecture:** `src/transport/quic_unified.rs` holds the transport (`QuicTransportImpl`, `QuicTransportFactory`, connection pool, send/receive); a separate `src/transport/quic_tls.rs` holds the self-signed certificate generation and the permissive server-certificate verifier, isolated per the spec's condition 1 so the dangerous verifier can't be found and reused by accident. One QUIC stream per message, opened on a pooled, per-peer connection; `Sent`-only delivery claims; 0-RTT disabled; `SynapseError::MessageRefused` for oversize sends.

**Tech Stack:** `quinn = "0.11.12"` (default features: `runtime-tokio`, `rustls-ring`), `rcgen = "0.14.10"` (default features: `crypto`, `pem`, `ring`) — both pinned to `ring` as the rustls crypto backend to match each other; `rustls = "0.23"` (already a transitive dependency via `tokio-rustls`, currently resolving to `0.23.45` with both `ring` and `aws-lc-rs` compiled in from different dependents — Task 1 makes the QUIC transport's own provider selection explicit rather than relying on init order, see Task 1 Step 3).

**Spec:** `docs/superpowers/specs/2026-09-22-quic-transport-design.md` (approved by CireSnave and the PM, 2026-09-22, three conditions folded into its §3).

## Global Constraints

- `max_message_size` default: 1 MiB of serialized JSON (spec §6), matching TCP/WebSocket/HTTP's post-repair default — never the old preset's "1GB theoretical" figure.
- Oversize sends are refused as `SynapseError::MessageRefused` before opening a stream, and must never count as a transport failure against the circuit breaker (spec §6, matching Task 7's fix).
- 0-RTT is disabled everywhere; every connection establishment, including pool reopens, is a full handshake (spec §5).
- `send_message` returns `Sent` and never `Delivered` (spec §4). No new ack code — `delivery_ack.rs` is untouched by this plan.
- The pool's mutex is never held across the connection-establishment `await` (spec §3, condition 2).
- The permissive certificate verifier lives in its own module, named so it cannot be mistaken for a general-purpose TLS helper (spec §3, condition 1).
- A pool entry is evicted on any application-layer verification failure for a message received over it (spec §3, condition 3).
- Every config value that can be invalid (unparseable, zero, or a `max_queued_bytes` below `max_message_size` or above `u32::MAX`) is refused at construction by `QuicTransportFactory::validate_config`, never silently defaulted (matching Tasks 7-9).
- No `unsafe` code. No new top-level dependency families beyond `quinn` and `rcgen` (both draw only on `rustls`, already present).
- Every test that starts a QUIC endpoint binds loopback only, following `crate::network_scope::BindScope` exactly as the other transports do — a wildcard bind triggers Windows Firewall event 2097 (this session's own lesson, carried into Task 5's verification).

---

## File Structure

- **Create:** `src/transport/quic_tls.rs` — self-signed certificate generation (`generate_self_signed_cert`) and `DangerAcceptAnyServerCert` (the permissive verifier), plus the `rustls::ServerConfig`/`rustls::ClientConfig` builders that use them. Nothing here knows about connections, streams, or messages.
- **Create:** `src/transport/quic_unified.rs` — `QuicTransportImpl` (the `Transport`/`TransportReceive` implementation), `QuicTransportFactory` (replaces the current stub in `abstraction.rs`), the connection pool, and the byte-budget receive queue. Config keys, defaults, and validation live here, documented in the module doc comment (matching `websocket_unified.rs`'s pattern).
- **Modify:** `src/transport/mod.rs` — declare `pub mod quic_tls;` and `pub mod quic_unified;` (both `#[cfg(not(target_arch = "wasm32"))]`, matching every other non-WASM transport module), and re-export `QuicTransportFactory`/`QuicTransportImpl` explicitly (not via the `abstraction::*` glob — the existing comment there already explains why TCP/HTTP need explicit re-exports to avoid being shadowed by `abstraction`'s old stub factories; QUIC's stub factory in `abstraction.rs` is removed in Task 1, so there is nothing left to shadow, but the explicit re-export keeps the pattern consistent and grep-able).
- **Modify:** `src/transport/abstraction.rs` — delete the current `QuicTransportFactory` stub (Task 1); replace `TransportCapabilities::quic()`'s aspirational figures with measured ones (Task 4).
- **Modify:** `Cargo.toml` — add `quinn` and `rcgen` under the existing `[dependencies]` (both plain, non-optional — every other unified transport is unconditionally compiled on non-WASM targets, and QUIC follows the same pattern; if a later slice wants QUIC to be feature-gated, that is its own decision, out of scope here).
- **Modify:** `tests/transport_repairs.rs` — add `TransportType::Quic` to `ALL_TRANSPORTS`, `port_key`, and `socket_of`; add a `quic()` factory helper; add the QUIC tests (one per task, below).
- **Modify:** `CAPABILITY_INVENTORY.md` — Task 5, per the established pattern of marking findings fixed with a ref.

---

## Task 1: Skeleton — one connection, one stream, one message, over loopback

**Files:**
- Create: `src/transport/quic_tls.rs`
- Create: `src/transport/quic_unified.rs`
- Modify: `src/transport/mod.rs`
- Modify: `src/transport/abstraction.rs` (delete the stub `QuicTransportFactory`, `impl TransportFactory for QuicTransportFactory` block, at the location noted in the spec §1)
- Modify: `Cargo.toml`
- Modify: `tests/transport_repairs.rs`
- Test: `tests/transport_repairs.rs` (`quic_carries_a_verified_message_end_to_end`)

**Interfaces:**
- Produces: `quic_tls::generate_self_signed_cert() -> Result<(rustls::pki_types::CertificateDer<'static>, rustls::pki_types::PrivateKeyDer<'static>)>`; `quic_tls::server_config(cert, key) -> Result<Arc<rustls::ServerConfig>>`; `quic_tls::client_config() -> Result<Arc<rustls::ClientConfig>>`; `quic_unified::QuicTransportImpl::new(config: &HashMap<String, String>) -> Result<Self>` (async); `QuicTransportFactory` implementing `abstraction::TransportFactory`, `transport_type() -> TransportType::Quic`.
- Consumes: `abstraction::{Transport, TransportReceive, TransportType, TransportTarget, TransportCapabilities, RawInbox, IncomingMessage, DeliveryReceipt, DeliveryConfirmation, TransportEstimate, ConnectivityResult, TransportStatus, TransportMetrics}`, `crate::error::{Result, SynapseError}`, `crate::types::SecureMessage`, `crate::network_scope::BindScope`.

- [ ] **Step 1: Add the dependencies**

```bash
cd /path/to/synapse
cargo add quinn@0.11.12
cargo add rcgen@0.14.10
```

Run `cargo tree -p quinn -p rcgen | grep -i rustls` afterward and confirm both resolve to `rustls 0.23.x` with the same crypto backend (`ring`) — if `cargo tree` shows `rustls-ring` under `quinn` and a `ring`-feature `rcgen`, they agree; if either pulls `aws-lc-rs` as its *own* provider choice, note it here and pin explicitly (`quinn = { version = "0.11.12", default-features = false, features = ["runtime-tokio", "rustls-ring"] }`) before continuing.

- [ ] **Step 2: Write `quic_tls.rs`'s failing test first**

```rust
// src/transport/quic_tls.rs
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Self-signed TLS for the QUIC transport. TLS here is wire encryption and connection setup
//! only, never identity -- real sender identity is the sealed and signed `SecureMessage`
//! envelope, exactly as for TCP/WebSocket/HTTP/NAT traversal. See
//! `docs/superpowers/specs/2026-09-22-quic-transport-design.md` §3.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_self_signed_cert_is_generated_each_call() {
        let (cert_a, _key_a) = generate_self_signed_cert().expect("cert a");
        let (cert_b, _key_b) = generate_self_signed_cert().expect("cert b");
        assert_ne!(
            cert_a.as_ref(),
            cert_b.as_ref(),
            "two calls must not reuse one certificate"
        );
    }
}
```

- [ ] **Step 3: Run it to verify it fails to compile** (nothing is defined yet)

Run: `cargo test --lib quic_tls:: 2>&1 | tail -20`
Expected: FAIL — `cannot find function generate_self_signed_cert`

- [ ] **Step 4: Implement `quic_tls.rs`**

```rust
// src/transport/quic_tls.rs (continued, above the #[cfg(test)] module)
use crate::error::{Result, SynapseError};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use std::sync::Arc;

/// One ephemeral, self-signed certificate and key pair. Generated fresh by every call --
/// this transport never stores, rotates, or compares certificates across connections (spec §3).
pub fn generate_self_signed_cert() -> Result<(CertificateDer<'static>, PrivateKeyDer<'static>)> {
    let rcgen::CertifiedKey { cert, signing_key } =
        rcgen::generate_simple_self_signed(vec!["synapse-quic".to_string()]).map_err(|e| {
            SynapseError::TransportError(format!("Failed to generate self-signed cert: {e}"))
        })?;
    let cert_der = cert.der().clone();
    let key_der = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(signing_key.serialize_der()));
    Ok((cert_der, key_der))
}

/// This transport's one `rustls` `CryptoProvider`, chosen explicitly rather than relying on
/// process-wide install order. This crate's own dependency graph already pulls in both `ring`
/// and `aws-lc-rs` transitively (via other, unrelated dependencies), so relying on
/// `CryptoProvider::install_default()` would make this transport's TLS behavior depend on
/// whichever unrelated code happens to install a default first -- fragile and non-obvious.
/// `ring` is chosen because it is what `quinn`'s and `rcgen`'s own default features already use.
fn crypto_provider() -> Arc<rustls::crypto::CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

/// A `rustls::ServerConfig` presenting the given self-signed certificate, restricted to TLS 1.3
/// (the only version QUIC ever negotiates). ALPN is set to `synapse-quic` so a peer speaking
/// anything else is refused at the handshake.
pub fn server_config(
    cert: CertificateDer<'static>,
    key: PrivateKeyDer<'static>,
) -> Result<Arc<rustls::ServerConfig>> {
    let mut config = rustls::ServerConfig::builder_with_provider(crypto_provider())
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|e| {
            SynapseError::TransportError(format!("Failed to select TLS 1.3 for QUIC server: {e}"))
        })?
        .with_no_client_auth()
        .with_single_cert(vec![cert], key)
        .map_err(|e| {
            SynapseError::TransportError(format!("Failed to build QUIC server TLS config: {e}"))
        })?;
    config.alpn_protocols = vec![b"synapse-quic".to_vec()];
    Ok(Arc::new(config))
}

/// A `rustls::ClientConfig` that accepts any server certificate (see [`DangerAcceptAnyServerCert`]
/// for why that is safe in this one place and nowhere else), restricted to TLS 1.3.
pub fn client_config() -> Result<Arc<rustls::ClientConfig>> {
    let mut config = rustls::ClientConfig::builder_with_provider(crypto_provider())
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|e| {
            SynapseError::TransportError(format!("Failed to select TLS 1.3 for QUIC client: {e}"))
        })?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(DangerAcceptAnyServerCert))
        .with_no_client_auth();
    config.alpn_protocols = vec![b"synapse-quic".to_vec()];
    Ok(Arc::new(config))
}

/// Accepts any server certificate without inspection. **Never reuse this outside the QUIC
/// transport's handshake.** It is sound *here* only because QUIC's TLS layer is wire encryption
/// and connection setup, never identity (spec §3, condition 1): real sender authentication is
/// the sealed and signed `SecureMessage` envelope, verified by `TransportManager` the same way
/// regardless of which transport carried it -- exactly as TCP and WebSocket already assume no
/// transport-level identity, and strictly better than TCP's fully plaintext connection.
/// Anywhere else in this codebase that TLS identity actually matters, this type is the wrong
/// answer; it is deliberately not named `Verifier` or anything a search for "how do we verify
/// TLS certs here" would surface as a general-purpose default.
#[derive(Debug)]
pub struct DangerAcceptAnyServerCert;

impl rustls::client::danger::ServerCertVerifier for DangerAcceptAnyServerCert {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> std::result::Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> std::result::Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> std::result::Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        crypto_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}
```

Verify the exact field name on `rcgen::CertifiedKey` (`key_pair` above) and the exact
`ServerCertVerifier` trait method signatures against the installed crate docs before running:
`cargo doc -p rcgen -p rustls --no-deps --open` (or read `~/.cargo/registry/src/*/rcgen-0.14.10/src/lib.rs` and `~/.cargo/registry/src/*/rustls-0.23.*/src/verify.rs` directly) — the crate's exact minor version may have shifted a field or signature since this plan was written; fix the code above to match what is actually installed, it must compile, not merely resemble this listing.

- [ ] **Step 5: Run it to verify it passes**

Run: `cargo test --lib quic_tls:: 2>&1 | tail -20`
Expected: PASS (1 test)

- [ ] **Step 6: Commit**

```bash
git add src/transport/quic_tls.rs Cargo.toml Cargo.lock
git commit -m "feat(quic): self-signed TLS for the QUIC transport, confined to its own module"
```

- [ ] **Step 7: Write `quic_unified.rs`'s failing end-to-end test**

Add to `tests/transport_repairs.rs`, following the exact shape of `nat_traversal_carries_a_verified_message_end_to_end` (which already uses `round_trip`/`node`/`Pair`):

```rust
// tests/transport_repairs.rs -- add TransportType::Quic to ALL_TRANSPORTS (now 9 entries)
// and to port_key/socket_of:
//   TransportType::Quic => "local_port",             // in port_key
//   TransportType::Quic => Socket::Udp,               // in socket_of (QUIC runs over UDP)

fn quic() -> Box<dyn TransportFactory> {
    Box::new(synapse::transport::QuicTransportFactory)
}

/// QUIC had no working implementation: `QuicTransportFactory::create_transport` always refused
/// (PR A, Task 3). This is the first test to prove it sends and receives at all.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn quic_carries_a_verified_message_end_to_end() {
    let (received, receipt) = round_trip(
        TransportType::Quic,
        quic(),
        quic(),
        free_port(),
        free_port(),
        b"repaired",
        DeliveryConfirmation::Sent,
    )
    .await;
    assert_eq!(received.incoming.transport_type, TransportType::Quic);
    assert_eq!(receipt.transport_used, TransportType::Quic);
    assert_eq!(received.payload, Payload::Opened(b"repaired".to_vec()));
}
```

- [ ] **Step 8: Run it to verify it fails**

Run: `cargo test --test transport_repairs quic_carries_a_verified_message_end_to_end 2>&1 | tail -30`
Expected: FAIL to compile — `QuicTransportFactory` does not exist in `synapse::transport` yet.

- [ ] **Step 9: Implement `quic_unified.rs`**

```rust
// src/transport/quic_unified.rs
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
    local_port: u16,
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
        let mut endpoint = quinn::Endpoint::server(server_quic_config, bind_addr)
            .map_err(|e| SynapseError::TransportError(format!("Failed to bind QUIC endpoint to {bind_addr}: {e}")))?;

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
        self.endpoint
            .local_addr()
            .map_err(|e| SynapseError::TransportError(format!("QUIC endpoint has no local address: {e}")))
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

        let data = serde_json::to_vec(message)
            .map_err(|e| SynapseError::TransportError(format!("Failed to serialize message: {e}")))?;
        send.write_all(&data)
            .await
            .map_err(|e| SynapseError::TransportError(format!("QUIC stream write failed: {e}")))?;
        send.finish()
            .map_err(|e| SynapseError::TransportError(format!("QUIC stream finish failed: {e}")))?;

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
            error: if can_reach { None } else { Some("invalid QUIC target".to_string()) },
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
```

Check `quinn::Endpoint`, `quinn::ServerConfig::with_crypto`, `quinn::crypto::rustls::QuicServerConfig`/`QuicClientConfig`, `Connection::open_bi`/`accept_bi`, `SendStream::write_all`/`finish`, and `RecvStream::read_to_end` against the installed `quinn` 0.11.12's actual docs (`cargo doc -p quinn --no-deps --open`) before running -- these are quinn's stable, documented API shapes as of this plan's writing, but confirm exact signatures (e.g. whether `finish()` returns `Result` or is infallible in the installed version) against the real crate rather than this listing.

Wire `QuicTransportImpl::stop()`'s incomplete accept-loop shutdown as a known Task 1 gap (the same one every other unified transport's `stop()` currently has -- `manager.rs`'s existing follow-up list already tracks this class of issue for TCP/WebSocket): `endpoint.close()` rejects new connections and closes existing ones, but the `tokio::spawn`ed accept loop above keeps running until `endpoint.accept()` returns `None`, which happens once the endpoint is fully closed and drained -- acceptable for Task 1; note it in Task 5's follow-up list rather than fixing it now.

- [ ] **Step 10: Wire up module declarations**

In `src/transport/mod.rs`, alongside the other non-WASM transport modules:

```rust
#[cfg(not(target_arch = "wasm32"))]
pub mod quic_tls;
#[cfg(not(target_arch = "wasm32"))]
pub mod quic_unified;
```

And alongside the other explicit non-glob re-exports (near the `tcp_unified`/`http_unified` re-exports, with a comment matching their existing one):

```rust
#[cfg(not(target_arch = "wasm32"))]
pub use quic_unified::{QuicTransportFactory, QuicTransportImpl};
```

- [ ] **Step 11: Delete the stub factory in `abstraction.rs`**

Find and delete the `// QUIC Transport Factory (RE-ENABLED)` block (`pub struct QuicTransportFactory;` and its `impl TransportFactory for QuicTransportFactory` block, currently returning `Err(SynapseError::TransportError("QUIC is not implemented yet..."))`). The real one now lives in `quic_unified.rs` and is re-exported explicitly (Step 10) — leaving both would be a silent-shadowing hazard identical to the one `tcp_unified`'s comment already warns about.

- [ ] **Step 12: Build and run the end-to-end test**

Run: `cargo build --lib 2>&1 | tail -60` — fix any compile errors against the real installed crate APIs (per the verification notes in Steps 4 and 9) before proceeding.
Run: `cargo test --test transport_repairs quic_carries_a_verified_message_end_to_end 2>&1 | tail -40`
Expected: PASS (1 test). If the handshake fails, check that both endpoints agree on ALPN (`synapse-quic`) and that the client's `connect` call uses the same server-name string the server's cert is generated for consistency (rcgen's `generate_simple_self_signed(vec!["synapse-quic".to_string()])` and `endpoint.connect(addr, "synapse-quic")` in Step 9 must match).

- [ ] **Step 13: Commit**

```bash
git add src/transport/quic_unified.rs src/transport/mod.rs src/transport/abstraction.rs tests/transport_repairs.rs
git commit -m "feat(quic): a real quinn-backed transport, one connection per message"
```

---

## Task 2: Connection pooling — reuse, multiplexed streams, idle-timeout eviction

**Files:**
- Modify: `src/transport/quic_unified.rs`
- Test: `tests/transport_repairs.rs`

**Interfaces:**
- Consumes: `QuicTransportImpl` from Task 1 (its `endpoint` field, `parse_target`).
- Produces: `QuicTransportImpl::pooled_connection(&self, addr: SocketAddr) -> Result<Arc<quinn::Connection>>` (new private method `send_message` calls instead of connecting fresh every time); `QuicTransportImpl::evict(&self, addr: SocketAddr)` (called on an application-layer verification failure — wired to the manager's verification path is a later concern; Task 2 provides the method and a direct test of it, per spec §3 condition 3).

- [ ] **Step 1: Write the failing test — a second concurrent send reuses the pooled connection**

```rust
// tests/transport_repairs.rs
/// Task 2: the pool reuses one connection for concurrent sends to the same peer instead of
/// opening a fresh one each time -- proven by counting `quinn::Endpoint`'s reported connection
/// count on Bob's side rather than guessing from timing.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn quic_loses_no_message_under_concurrent_sends() {
    loses_no_message_under_concurrent_sends(TransportType::Quic, quic(), quic(), 32, 200).await;
}
```

(This reuses the existing `loses_no_message_under_concurrent_sends` helper already shared by `tcp_loses_no_message_under_concurrent_sends` and `websocket_loses_no_message_under_concurrent_sends` — no new helper needed.)

- [ ] **Step 2: Run it against Task 1's code to verify it currently passes only by accident of correctness, not efficiency**

Run: `cargo test --test transport_repairs quic_loses_no_message_under_concurrent_sends 2>&1 | tail -20`
Expected: PASS even before Task 2's pooling exists — Task 1's one-connection-per-message code is *correct*, just wasteful (200 handshakes instead of 1). This test alone cannot distinguish pooled from unpooled; Step 3 adds the test that can.

- [ ] **Step 3: Write the test that actually proves pooling**

```rust
// tests/transport_repairs.rs
/// Two sends to the same peer must not open two connections: the second reuses the first's.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn quic_reuses_one_connection_for_two_sends_to_the_same_peer() {
    let pair = Pair::new(TransportType::Quic, quic(), quic(), free_port(), free_port()).await;
    let m1 = pair.signed(b"first");
    let m2 = pair.signed(b"second");
    pair.alice_node.send_message(&pair.bob_target(), &m1).await.expect("send 1");
    pair.alice_node.send_message(&pair.bob_target(), &m2).await.expect("send 2");
    // Bob's manager received both, over one underlying connection: assert via Bob's transport
    // metrics().active_connections (Task 1 leaves this at 0; Task 4 wires it to a real count,
    // but Task 2 must make it possible to observe the pool's single entry directly here).
    let received = poll_bob(&pair, 2, std::time::Duration::from_secs(3)).await;
    assert_eq!(received.len(), 2);
}
```

- [ ] **Step 4: Run it to verify it currently only checks message delivery, not pooling** (this is expected — the assertion strengthens in Step 6 once the pool is inspectable)

Run: `cargo test --test transport_repairs quic_reuses_one_connection_for_two_sends_to_the_same_peer 2>&1 | tail -20`
Expected: PASS on message delivery (Task 1 already handles two independent sends correctly).

- [ ] **Step 5: Implement the pool**

`quinn::proto::ConnectionStats`/`PathStats` (checked against the installed `quinn-proto 0.11.18`
source) expose RTT, congestion, and packet/byte counters — nothing resembling "time since last
use". So the pool tracks last-use time itself, as the value alongside each connection, updated on
every hit or insert:

```rust
// src/transport/quic_unified.rs -- add to QuicTransportImpl's fields:
    pool: Arc<Mutex<HashMap<SocketAddr, (Arc<quinn::Connection>, std::time::Instant)>>>,
    idle_timeout: std::time::Duration,

// In QuicTransportImpl::new, after building `endpoint`:
    pool: Arc::new(Mutex::new(HashMap::new())),
    idle_timeout: std::time::Duration::from_secs(300), // Step 7 replaces this with a config-driven value

// New method on QuicTransportImpl:
    /// Reuse a pooled connection to `addr`, or establish one. The pool's lock is never held
    /// across the handshake `await` (spec §3, condition 2): a miss releases the lock, connects,
    /// then re-acquires it to insert -- double-checking for a connection a racing sender
    /// established first, keeping that one and dropping the one just established, rather than
    /// overwrite it. Every return path refreshes the entry's last-use `Instant`, so the idle
    /// sweep (below) only evicts a connection nothing has used recently.
    async fn pooled_connection(&self, addr: SocketAddr) -> Result<Arc<quinn::Connection>> {
        {
            let mut pool = self.pool.lock().await;
            if let Some((conn, last_used)) = pool.get_mut(&addr) {
                if conn.close_reason().is_none() {
                    *last_used = std::time::Instant::now();
                    return Ok(Arc::clone(conn));
                }
            }
        }
        let connecting = self
            .endpoint
            .connect(addr, "synapse-quic")
            .map_err(|e| SynapseError::TransportError(format!("QUIC connect setup failed: {e}")))?;
        let fresh = Arc::new(
            connecting
                .await
                .map_err(|e| SynapseError::TransportError(format!("QUIC handshake failed: {e}")))?,
        );
        let mut pool = self.pool.lock().await;
        if let Some((existing, last_used)) = pool.get_mut(&addr) {
            if existing.close_reason().is_none() {
                *last_used = std::time::Instant::now();
                return Ok(Arc::clone(existing));
            }
        }
        pool.insert(addr, (Arc::clone(&fresh), std::time::Instant::now()));
        Ok(fresh)
    }

    /// Evict a pooled connection to `addr` -- called when a message received over it fails
    /// application-layer verification (spec §3, condition 3), so a connection that has started
    /// reaching the wrong peer (NAT rebinding, address reuse) is not silently reused again.
    pub async fn evict(&self, addr: SocketAddr) {
        if let Some((conn, _)) = self.pool.lock().await.remove(&addr) {
            conn.close(0u32.into(), b"evicted: verification failure");
        }
    }

// Replace send_message's connection setup (the `let connecting = ...` through `let connection = ...`
// lines from Task 1) with:
    let connection = self.pooled_connection(addr).await?;

// Add an idle-eviction sweep, spawned once in start():
    {
        let pool = Arc::clone(&self.pool);
        let idle_timeout = self.idle_timeout;
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(idle_timeout / 2);
            loop {
                interval.tick().await;
                let now = std::time::Instant::now();
                let mut pool = pool.lock().await;
                pool.retain(|_, (conn, last_used)| {
                    now.duration_since(*last_used) < idle_timeout && conn.close_reason().is_none()
                });
            }
        });
    }
```

- [ ] **Step 6: Strengthen the pooling test now that eviction/insertion is inspectable**

```rust
// tests/transport_repairs.rs -- extend quic_reuses_one_connection_for_two_sends_to_the_same_peer
// (Step 3's test) with a pool-size assertion once QuicTransportImpl exposes one. Add to
// quic_unified.rs, gated #[cfg(test)] or via a crate-visible accessor used only by tests:
    #[cfg(test)]
    pub(crate) async fn pool_size(&self) -> usize {
        self.pool.lock().await.len()
    }
```

Then in the test, after both sends: assert Alice's node's QUIC transport reports a pool of exactly 1 entry for Bob's address (requires threading a way to reach the underlying `QuicTransportImpl` from `TransportManager` in the test — follow the pattern `tests/transport_repairs.rs` already uses where a test needs to reach past the manager, e.g. how `websocket_closes_a_peer_that_sends_a_control_frame_and_frees_its_permit` opens a raw connection directly rather than going through the manager only).

- [ ] **Step 7: Write the idle-eviction test**

```rust
// tests/transport_repairs.rs
/// An idle connection is evicted and the next send transparently reopens one.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn quic_reopens_after_the_pool_evicts_an_idle_connection() {
    let mut config = HashMap::new();
    config.insert("idle_timeout_ms".to_string(), "200".to_string());
    let pair = Pair::with_config(
        TransportType::Quic, quic(), quic(), free_port(), free_port(), &config,
    ).await;
    let m1 = pair.signed(b"first");
    pair.alice_node.send_message(&pair.bob_target(), &m1).await.expect("send 1");
    tokio::time::sleep(std::time::Duration::from_millis(500)).await; // past the idle timeout
    let m2 = pair.signed(b"second");
    pair.alice_node.send_message(&pair.bob_target(), &m2).await.expect("send 2 after reopen");
    let received = poll_bob(&pair, 2, std::time::Duration::from_secs(3)).await;
    assert_eq!(received.len(), 2, "both messages must arrive even after an idle-timeout reopen");
}
```

This introduces a new config key, `idle_timeout_ms` — add it to `QuicTransportFactory::default_config` and `validate_config` (parse as `u64`, refuse if unparseable or zero, matching the pattern `websocket_unified.rs`'s `HANDSHAKE_TIMEOUT_MS_KEY` already follows) and wire it into `QuicTransportImpl::new` in place of the hardcoded `Duration::from_secs(300)` from Step 5.

- [ ] **Step 8: Run all three new tests**

Run: `cargo test --test transport_repairs quic_ 2>&1 | tail -60`
Expected: PASS (4 tests: the Task 1 end-to-end test plus this task's three).

- [ ] **Step 9: Commit**

```bash
git add src/transport/quic_unified.rs tests/transport_repairs.rs
git commit -m "feat(quic): pool connections per peer, evict on idle timeout or verification failure"
```

---

## Task 3: Size limits, backpressure, and config validation

**Files:**
- Modify: `src/transport/quic_unified.rs`
- Test: `tests/transport_repairs.rs`

**Interfaces:**
- Consumes: `QuicTransportImpl`/`QuicTransportFactory` from Tasks 1-2.
- Produces: config keys `max_message_size` (default `1048576`), `max_queued_bytes` (default `4194304`), `first_byte_timeout_ms`/`idle_read_timeout_ms` (default `5000` each) — following the exact naming and default-value conventions `tcp_unified.rs`/`websocket_unified.rs`/`http_unified.rs` already established, so a config consumer sees one consistent key vocabulary across every transport.

- [ ] **Step 1: Write the failing oversize-refusal test**

Follow `tcp_refuses_at_send_a_message_over_its_limit`'s exact pattern (same file) rather than
importing a new error type: assert on the refusal's `.to_string()` naming the limit and the
message's size, not on the error variant. This also matches this codebase's established
convention of never importing `SynapseError` into this test file.

```rust
// tests/transport_repairs.rs
const QUIC_LIMIT: usize = 1024;

fn quic_config(key: &str, value: &str) -> HashMap<String, String> {
    HashMap::from([(key.to_string(), value.to_string())])
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn quic_refuses_at_send_a_message_over_its_limit() {
    let config = quic_config("max_message_size", &QUIC_LIMIT.to_string());
    let pair = Pair::with_config(
        TransportType::Quic, quic(), quic(), free_port(), free_port(), &config,
    ).await;
    let (over, over_size) = measured(&pair, QUIC_LIMIT, false);

    let alice_transport = synapse::transport::QuicTransportFactory
        .create_transport(&config)
        .await
        .expect("a client-only transport with the same limit");
    let refusal = match alice_transport.send_message(&pair.bob_target(), &over).await {
        Ok(receipt) => panic!(
            "a {over_size}-byte message over the {QUIC_LIMIT}-byte limit must be refused, not {:?}",
            receipt.confirmation
        ),
        Err(e) => e.to_string(),
    };
    assert!(
        refusal.contains("max_message_size")
            && refusal.contains(&over_size.to_string())
            && refusal.contains(&QUIC_LIMIT.to_string()),
        "the refusal must name the limit and both sizes: {refusal}"
    );
    // And through the manager, the send fails rather than claiming a delivery.
    assert!(pair.alice_node.send_message(&pair.bob_target(), &over).await.is_err());
}
```

Match this refusal-text shape exactly in Task 3 Step 3's `SynapseError::MessageRefused(format!(...))`
call — the format string there must include the literal substring `"max_message_size"` plus both
the over-size byte count and the configured limit, or this test's `.contains(...)` assertions fail.

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test --test transport_repairs quic_refuses_at_send_a_message_over_its_limit 2>&1 | tail -20`
Expected: FAIL — Task 1/2's `send_message` has no size check yet, so the oversize message is sent successfully instead of refused.

- [ ] **Step 3: Add the config keys and validation**

```rust
// src/transport/quic_unified.rs
pub const MAX_MESSAGE_SIZE_KEY: &str = "max_message_size";
pub const DEFAULT_MAX_MESSAGE_SIZE: usize = 1024 * 1024;
pub const MAX_QUEUED_BYTES_KEY: &str = "max_queued_bytes";
pub const DEFAULT_MAX_QUEUED_BYTES: usize = 4 * 1024 * 1024;
pub const IDLE_TIMEOUT_MS_KEY: &str = "idle_timeout_ms"; // already added in Task 2 Step 7
pub const DEFAULT_IDLE_TIMEOUT_MS: u64 = 300_000;

pub fn validate_config(config: &HashMap<String, String>) -> Result<()> {
    if let Some(port_str) = config.get(LOCAL_PORT_KEY)
        && port_str.parse::<u16>().is_err()
    {
        return Err(SynapseError::Config("Invalid port number".to_string()));
    }
    let max_message_size = parse_positive(config, MAX_MESSAGE_SIZE_KEY, DEFAULT_MAX_MESSAGE_SIZE)?;
    let max_queued_bytes = parse_positive(config, MAX_QUEUED_BYTES_KEY, DEFAULT_MAX_QUEUED_BYTES)?;
    if max_queued_bytes < max_message_size || max_queued_bytes > u32::MAX as usize {
        return Err(SynapseError::Config(format!(
            "max_queued_bytes ({max_queued_bytes}) must be at least max_message_size \
             ({max_message_size}) and at most {}",
            u32::MAX
        )));
    }
    if let Some(ms_str) = config.get(IDLE_TIMEOUT_MS_KEY) {
        let ms: u64 = ms_str
            .parse()
            .map_err(|_| SynapseError::Config("Invalid idle_timeout_ms".to_string()))?;
        if ms == 0 {
            return Err(SynapseError::Config("idle_timeout_ms must not be zero".to_string()));
        }
    }
    Ok(())
}

fn parse_positive(config: &HashMap<String, String>, key: &str, default: usize) -> Result<usize> {
    match config.get(key) {
        None => Ok(default),
        Some(s) => {
            let value: usize = s
                .parse()
                .map_err(|_| SynapseError::Config(format!("Invalid {key}")))?;
            if value == 0 {
                return Err(SynapseError::Config(format!("{key} must not be zero")));
            }
            Ok(value)
        }
    }
}
```

Store `max_message_size` on `QuicTransportImpl` (a new field, set in `new()` via `parse_positive`), and at the top of `send_message`:

```rust
    let data = serde_json::to_vec(message)
        .map_err(|e| SynapseError::TransportError(format!("Failed to serialize message: {e}")))?;
    if data.len() > self.max_message_size {
        return Err(SynapseError::MessageRefused(format!(
            "message is {} bytes, over the max_message_size limit of {} bytes",
            data.len(),
            self.max_message_size
        )));
    }
```

(moved before the `pooled_connection` call, so an oversize message never opens a connection — matching Task 7's rule that a local refusal must not touch the circuit breaker).

- [ ] **Step 4: Run it to verify it passes, and add it to `default_config`**

Also update `QuicTransportFactory::default_config()` to return every key (`local_port`, `max_message_size`, `max_queued_bytes`, `idle_timeout_ms`), matching `WebSocketTransportFactory::default_config`'s pattern of returning every key rather than a partial set.

Run: `cargo test --test transport_repairs quic_refuses_at_send_a_message_over_its_limit 2>&1 | tail -20`
Expected: PASS

- [ ] **Step 5: Write the receive-side cap test and the byte-budget queue**

```rust
// tests/transport_repairs.rs
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn quic_refuses_an_invalid_config() {
    quic_refuses(HashMap::from([("max_message_size".to_string(), "0".to_string())]), "max_message_size");
    quic_refuses(HashMap::from([("max_queued_bytes".to_string(), "10".to_string()), ("max_message_size".to_string(), "1024".to_string())]), "max_queued_bytes");
    quic_refuses(HashMap::from([("idle_timeout_ms".to_string(), "0".to_string())]), "idle_timeout_ms");
}

fn quic_refuses(config: HashMap<String, String>, key: &str) {
    let err = synapse::transport::QuicTransportFactory
        .validate_config(&config)
        .expect_err(&format!("{key} must be refused"));
    assert!(format!("{err}").contains(key), "{err}");
}
```

Apply the receiver-side cap by bounding the `recv.read_to_end(...)` call in the accept loop (Task 1, Step 9) with `self.max_message_size` instead of the hardcoded `8 * 1024 * 1024`, and thread a byte-budget semaphore (`tokio::sync::Semaphore`, sized by `max_queued_bytes`, acquired via `acquire_many_owned(len as u32)` before parsing and held by the queued `IncomingMessage` until `receive_raw` drains it) into `QuicTransportImpl`, following `tcp_unified.rs`'s `queue_budget` field and its acquire-before-parse/hold-until-drained pattern exactly (read that file's relevant section before implementing this step — do not re-derive the pattern from scratch when a working, reviewed implementation already exists to copy the shape of).

- [ ] **Step 6: Run every QUIC test**

Run: `cargo test --test transport_repairs quic_ 2>&1 | tail -80`
Expected: PASS (7 tests total across Tasks 1-3)

- [ ] **Step 7: Commit**

```bash
git add src/transport/quic_unified.rs tests/transport_repairs.rs
git commit -m "feat(quic): size limits, backpressure, and config validation"
```

---

## Task 4: Honest capabilities, estimate_metrics, and metrics

**Files:**
- Modify: `src/transport/abstraction.rs` (`TransportCapabilities::quic()`)
- Modify: `src/transport/quic_unified.rs`
- Test: `tests/transport_repairs.rs`

**Interfaces:**
- Consumes: `QuicTransportImpl` from Tasks 1-3.
- Produces: `TransportCapabilities::quic()` returning measured-shape figures (called by nothing yet outside this module until `QuicTransportImpl::capabilities()` uses it — Task 1's inline capabilities() literal is replaced by a call to it, matching how `tcp_unified.rs`'s `capabilities()` calls `TransportCapabilities::tcp()`).

- [ ] **Step 1: Write the failing test for a real connectivity probe**

```rust
// tests/transport_repairs.rs
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn quic_reports_connectivity_only_after_a_real_handshake() {
    let pair = Pair::new(TransportType::Quic, quic(), quic(), free_port(), free_port()).await;
    let transport = synapse::transport::QuicTransportFactory
        .create_transport(&HashMap::new())
        .await
        .expect("construct");
    let live = transport.test_connectivity(&pair.bob_target()).await.expect("connectivity");
    assert!(live.connected, "{live:?}");
    assert!(live.rtt.is_some());

    let nobody = TransportTarget::new(BOB.to_string())
        .with_address(format!("127.0.0.1:{}", free_port()));
    let dead = transport.test_connectivity(&nobody).await.expect("connectivity");
    assert!(!dead.connected, "{dead:?}");
    assert_eq!(dead.rtt, None);
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test --test transport_repairs quic_reports_connectivity_only_after_a_real_handshake 2>&1 | tail -20`
Expected: FAIL — Task 1's `test_connectivity` only calls `can_reach` (an address-parse check), never actually connects, so `dead.connected` is `true` when it should be `false`.

- [ ] **Step 3: Implement a real probe**

```rust
// src/transport/quic_unified.rs -- replace test_connectivity's body:
    async fn test_connectivity(&self, target: &TransportTarget) -> Result<ConnectivityResult> {
        let addr = match parse_target(target) {
            Ok(addr) => addr,
            Err(e) => {
                return Ok(ConnectivityResult {
                    connected: false,
                    rtt: None,
                    error: Some(e.to_string()),
                    quality: 0.0,
                    details: HashMap::new(),
                });
            }
        };
        let start = std::time::Instant::now();
        match self.pooled_connection(addr).await {
            Ok(conn) => Ok(ConnectivityResult {
                connected: true,
                rtt: Some(start.elapsed()),
                error: None,
                quality: 0.9,
                details: {
                    let mut d = HashMap::new();
                    d.insert("rtt_ms".to_string(), conn.rtt().as_millis().to_string());
                    d
                },
            }),
            Err(e) => Ok(ConnectivityResult {
                connected: false,
                rtt: None,
                error: Some(e.to_string()),
                quality: 0.0,
                details: HashMap::new(),
            }),
        }
    }

// And estimate_metrics:
    async fn estimate_metrics(&self, target: &TransportTarget) -> Result<TransportEstimate> {
        let timeout = std::time::Duration::from_millis(2000);
        let live = tokio::time::timeout(timeout, self.test_connectivity(target)).await;
        match live {
            Ok(Ok(result)) if result.connected => Ok(TransportEstimate {
                latency: result.rtt.unwrap_or(timeout),
                reliability: 0.9,
                bandwidth: 1,
                cost: 1.0,
                available: true,
                confidence: 1.0,
            }),
            _ => Ok(TransportEstimate {
                latency: timeout,
                reliability: 0.0,
                bandwidth: 0,
                cost: 1.0,
                available: false,
                confidence: 0.3,
            }),
        }
    }
```

Check `quinn::Connection::rtt() -> Duration` against the installed version's actual method name (it may be `rtt()` returning `Duration` directly, or nested under `connection.stats().path.rtt` depending on the exact 0.11.12 API — verify with `cargo doc -p quinn --no-deps --open` before finalizing this step, same caveat as Task 1).

- [ ] **Step 4: Run it to verify it passes**

Run: `cargo test --test transport_repairs quic_reports_connectivity_only_after_a_real_handshake 2>&1 | tail -20`
Expected: PASS

- [ ] **Step 5: Replace the capabilities preset**

```rust
// src/transport/abstraction.rs -- replace TransportCapabilities::quic()'s body:
    /// QUIC transport capabilities, as actually implemented (`quic_unified.rs`) -- not what QUIC
    /// in general can do. `zero_rtt` and `connection_migration` are deliberately absent: this
    /// implementation disables 0-RTT (spec §5) and does not implement migration.
    pub fn quic() -> Self {
        Self {
            max_message_size: 1024 * 1024, // the configured default; see quic_unified::DEFAULT_MAX_MESSAGE_SIZE
            reliable: true,
            real_time: true,
            broadcast: false,
            bidirectional: true,
            encrypted: true,
            network_spanning: true,
            supported_urgencies: vec![
                MessageUrgency::Critical,
                MessageUrgency::RealTime,
                MessageUrgency::Interactive,
            ],
            features: vec![
                "multiplexed_streams".to_string(),
                "connection_pooling".to_string(),
            ],
        }
    }
```

Update `QuicTransportImpl::capabilities()` (Task 1's inline literal) to call `TransportCapabilities::quic()` with `max_message_size` overridden to `self.max_message_size` (the configured value, not the constant), following `tcp_unified.rs`'s exact pattern: `TransportCapabilities { max_message_size: self.max_message_size, ..TransportCapabilities::tcp() }`.

- [ ] **Step 6: Wire real metrics counters**

Add `sent: std::sync::atomic::AtomicU64` and `received: std::sync::atomic::AtomicU64` fields to `QuicTransportImpl` (matching the simplest existing counter pattern in the codebase — check `udp_unified.rs`'s `TransportMetrics` usage for the exact field names `metrics()` must populate), incremented in `send_message` and the accept loop respectively, and return them from `metrics()` instead of `TransportMetrics::default()`.

- [ ] **Step 7: Run every QUIC test**

Run: `cargo test --test transport_repairs quic_ 2>&1 | tail -100`
Expected: PASS (8 tests total across Tasks 1-4)

- [ ] **Step 8: Commit**

```bash
git add src/transport/abstraction.rs src/transport/quic_unified.rs tests/transport_repairs.rs
git commit -m "feat(quic): honest capabilities, a real connectivity probe, and real metrics"
```

---

## Task 5: Verification

**Files:** none (verification only; docs updates as findings emerge)

- [ ] **Step 1: Predict a mutation, in writing, before running anything**

Prediction: re-introducing a version of Task 2's bug — making `pooled_connection` always establish a fresh connection instead of checking the pool first (i.e., deleting the `if let Some(conn) = self.pool.lock()...` early-return block) — fails only `quic_reuses_one_connection_for_two_sends_to_the_same_peer` (Task 2, Step 6's strengthened pool-size assertion), not any other QUIC test and not any TCP/WebSocket/HTTP/NAT test.

- [ ] **Step 2: Apply the mutation and run, with the execution step capped by a hard timeout**

This session's own transport-contract work hit two multi-hour hangs from mutations that create a half-open connection nothing services, blocked on the OS's own TCP keepalive rather than any test-level timeout (see `.superpowers/sdd/2026-09-18-transport-contract/progress.md`, "Task 11"). QUIC's mutations are less likely to produce that specific failure mode (no half-open TCP-style connection is possible — QUIC either completes a real handshake or the `connect().await` fails outright), but cap the run anyway, on principle, rather than re-learning the lesson:

```bash
cargo build --tests   # compile step, no timeout -- this legitimately takes minutes
timeout 120 cargo test --test transport_repairs quic_ 2>&1 | tail -80
```

Expected: the predicted test fails; every other QUIC test and every non-QUIC test in the same binary still passes. Revert the mutation (`git diff --stat` should be empty against the last commit) and re-run to confirm all 8 QUIC tests pass again.

- [ ] **Step 3: Full run in a target directory that has only ever built the current source**

Per this session's own resolved lesson (a literally-fresh `CARGO_TARGET_DIR` hit transient `curve25519-dalek-derive` linker failures on this machine twice; a target directory that has only ever compiled the current, un-mutated source state satisfies the actual intent of "fresh" — avoiding stale binaries from a *different* source state):

```bash
date -u +"%Y-%m-%dT%H:%M:%SZ"   # record the window start
CARGO_TARGET_DIR=/some/dedicated/dir cargo test 2>&1 | tail -300
date -u +"%Y-%m-%dT%H:%M:%SZ"   # record the window end
```

Expected: every test binary reports `0 failed`. Record the exact window and the per-binary pass counts in a new entry under `.superpowers/sdd/2026-09-18-transport-contract/progress.md` (or a new QUIC-specific ledger file, if one is started for this slice) the same way PR B's Task 11 did.

- [ ] **Step 4: Firewall event 2097 count for that window, with a positive control**

```powershell
$start = [DateTime]::Parse("<window start>").ToUniversalTime()
$end = [DateTime]::Parse("<window end>").ToUniversalTime()
(Get-WinEvent -FilterHashtable @{LogName='Microsoft-Windows-Windows Firewall With Advanced Security/Firewall'; Id=2097; StartTime=$start; EndTime=$end} -ErrorAction SilentlyContinue).Count
(Get-WinEvent -FilterHashtable @{LogName='Microsoft-Windows-Windows Firewall With Advanced Security/Firewall'; Id=2097} -ErrorAction SilentlyContinue).Count
```

Expected: the windowed count is 0 (every QUIC test binds loopback via `BindScope`, per the Global Constraints); the unfiltered count is nonzero, proving the query itself works.

- [ ] **Step 5: fmt and clippy**

```bash
cargo fmt --check
cargo clippy --lib --tests 2>&1 | tail -100
```

Expected: no diffs from `fmt`; no new clippy warnings in `quic_unified.rs`, `quic_tls.rs`, or `abstraction.rs`'s changed sections (pre-existing warnings elsewhere are not this task's concern, per this session's own established practice of checking `grep -n "quic" <clippy output>` rather than treating an unrelated warning as a blocker).

- [ ] **Step 6: Docs**

Update `CAPABILITY_INVENTORY.md`: mark the QUIC-related findings noted in this plan's spec §1 (the "1GB theoretical"/aspirational-capabilities finding, if `CAPABILITY_INVENTORY.md` records one) as fixed, with a ref to this slice's PR (cite "the QUIC slice", not a SHA, until merged — matching Task 7's established practice for unmerged work).

Add the breaking-change list for the 2.0 release notes (`TransportType::Quic`'s factory now constructs a real transport instead of always refusing; `TransportCapabilities::quic()`'s figures changed from aspirational to measured; new config keys with construction-time validation) to whichever document collects the transport contract's breaking changes for the eventual 2.0.0 release notes.

- [ ] **Step 7: Open the PR**

Target `main`. Body: link the spec, the plan, the verification numbers with their ref (window, pass counts, firewall count with its control), the mutation-check result, and the breaking-change list. Send the PM a `[READY]` per `CLAUDE.md` §10, the same way PR B's did — including actual check-run states, not "in progress" (this session's own corrected mistake from PR B: a count on an unfinished check is not a number yet).

**Do not merge, and do not treat this task's completion as authorization to publish 2.0.0.** That remains CireSnave's approval alone, regardless of how many slices land.

---

## Self-Review Notes (for whoever executes this plan)

- **Spec coverage:** §2 (task shape) → Tasks 1-5 above. §3 (pool, identity, all three PM conditions) → Task 1 Steps 4/9 (verifier module, condition 1), Task 2 Step 5 (lock-release pattern, condition 2; eviction method, condition 3). §4 (delivery claim/ack composition) → Task 1's module doc and `send_message`'s `Sent`-only return; no ack code added anywhere, matching "QUIC needs zero new ack code." §5 (0-RTT) → Task 1's `quic_tls.rs` never enables it; no step in this plan turns it on. §6 (framing/size limits) → Task 3. §7 (capabilities/metrics) → Task 4. §8 (testing/verification) → Task 5.
- **API shapes verified against the actually-installed crates (PM review condition 1), not left as a plan-writing-time reconstruction.** `quinn = "0.11.12"` and `rcgen = "0.14.10"` were added temporarily and their real source (`~/.cargo/registry/src/*/{quinn-0.11.12,quinn-proto-0.11.18,rcgen-0.14.10,rustls-0.23.45}`) read directly. Two real corrections came out of this: `rcgen::CertifiedKey`'s field is `signing_key`, not `key_pair` (fixed throughout Task 1); and this workspace's dependency graph already pulls in *both* `ring` and `aws-lc-rs` transitively (via unrelated dependencies), so `quic_tls.rs` selects its `CryptoProvider` explicitly (`builder_with_provider`, restricted to TLS 1.3) rather than relying on `CryptoProvider::install_default()`'s process-wide, order-dependent behavior — a real hazard the original draft's "either provider wins, it doesn't matter" reasoning missed. Every other reconstructed signature (`ServerCertVerifier`'s three verify methods, `Connection::{open_bi,accept_bi,rtt,close_reason,remote_address,close}`, `SendStream::{write_all,finish}`, `RecvStream::read_to_end`, `Endpoint::{server,client,connect,accept,set_default_client_config}`, `quinn::crypto::rustls::{QuicServerConfig,QuicClientConfig}`'s `TryFrom<Arc<rustls::{Server,Client}Config>>` impls) matched the plan's original draft exactly. The temporary dependency additions were reverted from `Cargo.toml`/`Cargo.lock` after verification, so Task 1 Step 1 still does the real, first addition.
- **Task 2's idle-eviction design resolved (PM review condition 2), not left as a choice for whoever implements it.** `quinn-proto 0.11.18`'s `ConnectionStats`/`PathStats` (read directly) expose RTT, congestion, and packet/byte counters — nothing resembling "time since last use". The pool tracks last-use `Instant` itself, as part of each entry's value (`HashMap<SocketAddr, (Arc<Connection>, Instant)>`), refreshed on every hit or insert inside `pooled_connection`, and compared against `idle_timeout` by a periodic sweep. This is a decision, not a placeholder.
- **Type consistency check:** `QuicTransportImpl`/`QuicTransportFactory` names, `LOCAL_PORT_KEY`/`MAX_MESSAGE_SIZE_KEY`/`MAX_QUEUED_BYTES_KEY`/`IDLE_TIMEOUT_MS_KEY` constants, and `pooled_connection`/`evict`/`pool_size` method names are used identically across Tasks 1-4 as introduced in Task 1/2 — no renaming drift found on this pass.
