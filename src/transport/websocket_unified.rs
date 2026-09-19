// SPDX-License-Identifier: MIT OR Apache-2.0
//! WebSocket transport conforming to the unified Transport trait
//!
//! # Wire format
//!
//! One `SecureMessage` per connection, as for TCP. The sender opens a connection with a real
//! WebSocket handshake (`connect_async`, an HTTP upgrade), writes the message's JSON as one binary
//! message in one frame, flushes it, then starts the closing handshake and waits for the
//! receiver's close frame. The receiver completes the handshake (`accept_async`), reads that one
//! binary message, finishes the closing handshake, and only then queues the message.
//!
//! **Why one message per connection, not a connection kept open per peer.** A kept-open
//! connection would have to store each peer's write half, notice when a peer has gone away and
//! reconnect, and expire idle connections. It would also weaken `Sent`: a write into a
//! connection whose peer has silently died still succeeds, into the local socket buffer. Each of
//! those is a new way to fail. One message per connection costs one extra round trip per message
//! (the handshake), keeps no state between sends, and lets every limit and bound below carry over
//! from TCP unchanged: one connection permit covers one message.
//!
//! # Delivery claim
//!
//! `send_message` returns `Sent` once the frame is written and flushed to the socket, and never
//! `Delivered`: the receiver sends no application-level acknowledgement. The receiver's close
//! frame shows that its WebSocket stack read the frame. But the receiver sends it before it
//! queues the message, and a message can still be dropped after that (for instance if it does not
//! parse), so it is not treated as a delivery.
//!
//! # Config keys
//!
//! | key | default | meaning |
//! |---|---|---|
//! | [`LOCAL_PORT_KEY`] (`local_port`) | `0` | the port `start` listens on; `0` lets the OS choose |
//! | `bind_scope` | `loopback` | which interfaces the listener binds ([`crate::network_scope::BindScope`]) |
//! | [`CONNECTION_TIMEOUT_MS_KEY`] (`connection_timeout_ms`) | [`DEFAULT_CONNECTION_TIMEOUT_MS`] (30000) | how long `send_message` waits to connect and complete the handshake, and then to write the frame |
//! | [`MAX_MESSAGE_SIZE_KEY`] (`max_message_size`) | [`DEFAULT_MAX_MESSAGE_SIZE`] (1 MiB) | the largest message, **in bytes of serialized JSON**, the receiver reads and the sender sends; also tungstenite's `max_message_size` and `max_frame_size` |
//! | [`MAX_CONCURRENT_CONNECTIONS_KEY`] (`max_concurrent_connections`) | [`DEFAULT_MAX_CONCURRENT_CONNECTIONS`] (64) | how many inbound connections are handled at once |
//! | [`MAX_QUEUED_BYTES_KEY`] (`max_queued_bytes`) | [`DEFAULT_MAX_QUEUED_BYTES`] (4 MiB) | how many bytes of received messages, **counted as serialized JSON**, may wait for the application to poll; at least `max_message_size` and at most `u32::MAX` |
//! | [`HANDSHAKE_TIMEOUT_MS_KEY`] (`handshake_timeout_ms`) | [`DEFAULT_HANDSHAKE_TIMEOUT_MS`] (5000) | how long a new inbound connection may take to complete the WebSocket handshake |
//! | [`IDLE_TIMEOUT_MS_KEY`] (`idle_timeout_ms`) | [`DEFAULT_IDLE_TIMEOUT_MS`] (5000) | how long a connection may stay silent between two frames, on either side |
//!
//! `local_port` must parse as a port number, and every other key but `bind_scope` as a positive
//! integer: [`WebSocketTransportImpl::new`] (and so the factory's `create_transport`) refuses
//! anything else instead of falling back to the default. It also refuses a `max_queued_bytes`
//! below `max_message_size`, which would leave a message that passed the size check waiting
//! forever for room the queue can never have, and one above `u32::MAX`, the most one semaphore
//! acquire can take.
//!
//! `max_message_size` counts serialized bytes, not body bytes: `encrypted_content` serializes as
//! a JSON number array, a few characters per body byte. Sender and receiver apply the same number
//! to the same bytes, so a sender refuses exactly what a receiver with its limit would drop. The
//! refusal is a [`SynapseError::MessageRefused`](crate::error::SynapseError::MessageRefused),
//! which the transport manager does not count against the transport's health.
//!
//! # What an unauthenticated peer can make the receiver hold
//!
//! The same formula as TCP's (see `tcp_unified`), with the same adversarial parse factor. Nothing
//! below is authenticated: the receiver reads, parses and queues a message before anyone checks
//! its signature. Write `C` for `max_concurrent_connections`, `M` for `max_message_size`, `B` for
//! `max_queued_bytes`, and `f` for the parse factor -- the heap a `SecureMessage` takes, while it
//! is parsed and after, per byte of its JSON. The sender chooses the JSON, so `f` is adversarial;
//! as measured for TCP, take `f` ≈ 18 while parsing and ≈ 12 retained (the largest factors
//! measured, not proven maxima). The worst case is the sum of:
//!
//! - **connection buffers:** `C × (4M + 80 KiB)` bytes. At most `C` connections are handled at
//!   once. Each holds, from reading tungstenite 0.30 and `bytes` rather than from a measurement:
//!   the handshake buffer, which tungstenite refuses to grow past 64 KiB; a frame buffer, which
//!   starts as the 8 KiB read buffer and which tungstenite grows to hold one whole frame of at most
//!   `M` bytes (it checks the frame's length against `max_frame_size` before reserving), and whose
//!   growth may double, so up to `2(M + 8 KiB)`; and, for a message sent in fragments, a collector
//!   at most `M` long whose growth may also double, up to `2M`. Once the message is read, the
//!   handler keeps an exact-size copy (`M` at most) and drops the rest, before it waits for queue
//!   budget and while it parses; this term counts it in both.
//! - **parsed messages:** `B × f`. Before parsing, a handler takes budget for its message's JSON
//!   length, and the queued message keeps that budget until the application drains it, so every
//!   message being parsed or waiting in the queue holds budget for its own raw bytes, and together
//!   they hold at most `B`.
//!
//! With the defaults (`C` = 64, `M` = 1 MiB, `B` = 4 MiB) that is 64 × (4 MiB + 80 KiB) =
//! 261 MiB, plus 4 MiB × 18 = 72 MiB while parsing: 349,175,808 bytes, about 333 MiB at peak
//! (309 MiB with every message parsed and retained at `f` = 12). The first term is four times
//! TCP's because tungstenite, not this crate, sizes the frame buffers. Lower `M` or `C` to shrink
//! it, and `B` to shrink the second.
//!
//! The bound ends where a message is drained. `receive_raw` releases a message's budget before the
//! manager verifies and opens it, so while the manager processes a batch, up to `B × f` of drained
//! messages sit outside the bound while the queue refills another `B`.
//!
//! # Backpressure, not loss
//!
//! The accept loop takes a connection permit before each `accept`, so at `C` it waits and later
//! connections queue in the kernel's backlog instead of being closed. Unlike TCP, a sender waiting
//! in the backlog does not get as far as writing: its handshake needs the receiver to answer, so
//! it waits up to `connection_timeout_ms` and then fails with a `TransportError`, having claimed
//! nothing. A handler that has read a message waits for queue budget while still holding its
//! connection permit; nothing is dropped for want of space. If the application never polls
//! `receive_messages`, the budget runs out, every handler ends up waiting for it, the listener
//! stops accepting, and senders' handshakes time out. The timeouts cover the connection only,
//! never the wait for queue budget.
//!
//! # Slow and silent peers
//!
//! An inbound connection is closed if it has not completed the handshake within
//! `handshake_timeout_ms` of being accepted, if it sends no frame for `idle_timeout_ms`, or if it
//! has not ended 30 s after it was accepted. So a peer that connects and sends nothing holds a
//! permit for at most `handshake_timeout_ms` (5 s by default); one that trickles a frame (a ping,
//! say) just inside every `idle_timeout_ms` can hold it for the full 30 s. A message whose frame
//! arrived whole is kept even if its peer then fails to close.

use super::abstraction::*;
use crate::{
    error::{Result, SynapseError},
    types::SecureMessage,
};
use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use std::{
    collections::HashMap,
    sync::{
        Arc, RwLock,
        atomic::{AtomicU32, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::{TcpListener, TcpStream},
    sync::{Mutex, OwnedSemaphorePermit, Semaphore},
};
use tokio_tungstenite::{
    WebSocketStream, accept_async_with_config, connect_async_with_config,
    tungstenite::{Message, protocol::WebSocketConfig},
};
use tracing::{debug, error, info, warn};
use url::Url;

/// The config key for the port `start` listens on. `0`, the default, lets the OS choose.
pub const LOCAL_PORT_KEY: &str = "local_port";

/// The config key for how long, in milliseconds, `send_message` waits to connect and complete the
/// handshake, and then to write its frame.
pub const CONNECTION_TIMEOUT_MS_KEY: &str = "connection_timeout_ms";

/// The default for [`CONNECTION_TIMEOUT_MS_KEY`].
pub const DEFAULT_CONNECTION_TIMEOUT_MS: usize = 30_000;

/// The config key for the largest message, in bytes of serialized JSON, the receiver reads and
/// the sender sends.
pub const MAX_MESSAGE_SIZE_KEY: &str = "max_message_size";

/// The default for [`MAX_MESSAGE_SIZE_KEY`]: 1 MiB of serialized JSON.
pub const DEFAULT_MAX_MESSAGE_SIZE: usize = 1024 * 1024;

/// The config key for how many inbound connections are handled at once.
pub const MAX_CONCURRENT_CONNECTIONS_KEY: &str = "max_concurrent_connections";

/// The default for [`MAX_CONCURRENT_CONNECTIONS_KEY`].
pub const DEFAULT_MAX_CONCURRENT_CONNECTIONS: usize = 64;

/// The config key for how many bytes of received messages, counted as serialized JSON, may wait
/// for the application to poll. It must be at least [`MAX_MESSAGE_SIZE_KEY`] and at most
/// `u32::MAX`.
pub const MAX_QUEUED_BYTES_KEY: &str = "max_queued_bytes";

/// The default for [`MAX_QUEUED_BYTES_KEY`]: 4 MiB of serialized JSON. See the module documentation
/// for the memory this bounds.
pub const DEFAULT_MAX_QUEUED_BYTES: usize = 4 * 1024 * 1024;

/// The config key for how long, in milliseconds, a new inbound connection may take to complete the
/// WebSocket handshake before the receiver closes it.
pub const HANDSHAKE_TIMEOUT_MS_KEY: &str = "handshake_timeout_ms";

/// The default for [`HANDSHAKE_TIMEOUT_MS_KEY`].
pub const DEFAULT_HANDSHAKE_TIMEOUT_MS: usize = 5_000;

/// The config key for how long, in milliseconds, a connection may send no frame before it is
/// closed: the receiver's wait for each frame, and the sender's wait for the receiver's close.
pub const IDLE_TIMEOUT_MS_KEY: &str = "idle_timeout_ms";

/// The default for [`IDLE_TIMEOUT_MS_KEY`].
pub const DEFAULT_IDLE_TIMEOUT_MS: usize = 5_000;

/// How long the receiver keeps one inbound connection, however steadily it sends.
const CONNECTION_LIFETIME: Duration = Duration::from_secs(30);

/// tungstenite's read buffer, allocated eagerly per connection. Its default is 128 KiB; the
/// module documentation's bound assumes this size.
const READ_BUFFER_SIZE: usize = 8 * 1024;

/// The longest `estimate_metrics` waits for its probe handshake.
const PROBE_TIMEOUT: Duration = Duration::from_secs(3);

/// `config[key]` as a positive integer, or `default` when the key is absent. A value that does not
/// parse, or is zero, is an error: silently falling back to the default would hide a typo.
fn positive_limit(config: &HashMap<String, String>, key: &str, default: usize) -> Result<usize> {
    match config.get(key) {
        None => Ok(default),
        Some(value) => match value.trim().parse::<usize>() {
            Ok(0) => Err(SynapseError::Config(format!(
                "{key} must be a positive integer, got 0"
            ))),
            Ok(limit) => Ok(limit),
            Err(e) => Err(SynapseError::Config(format!(
                "{key} must be a positive integer, got {value:?}: {e}"
            ))),
        },
    }
}

/// Every limit and timeout the config sets, each checked by [`positive_limit`], and the port.
#[derive(Debug, Clone, Copy)]
struct Limits {
    local_port: u16,
    connection_timeout: Duration,
    max_message_size: usize,
    max_concurrent_connections: usize,
    max_queued_bytes: usize,
    handshake_timeout: Duration,
    idle_timeout: Duration,
}

impl Limits {
    fn from_config(config: &HashMap<String, String>) -> Result<Self> {
        let millis = |key, default| -> Result<Duration> {
            Ok(Duration::from_millis(
                positive_limit(config, key, default)? as u64
            ))
        };
        let local_port = match config.get(LOCAL_PORT_KEY) {
            None => 0,
            Some(value) => value.trim().parse::<u16>().map_err(|e| {
                SynapseError::Config(format!(
                    "{LOCAL_PORT_KEY} must be a port number from 0 to 65535, got {value:?}: {e}"
                ))
            })?,
        };
        let max_message_size =
            positive_limit(config, MAX_MESSAGE_SIZE_KEY, DEFAULT_MAX_MESSAGE_SIZE)?;
        let max_queued_bytes =
            positive_limit(config, MAX_QUEUED_BYTES_KEY, DEFAULT_MAX_QUEUED_BYTES)?;
        // A message the size check accepts must fit the queue budget, or its handler would wait
        // forever for room that never comes.
        if max_queued_bytes < max_message_size {
            return Err(SynapseError::Config(format!(
                "{MAX_QUEUED_BYTES_KEY} ({max_queued_bytes}) must be at least \
                 {MAX_MESSAGE_SIZE_KEY} ({max_message_size}), or a message of that size could \
                 never be queued"
            )));
        }
        // A handler takes budget for a whole message in one `acquire_many_owned`, which counts in
        // `u32`. A message is at most the budget, so a budget that fits `u32` makes every acquire
        // fit too.
        if u32::try_from(max_queued_bytes).is_err() {
            return Err(SynapseError::Config(format!(
                "{MAX_QUEUED_BYTES_KEY} ({max_queued_bytes}) must be at most {}",
                u32::MAX
            )));
        }
        Ok(Self {
            local_port,
            connection_timeout: millis(CONNECTION_TIMEOUT_MS_KEY, DEFAULT_CONNECTION_TIMEOUT_MS)?,
            max_message_size,
            max_concurrent_connections: positive_limit(
                config,
                MAX_CONCURRENT_CONNECTIONS_KEY,
                DEFAULT_MAX_CONCURRENT_CONNECTIONS,
            )?,
            max_queued_bytes,
            handshake_timeout: millis(HANDSHAKE_TIMEOUT_MS_KEY, DEFAULT_HANDSHAKE_TIMEOUT_MS)?,
            idle_timeout: millis(IDLE_TIMEOUT_MS_KEY, DEFAULT_IDLE_TIMEOUT_MS)?,
        })
    }
}

/// Check `config` exactly as [`WebSocketTransportImpl::new`] does, without building anything.
pub fn validate_config(config: &HashMap<String, String>) -> Result<()> {
    Limits::from_config(config)?;
    crate::network_scope::BindScope::from_config_map(config)?;
    Ok(())
}

/// tungstenite's limits for a connection that reads messages of at most `max_message_size` bytes.
/// Both the message and the frame limit are set: tungstenite checks a frame's length against
/// `max_frame_size` before it reserves room for the frame.
fn ws_config(max_message_size: usize) -> WebSocketConfig {
    WebSocketConfig::default()
        .read_buffer_size(READ_BUFFER_SIZE)
        .max_message_size(Some(max_message_size))
        .max_frame_size(Some(max_message_size))
}

/// The `ws://` URL for `target`'s address: a `ws://` URL as given, or `host:port`. There is no
/// default port, and `wss://` is refused, because this build has no TLS.
fn ws_url(target: &TransportTarget) -> Result<String> {
    let address = target.address.as_deref().ok_or_else(|| {
        SynapseError::TransportError(format!(
            "WebSocket target {} has no address; give a ws:// URL or host:port",
            target.identifier
        ))
    })?;
    if address.starts_with("wss://") {
        return Err(SynapseError::TransportError(format!(
            "WebSocket target {address}: wss:// is not supported, because this build of the \
             WebSocket transport has no TLS"
        )));
    }
    let url = if address.starts_with("ws://") {
        address.to_string()
    } else {
        match address.rsplit_once(':') {
            Some((host, port)) if !host.is_empty() && port.parse::<u16>().is_ok() => {
                format!("ws://{address}/")
            }
            _ => {
                return Err(SynapseError::TransportError(format!(
                    "WebSocket target {address:?} is neither a ws:// URL nor host:port"
                )));
            }
        }
    };
    Url::parse(&url).map_err(|e| {
        SynapseError::TransportError(format!("WebSocket target {address:?} is not a URL: {e}"))
    })?;
    Ok(url)
}

/// Start the closing handshake on `ws` and wait, for at most `wait`, for the peer's close frame.
/// Nothing depends on the outcome, so a failure is logged, not returned.
async fn close_gracefully<S>(mut ws: WebSocketStream<S>, wait: Duration, peer: &str)
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let finished = tokio::time::timeout(wait, async {
        ws.close(None).await?;
        // The peer's close frame arrives as a message; the stream ends after it.
        while let Some(frame) = ws.next().await {
            frame?;
        }
        Ok::<(), tokio_tungstenite::tungstenite::Error>(())
    })
    .await;
    match finished {
        Ok(Ok(())) => debug!("WebSocket connection to {} closed cleanly", peer),
        Ok(Err(e)) => debug!(
            "WebSocket connection to {} did not finish the closing handshake: {}",
            peer, e
        ),
        Err(_) => debug!(
            "WebSocket connection to {} did not answer our close within {:?}",
            peer, wait
        ),
    }
}

/// What an inbound connection's handler shares with the transport.
#[derive(Clone)]
struct Inbound {
    received_messages: Arc<Mutex<Vec<Queued>>>,
    queue_budget: Arc<Semaphore>,
    metrics: Arc<RwLock<TransportMetrics>>,
    active_connections: Arc<AtomicU32>,
    max_message_size: usize,
    handshake_timeout: Duration,
    idle_timeout: Duration,
}

/// A received message waiting for the application, holding queue budget for its JSON length until
/// it is drained.
struct Queued {
    message: IncomingMessage,
    _budget: OwnedSemaphorePermit,
}

/// Counts an inbound connection as active for as long as it lives.
struct ActiveConnection(Arc<AtomicU32>);

impl ActiveConnection {
    fn new(count: &Arc<AtomicU32>) -> Self {
        count.fetch_add(1, Ordering::Relaxed);
        Self(Arc::clone(count))
    }
}

impl Drop for ActiveConnection {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

/// WebSocket Transport implementation for unified abstraction
pub struct WebSocketTransportImpl {
    /// Which interfaces the server listens on
    bind_scope: crate::network_scope::BindScope,
    /// The configured limits and timeouts, and the port `start` binds.
    limits: Limits,
    /// The listener `start` bound. The accept loop serves this same listener (an `Arc` of it)
    /// rather than binding a second one, which on the same port fails and left the loop never
    /// running.
    listener: std::sync::Mutex<Option<Arc<TcpListener>>>,
    /// Inbound connections being handled at once. The accept loop takes a permit before each
    /// `accept`, so at the cap it waits and new connections queue in the kernel's backlog.
    connection_permits: Arc<Semaphore>,
    /// Free bytes of queue budget, one permit per byte of serialized JSON. A handler takes its
    /// message's length before it parses and keeps it with the queued message; `receive_raw`
    /// releases it by draining the message.
    queue_budget: Arc<Semaphore>,
    /// Received messages, each with the queue budget it holds.
    received_messages: Arc<Mutex<Vec<Queued>>>,
    /// Inbound connections being handled now.
    active_connections: Arc<AtomicU32>,
    /// Total time, in nanoseconds, the successful sends spent connecting and writing, from which
    /// `estimate_metrics` derives a bandwidth.
    send_nanos: AtomicU64,
    /// Current status
    status: Arc<RwLock<TransportStatus>>,
    /// Counters of what this transport actually did.
    metrics: Arc<RwLock<TransportMetrics>>,
}

impl WebSocketTransportImpl {
    /// Create a new WebSocket transport. Nothing is bound until `start`. See the module
    /// documentation for the config keys; fails if any of them is present but invalid.
    pub async fn new(config: &HashMap<String, String>) -> Result<Self> {
        let limits = Limits::from_config(config)?;
        let bind_scope = crate::network_scope::BindScope::from_config_map(config)?;
        let metrics = TransportMetrics {
            transport_type: TransportType::WebSocket,
            ..Default::default()
        };
        Ok(Self {
            bind_scope,
            limits,
            listener: std::sync::Mutex::new(None),
            connection_permits: Arc::new(Semaphore::new(limits.max_concurrent_connections)),
            queue_budget: Arc::new(Semaphore::new(limits.max_queued_bytes)),
            received_messages: Arc::new(Mutex::new(Vec::new())),
            active_connections: Arc::new(AtomicU32::new(0)),
            send_nanos: AtomicU64::new(0),
            status: Arc::new(RwLock::new(TransportStatus::Stopped)),
            metrics: Arc::new(RwLock::new(metrics)),
        })
    }

    /// The address the listener is bound to, once `start` has bound it.
    pub fn local_addr(&self) -> Option<std::net::SocketAddr> {
        self.listener
            .lock()
            .unwrap()
            .as_ref()
            .and_then(|listener| listener.local_addr().ok())
    }

    /// Serve `listener`: take a connection permit, accept, and hand the connection to its own task.
    fn spawn_accept_loop(&self, listener: Arc<TcpListener>, local_addr: std::net::SocketAddr) {
        let permits = Arc::clone(&self.connection_permits);
        let inbound = Inbound {
            received_messages: Arc::clone(&self.received_messages),
            queue_budget: Arc::clone(&self.queue_budget),
            metrics: Arc::clone(&self.metrics),
            active_connections: Arc::clone(&self.active_connections),
            max_message_size: self.limits.max_message_size,
            handshake_timeout: self.limits.handshake_timeout,
            idle_timeout: self.limits.idle_timeout,
        };
        tokio::spawn(async move {
            info!("WebSocket server listening on {}", local_addr);
            loop {
                // Backpressure: take a permit BEFORE accepting, so at the cap the loop waits for a
                // handler to finish and the waiting connections stay in the kernel's backlog. The
                // semaphore is never closed, so `acquire_owned` cannot fail while this task runs.
                let Ok(permit) = Arc::clone(&permits).acquire_owned().await else {
                    error!("WebSocket connection limit closed; the accept loop stops");
                    return;
                };
                match listener.accept().await {
                    Ok((stream, addr)) => {
                        debug!("Accepted WebSocket connection from {}", addr);
                        let inbound = inbound.clone();
                        tokio::spawn(async move {
                            Self::handle_connection(stream, addr.to_string(), inbound).await;
                            // Held for the whole connection and any wait for queue budget.
                            drop(permit);
                        });
                    }
                    Err(e) => {
                        error!("Failed to accept WebSocket connection: {}", e);
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                }
            }
        });
    }

    /// Complete the handshake on `stream` and read its one message into `message`. Returns when the
    /// connection ends, fails, times out or breaks the protocol; whatever whole message was read by
    /// then stays in `message`.
    async fn read_connection(
        stream: TcpStream,
        source: &str,
        inbound: &Inbound,
        message: &mut Option<Vec<u8>>,
    ) {
        let config = ws_config(inbound.max_message_size);
        let mut ws = match tokio::time::timeout(
            inbound.handshake_timeout,
            accept_async_with_config(stream, Some(config)),
        )
        .await
        {
            Ok(Ok(ws)) => ws,
            Ok(Err(e)) => {
                debug!("WebSocket handshake with {} failed: {}", source, e);
                return;
            }
            Err(_) => {
                debug!(
                    "Closed connection from {}: no WebSocket handshake within {:?}",
                    source, inbound.handshake_timeout
                );
                return;
            }
        };
        loop {
            let next = match tokio::time::timeout(inbound.idle_timeout, ws.next()).await {
                Ok(next) => next,
                Err(_) => {
                    debug!(
                        "Closed WebSocket connection from {}: no frame within {:?} ({})",
                        source,
                        inbound.idle_timeout,
                        if message.is_some() {
                            "after its message"
                        } else {
                            "and no message"
                        }
                    );
                    return;
                }
            };
            match next {
                // The closing handshake finished, or the stream ended.
                None => return,
                Some(Ok(Message::Binary(bytes))) => {
                    if message.is_some() {
                        warn!(
                            "WebSocket peer {} sent a second message on one connection; the wire \
                             format is one message per connection, so it is dropped",
                            source
                        );
                        return;
                    }
                    // An exact-size copy: `bytes` can share an allocation up to twice its length
                    // (see the module documentation), which this releases.
                    *message = Some(bytes.as_ref().to_vec());
                }
                Some(Ok(Message::Text(_))) => {
                    warn!(
                        "Dropped a text message from WebSocket peer {}: messages are binary frames",
                        source
                    );
                    return;
                }
                // tungstenite queues the reply to a close and sends it on the next poll, after
                // which the stream ends.
                Some(Ok(Message::Close(_))) => {}
                // tungstenite answers pings itself; a raw frame is never returned while reading.
                Some(Ok(Message::Ping(_) | Message::Pong(_) | Message::Frame(_))) => {}
                Some(Err(e)) => {
                    if message.is_some() {
                        debug!(
                            "WebSocket connection from {} ended badly after its message: {}",
                            source, e
                        );
                    } else {
                        warn!("Dropped WebSocket connection from {}: {}", source, e);
                    }
                    return;
                }
            }
        }
    }

    /// Read one message from an inbound connection, then queue it. Once the connection is finished,
    /// the handler waits for queue budget for the message's length -- with no timeout, still holding
    /// its connection permit -- and only then parses and queues it, so a full queue slows the
    /// listener down instead of losing messages. A connection that completes the handshake and
    /// closes having sent nothing is a probe (`estimate_metrics` and `test_connectivity` do that).
    async fn handle_connection(stream: TcpStream, source: String, inbound: Inbound) {
        let _active = ActiveConnection::new(&inbound.active_connections);
        let mut data = None;
        if tokio::time::timeout(
            CONNECTION_LIFETIME,
            Self::read_connection(stream, &source, &inbound, &mut data),
        )
        .await
        .is_err()
        {
            debug!(
                "Closed WebSocket connection from {}: it did not end within {:?}",
                source, CONNECTION_LIFETIME
            );
        }
        let Some(data) = data else {
            debug!(
                "WebSocket connection from {} ended without a message (a probe)",
                source
            );
            return;
        };
        let bytes_read = data.len();
        // tungstenite already refused anything longer; this keeps the budget arithmetic below
        // true whatever it does.
        if bytes_read > inbound.max_message_size {
            warn!(
                "Dropped WebSocket message from {}: larger than the {} byte limit",
                source, inbound.max_message_size
            );
            return;
        }

        // Wait for queue budget before parsing, so a waiting handler holds only its raw bytes, not
        // the several times larger parsed message. `Limits` guarantees
        // `bytes_read <= max_message_size <= max_queued_bytes <= u32::MAX`. The acquire fails only
        // once `stop` has closed the budget; the handler then drops the message and returns,
        // releasing its connection permit, instead of waiting forever.
        let Ok(wanted) = u32::try_from(bytes_read) else {
            error!(
                "Dropped WebSocket message from {}: {} bytes is over the queue budget's u32 limit",
                source, bytes_read
            );
            return;
        };
        let Ok(budget) = inbound.queue_budget.acquire_many_owned(wanted).await else {
            error!(
                "Dropped WebSocket message from {}: the receive queue was closed",
                source
            );
            return;
        };

        let message = match serde_json::from_slice::<SecureMessage>(&data) {
            Ok(message) => message,
            Err(e) => {
                warn!(
                    "Dropped WebSocket message from {}: {} bytes did not parse as a SecureMessage: {}",
                    source, bytes_read, e
                );
                return;
            }
        };
        drop(data);

        let incoming = IncomingMessage::new(message, TransportType::WebSocket, source);
        {
            let mut messages = inbound.received_messages.lock().await;
            messages.push(Queued {
                message: incoming,
                _budget: budget,
            });
            debug!("Queued WebSocket message, total: {}", messages.len());
        }
        let mut metrics = inbound.metrics.write().unwrap();
        metrics.messages_received += 1;
        metrics.bytes_received += bytes_read as u64;
        metrics.touch();
    }

    /// Connect to `url` and complete the handshake, within `wait`.
    async fn connect(
        &self,
        url: &str,
        wait: Duration,
    ) -> Result<WebSocketStream<tokio_tungstenite::MaybeTlsStream<TcpStream>>> {
        let config = ws_config(self.limits.max_message_size);
        match tokio::time::timeout(wait, connect_async_with_config(url, Some(config), true)).await {
            Ok(Ok((ws, _response))) => Ok(ws),
            Ok(Err(e)) => Err(SynapseError::TransportError(format!(
                "WebSocket connection to {url} failed: {e}"
            ))),
            Err(_) => Err(SynapseError::TransportError(format!(
                "WebSocket connection to {url} did not complete its handshake within {wait:?}"
            ))),
        }
    }

    /// Connect to `url`, complete a real handshake, and close. Returns the time the connect and
    /// handshake took: a measured round trip, since the handshake is a request and its response.
    async fn probe(&self, url: &str, wait: Duration) -> Result<Duration> {
        let start = Instant::now();
        let ws = self.connect(url, wait).await?;
        let rtt = start.elapsed();
        close_gracefully(ws, self.limits.idle_timeout, url).await;
        Ok(rtt)
    }

    /// Send `message` to `url` as one binary frame on a new connection.
    async fn connect_and_send(
        &self,
        url: &str,
        message: &SecureMessage,
    ) -> Result<DeliveryReceipt> {
        // Serialize first, and refuse what a receiver with this limit would drop: writing it
        // would claim `Sent` for a message that is never delivered.
        let json = serde_json::to_vec(message).map_err(|e| {
            SynapseError::SerializationError(format!("Failed to serialize message: {e}"))
        })?;
        let size = json.len();
        if size > self.limits.max_message_size {
            // A refusal of this message, not a transport failure: see `SynapseError::MessageRefused`.
            return Err(SynapseError::MessageRefused(format!(
                "WebSocket message {} serializes to {} bytes, over this transport's {} of {} bytes \
                 (serialized JSON); a receiver with that limit would drop it",
                message.message_id.0, size, MAX_MESSAGE_SIZE_KEY, self.limits.max_message_size
            )));
        }

        let start = Instant::now();
        let (connect_time, ws) = match self.write_one(url, json, start).await {
            Ok(done) => done,
            Err(e) => {
                let mut metrics = self.metrics.write().unwrap();
                metrics.send_failures += 1;
                metrics.touch();
                return Err(e);
            }
        };
        let total_time = start.elapsed();
        {
            let mut metrics = self.metrics.write().unwrap();
            metrics.messages_sent += 1;
            metrics.bytes_sent += size as u64;
            let sent = metrics.messages_sent;
            let average = (metrics.average_latency_ms as f64 * (sent - 1) as f64
                + total_time.as_millis() as f64)
                / sent as f64;
            metrics.average_latency_ms = average as u64;
            metrics.touch();
        }
        self.send_nanos.fetch_add(
            u64::try_from(total_time.as_nanos()).unwrap_or(u64::MAX),
            Ordering::Relaxed,
        );
        info!(
            "WebSocket message sent to {} in {:?} (connect and handshake: {:?})",
            url, total_time, connect_time
        );

        // After the claim's precondition has held: the close is courtesy, not part of `Sent`.
        close_gracefully(ws, self.limits.idle_timeout, url).await;

        Ok(DeliveryReceipt {
            message_id: message.message_id.0.to_string(),
            transport_used: TransportType::WebSocket,
            delivery_time: total_time,
            target_reached: url.to_string(),
            // The frame was written and flushed to a socket whose WebSocket handshake completed
            // (spec §4). The peer sends no application-level acknowledgement, so no more is claimed.
            confirmation: DeliveryConfirmation::Sent,
            metadata: HashMap::from([
                (
                    "connect_time_ms".to_string(),
                    connect_time.as_millis().to_string(),
                ),
                ("bytes".to_string(), size.to_string()),
            ]),
        })
    }

    /// Connect to `url`, and write and flush `json` as one binary frame. Returns the connect and
    /// handshake time, and the open connection for the caller to close.
    async fn write_one(
        &self,
        url: &str,
        json: Vec<u8>,
        start: Instant,
    ) -> Result<(
        Duration,
        WebSocketStream<tokio_tungstenite::MaybeTlsStream<TcpStream>>,
    )> {
        let wait = self.limits.connection_timeout;
        let mut ws = self.connect(url, wait).await?;
        let connect_time = start.elapsed();
        // `send` writes the frame and flushes it: when it returns `Ok`, the bytes are in the socket.
        match tokio::time::timeout(wait, ws.send(Message::Binary(json.into()))).await {
            Ok(Ok(())) => Ok((connect_time, ws)),
            Ok(Err(e)) => Err(SynapseError::TransportError(format!(
                "WebSocket write to {url} failed: {e}"
            ))),
            Err(_) => Err(SynapseError::TransportError(format!(
                "WebSocket write to {url} did not finish within {wait:?}"
            ))),
        }
    }

    /// What the sends so far show: successes, failures, and bytes per second while sending.
    /// `None` for the bandwidth until a send has been measured.
    fn observed(&self) -> (u64, u64, Option<u64>) {
        let metrics = self.metrics.read().unwrap();
        let nanos = self.send_nanos.load(Ordering::Relaxed);
        let bandwidth = (metrics.bytes_sent > 0 && nanos > 0).then(|| {
            (metrics.bytes_sent as u128 * 1_000_000_000 / nanos as u128).min(u64::MAX as u128)
                as u64
        });
        (metrics.messages_sent, metrics.send_failures, bandwidth)
    }
}

#[async_trait]
impl Transport for WebSocketTransportImpl {
    fn transport_type(&self) -> TransportType {
        TransportType::WebSocket
    }

    /// What this transport does, with `max_message_size` set to this instance's configured limit,
    /// in bytes of serialized JSON.
    fn capabilities(&self) -> TransportCapabilities {
        TransportCapabilities {
            max_message_size: self.limits.max_message_size,
            reliable: true,         // over TCP
            real_time: true,        // one handshake per message, then one frame
            broadcast: false,       // point to point
            bidirectional: true,    // it sends and it listens
            encrypted: false,       // no TLS in this build, so no wss://
            network_spanning: true, // any reachable host:port
            supported_urgencies: vec![
                MessageUrgency::Critical,
                MessageUrgency::RealTime,
                MessageUrgency::Interactive,
                MessageUrgency::Background,
            ],
            features: vec![
                "http_upgrade".to_string(),
                "binary_frames".to_string(),
                "one_message_per_connection".to_string(),
            ],
        }
    }

    /// Whether `target` has an address this transport can dial (a `ws://` URL or `host:port`).
    /// It touches no network; `test_connectivity` does.
    async fn can_reach(&self, target: &TransportTarget) -> bool {
        ws_url(target).is_ok()
    }

    /// A real handshake with `target`, as `test_connectivity` does, bounded at 3 s. Every number
    /// is observed: `latency` is the handshake's round trip (or the time it took to fail);
    /// `reliability` is the share of this transport's sends that succeeded, counting this probe
    /// as one attempt; `bandwidth` is bytes sent per second spent sending, or 1 until a send has
    /// been measured (the manager's score takes its log, so 1 counts for nothing and 0 would be
    /// minus infinity). `cost` is the same relative unit TCP reports.
    async fn estimate_metrics(&self, target: &TransportTarget) -> Result<TransportEstimate> {
        let url = ws_url(target)?;
        let wait = self.limits.connection_timeout.min(PROBE_TIMEOUT);
        let start = Instant::now();
        let probe = self.probe(&url, wait).await;
        let (sent, failures, bandwidth) = self.observed();
        let available = probe.is_ok();
        let successes = sent + u64::from(available);
        Ok(TransportEstimate {
            latency: probe.unwrap_or_else(|_| start.elapsed()),
            reliability: successes as f64 / (sent + failures + 1) as f64,
            bandwidth: bandwidth.unwrap_or(1),
            cost: 1.0,
            available,
            // Availability and latency were observed just now.
            confidence: 1.0,
        })
    }

    async fn send_message(
        &self,
        target: &TransportTarget,
        message: &SecureMessage,
    ) -> Result<DeliveryReceipt> {
        let url = ws_url(target)?;
        self.connect_and_send(&url, message).await
    }

    /// Connected only after a real WebSocket handshake, whose round trip is the reported `rtt`.
    async fn test_connectivity(&self, target: &TransportTarget) -> Result<ConnectivityResult> {
        let url = ws_url(target)?;
        let mut details = HashMap::from([("target_url".to_string(), url.clone())]);
        match self.probe(&url, self.limits.connection_timeout).await {
            Ok(rtt) => {
                details.insert("rtt_ms".to_string(), rtt.as_millis().to_string());
                Ok(ConnectivityResult {
                    connected: true,
                    rtt: Some(rtt),
                    error: None,
                    // The same measure TCP reports: falls with the measured round trip.
                    quality: 1.0 - (rtt.as_millis() as f64 / 10000.0).min(1.0),
                    details,
                })
            }
            Err(e) => {
                details.insert("error".to_string(), e.to_string());
                Ok(ConnectivityResult {
                    connected: false,
                    rtt: None,
                    error: Some(e.to_string()),
                    quality: 0.0,
                    details,
                })
            }
        }
    }

    /// Bind the listener once, keep it, and serve that same listener. Fails if the bind fails, or
    /// if this transport was already started.
    async fn start(&self) -> Result<()> {
        info!("Starting WebSocket transport");
        if self.listener.lock().unwrap().is_some() {
            return Err(SynapseError::TransportError(
                "WebSocket transport is already started".to_string(),
            ));
        }
        *self.status.write().unwrap() = TransportStatus::Starting;

        let addr = self.bind_scope.listen_addr(self.limits.local_port);
        let bound = match TcpListener::bind(addr).await {
            Ok(listener) => listener
                .local_addr()
                .map(|local| (Arc::new(listener), local)),
            Err(e) => Err(e),
        };
        let (listener, local_addr) = match bound {
            Ok(bound) => bound,
            Err(e) => {
                *self.status.write().unwrap() = TransportStatus::Failed;
                return Err(SynapseError::NetworkError(format!(
                    "WebSocket transport could not listen on {addr}: {e}"
                )));
            }
        };
        *self.listener.lock().unwrap() = Some(Arc::clone(&listener));
        self.spawn_accept_loop(listener, local_addr);

        *self.status.write().unwrap() = TransportStatus::Running;
        info!("WebSocket transport started on {}", local_addr);
        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        info!("Stopping WebSocket transport");
        *self.status.write().unwrap() = TransportStatus::Stopping;

        // Close the queue budget, so every handler waiting for it gets an error, drops its message
        // and returns, releasing its connection permit, instead of waiting forever for a poll that
        // will never come. A stopped transport is not restarted: the manager removes it and builds
        // a new one. As for TCP, this does not stop the accept loop, which keeps the listener; a
        // connection it accepts after this is read, then dropped here.
        self.queue_budget.close();

        *self.status.write().unwrap() = TransportStatus::Stopped;
        info!("WebSocket transport stopped");
        Ok(())
    }

    async fn status(&self) -> TransportStatus {
        *self.status.read().unwrap()
    }

    /// The counters of what this transport did. `reliability_score` is the share of sends that
    /// succeeded, and 0 before any send: no reliability is claimed that no send has earned.
    async fn metrics(&self) -> TransportMetrics {
        let mut metrics = self.metrics.read().unwrap().clone();
        let attempts = metrics.messages_sent + metrics.send_failures;
        metrics.reliability_score = if attempts == 0 {
            0.0
        } else {
            metrics.messages_sent as f64 / attempts as f64
        };
        metrics.active_connections = self.active_connections.load(Ordering::Relaxed);
        metrics
    }
}

#[async_trait]
impl TransportReceive for WebSocketTransportImpl {
    async fn receive_raw(&self, inbox: &mut RawInbox) -> Result<()> {
        // Draining a message drops the queue budget it held, so a handler waiting for budget can
        // queue its message as soon as this lock is released. Each message is handed out once.
        let mut messages = self.received_messages.lock().await;
        inbox.extend(messages.drain(..).map(|queued| queued.message));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The transport would once report `connected: true` after a bare TCP connect. A listener that
    /// accepts TCP but never answers the upgrade must be reported not connected, with no round
    /// trip, once the connection timeout passes.
    #[tokio::test]
    async fn a_bare_tcp_listener_is_not_a_websocket_connection() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let holder = tokio::spawn(async move {
            // Accept and hold, saying nothing.
            let mut held = Vec::new();
            while let Ok((stream, _)) = listener.accept().await {
                held.push(stream);
            }
        });
        let config = HashMap::from([(CONNECTION_TIMEOUT_MS_KEY.to_string(), "300".to_string())]);
        let transport = WebSocketTransportImpl::new(&config).await.unwrap();
        let target = TransportTarget::new("peer".to_string()).with_address(addr.to_string());
        let connectivity = transport.test_connectivity(&target).await.unwrap();
        assert!(!connectivity.connected);
        assert_eq!(connectivity.rtt, None);
        let why = connectivity.error.expect("says why it is not connected");
        assert!(why.contains("handshake"), "{why}");
        assert!(!transport.estimate_metrics(&target).await.unwrap().available);
        holder.abort();
    }

    /// `receive_raw` drains: each message is handed out once, however often it is polled. Before
    /// Task 8 it put back everything it drained. This is checked here, on the transport, because
    /// through the manager the replay record drops a signed message seen before, which hides a
    /// transport's repeats (the integration test alone passed with the repeats restored).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn receive_raw_hands_each_message_out_once() {
        const K: usize = 3;
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let config = HashMap::from([
            (LOCAL_PORT_KEY.to_string(), port.to_string()),
            (
                crate::network_scope::BIND_SCOPE_KEY.to_string(),
                crate::network_scope::BindScope::Loopback
                    .config_value()
                    .to_string(),
            ),
        ]);
        let bob = WebSocketTransportImpl::new(&config).await.unwrap();
        bob.start().await.unwrap();
        assert_eq!(bob.local_addr().map(|a| a.port()), Some(port));
        let alice = WebSocketTransportImpl::new(&HashMap::new()).await.unwrap();
        let target =
            TransportTarget::new("bob".to_string()).with_address(format!("127.0.0.1:{port}"));
        let mut sent = std::collections::HashSet::new();
        for i in 0..K {
            let message = SecureMessage::new(
                "bob",
                "alice",
                format!("once {i}").into_bytes(),
                crate::types::SecurityLevel::Public,
            );
            sent.insert(message.message_id.0.to_string());
            let receipt = alice.send_message(&target, &message).await.unwrap();
            assert!(matches!(receipt.confirmation, DeliveryConfirmation::Sent));
        }
        let mut arrived = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(5);
        while arrived.len() < K && Instant::now() < deadline {
            let mut inbox = RawInbox::new();
            bob.receive_raw(&mut inbox).await.unwrap();
            arrived.extend(inbox.drain());
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        for _ in 0..5 {
            let mut inbox = RawInbox::new();
            bob.receive_raw(&mut inbox).await.unwrap();
            arrived.extend(inbox.drain());
        }
        let ids: Vec<String> = arrived
            .iter()
            .map(|m| m.message.message_id.0.to_string())
            .collect();
        assert_eq!(
            ids.len(),
            K,
            "each message once, not {} deliveries",
            ids.len()
        );
        assert_eq!(
            ids.into_iter().collect::<std::collections::HashSet<_>>(),
            sent
        );
        assert_eq!(bob.metrics().await.messages_received, K as u64);
        assert_eq!(alice.metrics().await.messages_sent, K as u64);
    }

    /// A target address is a `ws://` URL or `host:port`; no default port is guessed and `wss://`
    /// is refused, since this build has no TLS.
    #[test]
    fn target_addresses() {
        let target =
            |address: &str| TransportTarget::new("t".to_string()).with_address(address.to_string());
        assert_eq!(
            ws_url(&target("127.0.0.1:9000")).unwrap(),
            "ws://127.0.0.1:9000/"
        );
        assert_eq!(ws_url(&target("[::1]:9000")).unwrap(), "ws://[::1]:9000/");
        assert_eq!(
            ws_url(&target("ws://example.test:81/x")).unwrap(),
            "ws://example.test:81/x"
        );
        for bad in ["example.test", "wss://example.test", ":9000", "host:port"] {
            assert!(ws_url(&target(bad)).is_err(), "{bad}");
        }
        assert!(ws_url(&TransportTarget::new("no-address".to_string())).is_err());
    }

    /// `validate_config` applies the same rule as `new`.
    #[test]
    fn validate_config_refuses_what_new_refuses() {
        let factory = WebSocketTransportFactory;
        for key in [
            CONNECTION_TIMEOUT_MS_KEY,
            MAX_MESSAGE_SIZE_KEY,
            MAX_CONCURRENT_CONNECTIONS_KEY,
            MAX_QUEUED_BYTES_KEY,
            HANDSHAKE_TIMEOUT_MS_KEY,
            IDLE_TIMEOUT_MS_KEY,
        ] {
            for bad in ["0", "abc", ""] {
                let config = HashMap::from([(key.to_string(), bad.to_string())]);
                assert!(factory.validate_config(&config).is_err(), "{key} = {bad:?}");
            }
            let good = if key == MAX_QUEUED_BYTES_KEY {
                DEFAULT_MAX_MESSAGE_SIZE.to_string()
            } else {
                "7".to_string()
            };
            let config = HashMap::from([(key.to_string(), good)]);
            assert!(factory.validate_config(&config).is_ok(), "{key}");
        }
        assert!(
            factory
                .validate_config(&HashMap::from([(
                    LOCAL_PORT_KEY.to_string(),
                    "70000".to_string()
                )]))
                .is_err()
        );
        assert!(factory.validate_config(&factory.default_config()).is_ok());
    }
}
