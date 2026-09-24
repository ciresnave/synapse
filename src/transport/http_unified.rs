// SPDX-License-Identifier: MIT OR Apache-2.0
//! HTTP transport conforming to the unified Transport trait
//!
//! # Wire format
//!
//! One JSON `SecureMessage` per request: `POST /synapse/message` ([`MESSAGE_PATH`]) with the
//! message's JSON as the body. The receiver's server has that one route. It answers:
//!
//! | status | when |
//! |---|---|
//! | `202 Accepted` | the message was read, parsed and **is in the queue** `receive_raw` drains |
//! | `400 Bad Request` | the body is empty, ends before its `Content-Length`, or does not parse as a `SecureMessage` |
//! | `408 Request Timeout` | the body went silent for `idle_timeout_ms` |
//! | `411 Length Required` | the request has no `Content-Length`, or has a `Transfer-Encoding` (chunked bodies are not accepted); nothing of the body is read |
//! | `413 Payload Too Large` | the `Content-Length` is over `max_message_size`; nothing of the body is read |
//! | `503 Service Unavailable` | the transport is stopping, or no queue budget came free before the request's deadline |
//!
//! Any other path is `404` and any other method `405`.
//!
//! **One request per connection.** The server turns HTTP/1.1 keep-alive off: it answers each
//! connection's first request with `Connection: close` and closes it, and a second request
//! pipelined behind the first is never read. A kept-alive connection could carry any number of
//! requests, each one a new message to buffer and parse under one connection permit, so the
//! connection cap would no longer bound the requests in flight, and a peer could hold a permit
//! indefinitely by sending a request just inside every timeout. With one request per connection,
//! every limit and bound below carries over from TCP unchanged: one connection permit covers one
//! message. The cost is one TCP handshake per message, as for TCP and WebSocket. HTTP/2 is not
//! served.
//!
//! # Delivery claim
//!
//! The server answers `202` only **after** the message is parsed and pushed into the queue, with
//! queue budget already taken for it. So a `2xx` from a peer running this server means its
//! transport stack holds the message for the application, and `send_message` returns
//! `Delivered` (spec §4: "an HTTP 2xx"). Any other status, or no response, is an `Err`, not
//! `Sent`: the bytes may have been written, but the peer refused them or never said it had them.
//! The claim rests on the peer being this server: an HTTP server that answers `2xx` without
//! queuing anything would be believed.
//!
//! One ambiguity is left, as in any acknowledged protocol: if the `2xx` is lost after the message
//! is queued (the sender's own `timeout_ms` passes first, or the connection breaks while the
//! response is written), the sender sees an `Err` for a message that was delivered. A resend is
//! then a duplicate, which the manager's replay record drops.
//!
//! # Config keys
//!
//! | key | default | meaning |
//! |---|---|---|
//! | [`SERVER_PORT_KEY`] (`server_port`) | `0` | the port `start` listens on; `0` means send only, with no server |
//! | `bind_scope` | `loopback` | which interfaces the server binds ([`crate::network_scope::BindScope`]) |
//! | [`TIMEOUT_MS_KEY`] (`timeout_ms`) | [`DEFAULT_TIMEOUT_MS`] (30000) | how long `send_message` waits for the whole exchange: connect, request and response |
//! | [`USE_HTTPS_KEY`] (`use_https`) | `false` | whether a `host:port` target is dialled with `https://` rather than `http://` |
//! | `user_agent` | `Synapse-HTTP-Transport/2.0` | the sender's `User-Agent` |
//! | [`MAX_MESSAGE_SIZE_KEY`] (`max_message_size`) | [`DEFAULT_MAX_MESSAGE_SIZE`] (1 MiB) | the largest message, **in bytes of serialized JSON** (the request body), the server reads and the sender sends |
//! | [`MAX_CONCURRENT_CONNECTIONS_KEY`] (`max_concurrent_connections`) | [`DEFAULT_MAX_CONCURRENT_CONNECTIONS`] (64) | how many inbound connections, and so requests, are handled at once |
//! | [`MAX_QUEUED_BYTES_KEY`] (`max_queued_bytes`) | [`DEFAULT_MAX_QUEUED_BYTES`] (4 MiB) | how many bytes of received messages, **counted as serialized JSON**, may wait for the application to poll; at least `max_message_size` and at most `u32::MAX` |
//! | [`HEADER_READ_TIMEOUT_MS_KEY`] (`header_read_timeout_ms`) | [`DEFAULT_HEADER_READ_TIMEOUT_MS`] (5000) | how long a new connection may take to send its whole request head (request line and headers), counted from when it is accepted |
//! | [`IDLE_TIMEOUT_MS_KEY`] (`idle_timeout_ms`) | [`DEFAULT_IDLE_TIMEOUT_MS`] (5000) | how long the body may go without a single byte arriving (a gap between reads, not a deadline for the whole body) |
//! | [`REQUEST_TIMEOUT_MS_KEY`] (`request_timeout_ms`) | [`DEFAULT_REQUEST_TIMEOUT_MS`] (20000) | the most one inbound connection may take, from accept to response, including any wait for queue budget |
//!
//! `server_port` must parse as a port number, `use_https` as `true` or `false`, and every other
//! key but `bind_scope` and `user_agent` as a positive integer: [`HttpTransportImpl::new`] (and so
//! the factory's `create_transport`) and [`validate_config`] refuse anything else, through the same
//! check, instead of falling back to the default. They also refuse a `max_queued_bytes` below
//! `max_message_size`, which would leave a message that passed the size check waiting for room the
//! queue can never have, and one above `u32::MAX`, the most one semaphore acquire can take; and the
//! keys `server_address`, superseded by `bind_scope`, and `max_connections`, superseded by
//! `max_concurrent_connections`, which are no longer read and would otherwise be silently ignored.
//!
//! `request_timeout_ms` defaults to 20 s, under the sender's 30 s `timeout_ms`, so that a sender
//! waiting on a full queue gets the `503` rather than timing out first, not knowing whether its
//! message was queued.
//!
//! `max_message_size` counts serialized bytes, not body bytes: `encrypted_content` serializes as
//! a JSON number array, a few characters per body byte. Sender and receiver apply the same number
//! to the same bytes, so a sender refuses exactly what a receiver with its limit would refuse with
//! `413`. The sender's refusal is a
//! [`SynapseError::MessageRefused`](crate::error::SynapseError::MessageRefused), returned before
//! connecting, which the transport manager does not count against the transport's health.
//!
//! # Targets
//!
//! A target's address is an `http://` URL, an `https://` URL, or `host:port` (dialled as
//! `http://host:port/synapse/message`, or `https://` with `use_https`). Either must name a
//! non-empty host and a port from 1 to 65535; no default port is guessed, not even 80 for
//! `http://`. A URL whose path is empty or `/` is sent to `/synapse/message`; any other path is
//! sent as given (a reverse proxy may route it). Refused: any other scheme, a target with no
//! address, userinfo (`user@host`), a fragment, and a host that the URL parser rewrites -- a
//! percent-encoded or non-ASCII name, or a numeric host that is not a plain address, such as
//! `0x7f.1` or `127.1`, which it reads as 127.0.0.1.
//!
//! The target is checked by the parser that dials it: the checked [`reqwest::Url`] is the value
//! handed to the client, and its host and port must be the ones the address names, so what is
//! accepted here is exactly what is dialled. The client follows no redirect (a `3xx` would send
//! the message somewhere that was never checked) and uses no proxy, whatever the environment's
//! `HTTP_PROXY` says.
//!
//! `https://` is accepted because TLS is compiled in: `reqwest`'s default features, which this
//! crate keeps, include `default-tls` (rustls), and the unit test
//! `https_targets_are_dialled_with_tls` checks that an `https://` send opens with a TLS handshake.
//! This transport's own server speaks plain HTTP only, so an `https://` target is some other
//! server, such as a TLS-terminating proxy in front of one.
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
//! - **connection buffers:** `C × (M + 140 KiB)` bytes. At most `C` connections are handled at
//!   once, and each carries one request. A body must declare its length: over `M` it is refused
//!   (`413`) and without a `Content-Length` it is refused (`411`), both before any of it is read.
//!   Otherwise the handler allocates exactly the declared length, once, when the first body byte
//!   arrives, and never grows it, so a body costs at most `M` bytes and no reallocation holds two
//!   copies. The rest of a connection -- hyper's read buffer (its length capped at
//!   [`READ_BUFFER_LIMIT`], 64 KiB, which also refuses a longer request head with `431`), the
//!   parsed head, the routing and the handler's future -- was **measured** at about 138 KiB. A
//!   handler holds its body while it waits for queue budget and while it parses, and this term
//!   counts it in both.
//! - **parsed messages:** `B × f`. Before parsing, a handler takes budget for its message's JSON
//!   length, and the queued message keeps that budget until the application drains it. So every
//!   message being parsed or waiting in the queue holds budget for its own raw bytes, and together
//!   they hold at most `B`.
//!
//! **Measured**, with a counting allocator around a release build on Windows, which counts a
//! reallocation as a new block, a copy and a free so its transient is in the peak: 16 connections
//! at once, each sending a whole body of `M` bytes and then waiting for queue budget, which a
//! queued message of nearly `M` bytes had filled. Above the heap before they connected, each held
//! 20.6 KiB once its head was read, and at the peak `M` plus 137 to 138 KiB -- the same overhead at
//! `M` = 256 KiB, 1 MiB and 4 MiB, with the body written in one write, in 64 KiB and 1 KiB writes,
//! and, at 256 KiB, in 7-byte writes. A chunked body was refused with `411` and held nothing.
//! **Derived, not measured:** that no other request shape makes hyper's buffers larger than these
//! did (the 140 KiB above rounds up the largest overhead measured; it is not a proven maximum), and
//! that the figure holds for `C` connections as it did for 16.
//!
//! With the defaults (`C` = 64, `M` = 1 MiB, `B` = 4 MiB) that is 64 × (1 MiB + 140 KiB) =
//! 76,283,904 bytes, plus 4 MiB × 18 = 75,497,472 bytes while parsing: 151,781,376 bytes, about
//! 145 MiB at peak (126,615,552 bytes, about 121 MiB, with every message parsed and retained at
//! `f` = 12). Lower `B` to shrink the second term, and `C` or `M` to shrink the first.
//!
//! The bound ends where a message is drained. `receive_raw` releases a message's budget before the
//! manager verifies and opens it, so while the manager processes a batch, up to `B × f` of drained
//! messages sit outside the bound while the queue refills another `B`.
//!
//! # Backpressure: requests wait
//!
//! The accept loop takes a connection permit before each `accept`, so at `C` it waits and later
//! connections queue in the kernel's backlog instead of being closed. A handler that has read a
//! message waits for queue budget while still holding its connection permit, and answers only once
//! the message is queued -- so a full queue slows senders down instead of losing messages, and a
//! sender's `send_message` does not return `Delivered` until its message is really queued. The
//! wait is bounded by the connection's `request_timeout_ms` deadline: a handler still without
//! budget then answers `503` and drops the message, and the sender gets an `Err`. If the
//! application never polls, the budget runs out, handlers wait and then answer `503`, and senders
//! behind them wait in the backlog.
//!
//! # Slow and silent peers
//!
//! A connection is closed if its request head is not complete within `header_read_timeout_ms` of
//! being accepted (hyper's `header_read_timeout`, which starts when the connection is first polled,
//! so it covers a peer that sends nothing at all); if its body goes `idle_timeout_ms` without a
//! byte (`408`); or if it is still open `request_timeout_ms` after it was accepted, however steadily
//! it sends. So a peer that connects and sends nothing, or trickles its head, holds a permit for at
//! most `header_read_timeout_ms` (5 s by default); one that trickles its body a byte just inside
//! every `idle_timeout_ms` can hold it for `request_timeout_ms` (20 s). The timeouts bound the
//! length of a round of `C` attacker connections, not the number of rounds.

use super::abstraction::*;
use crate::{
    circuit_breaker::{CircuitBreaker, CircuitBreakerConfig, RequestOutcome},
    error::{Result, SynapseError},
    types::SecureMessage,
};
use async_trait::async_trait;
use axum::{
    Router,
    body::Body,
    extract::{Request, State},
    http::StatusCode,
    routing::post,
};
use http_body_util::BodyExt;
use reqwest::{Client, Url};
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{
        Arc, RwLock,
        atomic::{AtomicU32, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::{Mutex, OwnedSemaphorePermit, Semaphore, watch},
};
use tower::ServiceExt;
use tracing::{debug, error, info, warn};

/// The one route the server serves: `POST` a JSON `SecureMessage` here.
pub const MESSAGE_PATH: &str = "/synapse/message";

/// The config key for the port `start` listens on. `0`, the default, means send only.
pub const SERVER_PORT_KEY: &str = "server_port";

/// The config key for how long, in milliseconds, `send_message` waits for the whole exchange.
pub const TIMEOUT_MS_KEY: &str = "timeout_ms";

/// The default for [`TIMEOUT_MS_KEY`].
pub const DEFAULT_TIMEOUT_MS: usize = 30_000;

/// The config key for whether a `host:port` target is dialled with `https://`.
pub const USE_HTTPS_KEY: &str = "use_https";

/// The config key for the sender's `User-Agent`.
pub const USER_AGENT_KEY: &str = "user_agent";

/// The default for [`USER_AGENT_KEY`].
pub const DEFAULT_USER_AGENT: &str = "Synapse-HTTP-Transport/2.0";

/// The config key for the largest message, in bytes of serialized JSON, the server reads and the
/// sender sends.
pub const MAX_MESSAGE_SIZE_KEY: &str = "max_message_size";

/// The default for [`MAX_MESSAGE_SIZE_KEY`]: 1 MiB of serialized JSON.
pub const DEFAULT_MAX_MESSAGE_SIZE: usize = 1024 * 1024;

/// The config key for how many inbound connections, and so requests, are handled at once.
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

/// The config key for how long, in milliseconds, a new connection may take to send its whole
/// request head.
pub const HEADER_READ_TIMEOUT_MS_KEY: &str = "header_read_timeout_ms";

/// The default for [`HEADER_READ_TIMEOUT_MS_KEY`].
pub const DEFAULT_HEADER_READ_TIMEOUT_MS: usize = 5_000;

/// The config key for how long, in milliseconds, a request body may go without a byte arriving.
pub const IDLE_TIMEOUT_MS_KEY: &str = "idle_timeout_ms";

/// The default for [`IDLE_TIMEOUT_MS_KEY`].
pub const DEFAULT_IDLE_TIMEOUT_MS: usize = 5_000;

/// The config key for the most one inbound connection may take, in milliseconds, from accept to
/// response, including any wait for queue budget.
pub const REQUEST_TIMEOUT_MS_KEY: &str = "request_timeout_ms";

/// The default for [`REQUEST_TIMEOUT_MS_KEY`]: under [`DEFAULT_TIMEOUT_MS`], so a sender waiting on
/// a full queue gets the `503` rather than timing out first.
pub const DEFAULT_REQUEST_TIMEOUT_MS: usize = 20_000;

/// Keys this transport once read and no longer does. Each is refused, naming its replacement,
/// rather than silently ignored.
const RETIRED_KEYS: [(&str, &str); 2] = [
    ("server_address", "bind_scope"),
    ("max_connections", MAX_CONCURRENT_CONNECTIONS_KEY),
];

/// hyper's read buffer cap per connection (`max_buf_size`). A request head larger than this is
/// refused with `431`. Body chunks are split off this buffer.
pub const READ_BUFFER_LIMIT: usize = 64 * 1024;

/// After a connection's deadline, how long it has left to write a `503` before it is dropped.
const RESPONSE_GRACE: Duration = Duration::from_secs(1);

/// The longest `estimate_metrics` waits for its probe.
const PROBE_TIMEOUT: Duration = Duration::from_secs(3);

/// The most of an error response's body the sender reads, to say why the peer refused.
const ERROR_BODY_LIMIT: usize = 256;

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

/// Every setting the config holds, each checked. Built by [`Limits::from_config`], which both
/// [`HttpTransportImpl::new`] and [`validate_config`] call, so they cannot disagree.
#[derive(Debug, Clone)]
struct Limits {
    server_port: u16,
    bind_scope: crate::network_scope::BindScope,
    timeout: Duration,
    use_https: bool,
    user_agent: String,
    max_message_size: usize,
    max_concurrent_connections: usize,
    max_queued_bytes: usize,
    header_read_timeout: Duration,
    idle_timeout: Duration,
    request_timeout: Duration,
}

impl Limits {
    fn from_config(config: &HashMap<String, String>) -> Result<Self> {
        for (retired, replacement) in RETIRED_KEYS {
            if config.contains_key(retired) {
                return Err(SynapseError::Config(format!(
                    "{retired} is no longer read by the HTTP transport; use {replacement}"
                )));
            }
        }
        let millis = |key, default| -> Result<Duration> {
            Ok(Duration::from_millis(
                positive_limit(config, key, default)? as u64
            ))
        };
        let server_port = match config.get(SERVER_PORT_KEY) {
            None => 0,
            Some(value) => value.trim().parse::<u16>().map_err(|e| {
                SynapseError::Config(format!(
                    "{SERVER_PORT_KEY} must be a port number from 0 to 65535, got {value:?}: {e}"
                ))
            })?,
        };
        let use_https = match config.get(USE_HTTPS_KEY).map(|v| v.trim()) {
            None | Some("false") => false,
            Some("true") => true,
            Some(other) => {
                return Err(SynapseError::Config(format!(
                    "{USE_HTTPS_KEY} must be true or false, got {other:?}"
                )));
            }
        };
        let max_message_size =
            positive_limit(config, MAX_MESSAGE_SIZE_KEY, DEFAULT_MAX_MESSAGE_SIZE)?;
        let max_queued_bytes =
            positive_limit(config, MAX_QUEUED_BYTES_KEY, DEFAULT_MAX_QUEUED_BYTES)?;
        // A message the size check accepts must fit the queue budget, or its handler would wait
        // for room that never comes.
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
            server_port,
            bind_scope: crate::network_scope::BindScope::from_config_map(config)?,
            timeout: millis(TIMEOUT_MS_KEY, DEFAULT_TIMEOUT_MS)?,
            use_https,
            user_agent: config
                .get(USER_AGENT_KEY)
                .cloned()
                .unwrap_or_else(|| DEFAULT_USER_AGENT.to_string()),
            max_message_size,
            max_concurrent_connections: positive_limit(
                config,
                MAX_CONCURRENT_CONNECTIONS_KEY,
                DEFAULT_MAX_CONCURRENT_CONNECTIONS,
            )?,
            max_queued_bytes,
            header_read_timeout: millis(
                HEADER_READ_TIMEOUT_MS_KEY,
                DEFAULT_HEADER_READ_TIMEOUT_MS,
            )?,
            idle_timeout: millis(IDLE_TIMEOUT_MS_KEY, DEFAULT_IDLE_TIMEOUT_MS)?,
            request_timeout: millis(REQUEST_TIMEOUT_MS_KEY, DEFAULT_REQUEST_TIMEOUT_MS)?,
        })
    }

    /// The sender's HTTP client. It follows no redirect, uses no proxy, and speaks HTTP/1.1 only,
    /// so the request goes to exactly the URL [`target_url`] checked, and nowhere else.
    fn client(&self) -> Result<Client> {
        Client::builder()
            .timeout(self.timeout)
            .user_agent(self.user_agent.as_str())
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .http1_only()
            .build()
            .map_err(|e| {
                SynapseError::Config(format!(
                    "the HTTP client could not be built (is {USER_AGENT_KEY} a valid header \
                     value?): {e}"
                ))
            })
    }
}

/// Check `config` exactly as [`HttpTransportImpl::new`] does, without building anything but the
/// client, which checks `user_agent`.
pub fn validate_config(config: &HashMap<String, String>) -> Result<()> {
    Limits::from_config(config)?.client()?;
    Ok(())
}

/// A refusal of `address` as an HTTP target, saying why.
fn bad_target(address: &str, why: &str) -> SynapseError {
    SynapseError::TransportError(format!(
        "HTTP target {address:?} is refused: {why}; give an http:// URL with a port, or host:port"
    ))
}

/// The URL `target` is sent to, checked by the parser that dials it. See the module documentation
/// ("Targets") for what is accepted. `host:port` becomes `http://host:port/synapse/message`, or
/// `https://` when `use_https` is set.
fn target_url(target: &TransportTarget, use_https: bool) -> Result<Url> {
    let address = target.address.as_deref().ok_or_else(|| {
        SynapseError::TransportError(format!(
            "HTTP target {} has no address; give an http:// URL or host:port",
            target.identifier
        ))
    })?;
    let (scheme, rest) = match address.split_once("://") {
        Some((scheme, rest)) if scheme.eq_ignore_ascii_case("http") => ("http", rest),
        Some((scheme, rest)) if scheme.eq_ignore_ascii_case("https") => ("https", rest),
        Some((scheme, _)) => {
            return Err(bad_target(
                address,
                &format!("{scheme}:// is not an HTTP scheme"),
            ));
        }
        None => {
            if address.contains(['/', '?', '#']) {
                return Err(bad_target(
                    address,
                    "host:port takes no path, query or fragment; give a URL for those",
                ));
            }
            (if use_https { "https" } else { "http" }, address)
        }
    };
    if rest.contains('#') {
        return Err(bad_target(
            address,
            "it has a fragment, which would not be sent",
        ));
    }
    // The authority ends at the first `/` or `?`.
    let authority = rest.split(['/', '?']).next().unwrap_or_default();
    if authority.contains('@') {
        return Err(bad_target(
            address,
            "it has userinfo (user@host), which would be sent as credentials",
        ));
    }
    let (host, port) = if let Some(inside) = authority.strip_prefix('[') {
        let Some((ip, after)) = inside.split_once(']') else {
            return Err(bad_target(address, "its IPv6 host has no closing bracket"));
        };
        let Some(port) = after.strip_prefix(':') else {
            return Err(bad_target(address, "it has no port"));
        };
        (&authority[..ip.len() + 2], port)
    } else {
        let Some((host, port)) = authority.rsplit_once(':') else {
            return Err(bad_target(address, "it has no port"));
        };
        (host, port)
    };
    if host.is_empty() || host == "[]" {
        return Err(bad_target(address, "its host is empty"));
    }
    let port = match port.parse::<u16>() {
        Ok(0) => return Err(bad_target(address, "port 0 cannot be dialled")),
        Ok(port) => port,
        Err(_) => {
            return Err(bad_target(
                address,
                &format!("its port {port:?} is not a number from 1 to 65535"),
            ));
        }
    };

    let mut url = Url::parse(&format!("{scheme}://{rest}"))
        .map_err(|e| bad_target(address, &format!("it is not a URL ({e})")))?;
    if url.path() == "/" {
        url.set_path(MESSAGE_PATH);
    }
    // What the parser made of it must be what the address says; otherwise what is dialled is not
    // what was checked.
    if url.scheme() != scheme {
        return Err(bad_target(address, "its scheme did not survive parsing"));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(bad_target(address, "it has userinfo"));
    }
    if url.fragment().is_some() {
        return Err(bad_target(
            address,
            "it has a fragment, which would not be sent",
        ));
    }
    match url.host_str() {
        Some(parsed) if parsed.eq_ignore_ascii_case(host) => {}
        parsed => {
            return Err(bad_target(
                address,
                &format!(
                    "its host {host:?} would be dialled as {parsed:?}; give the host in the form \
                     it is dialled (a plain name, a dotted-quad IPv4 address or a bracketed IPv6 \
                     address)"
                ),
            ));
        }
    }
    if url.port_or_known_default() != Some(port) {
        return Err(bad_target(
            address,
            &format!(
                "it would be dialled on port {:?}, not the port {port} it names",
                url.port_or_known_default()
            ),
        ));
    }
    Ok(url)
}

/// What the server's handlers share with the transport.
struct Inbound {
    received_messages: Arc<Mutex<Vec<Queued>>>,
    queue_budget: Arc<Semaphore>,
    metrics: Arc<RwLock<TransportMetrics>>,
    max_message_size: usize,
    idle_timeout: Duration,
}

/// A received message waiting for the application, holding queue budget for its JSON length until
/// it is drained.
struct Queued {
    message: IncomingMessage,
    _budget: OwnedSemaphorePermit,
}

/// What a handler knows about the connection its request came on, put in each request's
/// extensions by [`serve_connection`].
#[derive(Debug, Clone, Copy)]
struct ConnectionInfo {
    /// The connection must be finished by then: accept plus `request_timeout_ms`.
    deadline: tokio::time::Instant,
    peer: SocketAddr,
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

/// Why a request body was not read whole.
#[derive(Debug)]
enum BodyError {
    /// More bytes than its `Content-Length` (hyper does not deliver them; this is a second check).
    TooLarge,
    /// No byte arrived for `idle_timeout_ms`; the bytes read before.
    Idle(usize),
    /// The body failed to read, for instance because the peer closed before sending all of it.
    Failed(String),
}

/// Read `body`, whose `Content-Length` is `declared`, whole, or fail when no byte arrives for
/// `idle`. The buffer is allocated once, at exactly `declared` bytes, when the first byte arrives,
/// and never grows: a growing buffer's reallocation holds the old and the new allocation at once
/// (1.5 times the body at the last doubling), which would sit outside the module's memory bound.
/// The caller has checked `declared` against `max_message_size`.
async fn read_body(
    mut body: Body,
    declared: usize,
    idle: Duration,
) -> std::result::Result<Vec<u8>, BodyError> {
    let mut buffer: Vec<u8> = Vec::new();
    loop {
        let frame = match tokio::time::timeout(idle, body.frame()).await {
            Err(_) => return Err(BodyError::Idle(buffer.len())),
            Ok(None) => return Ok(buffer),
            Ok(Some(Err(e))) => return Err(BodyError::Failed(e.to_string())),
            Ok(Some(Ok(frame))) => frame,
        };
        // Trailers carry nothing of the message.
        let Ok(data) = frame.into_data() else {
            continue;
        };
        if buffer.len() + data.len() > declared {
            return Err(BodyError::TooLarge);
        }
        if buffer.capacity() == 0 {
            buffer.reserve_exact(declared);
        }
        buffer.extend_from_slice(&data);
    }
}

/// The request's `Content-Length`, if it has one and no `Transfer-Encoding`.
fn declared_length(headers: &axum::http::HeaderMap) -> Option<usize> {
    if headers.contains_key(axum::http::header::TRANSFER_ENCODING) {
        return None;
    }
    headers
        .get(axum::http::header::CONTENT_LENGTH)?
        .to_str()
        .ok()?
        .trim()
        .parse()
        .ok()
}

/// A response with a short plain-text reason.
fn answer(status: StatusCode, reason: &str) -> (StatusCode, String) {
    (status, reason.to_string())
}

/// `POST /synapse/message`: read the body, take queue budget for it, parse it, queue it, and only
/// then answer `202`. See the module documentation for every other answer.
async fn receive_message(
    State(inbound): State<Arc<Inbound>>,
    request: Request,
) -> (StatusCode, String) {
    let Some(connection) = request.extensions().get::<ConnectionInfo>().copied() else {
        // `serve_connection` inserts it into every request; without it there is no deadline.
        error!("HTTP request without connection info; refused");
        return answer(StatusCode::INTERNAL_SERVER_ERROR, "no connection info");
    };
    let source = connection.peer.to_string();
    if inbound.queue_budget.is_closed() {
        return answer(StatusCode::SERVICE_UNAVAILABLE, "the transport is stopping");
    }

    // A body must say its length up front, so it can be refused before any of it is read and
    // buffered in one allocation of exactly that size.
    let Some(declared) = declared_length(request.headers()) else {
        return answer(
            StatusCode::LENGTH_REQUIRED,
            "a Content-Length is required, and Transfer-Encoding is not accepted",
        );
    };
    if declared == 0 {
        return answer(StatusCode::BAD_REQUEST, "the body is empty");
    }
    // `RequestBodyLimitLayer` has already answered 413 for this; checked again so the allocation
    // below is bounded whatever the layer does.
    if declared > inbound.max_message_size {
        return answer(
            StatusCode::PAYLOAD_TOO_LARGE,
            "the body is over max_message_size",
        );
    }

    let data = match read_body(request.into_body(), declared, inbound.idle_timeout).await {
        Ok(data) if data.len() < declared => {
            return answer(StatusCode::BAD_REQUEST, "the body ended early");
        }
        Ok(data) => data,
        Err(BodyError::TooLarge) => {
            warn!(
                "Refused an HTTP message from {}: larger than the {} byte limit",
                source, inbound.max_message_size
            );
            return answer(
                StatusCode::PAYLOAD_TOO_LARGE,
                "the body is over max_message_size",
            );
        }
        Err(BodyError::Idle(read)) => {
            warn!(
                "Cut off an HTTP message from {}: no byte arrived for {:?}, with {} bytes of body \
                 read",
                source, inbound.idle_timeout, read
            );
            return answer(StatusCode::REQUEST_TIMEOUT, "the body went silent");
        }
        Err(BodyError::Failed(e)) => {
            warn!("Dropped an HTTP message from {}: {}", source, e);
            return answer(StatusCode::BAD_REQUEST, "the body could not be read");
        }
    };
    let bytes_read = data.len();

    // Wait for queue budget before parsing, so a waiting handler holds only its raw bytes, not the
    // several times larger parsed message. `Limits` guarantees
    // `bytes_read <= max_message_size <= max_queued_bytes <= u32::MAX`, so the conversion cannot
    // fail and the acquire never asks for more than the budget holds. The wait ends at the
    // connection's deadline, or when `stop` closes the budget.
    let Ok(wanted) = u32::try_from(bytes_read) else {
        return answer(
            StatusCode::PAYLOAD_TOO_LARGE,
            "the body is over the queue budget",
        );
    };
    let budget = match tokio::time::timeout_at(
        connection.deadline,
        Arc::clone(&inbound.queue_budget).acquire_many_owned(wanted),
    )
    .await
    {
        Ok(Ok(budget)) => budget,
        Ok(Err(_closed)) => {
            warn!(
                "Refused an HTTP message from {}: the transport is stopping",
                source
            );
            return answer(StatusCode::SERVICE_UNAVAILABLE, "the transport is stopping");
        }
        Err(_) => {
            warn!(
                "Refused an HTTP message from {}: no queue budget came free before its deadline",
                source
            );
            return answer(StatusCode::SERVICE_UNAVAILABLE, "the receive queue is full");
        }
    };

    let message = match serde_json::from_slice::<SecureMessage>(&data) {
        Ok(message) => message,
        Err(e) => {
            warn!(
                "Dropped an HTTP message from {}: {} bytes did not parse as a SecureMessage: {}",
                source, bytes_read, e
            );
            return answer(StatusCode::BAD_REQUEST, "the body is not a SecureMessage");
        }
    };
    drop(data);

    let incoming = IncomingMessage::new(message, TransportType::Http, source);
    {
        let mut messages = inbound.received_messages.lock().await;
        messages.push(Queued {
            message: incoming,
            _budget: budget,
        });
        debug!("Queued HTTP message, total: {}", messages.len());
    }
    {
        // `unwrap_or_else` recovers from a poisoned lock rather than panicking this connection's
        // handler over it: see `tcp_unified.rs`'s `handle_connection` for the full reasoning
        // (a poisoned metrics lock is not a reason to drop a counter update, still less inbound
        // traffic). Unified here with every other transport's metrics lock onto one policy.
        let mut metrics = inbound.metrics.write().unwrap_or_else(|e| e.into_inner());
        metrics.messages_received += 1;
        metrics.bytes_received += bytes_read as u64;
        metrics.touch();
    }
    // Only now, with the message in the queue, may the sender be told it was delivered.
    answer(StatusCode::ACCEPTED, "queued")
}

/// Serve one connection: one request, then close. The whole connection is bounded by its deadline
/// (plus [`RESPONSE_GRACE`] to write a `503`), and its head by hyper's `header_read_timeout`.
async fn serve_connection(stream: TcpStream, peer: SocketAddr, router: Router, limits: &Limits) {
    let deadline = tokio::time::Instant::now() + limits.request_timeout;
    let info = ConnectionInfo { deadline, peer };
    let service = hyper::service::service_fn(move |mut request: hyper::Request<_>| {
        request.extensions_mut().insert(info);
        router.clone().oneshot(request.map(Body::new))
    });
    let mut builder = hyper::server::conn::http1::Builder::new();
    builder
        .timer(hyper_util::rt::TokioTimer::new())
        .header_read_timeout(limits.header_read_timeout)
        .keep_alive(false)
        .max_buf_size(READ_BUFFER_LIMIT);
    let connection = builder.serve_connection(hyper_util::rt::TokioIo::new(stream), service);
    match tokio::time::timeout_at(deadline + RESPONSE_GRACE, connection).await {
        Ok(Ok(())) => debug!("HTTP connection from {} finished", peer),
        Ok(Err(e)) => debug!("HTTP connection from {} ended: {}", peer, e),
        Err(_) => warn!(
            "Closed HTTP connection from {}: it was still open {:?} after it was accepted",
            peer, limits.request_timeout
        ),
    }
}

/// HTTP transport: an `axum` server that receives, and a `reqwest` client that sends.
pub struct HttpTransportImpl {
    /// The checked config.
    limits: Limits,
    /// The sender's client, built once.
    client: Client,
    /// The listener `start` bound; the accept loop serves this same listener.
    listener: std::sync::Mutex<Option<Arc<TcpListener>>>,
    /// Whether `start` has run: a second `start` is an error.
    started: std::sync::atomic::AtomicBool,
    /// Tells the accept loop to stop; `stop` sends `true`.
    shutdown: watch::Sender<bool>,
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
    /// Total time, in nanoseconds, the successful sends took, from which `estimate_metrics`
    /// derives a bandwidth.
    send_nanos: AtomicU64,
    /// The transport's own circuit breaker over sends that reached the network.
    circuit_breaker: Arc<CircuitBreaker>,
    status: Arc<RwLock<TransportStatus>>,
    /// Counters of what this transport actually did.
    metrics: Arc<RwLock<TransportMetrics>>,
}

impl HttpTransportImpl {
    /// Create an HTTP transport. Nothing is bound until `start`. See the module documentation for
    /// the config keys; fails if any of them is present but invalid.
    pub async fn new(config: &HashMap<String, String>) -> Result<Self> {
        let limits = Limits::from_config(config)?;
        let client = limits.client()?;
        let metrics = TransportMetrics {
            transport_type: TransportType::Http,
            ..Default::default()
        };
        Ok(Self {
            client,
            listener: std::sync::Mutex::new(None),
            started: std::sync::atomic::AtomicBool::new(false),
            shutdown: watch::channel(false).0,
            connection_permits: Arc::new(Semaphore::new(limits.max_concurrent_connections)),
            queue_budget: Arc::new(Semaphore::new(limits.max_queued_bytes)),
            received_messages: Arc::new(Mutex::new(Vec::new())),
            active_connections: Arc::new(AtomicU32::new(0)),
            send_nanos: AtomicU64::new(0),
            circuit_breaker: Arc::new(CircuitBreaker::new(CircuitBreakerConfig {
                failure_threshold: 5,
                recovery_timeout: Duration::from_secs(60),
                ..Default::default()
            })),
            status: Arc::new(RwLock::new(TransportStatus::Stopped)),
            metrics: Arc::new(RwLock::new(metrics)),
            limits,
        })
    }

    /// The address the server is bound to, once `start` has bound it.
    pub fn local_addr(&self) -> Option<SocketAddr> {
        self.listener
            .lock()
            .unwrap()
            .as_ref()
            .and_then(|listener| listener.local_addr().ok())
    }

    /// The router: one route, behind a body limit that refuses a declared `Content-Length` over
    /// `max_message_size` with `413` before reading any of the body.
    fn router(&self) -> Router {
        let inbound = Arc::new(Inbound {
            received_messages: Arc::clone(&self.received_messages),
            queue_budget: Arc::clone(&self.queue_budget),
            metrics: Arc::clone(&self.metrics),
            max_message_size: self.limits.max_message_size,
            idle_timeout: self.limits.idle_timeout,
        });
        Router::new()
            .route(MESSAGE_PATH, post(receive_message))
            .layer(tower_http::limit::RequestBodyLimitLayer::new(
                self.limits.max_message_size,
            ))
            .with_state(inbound)
    }

    /// Serve `listener`: take a connection permit, accept, and serve the connection on its own
    /// task, until `stop`.
    fn spawn_accept_loop(&self, listener: Arc<TcpListener>, local_addr: SocketAddr) {
        let permits = Arc::clone(&self.connection_permits);
        let router = self.router();
        let limits = Arc::new(self.limits.clone());
        let active = Arc::clone(&self.active_connections);
        let mut shutdown = self.shutdown.subscribe();
        tokio::spawn(async move {
            info!("HTTP server listening on {}", local_addr);
            loop {
                // Backpressure: take a permit BEFORE accepting, so at the cap the loop waits for a
                // handler to finish and the waiting connections stay in the kernel's backlog.
                let permit = tokio::select! {
                    _ = shutdown.wait_for(|stop| *stop) => break,
                    permit = Arc::clone(&permits).acquire_owned() => match permit {
                        Ok(permit) => permit,
                        Err(_) => break,
                    },
                };
                let accepted = tokio::select! {
                    _ = shutdown.wait_for(|stop| *stop) => break,
                    accepted = listener.accept() => accepted,
                };
                match accepted {
                    Ok((stream, peer)) => {
                        debug!("Accepted HTTP connection from {}", peer);
                        let router = router.clone();
                        let limits = Arc::clone(&limits);
                        let active = ActiveConnection::new(&active);
                        tokio::spawn(async move {
                            serve_connection(stream, peer, router, &limits).await;
                            // Held for the whole connection, including any wait for queue budget.
                            drop(active);
                            drop(permit);
                        });
                    }
                    Err(e) => {
                        error!("Failed to accept HTTP connection: {}", e);
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                }
            }
            info!("HTTP server on {} stopped accepting", local_addr);
        });
    }

    /// POST `json` to `url`. Returns the status and how long the exchange took; the response body
    /// is read only for a non-2xx status, and only its first [`ERROR_BODY_LIMIT`] bytes.
    async fn post(&self, url: &Url, json: Vec<u8>) -> Result<(reqwest::StatusCode, Duration)> {
        let start = Instant::now();
        let mut response = self
            .client
            .post(url.clone())
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(json)
            .send()
            .await
            .map_err(|e| SynapseError::TransportError(format!("HTTP POST to {url} failed: {e}")))?;
        let status = response.status();
        let took = start.elapsed();
        if status.is_success() {
            return Ok((status, took));
        }
        let mut reason = Vec::new();
        while reason.len() < ERROR_BODY_LIMIT {
            match response.chunk().await {
                Ok(Some(chunk)) => reason.extend_from_slice(&chunk),
                _ => break,
            }
        }
        reason.truncate(ERROR_BODY_LIMIT);
        Err(SynapseError::TransportError(format!(
            "HTTP POST to {url} was refused with {status}: {}",
            String::from_utf8_lossy(&reason).trim()
        )))
    }

    /// Send `message` to `url`, recording the outcome.
    async fn send_to(&self, url: &Url, message: &SecureMessage) -> Result<DeliveryReceipt> {
        // Serialize first, and refuse what a receiver with this limit would refuse: sending it
        // would only earn a 413.
        let json = serde_json::to_vec(message).map_err(|e| {
            SynapseError::SerializationError(format!("Failed to serialize message: {e}"))
        })?;
        let size = json.len();
        if size > self.limits.max_message_size {
            // A refusal of this message, not a transport failure: see `SynapseError::MessageRefused`.
            return Err(SynapseError::MessageRefused(format!(
                "HTTP message {} serializes to {} bytes, over this transport's {} of {} bytes \
                 (serialized JSON); a receiver with that limit would refuse it",
                message.message_id.0, size, MAX_MESSAGE_SIZE_KEY, self.limits.max_message_size
            )));
        }
        if !self.circuit_breaker.can_proceed().await {
            return Err(SynapseError::TransportError(
                "HTTP circuit breaker is open".to_string(),
            ));
        }

        let outcome = self.post(url, json).await;
        self.circuit_breaker
            .record_outcome(match &outcome {
                Ok(_) => RequestOutcome::Success,
                Err(e) => RequestOutcome::Failure(e.to_string()),
            })
            .await;
        let (status, took) = match outcome {
            Ok(done) => done,
            Err(e) => {
                warn!("{}", e);
                // See `receive_message`'s comment on this same lock for why poison recovers
                // rather than panics.
                let mut metrics = self
                    .metrics
                    .write()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                metrics.send_failures += 1;
                metrics.touch();
                return Err(e);
            }
        };
        {
            let mut metrics = self.metrics.write().unwrap_or_else(|e| e.into_inner());
            metrics.messages_sent += 1;
            metrics.bytes_sent += size as u64;
            let sent = metrics.messages_sent;
            let average = (metrics.average_latency_ms as f64 * (sent - 1) as f64
                + took.as_millis() as f64)
                / sent as f64;
            metrics.average_latency_ms = average as u64;
            metrics.touch();
        }
        self.send_nanos.fetch_add(
            u64::try_from(took.as_nanos()).unwrap_or(u64::MAX),
            Ordering::Relaxed,
        );
        info!(
            "HTTP message delivered to {} in {:?} ({})",
            url, took, status
        );
        Ok(DeliveryReceipt {
            message_id: message.message_id.0.to_string(),
            target_reached: url.to_string(),
            transport_used: TransportType::Http,
            // protocol event: the peer's server answered 2xx, which this transport's server sends
            // only after the message is parsed and in its receive queue (module documentation).
            confirmation: DeliveryConfirmation::Delivered,
            delivery_time: took,
            metadata: HashMap::from([
                ("status_code".to_string(), status.as_u16().to_string()),
                ("bytes".to_string(), size.to_string()),
            ]),
        })
    }

    /// A `HEAD` request to `url`, within `wait`. Any HTTP response, whatever its status, shows an
    /// HTTP server answered there (this transport's server answers `405`, since its one route is
    /// `POST`); the time to the response is a measured round trip.
    async fn probe(&self, url: &Url, wait: Duration) -> Result<(Duration, reqwest::StatusCode)> {
        let start = Instant::now();
        match self.client.head(url.clone()).timeout(wait).send().await {
            Ok(response) => Ok((start.elapsed(), response.status())),
            Err(e) => Err(SynapseError::TransportError(format!(
                "HTTP probe of {url} failed: {e}"
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
impl Transport for HttpTransportImpl {
    fn transport_type(&self) -> TransportType {
        TransportType::Http
    }

    /// What this transport does, with `max_message_size` set to this instance's configured limit,
    /// in bytes of serialized JSON.
    fn capabilities(&self) -> TransportCapabilities {
        TransportCapabilities {
            max_message_size: self.limits.max_message_size,
            reliable: true,
            real_time: false,
            broadcast: false,
            // It receives only with a server.
            bidirectional: self.limits.server_port > 0,
            // The server speaks plain HTTP; only an `https://` target is encrypted.
            encrypted: false,
            network_spanning: true,
            supported_urgencies: vec![
                MessageUrgency::Interactive,
                MessageUrgency::Background,
                MessageUrgency::Batch,
            ],
            features: vec![
                "request_response".to_string(),
                "one_request_per_connection".to_string(),
                "firewall_friendly".to_string(),
            ],
            unmeasured_metrics: vec![],
        }
    }

    /// Whether `target` has an address this transport can dial. It touches no network;
    /// `test_connectivity` does.
    async fn can_reach(&self, target: &TransportTarget) -> bool {
        target_url(target, self.limits.use_https).is_ok()
    }

    /// A real HTTP exchange with `target` (see `probe`), bounded at 3 s. When it completes,
    /// `latency` is its measured round trip and `confidence` is 1. When it fails, the target is
    /// reported unavailable, with `latency` equal to the request timeout rather than the time the
    /// failure took, which would read as a fast link, and a low `confidence` of 0.3.
    /// `reliability` is the share of this transport's sends that succeeded, counting this probe as
    /// one attempt; `bandwidth` is bytes sent per second spent sending, or 1 until a send has been
    /// measured -- no figure is invented (TCP still reports an invented 1,000,000, a known
    /// follow-up). `cost` is the relative unit TCP reports.
    async fn estimate_metrics(&self, target: &TransportTarget) -> Result<TransportEstimate> {
        let url = target_url(target, self.limits.use_https)?;
        let wait = self.limits.timeout.min(PROBE_TIMEOUT);
        let probe = self.probe(&url, wait).await;
        let (sent, failures, bandwidth) = self.observed();
        let available = probe.is_ok();
        let successes = sent + u64::from(available);
        let (latency, confidence) = match probe {
            Ok((rtt, _)) => (rtt, 1.0),
            Err(_) => (self.limits.timeout, 0.3),
        };
        Ok(TransportEstimate {
            latency,
            reliability: successes as f64 / (sent + failures + 1) as f64,
            bandwidth: bandwidth.unwrap_or(1),
            cost: 1.0,
            available,
            confidence,
        })
    }

    async fn send_message(
        &self,
        target: &TransportTarget,
        message: &SecureMessage,
    ) -> Result<DeliveryReceipt> {
        let url = target_url(target, self.limits.use_https)?;
        self.send_to(&url, message).await
    }

    /// Connected only after a real HTTP exchange with the target, whose round trip is `rtt`.
    async fn test_connectivity(&self, target: &TransportTarget) -> Result<ConnectivityResult> {
        let url = target_url(target, self.limits.use_https)?;
        let mut details = HashMap::from([("url".to_string(), url.to_string())]);
        match self.probe(&url, self.limits.timeout).await {
            Ok((rtt, status)) => {
                details.insert("status_code".to_string(), status.as_u16().to_string());
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

    /// Bind the server once, keep the listener, and serve that same listener. With `server_port`
    /// 0 there is no server, and the transport only sends. Fails, with the status `Failed`, if the
    /// bind fails; a second `start` is an error.
    async fn start(&self) -> Result<()> {
        info!("Starting HTTP transport");
        if self.started.swap(true, Ordering::SeqCst) {
            return Err(SynapseError::TransportError(
                "HTTP transport is already started".to_string(),
            ));
        }
        *self.status.write().unwrap() = TransportStatus::Starting;

        if self.limits.server_port == 0 {
            debug!("HTTP transport has no server ({SERVER_PORT_KEY} is 0); it only sends");
        } else {
            let addr = self.limits.bind_scope.listen_addr(self.limits.server_port);
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
                        "HTTP transport could not listen on {addr}: {e}"
                    )));
                }
            };
            *self.listener.lock().unwrap() = Some(Arc::clone(&listener));
            self.spawn_accept_loop(listener, local_addr);
        }

        *self.status.write().unwrap() = TransportStatus::Running;
        info!("HTTP transport started");
        Ok(())
    }

    /// Stop accepting, release the listener, and close the queue budget, so every handler waiting
    /// for budget answers `503` and returns instead of waiting for a poll that will never come. A
    /// connection already accepted finishes its one request (a message it sends is refused with
    /// `503`). A stopped transport is not restarted: the manager builds a new one.
    async fn stop(&self) -> Result<()> {
        info!("Stopping HTTP transport");
        *self.status.write().unwrap() = TransportStatus::Stopping;
        self.queue_budget.close();
        self.shutdown.send_replace(true);
        self.listener.lock().unwrap().take();
        *self.status.write().unwrap() = TransportStatus::Stopped;
        info!("HTTP transport stopped");
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
impl TransportReceive for HttpTransportImpl {
    async fn receive_raw(&self, inbox: &mut RawInbox) -> Result<()> {
        // Draining a message drops the queue budget it held, so a handler waiting for budget can
        // queue its message as soon as this lock is released. Each message is handed out once.
        let mut messages = self.received_messages.lock().await;
        inbox.extend(messages.drain(..).map(|queued| queued.message));
        Ok(())
    }
}

/// Factory for the HTTP transport. The config keys, their defaults, and what the limits bound are
/// in this module's documentation; `create_transport` and `validate_config` refuse the same invalid
/// values.
pub struct HttpTransportFactory;

#[async_trait]
impl TransportFactory for HttpTransportFactory {
    async fn create_transport(
        &self,
        config: &HashMap<String, String>,
    ) -> Result<Box<dyn Transport>> {
        Ok(Box::new(HttpTransportImpl::new(config).await?))
    }

    fn transport_type(&self) -> TransportType {
        TransportType::Http
    }

    fn default_config(&self) -> HashMap<String, String> {
        let mut config: HashMap<String, String> = [
            (SERVER_PORT_KEY, 0),
            (TIMEOUT_MS_KEY, DEFAULT_TIMEOUT_MS),
            // In bytes of serialized JSON, the unit sender and receiver both enforce.
            (MAX_MESSAGE_SIZE_KEY, DEFAULT_MAX_MESSAGE_SIZE),
            (
                MAX_CONCURRENT_CONNECTIONS_KEY,
                DEFAULT_MAX_CONCURRENT_CONNECTIONS,
            ),
            (MAX_QUEUED_BYTES_KEY, DEFAULT_MAX_QUEUED_BYTES),
            (HEADER_READ_TIMEOUT_MS_KEY, DEFAULT_HEADER_READ_TIMEOUT_MS),
            (IDLE_TIMEOUT_MS_KEY, DEFAULT_IDLE_TIMEOUT_MS),
            (REQUEST_TIMEOUT_MS_KEY, DEFAULT_REQUEST_TIMEOUT_MS),
        ]
        .into_iter()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect();
        config.insert(USE_HTTPS_KEY.to_string(), "false".to_string());
        config.insert(USER_AGENT_KEY.to_string(), DEFAULT_USER_AGENT.to_string());
        config
    }

    fn validate_config(&self, config: &HashMap<String, String>) -> Result<()> {
        // The same check `new` applies, so validating and constructing cannot disagree.
        validate_config(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::SecurityLevel;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn loopback_server(extra: &[(&str, &str)]) -> HashMap<String, String> {
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let mut config = HashMap::from([
            (SERVER_PORT_KEY.to_string(), port.to_string()),
            (
                crate::network_scope::BIND_SCOPE_KEY.to_string(),
                crate::network_scope::BindScope::Loopback
                    .config_value()
                    .to_string(),
            ),
        ]);
        for (key, value) in extra {
            config.insert(key.to_string(), value.to_string());
        }
        config
    }

    fn target_at(addr: SocketAddr) -> TransportTarget {
        TransportTarget::new("bob".to_string()).with_address(addr.to_string())
    }

    fn message(body: &str) -> SecureMessage {
        SecureMessage::new(
            "bob",
            "alice",
            body.as_bytes().to_vec(),
            SecurityLevel::Public,
        )
    }

    async fn drain(transport: &HttpTransportImpl) -> Vec<IncomingMessage> {
        let mut inbox = RawInbox::new();
        transport.receive_raw(&mut inbox).await.unwrap();
        inbox.drain()
    }

    /// `receive_raw` drains: each message is handed out once, however often it is polled. Checked
    /// here, on the transport, because through the manager the replay record drops a signed
    /// message seen before, which would hide a transport's repeats.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn receive_raw_hands_each_message_out_once() {
        const K: usize = 3;
        let bob = HttpTransportImpl::new(&loopback_server(&[])).await.unwrap();
        bob.start().await.unwrap();
        let alice = HttpTransportImpl::new(&HashMap::new()).await.unwrap();
        let target = target_at(bob.local_addr().unwrap());
        let mut sent = std::collections::HashSet::new();
        for i in 0..K {
            let message = message(&format!("once {i}"));
            sent.insert(message.message_id.0.to_string());
            let receipt = alice.send_message(&target, &message).await.unwrap();
            assert!(matches!(
                receipt.confirmation,
                DeliveryConfirmation::Delivered
            ));
        }
        // Every send has returned Delivered, so every message is already queued: one drain finds
        // all of them, and no later drain finds any again.
        let mut arrived = drain(&bob).await;
        assert_eq!(arrived.len(), K, "Delivered means already queued");
        for _ in 0..5 {
            arrived.extend(drain(&bob).await);
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

    /// Every non-2xx answer is an error, never `Sent` or `Delivered`: the bytes were written, but
    /// the peer refused them. A server that answers every request with `status` stands in for the
    /// peer.
    #[tokio::test]
    async fn a_non_2xx_answer_is_an_error() {
        for status in ["500 Internal Server Error", "404 Not Found", "302 Found"] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let response = format!(
                "HTTP/1.1 {status}\r\nlocation: http://127.0.0.1:9/\r\ncontent-length: 4\r\n\
                 connection: close\r\n\r\nnope"
            );
            let server = tokio::spawn(async move {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = vec![0u8; 64 * 1024];
                let _ = stream.read(&mut request).await;
                stream.write_all(response.as_bytes()).await.unwrap();
                let _ = stream.shutdown().await;
            });
            let alice = HttpTransportImpl::new(&HashMap::new()).await.unwrap();
            match alice.send_message(&target_at(addr), &message("x")).await {
                Err(SynapseError::TransportError(why)) => {
                    assert!(why.contains(&status[..3]), "{status}: {why}")
                }
                other => panic!("{status} must be an error, not {other:?}"),
            }
            server.await.unwrap();
            let metrics = alice.metrics().await;
            assert_eq!((metrics.messages_sent, metrics.send_failures), (0, 1));
        }
    }

    /// `https://` is accepted because TLS is compiled in; this shows it is: an `https://` send
    /// opens with a TLS handshake record (content type 22, 0x16). Were `reqwest` built without TLS,
    /// it would refuse the scheme without connecting, and nothing would arrive.
    #[tokio::test]
    async fn https_targets_are_dialled_with_tls() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut first = [0u8; 1];
            stream.read_exact(&mut first).await.unwrap();
            first[0]
        });
        let alice = HttpTransportImpl::new(&HashMap::from([(
            TIMEOUT_MS_KEY.to_string(),
            "2000".to_string(),
        )]))
        .await
        .unwrap();
        let target =
            TransportTarget::new("tls".to_string()).with_address(format!("https://{addr}"));
        // The listener is not a TLS server, so the send fails; what matters is how it began.
        assert!(alice.send_message(&target, &message("x")).await.is_err());
        let first = tokio::time::timeout(Duration::from_secs(5), server)
            .await
            .expect("the client connected")
            .unwrap();
        assert_eq!(first, 0x16, "the first byte must open a TLS handshake");
    }

    /// `stop` closes the queue budget, so a handler waiting for it answers `503` at once, and the
    /// sender gets an error, not a delivery. The budget holds one message; the first send fills it,
    /// the second waits (it has not returned after a pause: the pre-state), and `stop` releases it
    /// with `503`.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn stop_answers_a_waiting_request_with_503() {
        let first = message("fills the budget");
        let size = serde_json::to_vec(&first).unwrap().len();
        let bob = Arc::new(
            HttpTransportImpl::new(&loopback_server(&[
                (MAX_MESSAGE_SIZE_KEY, &(size + 64).to_string()),
                (MAX_QUEUED_BYTES_KEY, &(size + 64).to_string()),
            ]))
            .await
            .unwrap(),
        );
        bob.start().await.unwrap();
        let alice = Arc::new(HttpTransportImpl::new(&HashMap::new()).await.unwrap());
        let target = target_at(bob.local_addr().unwrap());
        alice.send_message(&target, &first).await.unwrap();

        let waiting = {
            let (alice, target) = (Arc::clone(&alice), target.clone());
            tokio::spawn(async move { alice.send_message(&target, &message("waits")).await })
        };
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert!(
            !waiting.is_finished(),
            "with the budget full and nobody polling, the second send must wait"
        );
        let stopped = Instant::now();
        bob.stop().await.unwrap();
        let outcome = tokio::time::timeout(Duration::from_secs(3), waiting)
            .await
            .expect("stop must release the waiting request")
            .unwrap();
        match outcome {
            Err(SynapseError::TransportError(why)) => assert!(why.contains("503"), "{why}"),
            other => panic!("a request released by stop must be a 503 error, not {other:?}"),
        }
        assert!(stopped.elapsed() < Duration::from_secs(2));
        assert_eq!(drain(&bob).await.len(), 1, "only the first message queued");
        // And the listener is gone: nothing accepts on the port any more.
        assert!(bob.local_addr().is_none());
    }

    /// A failed probe says so: unavailable, with the request timeout as its latency and a low
    /// confidence, and no bandwidth claimed before any send has been measured.
    #[tokio::test]
    async fn a_failed_probe_reports_no_link_not_a_fast_one() {
        let nobody = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap();
        let transport = HttpTransportImpl::new(&HashMap::new()).await.unwrap();
        let estimate = transport
            .estimate_metrics(&target_at(nobody))
            .await
            .unwrap();
        assert!(!estimate.available);
        assert_eq!(
            estimate.latency,
            Duration::from_millis(DEFAULT_TIMEOUT_MS as u64)
        );
        assert!(estimate.confidence < 0.5, "{}", estimate.confidence);
        assert_eq!(estimate.bandwidth, 1);
        let connectivity = transport
            .test_connectivity(&target_at(nobody))
            .await
            .unwrap();
        assert!(!connectivity.connected);
        assert_eq!(connectivity.rtt, None);
        assert_eq!(transport.metrics().await.reliability_score, 0.0);
    }

    /// What a target is accepted as, and what is refused. Whatever is accepted is the URL the
    /// client is handed, and its host and port are the ones the address names.
    #[test]
    fn target_addresses() {
        let target =
            |address: &str| TransportTarget::new("t".to_string()).with_address(address.to_string());
        let url = |address: &str| target_url(&target(address), false).map(|u| u.to_string());
        for (address, expected) in [
            ("127.0.0.1:9000", "http://127.0.0.1:9000/synapse/message"),
            ("[::1]:9000", "http://[::1]:9000/synapse/message"),
            ("a-b.test:81", "http://a-b.test:81/synapse/message"),
            ("http://h.test:81", "http://h.test:81/synapse/message"),
            ("http://h.test:81/", "http://h.test:81/synapse/message"),
            ("http://h.test:81/in/x?q=1", "http://h.test:81/in/x?q=1"),
            ("HTTP://h.test:81", "http://h.test:81/synapse/message"),
            // A port equal to the scheme's default is still a port the address names.
            ("http://h.test:80", "http://h.test/synapse/message"),
            ("https://h.test:443/x", "https://h.test/x"),
            ("https://h.test:8443", "https://h.test:8443/synapse/message"),
            ("http://[::1]:9000/", "http://[::1]:9000/synapse/message"),
            ("http://node.1a:80", "http://node.1a/synapse/message"),
        ] {
            assert_eq!(url(address).unwrap(), expected, "{address}");
        }
        // `use_https` makes host:port an https URL; a URL keeps its own scheme.
        assert_eq!(
            target_url(&target("h.test:8443"), true)
                .unwrap()
                .to_string(),
            "https://h.test:8443/synapse/message"
        );
        assert_eq!(
            target_url(&target("http://h.test:8080"), true)
                .unwrap()
                .scheme(),
            "http"
        );
        for bad in [
            // Not HTTP schemes.
            "ws://h.test:80",
            "ftp://h.test:21",
            "file://h.test:1/x",
            // A missing, empty or invalid port, or an empty host.
            "h.test",
            "http://h.test",
            "https://h.test/x",
            "http://h.test:/",
            ":80",
            "http://:80/",
            "[]:80",
            "http://[::1]/",
            "h.test:0",
            "h.test:99999",
            "h.test:port",
            // Userinfo and fragments.
            "http://user@h.test:80/",
            "http://user:pw@h.test:80/",
            "http://h.test:80/#frag",
            "http://h.test:80#f",
            // A path on host:port, which must be a URL.
            "h.test:80/x",
            "a/b:80",
            // Hosts the URL parser would rewrite, so what is dialled is not what is named.
            "http://0x7f.1:80",
            "http://127.1:80",
            "http://%61:80",
            "http://bücher.test:80",
            "http://h\t.test:80",
            "::1:9000",
        ] {
            assert!(
                url(bad).is_err(),
                "{bad} must be refused, got {:?}",
                url(bad)
            );
        }
        assert!(
            target_url(&TransportTarget::new("no-address".to_string()), false).is_err(),
            "a target without an address is refused"
        );
    }

    /// `validate_config` applies the same rule as `new`.
    #[tokio::test]
    async fn validate_config_refuses_what_new_refuses() {
        async fn refused(config: HashMap<String, String>) {
            assert!(
                HttpTransportFactory.validate_config(&config).is_err(),
                "{config:?}"
            );
            assert!(HttpTransportImpl::new(&config).await.is_err(), "{config:?}");
        }
        let factory = HttpTransportFactory;
        for key in [
            TIMEOUT_MS_KEY,
            MAX_MESSAGE_SIZE_KEY,
            MAX_CONCURRENT_CONNECTIONS_KEY,
            MAX_QUEUED_BYTES_KEY,
            HEADER_READ_TIMEOUT_MS_KEY,
            IDLE_TIMEOUT_MS_KEY,
            REQUEST_TIMEOUT_MS_KEY,
        ] {
            for bad in ["0", "abc", ""] {
                refused(HashMap::from([(key.to_string(), bad.to_string())])).await;
            }
            let good = if key == MAX_QUEUED_BYTES_KEY {
                DEFAULT_MAX_MESSAGE_SIZE.to_string()
            } else {
                "7".to_string()
            };
            let config = HashMap::from([(key.to_string(), good)]);
            assert!(factory.validate_config(&config).is_ok(), "{key}");
            assert!(HttpTransportImpl::new(&config).await.is_ok(), "{key}");
        }
        for (key, bad) in [
            (SERVER_PORT_KEY, "70000"),
            (SERVER_PORT_KEY, "-1"),
            (USE_HTTPS_KEY, "yes"),
            (USER_AGENT_KEY, "bad\nagent"),
            ("server_address", "127.0.0.1"),
            ("max_connections", "10"),
            ("bind_scope", "everywhere"),
        ] {
            refused(HashMap::from([(key.to_string(), bad.to_string())])).await;
        }
        assert!(factory.validate_config(&factory.default_config()).is_ok());
    }
}
