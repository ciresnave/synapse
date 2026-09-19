// SPDX-License-Identifier: MIT OR Apache-2.0
//! TCP Transport implementation conforming to the unified Transport trait
//!
//! # Wire format
//!
//! One JSON `SecureMessage` per connection. The sender writes it, then shuts down its write half;
//! the receiver reads until the connection ends.
//!
//! # Config keys
//!
//! | key | default | meaning |
//! |---|---|---|
//! | `listen_port` | none (client-only) | the port to listen on; absent or `0` means send only |
//! | `connection_timeout_ms` | `10000` | how long `send_message` waits to connect |
//! | [`MAX_MESSAGE_SIZE_KEY`] (`max_message_size`) | [`DEFAULT_MAX_MESSAGE_SIZE`] (1 MiB) | the largest message, **in bytes of serialized JSON**, the receiver reads and the sender sends |
//! | [`MAX_CONCURRENT_CONNECTIONS_KEY`] (`max_concurrent_connections`) | [`DEFAULT_MAX_CONCURRENT_CONNECTIONS`] (64) | how many inbound connections are handled at once |
//! | [`MAX_QUEUED_MESSAGES_KEY`] (`max_queued_messages`) | [`DEFAULT_MAX_QUEUED_MESSAGES`] (1024) | how many received messages wait for the application to poll |
//! | [`FIRST_BYTE_TIMEOUT_MS_KEY`] (`first_byte_timeout_ms`) | [`DEFAULT_FIRST_BYTE_TIMEOUT_MS`] (5000) | how long a new connection may stay silent before it is closed |
//! | [`IDLE_TIMEOUT_MS_KEY`] (`idle_timeout_ms`) | [`DEFAULT_IDLE_TIMEOUT_MS`] (5000) | how long a connection may stay silent between two reads |
//!
//! Every key but `listen_port` and `connection_timeout_ms` must parse as a positive integer:
//! [`TcpTransportImpl::new`] (and so the factory's `create_transport`) refuses an unparseable or
//! zero value instead of falling back to the default.
//!
//! `max_message_size` counts serialized bytes, not body bytes: `encrypted_content` serializes as
//! a JSON number array, a few characters per body byte, so the largest body that fits is several
//! times smaller than the limit. Sender and receiver apply the same number to the same bytes, so a
//! sender refuses exactly what a receiver with its limit would drop. The refusal is a
//! [`SynapseError::MessageRefused`](crate::error::SynapseError::MessageRefused), which the
//! transport manager does not count against the transport's health.
//!
//! # What an unauthenticated peer can make the receiver hold
//!
//! Nothing below is authenticated: the receiver reads, parses and queues a message before anyone
//! checks its signature. Write `C` for `max_concurrent_connections`, `M` for `max_message_size`,
//! `Q` for `max_queued_messages`, and `f` for the parse factor -- the heap a parsed
//! `SecureMessage` takes per byte of its JSON. `f` depends on the data: one 1 MiB JSON message was
//! measured parsing to about 6.9 MB, `f` ≈ 6.7, which is an example, not a bound. The worst case
//! is the sum of:
//!
//! - **read buffers:** `C × (M + 1)` bytes. At most `C` connections are handled at once, each read
//!   into a buffer that never reserves more than `M + 1` bytes. A handler waiting for queue space
//!   still holds its buffer, and this term already counts it.
//! - **parse peak:** about `C × (1 + f) × M`. A handler parses only once it holds a queue slot, and
//!   while it parses it holds both the buffer and the parsed message.
//! - **queued:** `Q × f × M`, the queue's cap times the parsed size of a message.
//!
//! With the defaults and `f` = 6.7 that is about 64 MiB + 490 MiB + 6.7 GiB; lower `Q` or `M` where
//! that is too much.
//!
//! # Backpressure, not loss
//!
//! The accept loop takes a connection permit before each `accept`, so at `C` it waits and later
//! connections queue in the kernel's backlog instead of being closed. A handler that has read a
//! message waits for a queue slot while still holding its connection permit; nothing is dropped
//! for want of space. If the application never polls `receive_messages`, the queue fills, every
//! handler ends up waiting for a slot, the listener stops accepting, and senders queue in the
//! kernel's backlog until it too is full and their connects fail. Each message already read is
//! kept until the application polls. The read timeouts cover the read only, never the wait for a
//! queue slot.
//!
//! # Slow and silent peers
//!
//! Each read has three timeouts: a connection is closed if it sends nothing for
//! `first_byte_timeout_ms` after it is accepted, if it goes silent for `idle_timeout_ms` between
//! two reads, or if it has not ended 30 s after it was accepted. So a peer that connects and sends
//! nothing holds a permit for at most `first_byte_timeout_ms` (5 s by default); one that trickles a
//! byte just inside every `idle_timeout_ms` can hold it for the full 30 s. A round of `C` attacker
//! connections therefore blocks the accept loop for at most `first_byte_timeout_ms` if they are
//! silent, or 30 s if they trickle. A legitimate connection waits in the kernel's backlog behind
//! every connection queued ahead of it, and an attacker who keeps the backlog refilled with `k`
//! connections ahead of it delays it by about `k / C` such rounds. The timeouts bound the length of
//! a round, not the number of rounds.

use super::abstraction::*;
use crate::{
    circuit_breaker::{CircuitBreaker, CircuitBreakerConfig},
    error::Result,
    types::SecureMessage,
};
use async_trait::async_trait;
use serde_json;
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{Mutex, OwnedSemaphorePermit, Semaphore},
};
use tracing::{debug, error, info, warn};

/// The config key for the largest message, in bytes of serialized JSON, the receiver reads and
/// the sender sends.
pub const MAX_MESSAGE_SIZE_KEY: &str = "max_message_size";

/// The default for [`MAX_MESSAGE_SIZE_KEY`]: 1 MiB of serialized JSON.
pub const DEFAULT_MAX_MESSAGE_SIZE: usize = 1024 * 1024;

/// The config key for how many inbound connections are handled at once.
pub const MAX_CONCURRENT_CONNECTIONS_KEY: &str = "max_concurrent_connections";

/// The default for [`MAX_CONCURRENT_CONNECTIONS_KEY`].
pub const DEFAULT_MAX_CONCURRENT_CONNECTIONS: usize = 64;

/// The config key for how many received messages may wait for the application to poll.
pub const MAX_QUEUED_MESSAGES_KEY: &str = "max_queued_messages";

/// The default for [`MAX_QUEUED_MESSAGES_KEY`].
pub const DEFAULT_MAX_QUEUED_MESSAGES: usize = 1024;

/// The config key for how long, in milliseconds, a new connection may send nothing before the
/// receiver closes it.
pub const FIRST_BYTE_TIMEOUT_MS_KEY: &str = "first_byte_timeout_ms";

/// The default for [`FIRST_BYTE_TIMEOUT_MS_KEY`].
pub const DEFAULT_FIRST_BYTE_TIMEOUT_MS: usize = 5_000;

/// The config key for how long, in milliseconds, a connection may send nothing between two reads
/// before the receiver closes it.
pub const IDLE_TIMEOUT_MS_KEY: &str = "idle_timeout_ms";

/// The default for [`IDLE_TIMEOUT_MS_KEY`].
pub const DEFAULT_IDLE_TIMEOUT_MS: usize = 5_000;

/// How long the receiver waits for one connection to end, however steadily it sends.
const READ_TIMEOUT: Duration = Duration::from_secs(30);

/// The most the receiver reads, and the most it first reserves, in one step.
const READ_CHUNK: usize = 8 * 1024;

/// `config[key]` as a positive integer, or `default` when the key is absent. A value that does not
/// parse, or is zero, is an error: silently falling back to the default would hide a typo.
fn positive_limit(config: &HashMap<String, String>, key: &str, default: usize) -> Result<usize> {
    match config.get(key) {
        None => Ok(default),
        Some(value) => match value.trim().parse::<usize>() {
            Ok(0) => Err(crate::error::SynapseError::Config(format!(
                "{key} must be a positive integer, got 0"
            ))),
            Ok(limit) => Ok(limit),
            Err(e) => Err(crate::error::SynapseError::Config(format!(
                "{key} must be a positive integer, got {value:?}: {e}"
            ))),
        },
    }
}

/// How long one read may wait for data: the first read of a connection, and every later one.
#[derive(Debug, Clone, Copy)]
struct ReadTimeouts {
    first_byte: Duration,
    idle: Duration,
}

/// Every limit and timeout the config sets, each checked by [`positive_limit`].
struct Limits {
    max_message_size: usize,
    max_concurrent_connections: usize,
    max_queued_messages: usize,
    read_timeouts: ReadTimeouts,
}

impl Limits {
    fn from_config(config: &HashMap<String, String>) -> Result<Self> {
        // `capabilities()` reports `max_message_size`, so what is advertised is what is enforced.
        let millis = |key, default| -> Result<Duration> {
            Ok(Duration::from_millis(
                positive_limit(config, key, default)? as u64
            ))
        };
        Ok(Self {
            max_message_size: positive_limit(
                config,
                MAX_MESSAGE_SIZE_KEY,
                DEFAULT_MAX_MESSAGE_SIZE,
            )?,
            max_concurrent_connections: positive_limit(
                config,
                MAX_CONCURRENT_CONNECTIONS_KEY,
                DEFAULT_MAX_CONCURRENT_CONNECTIONS,
            )?,
            max_queued_messages: positive_limit(
                config,
                MAX_QUEUED_MESSAGES_KEY,
                DEFAULT_MAX_QUEUED_MESSAGES,
            )?,
            read_timeouts: ReadTimeouts {
                first_byte: millis(FIRST_BYTE_TIMEOUT_MS_KEY, DEFAULT_FIRST_BYTE_TIMEOUT_MS)?,
                idle: millis(IDLE_TIMEOUT_MS_KEY, DEFAULT_IDLE_TIMEOUT_MS)?,
            },
        })
    }
}

/// Read from `reader` until it ends or until `buffer` holds `limit` bytes, whichever comes first.
/// The buffer never reserves more than `limit` bytes: it starts at no more than one chunk and at
/// most doubles, capped at `limit`, so a peer cannot make it over-reserve the way `Vec`'s
/// amortised growth would (up to twice the bytes read).
///
/// The first read fails with [`std::io::ErrorKind::TimedOut`] if no data arrives within
/// `timeouts.first_byte`, and every later read if none arrives within `timeouts.idle`. The total
/// is the caller's to bound.
async fn read_bounded<R: AsyncRead + Unpin>(
    reader: &mut R,
    buffer: &mut Vec<u8>,
    limit: usize,
    timeouts: ReadTimeouts,
) -> std::io::Result<()> {
    let mut chunk = [0u8; READ_CHUNK];
    let mut first = true;
    while buffer.len() < limit {
        let want = READ_CHUNK.min(limit - buffer.len());
        let (wait, what) = if first {
            (timeouts.first_byte, "first byte")
        } else {
            (timeouts.idle, "next byte")
        };
        first = false;
        let n = tokio::time::timeout(wait, reader.read(&mut chunk[..want]))
            .await
            .map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    format!("no {what} within {wait:?}"),
                )
            })??;
        if n == 0 {
            break;
        }
        if buffer.capacity() - buffer.len() < n {
            let grown = buffer
                .capacity()
                .saturating_mul(2)
                .max(buffer.len() + n)
                .min(limit);
            buffer.reserve_exact(grown - buffer.len());
        }
        buffer.extend_from_slice(&chunk[..n]);
    }
    Ok(())
}

/// What an inbound connection's handler shares with the transport.
struct Inbound {
    received_messages: Arc<Mutex<Vec<Queued>>>,
    queue_slots: Arc<Semaphore>,
    metrics: Arc<RwLock<TransportMetrics>>,
    max_message_size: usize,
    read_timeouts: ReadTimeouts,
}

/// A received message waiting for the application, holding its queue slot until it is drained.
struct Queued {
    message: IncomingMessage,
    _slot: OwnedSemaphorePermit,
}

/// TCP Transport implementation
pub struct TcpTransportImpl {
    /// Local listening port
    #[expect(
        dead_code,
        reason = "stored and never read; the bound socket in `listener` is the source of truth"
    )]
    listen_port: u16,
    /// TCP listener (if acting as server). `Arc` so the accept loop spawned by
    /// `start_server` serves THIS listener instead of binding a second one.
    listener: Option<Arc<TcpListener>>,
    /// Connection timeout
    connection_timeout: Duration,
    /// The largest message, in bytes of serialized JSON, the receiver will accept on one
    /// connection and the sender will send. The receiver reads at most this bound plus one byte,
    /// then refuses and logs a larger message; the sender refuses it before connecting.
    max_message_size: usize,
    /// Inbound connections being handled at once. The accept loop takes a permit before each
    /// `accept`, so at the cap it waits and new connections queue in the kernel's backlog.
    connection_permits: Arc<Semaphore>,
    /// Timeouts for each read of an inbound connection.
    read_timeouts: ReadTimeouts,
    /// Free places in `received_messages`. A handler takes one before it parses and keeps it with
    /// the queued message; `receive_raw` releases it by draining the message.
    queue_slots: Arc<Semaphore>,
    /// Received messages, each with the queue slot it holds.
    received_messages: Arc<Mutex<Vec<Queued>>>,
    /// Current status
    status: Arc<RwLock<TransportStatus>>,
    /// Performance metrics
    metrics: Arc<RwLock<TransportMetrics>>,
    /// Circuit breaker for reliability
    #[expect(
        dead_code,
        reason = "constructed and never consulted -- the TCP path has no circuit breaking; see CAPABILITY_INVENTORY.md"
    )]
    circuit_breaker: Arc<CircuitBreaker>,
}

impl TcpTransportImpl {
    /// Create a new TCP transport instance. See the module documentation for the config keys.
    /// Fails if any of the limit and timeout keys is present but not a positive integer.
    pub async fn new(config: &HashMap<String, String>) -> Result<Self> {
        let listen_port = config
            .get("listen_port")
            .and_then(|p| p.parse().ok())
            .unwrap_or(0); // 0 means let OS choose port

        let connection_timeout = config
            .get("connection_timeout_ms")
            .and_then(|t| t.parse().ok())
            .map(Duration::from_millis)
            .unwrap_or(Duration::from_secs(10));

        let Limits {
            max_message_size,
            max_concurrent_connections,
            max_queued_messages,
            read_timeouts,
        } = Limits::from_config(config)?;

        let bind_scope = crate::network_scope::BindScope::from_config_map(config)?;
        let listener = if listen_port > 0 {
            match TcpListener::bind(bind_scope.listen_addr(listen_port)).await {
                Ok(listener) => {
                    info!("TCP transport listening on port {}", listen_port);
                    Some(Arc::new(listener))
                }
                Err(e) => {
                    warn!("Failed to bind TCP port {}: {}", listen_port, e);
                    None
                }
            }
        } else {
            None
        };

        let metrics = TransportMetrics {
            transport_type: TransportType::Tcp,
            ..Default::default()
        };

        Ok(Self {
            listen_port,
            listener,
            connection_timeout,
            max_message_size,
            connection_permits: Arc::new(Semaphore::new(max_concurrent_connections)),
            read_timeouts,
            queue_slots: Arc::new(Semaphore::new(max_queued_messages)),
            received_messages: Arc::new(Mutex::new(Vec::new())),
            status: Arc::new(RwLock::new(TransportStatus::Stopped)),
            metrics: Arc::new(RwLock::new(metrics)),
            circuit_breaker: Arc::new(CircuitBreaker::new(CircuitBreakerConfig::default())),
        })
    }

    /// Start the TCP server for incoming connections
    async fn start_server(&self) -> Result<()> {
        if let Some(listener) = &self.listener {
            let received_messages = Arc::clone(&self.received_messages);
            let metrics = Arc::clone(&self.metrics);
            let max_message_size = self.max_message_size;
            let permits = Arc::clone(&self.connection_permits);
            let queue_slots = Arc::clone(&self.queue_slots);
            let read_timeouts = self.read_timeouts;
            // Serve the listener the constructor already bound. The previous version
            // bound a SECOND listener on the same port inside this task; that bind
            // fails because the port is already owned by this struct's own listener,
            // the error was swallowed (the `if let Ok` fell through to a logged-only
            // `else`), so the accept loop never ran. Meanwhile the constructor's
            // listener held the port and its backlog absorbed connections that were
            // never accepted — so `send_message` connected, wrote, and receipted
            // `Sent` while `receive_messages` returned nothing. Sharing the existing
            // listener via `Arc` removes the second bind entirely.
            let listener = Arc::clone(listener);
            let local_addr = listener.local_addr().map_err(|e| {
                crate::error::SynapseError::TransportError(format!(
                    "Failed to get local address: {}",
                    e
                ))
            })?;

            tokio::spawn(async move {
                info!("TCP server started on {}", local_addr);
                loop {
                    // Backpressure: take a permit BEFORE accepting, so at the cap the loop waits
                    // for a handler to finish and the waiting connections stay in the kernel's
                    // backlog rather than each holding a buffer here. The semaphore is never
                    // closed, so `acquire_owned` cannot fail while this task runs.
                    let Ok(permit) = Arc::clone(&permits).acquire_owned().await else {
                        error!("TCP connection limit closed; the accept loop stops");
                        return;
                    };
                    match listener.accept().await {
                        Ok((stream, addr)) => {
                            debug!("Accepted TCP connection from {}", addr);
                            let messages_clone = Arc::clone(&received_messages);
                            let metrics_clone = Arc::clone(&metrics);
                            let slots_clone = Arc::clone(&queue_slots);

                            tokio::spawn(async move {
                                Self::handle_connection(
                                    stream,
                                    addr.to_string(),
                                    Inbound {
                                        received_messages: messages_clone,
                                        queue_slots: slots_clone,
                                        metrics: metrics_clone,
                                        max_message_size,
                                        read_timeouts,
                                    },
                                )
                                .await;
                                // Held for the whole read and any wait for a queue slot, released
                                // when the handler finishes.
                                drop(permit);
                            });
                        }
                        Err(e) => {
                            error!("Failed to accept TCP connection: {}", e);
                            tokio::time::sleep(Duration::from_millis(100)).await;
                        }
                    }
                }
            });

            Ok(())
        } else {
            debug!("TCP transport running in client-only mode");
            Ok(())
        }
    }

    /// Read one message from `stream`. The wire format is one JSON `SecureMessage` per
    /// connection, terminated by the sender's shutdown (EOF), so the receiver reads to EOF --
    /// never a single `read`, which returns whatever one segment happened to carry and silently
    /// truncated any message over 8 KiB. A connection that closes having sent nothing is a
    /// reachability probe (`can_reach`, `estimate_metrics` and `test_connectivity` connect and
    /// close), not a dropped message, and is logged at debug.
    ///
    /// Once the message is read, the handler waits for a queue slot -- with no timeout, still
    /// holding its connection permit -- and only then parses and queues it, so a full queue slows
    /// the listener down instead of losing messages.
    async fn handle_connection(stream: TcpStream, source_addr: String, inbound: Inbound) {
        let Inbound {
            received_messages,
            queue_slots,
            metrics,
            max_message_size,
            read_timeouts,
        } = inbound;
        let mut stream = stream;
        // One byte past the bound, so an oversize message is detected rather than truncated.
        let limit = max_message_size.saturating_add(1);
        let mut buffer = Vec::with_capacity(limit.min(READ_CHUNK));

        // The total timeout, and `read_bounded`'s first-byte and idle timeouts, cover the read
        // only; the wait for a queue slot below is deliberately outside them.
        let outcome = tokio::time::timeout(
            READ_TIMEOUT,
            read_bounded(&mut stream, &mut buffer, limit, read_timeouts),
        )
        .await;
        // Nothing more is read; release the socket while waiting for a queue slot.
        drop(stream);
        let bytes_read = buffer.len();
        match outcome {
            Ok(Ok(())) if bytes_read == 0 => {
                debug!(
                    "TCP connection from {} closed without sending anything (a probe)",
                    source_addr
                );
                return;
            }
            Ok(Ok(())) => {}
            Ok(Err(e)) if bytes_read == 0 => {
                debug!(
                    "TCP connection from {} failed before sending anything: {}",
                    source_addr, e
                );
                return;
            }
            Ok(Err(e)) => {
                warn!(
                    "Dropped TCP message from {}: read failed after {} bytes: {}",
                    source_addr, bytes_read, e
                );
                return;
            }
            Err(_) => {
                warn!(
                    "Closed TCP connection from {}: it did not end within {:?} ({} bytes read)",
                    source_addr, READ_TIMEOUT, bytes_read
                );
                return;
            }
        }

        if bytes_read > max_message_size {
            warn!(
                "Dropped TCP message from {}: larger than the {} byte limit",
                source_addr, max_message_size
            );
            return;
        }
        debug!("Received {} bytes via TCP from {}", bytes_read, source_addr);

        // Wait for queue space before parsing, so a waiting handler holds only its raw bytes, not
        // the several times larger parsed message. The semaphore is never closed, so
        // `acquire_owned` cannot fail while the transport exists.
        let Ok(slot) = queue_slots.acquire_owned().await else {
            error!(
                "Dropped TCP message from {}: the receive queue was closed",
                source_addr
            );
            return;
        };

        let message = match serde_json::from_slice::<SecureMessage>(&buffer) {
            Ok(message) => message,
            Err(e) => {
                warn!(
                    "Dropped TCP message from {}: {} bytes did not parse as a SecureMessage: {}",
                    source_addr, bytes_read, e
                );
                return;
            }
        };

        drop(buffer);

        let incoming = IncomingMessage::new(message, TransportType::Tcp, source_addr);
        {
            // Wait for the lock. `try_lock` here dropped the message whenever `receive_raw`
            // or another connection held the lock.
            let mut messages = received_messages.lock().await;
            messages.push(Queued {
                message: incoming,
                _slot: slot,
            });
            debug!("Queued TCP message, total: {}", messages.len());
        }

        // Update metrics
        if let Ok(mut metrics) = metrics.try_write() {
            metrics.messages_received += 1;
            metrics.bytes_received += bytes_read as u64;
            metrics.touch();
        }
    }

    async fn connect_and_send(
        &self,
        address: &str,
        port: u16,
        message: &SecureMessage,
    ) -> Result<DeliveryReceipt> {
        let target_addr = format!("{}:{}", address, port);

        // Serialize first, and refuse what a receiver with this limit would drop: writing it
        // would claim `Sent` for a message that is never delivered.
        let message_json = serde_json::to_string(message).map_err(|e| {
            crate::error::SynapseError::TransportError(format!(
                "Failed to serialize message: {}",
                e
            ))
        })?;
        if message_json.len() > self.max_message_size {
            // A refusal of this message, not a transport failure: see `SynapseError::MessageRefused`.
            return Err(crate::error::SynapseError::MessageRefused(format!(
                "TCP message {} serializes to {} bytes, over this transport's {} of {} bytes \
                 (serialized JSON); a receiver with that limit would drop it",
                message.message_id.0,
                message_json.len(),
                MAX_MESSAGE_SIZE_KEY,
                self.max_message_size
            )));
        }

        debug!("Connecting to TCP target: {}", target_addr);
        let start_time = Instant::now();

        // Connect with timeout
        let stream =
            tokio::time::timeout(self.connection_timeout, TcpStream::connect(&target_addr))
                .await
                .map_err(|_| {
                    crate::error::SynapseError::TransportError("TCP connection timeout".to_string())
                })?
                .map_err(|e| {
                    crate::error::SynapseError::TransportError(format!(
                        "TCP connection failed: {}",
                        e
                    ))
                })?;

        let connect_time = start_time.elapsed();
        debug!("TCP connection established in {:?}", connect_time);

        // Send message
        let mut stream = stream;
        let send_start = Instant::now();

        stream
            .write_all(message_json.as_bytes())
            .await
            .map_err(|e| {
                crate::error::SynapseError::TransportError(format!(
                    "Failed to send TCP message: {}",
                    e
                ))
            })?;

        stream.flush().await.map_err(|e| {
            crate::error::SynapseError::TransportError(format!("Failed to flush TCP stream: {}", e))
        })?;

        // Close the write half: the receiver reads to EOF, so the end of the message is this
        // deliberate shutdown rather than whatever happens when the stream is dropped.
        stream.shutdown().await.map_err(|e| {
            crate::error::SynapseError::TransportError(format!(
                "Failed to shut down TCP stream: {}",
                e
            ))
        })?;

        let send_time = send_start.elapsed();
        let total_time = start_time.elapsed();

        info!(
            "TCP message sent to {} in {:?} (connect: {:?}, send: {:?})",
            target_addr, total_time, connect_time, send_time
        );

        // Update metrics
        if let Ok(mut metrics) = self.metrics.try_write() {
            metrics.messages_sent += 1;
            metrics.bytes_sent += message_json.len() as u64;

            // Update average latency
            let total_messages = metrics.messages_sent;
            let old_avg_ms = metrics.average_latency_ms as f64;
            let new_latency_ms = total_time.as_millis() as f64;
            let new_avg_ms =
                (old_avg_ms * (total_messages - 1) as f64 + new_latency_ms) / total_messages as f64;
            metrics.average_latency_ms = new_avg_ms as u64;

            metrics.touch();
        }

        Ok(DeliveryReceipt {
            message_id: message.message_id.0.to_string(),
            transport_used: TransportType::Tcp,
            delivery_time: total_time,
            target_reached: target_addr,
            confirmation: DeliveryConfirmation::Sent,
            metadata: {
                let mut meta = HashMap::new();
                meta.insert(
                    "connect_time_ms".to_string(),
                    connect_time.as_millis().to_string(),
                );
                meta.insert(
                    "send_time_ms".to_string(),
                    send_time.as_millis().to_string(),
                );
                meta
            },
        })
    }

    fn parse_target_address(&self, target: &TransportTarget) -> Result<(String, u16)> {
        if let Some(address) = &target.address {
            // Try to parse as host:port
            if let Some(colon_pos) = address.rfind(':') {
                let host = address[..colon_pos].to_string();
                let port_str = &address[colon_pos + 1..];
                let port = port_str.parse::<u16>().map_err(|_| {
                    crate::error::SynapseError::TransportError(format!(
                        "Invalid port in address: {}",
                        address
                    ))
                })?;
                Ok((host, port))
            } else {
                // No port specified, use default
                Ok((address.clone(), 8080))
            }
        } else {
            // Use identifier as hostname with default port
            Ok((target.identifier.clone(), 8080))
        }
    }
}

#[async_trait]
impl Transport for TcpTransportImpl {
    fn transport_type(&self) -> TransportType {
        TransportType::Tcp
    }

    /// TCP's capabilities, with `max_message_size` set to this instance's configured limit. That
    /// limit is in bytes of serialized JSON, not body bytes: see the module documentation.
    fn capabilities(&self) -> TransportCapabilities {
        TransportCapabilities {
            max_message_size: self.max_message_size,
            ..TransportCapabilities::tcp()
        }
    }

    async fn can_reach(&self, target: &TransportTarget) -> bool {
        // Try to parse the target address
        if let Ok((host, port)) = self.parse_target_address(target) {
            // Attempt a quick connection test
            matches!(
                tokio::time::timeout(
                    Duration::from_secs(5),
                    TcpStream::connect(format!("{}:{}", host, port)),
                )
                .await,
                Ok(Ok(_))
            )
        } else {
            false
        }
    }

    async fn estimate_metrics(&self, target: &TransportTarget) -> Result<TransportEstimate> {
        let (host, port) = self.parse_target_address(target)?;

        // Perform a quick connection test to estimate metrics
        let start = Instant::now();
        let can_connect = matches!(
            tokio::time::timeout(
                Duration::from_secs(3),
                TcpStream::connect(format!("{}:{}", host, port)),
            )
            .await,
            Ok(Ok(_))
        );
        let rtt = start.elapsed();

        Ok(TransportEstimate {
            latency: if can_connect {
                rtt
            } else {
                Duration::from_secs(30)
            },
            reliability: if can_connect { 0.9 } else { 0.1 },
            bandwidth: 1_000_000, // 1MB/s estimate for TCP
            cost: 1.0,            // Relative cost
            available: can_connect,
            confidence: if can_connect { 0.8 } else { 0.3 },
        })
    }

    async fn send_message(
        &self,
        target: &TransportTarget,
        message: &SecureMessage,
    ) -> Result<DeliveryReceipt> {
        let (host, port) = self.parse_target_address(target)?;
        self.connect_and_send(&host, port, message).await
    }

    async fn test_connectivity(&self, target: &TransportTarget) -> Result<ConnectivityResult> {
        let (host, port) = self.parse_target_address(target)?;
        let target_addr = format!("{}:{}", host, port);

        let start = Instant::now();
        match tokio::time::timeout(Duration::from_secs(10), TcpStream::connect(&target_addr)).await
        {
            Ok(Ok(_)) => {
                let rtt = start.elapsed();
                Ok(ConnectivityResult {
                    connected: true,
                    rtt: Some(rtt),
                    error: None,
                    quality: 1.0 - (rtt.as_millis() as f64 / 10000.0).min(1.0), // Quality decreases with latency
                    details: {
                        let mut details = HashMap::new();
                        details.insert("target".to_string(), target_addr);
                        details.insert("rtt_ms".to_string(), rtt.as_millis().to_string());
                        details
                    },
                })
            }
            Ok(Err(e)) => Ok(ConnectivityResult {
                connected: false,
                rtt: None,
                error: Some(format!("Connection failed: {}", e)),
                quality: 0.0,
                details: {
                    let mut details = HashMap::new();
                    details.insert("target".to_string(), target_addr);
                    details.insert("error".to_string(), e.to_string());
                    details
                },
            }),
            Err(_) => Ok(ConnectivityResult {
                connected: false,
                rtt: None,
                error: Some("Connection timeout".to_string()),
                quality: 0.0,
                details: {
                    let mut details = HashMap::new();
                    details.insert("target".to_string(), target_addr);
                    details.insert("error".to_string(), "timeout".to_string());
                    details
                },
            }),
        }
    }

    async fn start(&self) -> Result<()> {
        info!("Starting TCP transport");

        {
            let mut status = self.status.write().unwrap();
            *status = TransportStatus::Starting;
        }

        // Start server if we have a listener
        self.start_server().await?;

        {
            let mut status = self.status.write().unwrap();
            *status = TransportStatus::Running;
        }

        info!("TCP transport started successfully");
        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        info!("Stopping TCP transport");

        {
            let mut status = self.status.write().unwrap();
            *status = TransportStatus::Stopping;
        }

        // TCP transport doesn't need explicit cleanup
        // Connections will be closed when dropped

        {
            let mut status = self.status.write().unwrap();
            *status = TransportStatus::Stopped;
        }

        info!("TCP transport stopped");
        Ok(())
    }

    async fn status(&self) -> TransportStatus {
        *self.status.read().unwrap()
    }

    async fn metrics(&self) -> TransportMetrics {
        self.metrics.read().unwrap().clone()
    }
}

#[async_trait]
impl TransportReceive for TcpTransportImpl {
    async fn receive_raw(&self, inbox: &mut RawInbox) -> Result<()> {
        // Draining a message drops the queue slot it held, so a handler waiting for space can
        // queue its message as soon as this lock is released.
        let mut messages = self.received_messages.lock().await;
        inbox.extend(messages.drain(..).map(|queued| queued.message));
        Ok(())
    }
}

/// Factory for creating TCP transport instances. The config keys, their defaults, and what the
/// limits bound are in the module documentation; `create_transport` refuses an unparseable or
/// zero value for any limit or timeout key.
pub struct TcpTransportFactory;

#[async_trait]
impl TransportFactory for TcpTransportFactory {
    async fn create_transport(
        &self,
        config: &HashMap<String, String>,
    ) -> Result<Box<dyn Transport>> {
        let transport = TcpTransportImpl::new(config).await?;
        Ok(Box::new(transport))
    }

    fn transport_type(&self) -> TransportType {
        TransportType::Tcp
    }

    fn default_config(&self) -> HashMap<String, String> {
        let mut config = HashMap::new();
        config.insert("listen_port".to_string(), "8080".to_string());
        config.insert("connection_timeout_ms".to_string(), "10000".to_string());
        // In bytes of serialized JSON, the unit sender and receiver both enforce.
        config.insert(
            MAX_MESSAGE_SIZE_KEY.to_string(),
            DEFAULT_MAX_MESSAGE_SIZE.to_string(),
        );
        config.insert(
            MAX_CONCURRENT_CONNECTIONS_KEY.to_string(),
            DEFAULT_MAX_CONCURRENT_CONNECTIONS.to_string(),
        );
        config.insert(
            MAX_QUEUED_MESSAGES_KEY.to_string(),
            DEFAULT_MAX_QUEUED_MESSAGES.to_string(),
        );
        config.insert(
            FIRST_BYTE_TIMEOUT_MS_KEY.to_string(),
            DEFAULT_FIRST_BYTE_TIMEOUT_MS.to_string(),
        );
        config.insert(
            IDLE_TIMEOUT_MS_KEY.to_string(),
            DEFAULT_IDLE_TIMEOUT_MS.to_string(),
        );
        config
    }

    fn validate_config(&self, config: &HashMap<String, String>) -> Result<()> {
        if let Some(port_str) = config.get("listen_port")
            && port_str.parse::<u16>().is_err()
        {
            return Err(crate::error::SynapseError::TransportError(format!(
                "Invalid listen_port: {}",
                port_str
            )));
        }

        // The same check `new` applies, so validating and constructing cannot disagree.
        Limits::from_config(config)?;

        if let Some(timeout_str) = config.get("connection_timeout_ms")
            && timeout_str.parse::<u64>().is_err()
        {
            return Err(crate::error::SynapseError::TransportError(format!(
                "Invalid connection_timeout_ms: {}",
                timeout_str
            )));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Timeouts no in-memory reader comes near.
    const NO_WAIT: ReadTimeouts = ReadTimeouts {
        first_byte: Duration::from_secs(60),
        idle: Duration::from_secs(60),
    };

    /// The buffer holds at most `limit` bytes and never reserves more, however much the peer
    /// sends; a short message reserves no more than one chunk.
    #[tokio::test]
    async fn read_bounded_never_reserves_past_its_limit() {
        let data = vec![b'x'; 200_000];
        for limit in [
            1,
            100,
            READ_CHUNK,
            READ_CHUNK + 1,
            50_001,
            199_999,
            200_000,
            200_001,
        ] {
            let mut reader: &[u8] = &data;
            let mut buffer = Vec::with_capacity(limit.min(READ_CHUNK));
            read_bounded(&mut reader, &mut buffer, limit, NO_WAIT)
                .await
                .expect("read");
            assert_eq!(buffer.len(), limit.min(data.len()), "limit {limit}");
            assert!(
                buffer.capacity() <= limit,
                "limit {limit}: capacity {} exceeds it",
                buffer.capacity()
            );
        }
        let mut reader: &[u8] = b"short";
        let mut buffer = Vec::with_capacity(READ_CHUNK);
        read_bounded(&mut reader, &mut buffer, 1024 * 1024 + 1, NO_WAIT)
            .await
            .expect("read");
        assert_eq!(buffer, b"short");
        assert_eq!(buffer.capacity(), READ_CHUNK);
    }

    /// `validate_config` applies the same rule as `new`.
    #[test]
    fn validate_config_refuses_what_new_refuses() {
        for key in [
            MAX_MESSAGE_SIZE_KEY,
            MAX_CONCURRENT_CONNECTIONS_KEY,
            MAX_QUEUED_MESSAGES_KEY,
            FIRST_BYTE_TIMEOUT_MS_KEY,
            IDLE_TIMEOUT_MS_KEY,
        ] {
            for bad in ["0", "abc", ""] {
                let config = HashMap::from([(key.to_string(), bad.to_string())]);
                assert!(
                    TcpTransportFactory.validate_config(&config).is_err(),
                    "{key} = {bad:?}"
                );
            }
            let config = HashMap::from([(key.to_string(), "7".to_string())]);
            assert!(
                TcpTransportFactory.validate_config(&config).is_ok(),
                "{key}"
            );
        }
        assert!(
            TcpTransportFactory
                .validate_config(&TcpTransportFactory.default_config())
                .is_ok()
        );
    }
}
