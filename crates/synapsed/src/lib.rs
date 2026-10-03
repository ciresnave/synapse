// SPDX-License-Identifier: MIT OR Apache-2.0
//! `synapsed`: one daemon per user per machine, on loopback, serving the durable role mailbox
//! (M5a of the claude-peers replacement plan). Design:
//! `docs/superpowers/specs/2026-10-02-m5-synapsed-design.md`.
//!
//! **Security (PM conditions, 2026-10-02):**
//! - A session token is issued only by `/v1/claim`, and only after M3's claim verifies: an Ed25519
//!   signature by the role key over a domain-separated text with a fresh nonce, inside the
//!   freshness window, not replayed, on a certificate the account key signed.
//! - Every request's `Host` must be `127.0.0.1:<port>` or `localhost:<port>` (421 otherwise). No
//!   CORS headers are ever sent. POST bodies must be `application/json` (415), which forces a
//!   browser preflight that this daemon never answers. Every route except `/v1/health` and
//!   `/v1/claim` needs `Authorization: Bearer <token>` (401).
//! - `/v1/health?challenge=<hex>` proves this daemon holds the instance id from the owner-only
//!   announce file (#74): clients send no claim or token to a listener that cannot prove it, so a
//!   process that squats a stale announced port learns nothing.
//! - Tokens are kept only as SHA-256 hashes and looked up by hash: the secret is never compared
//!   byte-wise, stored, or logged.
//! - A message's sender is always the session's verified role; a client-supplied `from` is refused.

use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Bytes;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode, Uri, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use synapse::certificate::{RevocationLookup, chain_from_pem};
use synapse::keystore::{Keystore, KeystoreError};
use synapse::mailbox::{
    Acked, Enqueued, Envelope, MailConfig, MailError, Mailbox, RedbStore, StoreError,
};
use synapse::roles::ClaimRequest;

/// Where and how the daemon runs.
#[derive(Debug, Clone)]
pub struct DaemonConfig {
    /// The Synapse home (M2): holds the keystore and `mailbox.redb`.
    pub home: PathBuf,
    /// Must be a loopback address.
    pub addr: SocketAddr,
    /// How often acked history is swept (an M4 obligation).
    pub sweep_every: std::time::Duration,
}

#[derive(Debug)]
pub enum DaemonError {
    /// The configured address is not loopback.
    NotLoopback,
    /// The system random source failed.
    NoRandomness,
    Keystore(KeystoreError),
    Store(StoreError),
    Mail(MailError),
}

impl fmt::Display for DaemonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DaemonError::NotLoopback => write!(f, "synapsed listens on loopback addresses only"),
            DaemonError::NoRandomness => write!(f, "the system random source failed"),
            DaemonError::Keystore(e) => write!(f, "keystore: {e}"),
            DaemonError::Store(e) => write!(f, "{e}"),
            DaemonError::Mail(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for DaemonError {}

/// How recently a role must have been heard from to count as online.
const ONLINE_WINDOW: chrono::Duration = chrono::Duration::seconds(90);
const MAX_FETCH: usize = 100;
/// The longest client-chosen message id; the stored id is `<sender>/<client id>` (review I1).
const MAX_CLIENT_ID: usize = 100;
const MAX_SUMMARY: usize = 500;
/// How many superseded sessions per role still answer `superseded` instead of `unauthorized`.
const KEEP_SUPERSEDED: u64 = 8;

#[derive(Clone)]
struct Session {
    global_id: String,
    epoch: u64,
}

struct Presence {
    last_seen: DateTime<Utc>,
    summary: Option<String>,
}

struct Shared {
    /// Never held across an `.await`.
    mailbox: Mutex<Mailbox<RedbStore>>,
    /// SHA-256(token) -> session.
    sessions: Mutex<HashMap<[u8; 32], Session>>,
    presence: Mutex<HashMap<String, Presence>>,
    /// This daemon's account name: it serves `<role>@<account>` only.
    account: String,
    account_key_id: String,
    account_key: [u8; 32],
    /// Random per daemon start, hex. Claims must be signed for it (review I3); it is published only
    /// in the owner-only announce file, never over HTTP. Its raw bytes key the health proof (#74).
    instance_id: String,
    proof_key: ring::hmac::Key,
    home: PathBuf,
    started_at: DateTime<Utc>,
    sweep_every: std::time::Duration,
}

/// The owner-only file in the home where a daemon announces its address and instance id. Only the
/// same user can read it, so only that user's clients learn which daemon is real (review I3).
pub const ANNOUNCE_FILE: &str = "synapsed.json";

/// An opened daemon, ready to serve.
pub struct Daemon {
    shared: Arc<Shared>,
}

struct NoRevocations;
impl RevocationLookup for NoRevocations {
    fn is_revoked(&self, _issuer_key_id: &str, _serial: &[u8; 16]) -> bool {
        false
    }
}

impl Daemon {
    /// Open the keystore and the store under `cfg.home`. Refuses a non-loopback address, and a
    /// second daemon on the same home (redb's file lock).
    pub fn open(cfg: &DaemonConfig) -> Result<Daemon, DaemonError> {
        if !cfg.addr.ip().is_loopback() {
            return Err(DaemonError::NotLoopback);
        }
        let keystore = Keystore::open(&cfg.home).map_err(DaemonError::Keystore)?;
        let store = RedbStore::open(&cfg.home.join("mailbox.redb")).map_err(DaemonError::Store)?;
        let started_at = Utc::now();
        let mailbox =
            Mailbox::open(store, MailConfig::default(), started_at).map_err(DaemonError::Mail)?;
        let instance = random_bytes::<16>().ok_or(DaemonError::NoRandomness)?;
        let instance_id = hex(&instance);
        let summary = keystore.account();
        Ok(Daemon {
            shared: Arc::new(Shared {
                mailbox: Mutex::new(mailbox),
                sessions: Mutex::new(HashMap::new()),
                presence: Mutex::new(HashMap::new()),
                account: summary.account,
                account_key_id: summary.key_id,
                account_key: keystore.account_public_key(),
                instance_id,
                proof_key: ring::hmac::Key::new(ring::hmac::HMAC_SHA256, &instance),
                home: cfg.home.clone(),
                started_at,
                sweep_every: cfg.sweep_every,
            }),
        })
    }

    /// When the daemon opened its role table. Claims must be signed after this (M3), so clients
    /// read it from `/v1/health`.
    #[must_use]
    pub fn started_at(&self) -> DateTime<Utc> {
        self.shared.started_at
    }

    /// Serve on `listener` until `shutdown` resolves.
    pub async fn serve(
        self,
        listener: tokio::net::TcpListener,
        shutdown: impl Future<Output = ()> + Send + 'static,
    ) -> std::io::Result<()> {
        let bound = listener.local_addr()?;
        if !bound.ip().is_loopback() {
            // Review M2: whatever the config said, never serve off loopback.
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "synapsed serves on loopback addresses only",
            ));
        }
        write_announce(&self.shared.home, bound, &self.shared.instance_id)?;
        // Removed however `serve` ends: shutdown, error or unwind (#74). A kill runs no code, which
        // is why clients rely on the health proof, not on the file's absence.
        let _announced = Announced {
            home: self.shared.home.clone(),
            instance_id: self.shared.instance_id.clone(),
        };
        let sweeper = {
            let shared = self.shared.clone();
            tokio::spawn(async move {
                let mut tick = tokio::time::interval(shared.sweep_every);
                tick.tick().await; // the first tick is immediate
                loop {
                    tick.tick().await;
                    if let Ok(mut mb) = shared.mailbox.lock() {
                        let _ = mb.sweep(Utc::now());
                    }
                }
            })
        };
        let app = Router::new()
            .route(
                "/v1/health",
                get(move |state: State<Arc<Shared>>, uri: Uri| health(state, bound, uri)),
            )
            .route("/v1/claim", post(claim))
            .route("/v1/send", post(send))
            .route("/v1/fetch", post(fetch))
            .route("/v1/ack", post(ack))
            .route("/v1/heartbeat", post(heartbeat))
            .route("/v1/list", get(list))
            .layer(middleware::from_fn(move |req: Request, next: Next| {
                host_guard(bound, req, next)
            }))
            .with_state(self.shared.clone());
        let result = axum::serve(listener, app)
            .with_graceful_shutdown(shutdown)
            .await;
        sweeper.abort();
        result
    }
}

/// DNS-rebinding and drive-by protection: exactly one `Host`, naming the address we are bound to
/// (or `localhost` on its port). Duplicate Host headers are refused outright (review M3).
async fn host_guard(bound: SocketAddr, req: Request, next: Next) -> Response {
    let mut hosts = req.headers().get_all(header::HOST).iter();
    let host = match (hosts.next(), hosts.next()) {
        (Some(only), None) => only.to_str().unwrap_or(""),
        _ => "",
    };
    let port = bound.port();
    let ours = host == bound.to_string() || host == format!("localhost:{port}");
    if !ours {
        return ApiError::new(
            StatusCode::MISDIRECTED_REQUEST,
            "wrong_host",
            "unknown host",
        )
        .into_response();
    }
    next.run(req).await
}

struct ApiError {
    status: StatusCode,
    body: Value,
}

impl ApiError {
    fn new(status: StatusCode, kind: &str, message: impl fmt::Display) -> Self {
        ApiError {
            status,
            body: json!({"error": kind, "message": message.to_string()}),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, axum::Json(self.body)).into_response()
    }
}

impl From<MailError> for ApiError {
    fn from(e: MailError) -> Self {
        match &e {
            MailError::Superseded(s) => ApiError {
                status: StatusCode::CONFLICT,
                body: json!({"error": "superseded", "current": s.current, "message": e.to_string()}),
            },
            MailError::MailboxFull => {
                ApiError::new(StatusCode::PAYLOAD_TOO_LARGE, "mailbox_full", e)
            }
            MailError::Malformed => ApiError::new(StatusCode::BAD_REQUEST, "malformed", e),
            MailError::LeaseOutOfRange => {
                ApiError::new(StatusCode::BAD_REQUEST, "lease_out_of_range", e)
            }
            MailError::NotFound => ApiError::new(StatusCode::NOT_FOUND, "not_found", e),
            MailError::NotYourLease => ApiError::new(StatusCode::FORBIDDEN, "not_your_lease", e),
            MailError::Claim(c) => ApiError::new(StatusCode::FORBIDDEN, "claim_refused", c),
            MailError::InvalidState(_) | MailError::Store(_) | MailError::Poisoned => {
                ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "store_unavailable", e)
            }
        }
    }
}

fn malformed(what: &str) -> ApiError {
    ApiError::new(StatusCode::BAD_REQUEST, "malformed", what)
}

fn locked<T>(m: &Mutex<T>) -> Result<std::sync::MutexGuard<'_, T>, ApiError> {
    m.lock().map_err(|_| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "store_unavailable",
            "lock poisoned",
        )
    })
}

/// A JSON body: `application/json` only (415 otherwise), unknown fields refused (400).
fn parse<T: DeserializeOwned>(headers: &HeaderMap, body: &Bytes) -> Result<T, ApiError> {
    let json = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| {
            v.split(';')
                .next()
                .is_some_and(|m| m.trim().eq_ignore_ascii_case("application/json"))
        });
    if !json {
        return Err(ApiError::new(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "unsupported_media_type",
            "bodies must be application/json",
        ));
    }
    serde_json::from_slice(body).map_err(|e| malformed(&format!("invalid body: {e}")))
}

fn token_hash(token: &[u8]) -> [u8; 32] {
    let digest = ring::digest::digest(&ring::digest::SHA256, token);
    let mut out = [0u8; 32];
    out.copy_from_slice(digest.as_ref());
    out
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn random_bytes<const N: usize>() -> Option<[u8; N]> {
    use ring::rand::SecureRandom;
    let mut out = [0u8; N];
    ring::rand::SystemRandom::new().fill(&mut out).ok()?;
    Some(out)
}

/// Write `<home>/synapsed.json` (address and instance id) atomically and owner-only: a temp file
/// created `0600` on Unix (on Windows it inherits the user-profile ACL of the home), then renamed.
fn write_announce(
    home: &std::path::Path,
    bound: SocketAddr,
    instance_id: &str,
) -> std::io::Result<()> {
    let body = json!({
        "addr": bound.to_string(),
        "instance_id": instance_id,
        "pid": std::process::id(),
    })
    .to_string();
    let temp = home.join(format!(".tmp-synapsed-{}", std::process::id()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    {
        use std::io::Write;
        let mut file = options.open(&temp)?;
        file.write_all(body.as_bytes())?;
        file.sync_all()?;
    }
    std::fs::rename(&temp, home.join(ANNOUNCE_FILE))
}

/// The announce file's owner: removes it when dropped, but only while it still names this daemon
/// instance, so a newer daemon's announcement is never deleted.
struct Announced {
    home: PathBuf,
    instance_id: String,
}

impl Drop for Announced {
    fn drop(&mut self) {
        let path = self.home.join(ANNOUNCE_FILE);
        let ours = std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            .is_some_and(|v| v["instance_id"] == self.instance_id.as_str());
        if ours {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// What the health proof signs (#74): a domain tag, the address this daemon is bound to, and the
/// client's challenge. The address makes a relayed proof useless: a squatter on a stale port that
/// forwards the challenge to a live daemon elsewhere gets back a MAC over the wrong address.
/// `synapse-cli` builds the same bytes; its tests run against this daemon, so the two must agree.
#[must_use]
pub fn health_proof_message(bound: SocketAddr, challenge: &[u8; 16]) -> Vec<u8> {
    let mut msg = format!("synapsed-health-v1\n{bound}\n").into_bytes();
    msg.extend_from_slice(challenge);
    msg
}

/// Strict lowercase-or-uppercase hex only: `u8::from_str_radix` alone would also accept `+a`,
/// giving tokens and nonces alternative encodings (review M8).
fn unhex<const N: usize>(s: &str) -> Option<[u8; N]> {
    if s.len() != N * 2 || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let mut out = [0u8; N];
    for (i, chunk) in s.as_bytes().chunks(2).enumerate() {
        let pair = std::str::from_utf8(chunk).ok()?;
        out[i] = u8::from_str_radix(pair, 16).ok()?;
    }
    Some(out)
}

/// The caller's session: Bearer token, looked up by its hash, then checked against the role's
/// current epoch. Every authenticated call also counts as presence.
fn authed(shared: &Shared, headers: &HeaderMap) -> Result<Session, ApiError> {
    let unauthorized = || {
        ApiError::new(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "a valid session is required",
        )
    };
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .and_then(unhex::<32>)
        .ok_or_else(unauthorized)?;
    let session = locked(&shared.sessions)?
        .get(&token_hash(&token))
        .cloned()
        .ok_or_else(unauthorized)?;
    locked(&shared.mailbox)?.check(&session.global_id, session.epoch)?;
    let now = Utc::now();
    let mut presence = locked(&shared.presence)?;
    let entry = presence
        .entry(session.global_id.clone())
        .or_insert(Presence {
            last_seen: now,
            summary: None,
        });
    entry.last_seen = now;
    Ok(session)
}

/// Liveness, and with `?challenge=<32 hex>` proof that this daemon holds the announced instance id:
/// `proof` = HMAC-SHA256(instance id, [`health_proof_message`]). Any other query is malformed.
async fn health(
    State(shared): State<Arc<Shared>>,
    bound: SocketAddr,
    uri: Uri,
) -> Result<Response, ApiError> {
    let mut reply = json!({
        "ok": true,
        "version": env!("CARGO_PKG_VERSION"),
        "started_at": shared.started_at.to_rfc3339(),
    });
    if let Some(query) = uri.query() {
        let challenge = query
            .strip_prefix("challenge=")
            .and_then(unhex::<16>)
            .ok_or_else(|| malformed("challenge"))?;
        let tag = ring::hmac::sign(&shared.proof_key, &health_proof_message(bound, &challenge));
        reply["proof"] = Value::String(hex(tag.as_ref()));
    }
    Ok(axum::Json(reply).into_response())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClaimBody {
    #[serde(default)]
    audience: Option<String>,
    chain_pem: String,
    nonce_hex: String,
    signed_at: DateTime<Utc>,
    signature_b64: String,
}

async fn claim(
    State(shared): State<Arc<Shared>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let body: ClaimBody = parse(&headers, &body)?;
    let req = ClaimRequest {
        chain: chain_from_pem(&body.chain_pem).map_err(|_| malformed("chain_pem"))?,
        nonce: unhex::<16>(&body.nonce_hex).ok_or_else(|| malformed("nonce_hex"))?,
        signed_at: body.signed_at,
        signature: B64
            .decode(&body.signature_b64)
            .map_err(|_| malformed("signature_b64"))?,
        audience: body.audience,
    };
    // Review I3: the claim must be signed for THIS daemon instance. Checked before the claim is
    // verified, so a claim for another audience never consumes its nonce here.
    if req.audience.as_deref() != Some(shared.instance_id.as_str()) {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "claim_refused",
            "the claim is not addressed to this daemon",
        ));
    }
    let (key_id, key) = (shared.account_key_id.clone(), shared.account_key);
    let lookup = move |kid: &str| (kid == key_id).then_some(key);
    let grant = locked(&shared.mailbox)?.claim(&req, &lookup, &NoRevocations, Utc::now())?;

    let token = random_bytes::<32>().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "store_unavailable",
            "no randomness",
        )
    })?;
    {
        let mut sessions = locked(&shared.sessions)?;
        // Superseded sessions are kept so their calls answer 409 `superseded` (telling the old
        // holder what happened) rather than 401, but only the most recent few per role.
        let floor = grant.epoch.saturating_sub(KEEP_SUPERSEDED);
        sessions.retain(|_, s| s.global_id != grant.global_id || s.epoch > floor);
        sessions.insert(
            token_hash(&token),
            Session {
                global_id: grant.global_id.clone(),
                epoch: grant.epoch,
            },
        );
    }
    Ok(axum::Json(json!({
        "global_id": grant.global_id,
        "epoch": grant.epoch,
        "session": hex(&token),
    }))
    .into_response())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SendBody {
    to: String,
    #[serde(default)]
    message_id: Option<String>,
    body_b64: String,
}

async fn send(
    State(shared): State<Arc<Shared>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let session = authed(&shared, &headers)?;
    let body: SendBody = parse(&headers, &body)?;
    // Review I1: ids are scoped by the verified sender, so no role can take another's id first.
    // A client id stays useful for its own idempotent resends.
    let client_id = match body.message_id {
        Some(id) => {
            if id.is_empty()
                || id.len() > MAX_CLIENT_ID
                || id.contains('/')
                || id.chars().any(char::is_control)
            {
                return Err(malformed("message_id"));
            }
            id
        }
        None => uuid::Uuid::new_v4().to_string(),
    };
    let message_id = format!("{}/{client_id}", session.global_id);
    let envelope = Envelope {
        message_id: message_id.clone(),
        to: body.to,
        from: session.global_id.clone(),
        body: B64
            .decode(&body.body_b64)
            .map_err(|_| malformed("body_b64"))?,
    };
    let outcome = {
        let mut mailbox = locked(&shared.mailbox)?;
        // Review M5: re-check the sender's epoch under the same lock as the enqueue.
        mailbox.check(&session.global_id, session.epoch)?;
        // Review I2: serve this daemon's account and roles that have claimed at least once, so no
        // role can fill the disk with queues for recipients that can never read them.
        let known = envelope
            .to
            .split_once('@')
            .is_some_and(|(_, account)| account == shared.account)
            && mailbox.roles_snapshot().roles.contains_key(&envelope.to);
        if !known {
            return Err(ApiError::new(
                StatusCode::NOT_FOUND,
                "unknown_recipient",
                "no such role on this daemon",
            ));
        }
        mailbox.enqueue(envelope, Utc::now())?
    };
    Ok(axum::Json(json!({
        "message_id": message_id,
        "outcome": match outcome { Enqueued::Queued => "queued", Enqueued::Duplicate => "duplicate" },
    }))
    .into_response())
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct FetchBody {
    #[serde(default)]
    max: Option<usize>,
    #[serde(default)]
    lease_secs: Option<i64>,
}

async fn fetch(
    State(shared): State<Arc<Shared>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let session = authed(&shared, &headers)?;
    let body: FetchBody = parse(&headers, &body)?;
    let max = body.max.unwrap_or(10).min(MAX_FETCH);
    // Review M1: `Duration::seconds` panics on huge values; an absurd lease is a 400.
    let lease = match body.lease_secs {
        None => None,
        Some(secs) => {
            Some(chrono::Duration::try_seconds(secs).ok_or_else(|| malformed("lease_secs"))?)
        }
    };
    let deliveries = locked(&shared.mailbox)?.fetch(
        &session.global_id,
        session.epoch,
        max,
        lease,
        Utc::now(),
    )?;
    let messages: Vec<Value> = deliveries
        .into_iter()
        .map(|d| {
            json!({
                "message_id": d.envelope.message_id,
                "from": d.envelope.from,
                "body_b64": B64.encode(&d.envelope.body),
                "enqueued_at": d.enqueued_at.to_rfc3339(),
                "lease_until": d.lease_until.to_rfc3339(),
                "attempts": d.attempts,
            })
        })
        .collect();
    Ok(axum::Json(json!({ "messages": messages })).into_response())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AckBody {
    message_id: String,
}

async fn ack(
    State(shared): State<Arc<Shared>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let session = authed(&shared, &headers)?;
    let body: AckBody = parse(&headers, &body)?;
    let outcome = locked(&shared.mailbox)?.ack(
        &session.global_id,
        session.epoch,
        &body.message_id,
        Utc::now(),
    )?;
    Ok(axum::Json(json!({
        "outcome": match outcome { Acked::Removed => "removed", Acked::AlreadyAcked => "already_acked" },
    }))
    .into_response())
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct HeartbeatBody {
    #[serde(default)]
    summary: Option<String>,
}

async fn heartbeat(
    State(shared): State<Arc<Shared>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let session = authed(&shared, &headers)?;
    let body: HeartbeatBody = parse(&headers, &body)?;
    if let Some(summary) = body.summary {
        if summary.len() > MAX_SUMMARY || summary.chars().any(char::is_control) {
            return Err(malformed("summary"));
        }
        if let Some(p) = locked(&shared.presence)?.get_mut(&session.global_id) {
            p.summary = Some(summary);
        }
    }
    Ok(axum::Json(json!({})).into_response())
}

async fn list(State(shared): State<Arc<Shared>>, headers: HeaderMap) -> Result<Response, ApiError> {
    authed(&shared, &headers)?;
    let roles = locked(&shared.mailbox)?.roles_snapshot();
    let presence = locked(&shared.presence)?;
    let now = Utc::now();
    let out: Vec<Value> = roles
        .roles
        .iter()
        .map(|(global_id, record)| {
            let seen = presence.get(global_id);
            json!({
                "global_id": global_id,
                "epoch": record.epoch,
                "online": seen.is_some_and(|p| now - p.last_seen <= ONLINE_WINDOW),
                "last_seen": seen.map(|p| p.last_seen.to_rfc3339()),
                "summary": seen.and_then(|p| p.summary.clone()),
            })
        })
        .collect();
    Ok(axum::Json(json!({ "roles": out })).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #77 D2: `locked` on a poisoned mutex is 503 `store_unavailable`; on a healthy one it yields
    /// the guard. A poisoned lock is unreachable over HTTP without a test hook, so it is poisoned
    /// directly here: a thread panics while holding the guard.
    #[test]
    fn locked_maps_a_poisoned_mutex_to_503_store_unavailable() {
        let healthy = Mutex::new(1u8);
        let Ok(guard) = locked(&healthy) else {
            panic!("a healthy mutex must lock");
        };
        assert_eq!(*guard, 1);
        drop(guard);

        let poisoned = Arc::new(Mutex::new(1u8));
        let holder = Arc::clone(&poisoned);
        let joined = std::thread::spawn(move || {
            let _guard = holder.lock().unwrap();
            panic!("poison the mutex on purpose");
        })
        .join();
        assert!(joined.is_err(), "precondition: the thread panicked");
        assert!(
            poisoned.is_poisoned(),
            "precondition: the mutex is poisoned"
        );

        let Err(error) = locked(&poisoned) else {
            panic!("a poisoned mutex must not lock");
        };
        assert_eq!(error.status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(error.body["error"], "store_unavailable");
    }
}
