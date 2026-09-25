# Email Transport Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace `SimpleEmailTransport`'s permanent honest refusal with a real, `Transport`-conformant email transport that sends via SMTP and receives via a locally-run SMTP server (Direct mode) or, when direct SMTP isn't possible, via a relay for send and IMAP polling of a proxy mailbox for receive (Relay-out/External modes).

**Architecture:** One new `EmailTransportImpl` (`src/transport/email_unified.rs`) implementing `abstraction::Transport` + `TransportReceive`, dispatching on a mode selected at `start()` via `email_server::connectivity::ConnectivityDetector`. Reuses `src/email.rs`'s `lettre` SMTP-client wiring for the send path (rewritten message-body construction, per Task 1's finding below); reuses and repairs `email_server::SynapseSmtpServer`/`SynapseImapServer` for the receive path, which today have never been wired to anything and have a real message-store disconnect (Task 1).

**Tech Stack:** `lettre` 0.11 (SMTP client), `async-imap` 0.10 (IMAP client, Task 3), existing `email_server` module (SMTP/IMAP servers), existing `abstraction::Transport`/`TransportReceive`/`TransportCapabilities` contract.

**Spec:** `docs/superpowers/specs/2026-09-24-email-transport-design.md` (commit `f390258`) — executors read both documents; this plan argues from that spec and does not restate its reasoning, only its binding text.

## Global Constraints

Copied verbatim from the spec. Every task's requirements implicitly include this section.

- §3: *"This design never reads or relies on the SMTP-level sender identity for anything security-relevant."* Sender authentication is the `SecureMessage` signature only, verified downstream by `TransportManager` exactly as for every other transport. No task in this plan may add any check, trust decision, or capability that depends on `MAIL FROM`, `From:`, or the connecting client's address/identity.
- §3: *"These two facts must not be conflated: a message can arrive over an authenticated SMTP submission and still carry a `SecureMessage` whose signature fails verification ... and a message can arrive over an unauthenticated inbound connection from the public Internet and still verify perfectly."* SMTP `AUTH` (Task 3, relay-out mode) governs who may submit to our server for relay; it must never be read as, or documented as, a statement about message sender identity.
- §4: *"Serialize the whole `SecureMessage` via `serde_json::to_vec`, never hand-pick fields into a template string."* Every send and receive path in this plan carries the whole `SecureMessage` as JSON, byte-for-byte, both directions.
- §4: `send_message` returns `DeliveryConfirmation::Sent` on a `250 OK` from the immediate next hop, **never** `Delivered`.
- §5: *"`Serialize`/`Deserialize` must round-trip the real value"* for `SmtpConfig`/`ImapConfig`'s password wrapper — `Config::to_file`/`from_file` (`config.rs:101-114`) depend on this. Only `Debug` redacts.
- §5 (as narrowed by PM review): *"The protection boundary is narrower than 'accidental exposure' in general ... `SecretString` redacts `Debug`, and nothing else."* Do not write or imply broader protection than that anywhere in code comments or tests.
- §6: `reliability_score` computed as `messages_sent / (messages_sent + send_failures)`, `1.0` when untried — QUIC's exact formula (`quic_unified.rs`), not a new one.
- §6: *"`average_latency_ms` should be declared via `unmeasured_metrics` unless this slice actually instruments it ... measure it for real ... or declare it absent; do not estimate."*
- §7: *"The hard constraint: no test may depend on a live external mail provider (Gmail, etc.) — that is both a CI-reliability risk and, per §1a, not even the primary path this slice builds."*
- §1a(4), CireSnave verbatim: *"IMAP ... allow[s] one server to send and receive messages on behalf of another as a sort of proxy"* — not for a human to read agent traffic. Task 3's IMAP integration must not add any human-mailbox-reading capability.

**Deviations from the spec found while planning, stated here per the spec's §8 requirement (none require correcting the spec — they are implementation-level facts the spec did not anticipate at its level of detail):**

1. `email_server::SynapseSmtpServer::new` does not accept a shared message store — it creates its own internal `Arc<Mutex<HashMap<...>>>` (`smtp_server.rs:138`), while `SynapseImapServer::new` **does** accept one as a parameter (`imap_server.rs:87-94`). `SynapseEmailServer::new`/`new_with_scope` (`email_server/mod.rs`) constructs one shared store and gives it only to the IMAP server — so today, mail received via SMTP never reaches the IMAP server's store at all. This is a real, previously-uncaught wiring defect (consistent with CAPABILITY_INVENTORY.md's "no test binds a port or speaks SMTP/IMAP"). Task 1 fixes `SynapseSmtpServer::new`'s signature to match `SynapseImapServer::new`'s (accept the shared store), which is a breaking change to that constructor's signature.
2. `SynapseSmtpServer::store_message` (`smtp_server.rs:379-421`, currently `#[allow(dead_code)]` — never called) hand-picks the raw SMTP `DATA` bytes directly into `SecureMessage.encrypted_content`, discarding any structure the sender might have put there and never treating the body as a JSON-encoded whole `SecureMessage`. This is incompatible with the spec's §4 wire format. Task 1 rewrites `store_message` to `serde_json::from_slice::<SecureMessage>` the body instead, and wires it to actually be called from the DATA-command handler (currently nothing calls it).

## File Structure

- **Create** `src/transport/email_unified.rs` — `EmailTransportImpl`, `EmailTransportFactory`, mode dispatch. Replaces `SimpleEmailTransport` as the constructor `TransportType::Email`'s factory produces (Task 1).
- **Modify** `src/email_server/smtp_server.rs` — thread a shared message store through `SynapseSmtpServer::new`; rewrite and wire up `store_message` (Task 1).
- **Modify** `src/email_server/mod.rs` — `SynapseEmailServer::new_with_scope`/`with_config`/`create_test_email_server` updated for `SynapseSmtpServer::new`'s new signature (Task 1).
- **Modify** `src/types.rs` — `SecretString` wrapper type; `SmtpConfig`/`ImapConfig`'s `password` field changes type (Task 2).
- **Modify** `src/config.rs` — `test_config_file_operations` gains a password-round-trip assertion (Task 2).
- **Modify** `src/transport/email_unified.rs` (continued) — relay-out/external mode dispatch via `ConnectivityDetector`, IMAP polling receive (Task 3); size limits/queue (Task 4); final `capabilities()`/`metrics()`/`estimate_metrics()` (Task 5).
- **Modify** `src/transport/abstraction.rs` — remove the now-superseded `SimpleEmailTransport`/`SimpleEmailTransportFactory` registration if `transport/mod.rs` wires factories by type there; register `EmailTransportFactory` in whatever central place NAT/QUIC's factories are registered (confirm exact registration point at Task 1 time — see Task 1 Step 1).
- **Modify** `tests/transport_repairs.rs` — `TransportType::Email` added to `port_key`/`socket_of`; `email()` factory helper; tests per task.

## Task 1: Skeleton — Direct mode send and receive

**Files:**
- Create: `src/transport/email_unified.rs`
- Modify: `src/email_server/smtp_server.rs:17-28,133-144,379-421`
- Modify: `src/email_server/mod.rs` (every `SynapseSmtpServer::new` call site)
- Modify: `src/transport/mod.rs` (wherever `SimpleEmailTransportFactory` is currently registered/exported — grep `SimpleEmailTransportFactory` first)
- Test: `tests/transport_repairs.rs`

**Interfaces:**
- Consumes: `abstraction::{Transport, TransportReceive, TransportCapabilities, TransportType, TransportTarget, TransportEstimate, DeliveryReceipt, DeliveryConfirmation, ConnectivityResult, TransportStatus, TransportMetrics, RawInbox, UnmeasuredMetric}` (existing, `abstraction.rs`); `types::{EmailConfig, SecureMessage}`; `email_server::{SynapseSmtpServer, SmtpServerConfig}`.
- Produces: `pub struct EmailTransportImpl` with `pub async fn new(config: &HashMap<String, String>) -> Result<Self>` (matching every other `_unified.rs`'s factory-callable constructor shape, e.g. `QuicTransportImpl::new`); `pub struct EmailTransportFactory;` implementing `abstraction::TransportFactory`. Later tasks (2-5) extend this same struct — do not rename fields task-to-task.

- [ ] **Step 1: Locate and remove the `SimpleEmailTransport` registration**

Run: `grep -rn "SimpleEmailTransportFactory\|SimpleEmailTransport" src/transport/mod.rs src/transport/manager.rs`

This finds the exact place `TransportType::Email` currently maps to `SimpleEmailTransportFactory`. Note the file and line — Step 8 replaces it with `EmailTransportFactory`. Do not delete `email_simple.rs` itself yet; leave it in the tree (it is still a legitimate honest-stub reference until this task's factory is proven working, then delete it in Step 9).

- [ ] **Step 2: Write the failing end-to-end test**

Add to `tests/transport_repairs.rs`, in `port_key`:

```rust
fn port_key(kind: TransportType) -> &'static str {
    match kind {
        TransportType::Tcp => "listen_port",
        TransportType::Udp => "bind_port",
        TransportType::Http => "server_port",
        TransportType::WebSocket => "local_port",
        TransportType::NatTraversal => "local_port",
        TransportType::Quic => "local_port",
        TransportType::Email => "local_port",
        other => panic!("no port key recorded for {other:?}; add it to port_key"),
    }
}
```

And in `socket_of`:

```rust
fn socket_of(kind: TransportType) -> Socket {
    match kind {
        TransportType::Tcp | TransportType::Http | TransportType::WebSocket => Socket::Tcp,
        TransportType::Udp | TransportType::NatTraversal | TransportType::Quic => Socket::Udp,
        TransportType::Email => Socket::Tcp,
        other => panic!("no socket family recorded for {other:?}; add it to socket_of"),
    }
}
```

Then, near the other transports' factory helpers (after `fn nat()`):

```rust
fn email() -> Box<dyn TransportFactory> {
    Box::new(synapse::transport::EmailTransportFactory)
}

/// EmailTransportFactory always refused (SimpleEmailTransport's honest stub) until this task.
/// This is the first test proving Direct mode actually sends and receives: alice's
/// EmailTransportImpl relays via lettre to bob's own EmailTransportImpl, whose EmailTransportFactory
/// starts a SynapseSmtpServer on `local_port`; bob's `receive_raw` drains that server's message
/// store.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn email_direct_mode_carries_a_verified_message_end_to_end() {
    let (received, receipt) = round_trip(
        TransportType::Email,
        email(),
        email(),
        free_port(),
        free_port(),
        b"repaired",
        DeliveryConfirmation::Sent,
    )
    .await;
    assert_eq!(received.incoming.transport_type, TransportType::Email);
    assert_eq!(receipt.transport_used, TransportType::Email);
    assert_eq!(received.payload, Payload::Opened(b"repaired".to_vec()));
}
```

- [ ] **Step 2b: Run test to verify it fails**

Run: `cargo test --test transport_repairs email_direct_mode_carries_a_verified_message_end_to_end -- --exact`
Expected: FAIL to compile — `synapse::transport::EmailTransportFactory` does not exist yet.

- [ ] **Step 3: Thread a shared message store through `SynapseSmtpServer`**

In `src/email_server/smtp_server.rs`, change the constructor (line 133-144) to accept the store, matching `SynapseImapServer::new`'s existing pattern exactly:

```rust
impl SynapseSmtpServer {
    /// Create a new SMTP server. `message_store` is shared with whatever else needs to read
    /// received mail (an `EmailTransportImpl`'s `receive_raw`, or a paired `SynapseImapServer` for
    /// the relay-out/external proxy case) -- previously this server held its own private store
    /// that nothing else could ever read (see this plan's "Deviations" section).
    pub fn new(
        config: SmtpServerConfig,
        auth_handler: Arc<dyn AuthHandler + Send + Sync>,
        message_store: Arc<Mutex<HashMap<String, Vec<SecureMessage>>>>,
    ) -> Self {
        Self {
            config,
            message_store,
            clients: Arc::new(Mutex::new(HashMap::new())),
            auth_handler,
            metrics: Arc::new(Mutex::new(ServerMetrics::default())),
        }
    }
    // ... get_messages, get_metrics, start, handle_connection unchanged ...
}
```

Add a drain method next to the existing `get_messages` (which clones without removing — receive_raw must remove, matching every other transport's destructive `receive_raw`):

```rust
    /// Take every message queued for `recipient`, removing them from the store. `receive_raw`
    /// calls this, not `get_messages` (which is non-destructive and used elsewhere for
    /// inspection/tests).
    pub fn drain_messages(&self, recipient: &str) -> Result<Vec<SecureMessage>> {
        let mut store = self.message_store.lock().unwrap();
        Ok(store.remove(recipient).unwrap_or_default())
    }
```

- [ ] **Step 4: Fix `store_message` to parse the JSON wire format, and wire it up**

Replace `store_message` (`smtp_server.rs:379-421`) — delete the `#[allow(dead_code)]` attribute, since Step 4b makes it reachable:

```rust
    /// Store a message received over SMTP. The `DATA` command's body is the whole `SecureMessage`
    /// as JSON (per the transport contract's wire format -- see this plan's Global Constraints),
    /// not raw content to wrap: hand-picking fields here was the exact bug NAT traversal's PR B
    /// fixed, and this server had the same one, just never reachable until now.
    async fn store_message(&self, message: SmtpMessage) -> Result<()> {
        let start_time = SystemTime::now();

        let secure_message: SecureMessage = serde_json::from_slice(&message.data).map_err(|e| {
            SynapseError::InvalidMessageFormat(format!(
                "SMTP DATA body did not parse as a SecureMessage: {e}"
            ))
        })?;

        {
            let mut store = self.message_store.lock().unwrap();
            for recipient in &message.to {
                let messages = store.entry(recipient.clone()).or_default();
                messages.push(secure_message.clone());
            }
        }

        {
            let mut metrics = self.metrics.lock().unwrap();
            metrics.messages_received += 1;
            if let Ok(elapsed) = start_time.elapsed() {
                let processing_time = elapsed.as_millis() as f64;
                metrics.average_processing_time_ms =
                    (metrics.average_processing_time_ms + processing_time) / 2.0;
            }
        }

        info!(
            "Stored message from {} to {:?}",
            secure_message.from_global_id, message.to
        );
        Ok(())
    }
```

`SynapseError::InvalidMessageFormat(String)` already exists (`error.rs:57`) and is the right variant for "did not parse" — used verbatim above, not a guess.

**Note the deliberate divergence from NAT traversal's own convention:** `nat_traversal.rs`'s `parse_nat_message` returns `Ok(None)` on a parse failure (logs a warning, silently drops the message) rather than an `Err` — appropriate there because UDP has no synchronous response channel back to the sender. SMTP does: a `DATA` command that fails to parse should cause the transaction to be **rejected with an SMTP error response**, which requires `store_message` to return `Err`, not silently succeed-and-drop. Step 4b's handler must translate this `Err` into a `5xx` SMTP response (e.g. `"554 Transaction failed: message body did not parse\r\n"`), not swallow it.

- [ ] **Step 4a: Write the two-arm test for `store_message` directly — not just the end-to-end test**

**Required because of how Steps 1-4 combine: `store_message` is being fixed and wired up (made reachable) in the same task.** A passing end-to-end test (Step 2) after this change proves only that the new path works; it demonstrates nothing about the old hand-picking behavior being wrong, because that behavior was never reachable to regress. The test must assert the positive property directly, both arms, in `src/email_server/smtp_server.rs`'s own `#[cfg(test)] mod tests` (check `grep -n "mod tests" src/email_server/smtp_server.rs` for whether one exists already):

```rust
#[tokio::test]
async fn store_message_round_trips_a_json_secure_message_byte_identical() {
    let auth_handler: Arc<dyn AuthHandler + Send + Sync> = Arc::new(create_test_auth_handler());
    let message_store = Arc::new(Mutex::new(HashMap::new()));
    let server = SynapseSmtpServer::new(
        SmtpServerConfig::default(),
        auth_handler,
        Arc::clone(&message_store),
    );

    let original = SecureMessage::new(
        "bob@synapse.local",
        "alice@synapse.local",
        b"hello".to_vec(),
        crate::types::SecurityLevel::Public,
    );
    let body = serde_json::to_vec(&original).unwrap();

    server
        .store_message(SmtpMessage {
            from: "alice@synapse.local".to_string(),
            to: vec!["bob@synapse.local".to_string()],
            data: body,
        })
        .await
        .expect("a valid JSON SecureMessage must be accepted");

    let stored = server.drain_messages("bob@synapse.local").unwrap();
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].message_id, original.message_id, "must round-trip byte-identical, not re-derived");
    assert_eq!(stored[0].encrypted_content, original.encrypted_content);
}

#[tokio::test]
async fn store_message_refuses_a_non_json_body() {
    let auth_handler: Arc<dyn AuthHandler + Send + Sync> = Arc::new(create_test_auth_handler());
    let message_store = Arc::new(Mutex::new(HashMap::new()));
    let server = SynapseSmtpServer::new(SmtpServerConfig::default(), auth_handler, message_store);

    let err = server
        .store_message(SmtpMessage {
            from: "alice@synapse.local".to_string(),
            to: vec!["bob@synapse.local".to_string()],
            data: b"not json at all".to_vec(),
        })
        .await
        .expect_err("a non-JSON body must be refused, not silently wrapped as encrypted_content");
    assert!(matches!(err, SynapseError::InvalidMessageFormat(_)));
}
```

`SmtpMessage` and `store_message` are both private to `smtp_server.rs` today — a `#[cfg(test)] mod tests` declared inside that same file can see both without any visibility change, since a child module always sees its parent's private items. Do not make either `pub` for this. Run both new tests before Step 4's rewrite: expect `store_message_round_trips_a_json_secure_message_byte_identical` to fail (the old code wraps `message.data` directly into `encrypted_content` rather than parsing it as JSON, so `stored[0].message_id` won't equal `original.message_id`) and `store_message_refuses_a_non_json_body` to fail (the old code never validates the body at all, so it succeeds where it should error). After Step 4's rewrite, both pass.

- [ ] **Step 4b: Wire `store_message` to the DATA command handler**

Run: `grep -n "\"DATA\"\|current_message\|fn handle_connection" src/email_server/smtp_server.rs` to find where a completed `DATA` command currently builds a `SmtpMessage` but never calls `store_message` on it (this is why the method is dead code today). Add the call at that point — read the surrounding ~40 lines to match the existing control flow's variable names exactly before editing, since this plan's research did not trace that handler in full.

- [ ] **Step 5: Update `SynapseEmailServer`'s constructors for the new `SynapseSmtpServer::new` signature**

In `src/email_server/mod.rs`, every call to `SynapseSmtpServer::new(smtp_config, auth_handler_clone)` (three call sites: `new_with_scope`, `with_config`, `create_test_email_server`) becomes `SynapseSmtpServer::new(smtp_config, auth_handler_clone, Arc::clone(&message_store))`, using the same `message_store` each function already constructs and already passes to `SynapseImapServer::new` — this makes the two servers finally share one store, closing the deviation noted above.

- [ ] **Step 6: Write `EmailTransportImpl`'s skeleton — Direct mode only**

Create `src/transport/email_unified.rs`:

```rust
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Email transport: SMTP send, SMTP-server receive (Direct mode). Relay-out/External modes and
//! IMAP polling arrive in Task 3.

use super::abstraction::*;
use crate::{
    email_server::{AuthHandler, SmtpServerConfig, SynapseSmtpServer, auth::SynapseAuthHandler},
    error::{Result, SynapseError},
    types::{EmailConfig, SecureMessage},
};
use async_trait::async_trait;
use lettre::{
    SmtpTransport, Transport as LettreTransport,
    message::{Mailbox, Message, SinglePart, header},
    transport::smtp::authentication::Credentials,
};
use std::{
    collections::{HashMap, HashMap as StdHashMap},
    sync::{Arc, Mutex as StdMutex},
};

/// Direct-mode email transport: sends via `lettre` straight to the configured SMTP host, receives
/// via its own `SynapseSmtpServer` listening on `local_port`. `local_address` is this transport's
/// own email address (`smtp_username`'s value) -- the key `SynapseSmtpServer`'s message store
/// indexes received mail under, since `store_message` stores by `RCPT TO`'s parsed recipient
/// address (`smtp_server.rs`'s `store_message`/`extract_email_from_rcpt_to`, line 369), and
/// `email_server::auth::SynapseAuthHandler::is_authorized_recipient` authorizes by the same
/// full email address, never by port. Confirmed by reading both call sites; this is not a guess.
pub struct EmailTransportImpl {
    config: EmailConfig,
    smtp_client: SmtpTransport,
    smtp_server: Arc<SynapseSmtpServer>,
    local_address: String,
}

impl EmailTransportImpl {
    pub async fn new(config: &HashMap<String, String>) -> Result<Self> {
        let local_port: u16 = config
            .get("local_port")
            .map(|p| p.parse())
            .transpose()
            .map_err(|_| SynapseError::Config("invalid local_port: must be a u16".to_string()))?
            .unwrap_or(0);
        let bind_scope = crate::network_scope::BindScope::from_config_map(config)?;

        let email_config = Self::email_config_from_map(config)?;
        let local_address = email_config.smtp.username.clone();

        let smtp_client = SmtpTransport::relay(&email_config.smtp.host)
            .map_err(|e| SynapseError::TransportError(format!("invalid SMTP host: {e}")))?
            .port(email_config.smtp.port)
            .credentials(Credentials::new(
                email_config.smtp.username.clone(),
                email_config.smtp.password.expose().to_string(),
            ))
            .build();

        let auth_handler: Arc<dyn AuthHandler + Send + Sync> = Arc::new(SynapseAuthHandler::new());
        let message_store = Arc::new(StdMutex::new(StdHashMap::new()));
        let smtp_server = Arc::new(SynapseSmtpServer::new(
            SmtpServerConfig {
                port: local_port,
                bind_scope,
                ..Default::default()
            },
            auth_handler,
            message_store,
        ));

        Ok(Self {
            config: email_config,
            smtp_client,
            smtp_server,
            local_address,
        })
    }

    /// Build `EmailConfig` from the factory's `HashMap<String, String>` config, the same shape
    /// every other `_unified.rs` factory takes. Config keys, all required except the two `use_*`
    /// booleans (default `false` if absent, matching `bool::from_str`'s own parsing): `smtp_host`,
    /// `smtp_port`, `smtp_username`, `smtp_password`, `smtp_use_tls`, `smtp_use_ssl`, `imap_host`,
    /// `imap_port`, `imap_username`, `imap_password`, `imap_use_ssl` (Task 3 adds relay/proxy-mode
    /// keys to this same map -- do not scatter config parsing across two functions).
    fn email_config_from_map(config: &HashMap<String, String>) -> Result<EmailConfig> {
        fn required(config: &HashMap<String, String>, key: &str) -> Result<String> {
            config
                .get(key)
                .cloned()
                .filter(|v| !v.is_empty())
                .ok_or_else(|| SynapseError::Config(format!("{key} is required and must be non-empty")))
        }
        fn port(config: &HashMap<String, String>, key: &str) -> Result<u16> {
            required(config, key)?
                .parse()
                .map_err(|_| SynapseError::Config(format!("{key} must be a valid port number")))
        }
        fn flag(config: &HashMap<String, String>, key: &str) -> bool {
            config.get(key).map(|v| v == "true").unwrap_or(false)
        }

        Ok(EmailConfig {
            smtp: crate::types::SmtpConfig {
                host: required(config, "smtp_host")?,
                port: port(config, "smtp_port")?,
                username: required(config, "smtp_username")?,
                password: crate::types::SecretString::new(required(config, "smtp_password")?),
                use_tls: flag(config, "smtp_use_tls"),
                use_ssl: flag(config, "smtp_use_ssl"),
            },
            imap: crate::types::ImapConfig {
                host: required(config, "imap_host")?,
                port: port(config, "imap_port")?,
                username: required(config, "imap_username")?,
                password: crate::types::SecretString::new(required(config, "imap_password")?),
                use_ssl: flag(config, "imap_use_ssl"),
            },
        })
    }
}
```

- [ ] **Step 7: Implement `Transport`/`TransportReceive` for `EmailTransportImpl`**

```rust
#[async_trait]
impl Transport for EmailTransportImpl {
    fn transport_type(&self) -> TransportType {
        TransportType::Email
    }

    fn capabilities(&self) -> TransportCapabilities {
        // Task 5 finalizes this; Task 1 only needs enough to pass its own test.
        TransportCapabilities {
            max_message_size: 10 * 1024 * 1024,
            reliable: true,
            real_time: false,
            broadcast: false,
            bidirectional: true,
            encrypted: self.config.smtp.use_tls || self.config.smtp.use_ssl,
            network_spanning: true,
            supported_urgencies: vec![MessageUrgency::Background, MessageUrgency::Batch],
            features: vec!["direct_smtp".to_string()],
            unmeasured_metrics: vec![
                UnmeasuredMetric::AverageLatency,
                UnmeasuredMetric::ReliabilityScore,
            ],
        }
    }

    async fn can_reach(&self, target: &TransportTarget) -> bool {
        target.identifier.contains('@')
    }

    async fn estimate_metrics(&self, target: &TransportTarget) -> Result<TransportEstimate> {
        let available = self.can_reach(target).await;
        Ok(TransportEstimate {
            latency: std::time::Duration::from_secs(5),
            reliability: if available { 0.5 } else { 0.0 },
            bandwidth: 0,
            cost: 0.1,
            available,
            confidence: 0.3,
        })
    }

    async fn send_message(
        &self,
        target: &TransportTarget,
        message: &SecureMessage,
    ) -> Result<DeliveryReceipt> {
        let start = std::time::Instant::now();
        let to_email = &target.identifier;

        // Whole SecureMessage as JSON -- never hand-picked fields (Global Constraints, §4).
        let body = serde_json::to_vec(message)
            .map_err(|e| SynapseError::TransportError(format!("serialize failed: {e}")))?;

        let from_mailbox: Mailbox = self
            .config
            .smtp
            .username
            .parse()
            .map_err(|e| SynapseError::TransportError(format!("invalid from address: {e}")))?;
        let to_mailbox: Mailbox = to_email
            .parse()
            .map_err(|e| SynapseError::TransportError(format!("invalid to address: {e}")))?;

        let email = Message::builder()
            .from(from_mailbox)
            .to(to_mailbox)
            .subject("Synapse")
            .singlepart(
                SinglePart::builder()
                    .header(header::ContentType::TEXT_PLAIN)
                    .body(String::from_utf8_lossy(&body).into_owned()),
            )
            .map_err(|e| SynapseError::TransportError(format!("build failed: {e}")))?;

        LettreTransport::send(&self.smtp_client, &email)
            .map_err(|e| SynapseError::TransportError(format!("SMTP send failed: {e}")))?;

        Ok(DeliveryReceipt {
            message_id: message.message_id.0.to_string(),
            transport_used: TransportType::Email,
            delivery_time: start.elapsed(),
            target_reached: to_email.clone(),
            confirmation: DeliveryConfirmation::Sent,
            metadata: HashMap::new(),
        })
    }

    async fn test_connectivity(&self, target: &TransportTarget) -> Result<ConnectivityResult> {
        Ok(ConnectivityResult {
            connected: self.can_reach(target).await,
            rtt: None,
            error: None,
            quality: 0.5,
            details: HashMap::new(),
        })
    }

    async fn start(&self) -> Result<()> {
        let server = Arc::clone(&self.smtp_server);
        tokio::spawn(async move {
            if let Err(e) = server.start().await {
                tracing::warn!("email SMTP server error: {e}");
            }
        });
        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        Ok(())
    }

    async fn status(&self) -> TransportStatus {
        TransportStatus::Running
    }

    async fn metrics(&self) -> TransportMetrics {
        // Task 5 finalizes this with real counters.
        TransportMetrics {
            transport_type: TransportType::Email,
            ..Default::default()
        }
    }
}

#[async_trait]
impl TransportReceive for EmailTransportImpl {
    async fn receive_raw(&self, inbox: &mut RawInbox) -> Result<()> {
        // Direct mode: drain our own SMTP server's message store. `local_address` doubles as the
        // store's recipient key today (Step 6's placeholder) -- Task 1's implementer must confirm
        // against `SynapseAuthHandler`'s actual local-domain/recipient model (`email_server/auth.rs`)
        // rather than trusting this plan's guess, since this plan did not trace that file.
        let messages = self.smtp_server.drain_messages(&self.local_address)?;
        for secure_message in messages {
            inbox.push(IncomingMessage::new(
                secure_message,
                TransportType::Email,
                "smtp".to_string(),
            ));
        }
        Ok(())
    }
}

pub struct EmailTransportFactory;

#[async_trait]
impl TransportFactory for EmailTransportFactory {
    async fn create_transport(&self, config: &HashMap<String, String>) -> Result<Box<dyn Transport>> {
        Ok(Box::new(EmailTransportImpl::new(config).await?))
    }

    fn transport_type(&self) -> TransportType {
        TransportType::Email
    }

    fn default_config(&self) -> HashMap<String, String> {
        HashMap::new()
    }

    fn validate_config(&self, _config: &HashMap<String, String>) -> Result<()> {
        Ok(())
    }
}
```

**One sequencing note:** the code above already assumes Task 2's `SecretString` wrapper (`.expose()` calls in `new`). If Task 2 has not landed yet when Task 1 is implemented, implement Task 2 first — a real credential should never sit as a plain `String` even transiently in a merged commit — or, if strict task order is required, use `.clone()` on a plain `String` field temporarily and replace it with `.expose()` calls when Task 2 lands, in the same commit that changes the field type.

- [ ] **Step 8: Register `EmailTransportFactory`, run the test**

Replace whatever Step 1 found (the `SimpleEmailTransportFactory` registration) with `EmailTransportFactory`.

Run: `cargo test --test transport_repairs email_direct_mode_carries_a_verified_message_end_to_end -- --exact`
Expected: PASS. If it fails, the two implementer notes above (Steps 6-7) are the most likely gaps — resolve them against the actual `email_server::auth` code, not by adjusting the test's expectations.

- [ ] **Step 9: Delete `email_simple.rs`**

Its honest-refusal job is done; `TransportType::Email` now constructs something real. Run `grep -rn "email_simple\|SimpleEmailTransport" src/` to confirm nothing else references it before deleting, then remove the file and its `mod`/`pub use` declarations in `transport/mod.rs`.

- [ ] **Step 10: Commit**

```bash
git add src/transport/email_unified.rs src/transport/mod.rs src/email_server/smtp_server.rs src/email_server/mod.rs tests/transport_repairs.rs
git rm src/transport/email_simple.rs
git commit -m "feat: email transport, Direct mode -- real SMTP send/receive"
```

## Task 2: Credential handling

**Files:**
- Modify: `src/types.rs` (`SmtpConfig`, `ImapConfig`, new `SecretString`)
- Modify: `src/config.rs` (`test_config_file_operations`)
- Test: `src/types.rs` (inline `#[cfg(test)]`)

**Interfaces:**
- Produces: `pub struct SecretString(String)` with `pub fn new(s: impl Into<String>) -> Self`, `pub fn expose(&self) -> &str` (Task 1's `.expose()` call depends on this exact name), custom `Debug` (redacts), and passthrough `Serialize`/`Deserialize` (as a newtype deriving neither directly — see Step 1).

- [ ] **Step 1: Write the failing redaction test**

In `src/types.rs`, in its existing `#[cfg(test)] mod tests` (or create one if none exists — check first with `grep -n "mod tests" src/types.rs`):

```rust
#[test]
fn secret_string_redacts_debug_but_round_trips_through_serde() {
    let secret = SecretString::new("hunter2");
    assert_eq!(format!("{secret:?}"), "SecretString(\"[redacted]\")");
    assert!(!format!("{secret:?}").contains("hunter2"));

    let json = serde_json::to_string(&secret).expect("serialize");
    assert_eq!(json, "\"hunter2\"", "Serialize must pass the real value through");
    let round_tripped: SecretString = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(round_tripped.expose(), "hunter2");
}

#[test]
fn email_config_debug_never_contains_the_password() {
    let config = EmailConfig {
        smtp: SmtpConfig {
            host: "smtp.example.com".to_string(),
            port: 587,
            username: "user@example.com".to_string(),
            password: SecretString::new("hunter2"),
            use_tls: true,
            use_ssl: false,
        },
        imap: ImapConfig {
            host: "imap.example.com".to_string(),
            port: 993,
            username: "user@example.com".to_string(),
            password: SecretString::new("hunter2"),
            use_ssl: true,
        },
    };
    assert!(!format!("{config:?}").contains("hunter2"));
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --lib secret_string_redacts_debug_but_round_trips_through_serde -- --exact`
Expected: FAIL to compile — `SecretString` does not exist.

- [ ] **Step 3: Implement `SecretString`**

In `src/types.rs`:

```rust
/// A credential value that redacts on `Debug` and nothing else. `Serialize`/`Deserialize` pass
/// the real value through unchanged: `Config::to_file`/`from_file` (`config.rs`) round-trip a
/// whole `Config`, including this value, to and from a TOML file the operator controls, and a
/// redacted round-trip would silently corrupt every saved credential. This wrapper protects
/// against accidental exposure via `Debug`/logging ONLY -- it does nothing to stop a generic
/// `Serialize` sink (e.g. a struct that serializes itself into a cache or telemetry payload) from
/// leaking the real value if `SmtpConfig`/`ImapConfig` is ever routed through one; nothing at the
/// type level prevents that, by design, because `Config::to_file` needs the opposite behavior.
#[derive(Clone, PartialEq, Eq)]
pub struct SecretString(String);

impl SecretString {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for SecretString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("SecretString").field(&"[redacted]").finish()
    }
}

impl Serialize for SecretString {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for SecretString {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        Ok(Self(String::deserialize(deserializer)?))
    }
}
```

- [ ] **Step 4: Change `SmtpConfig`/`ImapConfig`'s `password` field type**

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmtpConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: SecretString,
    pub use_tls: bool,
    pub use_ssl: bool,
}
```

(same change to `ImapConfig`). The struct's own `#[derive(Debug, ...)]` now calls `SecretString`'s custom `Debug`, so the derived `Debug` for `SmtpConfig`/`ImapConfig`/`EmailConfig`/`Config` all redact automatically — no other struct needs a manual `Debug` impl.

- [ ] **Step 5: Fix every construction site**

Run: `cargo build --lib 2>&1 | grep "expected.*SecretString"` to find every `password: "...".to_string()` literal that now needs `SecretString::new("...")` instead (`config.rs`'s `default_for_entity`/`gmail_config`, `email.rs`'s tests, `email_simple.rs`'s tests — though Task 1 Step 9 already deletes that file; if Task 1 hasn't landed yet in your branch, update it too, then let Task 1's own deletion remove it).

- [ ] **Step 6: Extend the config round-trip test with the password assertion this plan's spec review found missing**

In `src/config.rs`'s `test_config_file_operations`:

```rust
#[test]
fn test_config_file_operations() {
    let config = Config::default_for_entity("Test", "tool");

    let temp_file = NamedTempFile::new().unwrap();

    assert!(config.to_file(temp_file.path()).is_ok());

    let loaded_config = Config::from_file(temp_file.path()).unwrap();
    assert_eq!(config.entity.local_name, loaded_config.entity.local_name);
    // Previously unchecked: a blanket-redact SecretString would have passed this test's other
    // assertion while silently corrupting every saved credential. See this plan's spec review.
    assert_eq!(
        config.email.smtp.password.expose(),
        loaded_config.email.smtp.password.expose(),
        "the real password must survive a save/load round-trip"
    );
}
```

- [ ] **Step 7: Run all new/changed tests**

Run: `cargo test --lib secret_string_redacts_debug_but_round_trips_through_serde email_config_debug_never_contains_the_password test_config_file_operations`
Expected: PASS, all three.

- [ ] **Step 8: Commit**

```bash
git add src/types.rs src/config.rs
git commit -m "feat: SecretString credential wrapper -- redacts Debug, round-trips through Serialize"
```

## Task 3: Relay-out and external modes

**Files:**
- Modify: `src/transport/email_unified.rs`
- Test: `tests/transport_repairs.rs`

**Interfaces:**
- Consumes: `EmailTransportImpl` (Task 1); `email_server::connectivity::{ConnectivityDetector, ConnectivityAssessment, ServerRecommendation}` (existing, unused until now); `async-imap` (new dependency — add to `Cargo.toml`, confirm current version per CireSnave's standing dependency rule).
- Produces: `EmailTransportImpl` gains a `mode: EmailMode` field (`enum EmailMode { Direct, RelayOut { relay_host: String }, External { imap_host: String } }`, set at construction from `ConnectivityDetector::assess_connectivity()`'s result) and dispatches `send_message`/`receive_raw` on it.

- [ ] **Step 1: Write the failing test for relay-out receive**

```rust
/// Relay-out/external mode: this transport cannot run its own inbound listener, so receive_raw
/// polls a configured IMAP mailbox instead -- our own SynapseImapServer on loopback, pre-seeded,
/// standing in for "the external mailbox already has mail" (spec §7: no live external provider in
/// any test).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn email_relay_out_mode_receives_via_imap_polling() {
    // Implementer: seed a SynapseImapServer's message store directly (bypassing SMTP entirely,
    // since this test is about the IMAP-receive path in isolation), start it on a free port,
    // configure an EmailTransportImpl in RelayOut mode pointed at that port, and assert
    // receive_raw's inbox contains the seeded SecureMessage after polling. This plan's research
    // did not trace SynapseImapServer's IMAP protocol handler in enough detail to hand over exact
    // wire-level test code here -- read imap_server.rs's `handle_connection` before writing this
    // test's assertions, and confirm whether "seed the store, then poll over the real IMAP
    // protocol" or "seed the store, call a lower-level fetch method directly" is the right layer
    // to test at, given what that file's public surface actually exposes.
}
```

- [ ] **Step 2: Confirm `async-imap`'s current version, add the dependency**

Run: `cargo search async-imap` (or check crates.io directly) for the current version; it is already an optional dependency behind the `email` Cargo feature (`Cargo.toml`) — per CireSnave's standing rule (dependencies on their most recent versions) and this contract's established pattern (QUIC added `quinn`/`rcgen` as regular, non-optional dependencies rather than feature-gating a real transport), make `async-imap` (and `lettre`, already used by Task 1) regular dependencies, not optional behind `email`. State this explicitly as a plan decision: every other `TransportType` in this crate is unconditionally compiled, and gating email behind an opt-in feature would make it the only transport requiring a non-default build flag to exist at all -- inconsistent with the uniform factory-registration pattern in `transport/mod.rs`.

- [ ] **Step 3: Implement mode selection at `start()`**

```rust
pub enum EmailMode {
    Direct,
    RelayOut { relay_host: String, imap_host: String, imap_port: u16 },
    External { imap_host: String, imap_port: u16 },
}
```

Extend `EmailTransportImpl::new` to call `ConnectivityDetector::default().with_bind_scope(bind_scope).assess_connectivity().await?` and match on the resulting `ServerRecommendation` to set `self.mode`. `send_message` in `RelayOut`/`External` mode uses `relay_host`/the external provider's SMTP host as the `lettre::SmtpTransport::relay` target instead of dialing the recipient's own host directly.

- [ ] **Step 4: Implement IMAP-polling receive for `RelayOut`/`External` modes**

`receive_raw` in these modes connects via `async-imap`, selects `INBOX`, searches for unseen messages, and for each: fetches the body, `serde_json::from_slice::<SecureMessage>`s it (same wire format as Task 1's SMTP path — do not invent a second parsing path), and pushes it into `inbox`. A message that fails to parse increments a receive-failure counter (Task 5) and is skipped, not fatal — matching NAT traversal's handling of unparseable datagrams.

- [ ] **Step 5: Run the test, iterate against the implementer note in Step 1**

- [ ] **Step 6: Commit**

## Task 4: Size limits & backpressure

**Files:**
- Modify: `src/transport/email_unified.rs`
- Test: `tests/transport_repairs.rs`

**Interfaces:**
- Consumes: `EmailTransportImpl` (Tasks 1, 3).
- Produces: `EmailTransportFactory::validate_config` refuses unparseable/zero `max_message_size`/`max_queued_bytes`; `send_message` refuses oversize messages as `SynapseError::MessageRefused` before any SMTP handshake.

- [ ] **Step 1: Measure the adversarial parsing factor for this transport's actual path**

Per the spec's §6 (and Global Constraints), do not assume TCP's measured `f≈18`/`12` factor applies here. Write a small measurement harness (mirroring whatever method `tcp_unified.rs`'s module doc describes — a counting allocator over representative JSON shapes) against `EmailTransportImpl::send_message`'s actual serialize-then-base64/JSON-body path, and record the result in this file's module doc, stating it as measured with the method used, per CLAUDE.md §7.

- [ ] **Step 2: Write the failing oversize-refusal test**

```rust
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn email_refuses_at_send_a_message_over_its_limit() {
    // Implementer: mirror tcp_refuses_at_send_a_message_over_its_limit's shape exactly
    // (tests/transport_repairs.rs) -- construct an EmailTransportImpl with a small
    // max_message_size config value, attempt to send an oversize message, and assert both that
    // the error is SynapseError::MessageRefused (not TransportError) and that no SMTP connection
    // was attempted (e.g. via a config pointing at a host that would fail loudly if dialed).
}
```

- [ ] **Step 3: Implement the refusal, config validation, and byte-budget queue**

Add `max_message_size`/`max_queued_bytes` config keys to `EmailTransportFactory::default_config`/`validate_config`, following `tcp_unified.rs`'s exact validation pattern (refuse unparseable or zero, refuse `max_queued_bytes` below `max_message_size` or above `u32::MAX`). In `send_message`, check serialized size against `max_message_size` before constructing the `lettre::Message` or touching the SMTP client, returning `SynapseError::MessageRefused`. `receive_raw`'s Direct-mode path (the SMTP server) needs the same check applied server-side before `store_message` accepts a `DATA` body, and a semaphore-based byte-budget queue gating how many received-but-undrained messages may accumulate, matching `tcp_unified.rs`'s `queue_budget` pattern.

- [ ] **Step 4: Run the test, then the full suite so far**

Run: `cargo test --test transport_repairs email_ -- --test-threads=1`

- [ ] **Step 5: Commit**

## Task 5: Honest capabilities & metrics

**Files:**
- Modify: `src/transport/email_unified.rs`
- Test: `tests/transport_repairs.rs`

**Interfaces:**
- Consumes: `EmailTransportImpl` (Tasks 1, 3, 4); `abstraction::UnmeasuredMetric` (existing, `#54`).
- Produces: finalized `capabilities()`/`metrics()`/`estimate_metrics()`, replacing Task 1's Step 7 placeholders.

- [ ] **Step 1: Add real `AtomicU64` counters**

```rust
messages_sent: AtomicU64,
bytes_sent: AtomicU64,
send_failures: AtomicU64,
messages_received: AtomicU64,
bytes_received: AtomicU64,
receive_failures: AtomicU64,
```

Wire into `send_message` (increment on real success/failure, matching Task 1's NAT-traversal-precedent pattern of only counting network-touching failures, never local refusals) and both receive paths (Direct's `drain_messages` call site, and Task 3's IMAP fetch loop).

- [ ] **Step 2: Decide `average_latency_ms`: instrument for real, or declare unmeasured**

Per the Global Constraints, do not estimate. If Step 1's measurement work makes a real send-to-`250-OK` timing trivial to add (it should — `send_message` already has a `start = Instant::now()`), add it as a real running average, matching `tcp_unified.rs`'s exact formula. If not instrumented, `capabilities()` must include `unmeasured_metrics: vec![UnmeasuredMetric::AverageLatency]` and `metrics()` must return `average_latency_ms: 0` — never a plausible-looking constant.

- [ ] **Step 3: `reliability_score`**

```rust
let messages_sent = self.messages_sent.load(Ordering::Relaxed);
let send_failures = self.send_failures.load(Ordering::Relaxed);
let attempts = messages_sent + send_failures;
let reliability_score = if attempts == 0 { 1.0 } else { messages_sent as f64 / attempts as f64 };
```

QUIC's exact formula (Global Constraints) — do not invent a second one.

- [ ] **Step 4: Write the metrics test**

```rust
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn email_metrics_counts_real_sends() {
    // Implementer: mirror quic_metrics_counts_real_sends_and_receives's shape (transport_repairs.rs)
    // -- construct a raw EmailTransportImpl, send one message, assert messages_sent == 1,
    // bytes_sent > 0, send_failures == 0, reliability_score == 1.0. Assert
    // capabilities().unmeasured_metrics matches whatever Step 2 decided (either empty, if latency
    // was instrumented, or [UnmeasuredMetric::AverageLatency] if not) -- do not leave this
    // assertion out; it is the test that would have caught NAT traversal's board-item-53 defect had
    // it existed for that transport from the start.
}
```

- [ ] **Step 5: Run, commit**

## Task 6: Verification

**Files:** none (verification only, plus `CAPABILITY_INVENTORY.md` if this slice fixes/supersedes a documented finding there).

- [ ] **Step 1: Mutation check**

Predict the shape of a plausible mutation (e.g., reverting Task 1's `store_message` fix to hand-pick `encrypted_content` again, or Task 4's `MessageRefused` check), apply it, run the specific test(s) that should catch it under a hard execution timeout (`timeout 120 cargo test ...` — the lesson this session's earlier work paid for twice, per the transport-contract ledger), confirm exactly the expected test(s) fail, then revert.

- [ ] **Step 2: Full suite in a target directory that has only built current source**

Run: `cargo build --tests && cargo test --lib --tests` in a directory whose `target/` has not built any other branch's source (the practical reading of "fresh" this session's earlier work settled on — a literally-new directory risks unrelated link failures unrelated to this change).

- [ ] **Step 3: Windows Firewall event count, with a positive control**

Record the Windows Firewall event 2097 count for the test run's window; as a positive control, confirm the same query finds a nonzero count for a window known to include firewall-relevant activity (e.g. an earlier transport's test run), so an absence here is evidence of nothing, not proof of a clean run.

- [ ] **Step 4: fmt and clippy**

Run: `cargo fmt --check` then `cargo clippy -- -D warnings` (CI's exact invocation) **and** `cargo clippy --lib --tests --all-targets -- -D warnings` (per issue #53's documented gap in what CI actually covers — this slice's own new test code must not add to the unlinted region issue #53 describes).

- [ ] **Step 5: Docs and breaking-change list**

Update `CAPABILITY_INVENTORY.md` for any email-specific finding this slice fixes or supersedes (the `store_message`/shared-message-store deviations noted in this plan's header are exactly this kind of finding). Breaking changes for the eventual PR body: `TransportType::Email`'s factory now constructs a real transport instead of `SimpleEmailTransport`'s permanent refusal; `SmtpConfig`/`ImapConfig`'s `password` field type changes from `String` to `SecretString` (source-breaking for any external code constructing these structs directly); `SynapseSmtpServer::new`'s signature gains a `message_store` parameter (source-breaking for any external code constructing one directly).

- [ ] **Step 6: Final commit and PR**

No version bump — synapse gets none until 2.0.0 publishes, which needs CireSnave's explicit approval.

## Self-Review

**Spec coverage:** §1a (mode/reuse/credential/proxy decisions) → Tasks 1/3. §2 (six-task staging) → this plan's six tasks, same order. §3 (retracted authentication claim) → Global Constraints, enforced by "no SMTP-envelope-based trust" appearing nowhere in any task's code. §4 (wire format, delivery claim) → Task 1 Steps 4/7, Global Constraints. §5 (credential handling, narrowed protection claim) → Task 2 entire. §6 (capabilities/metrics) → Task 5 entire, Task 1 Step 7 as placeholder. §7 (no live external provider in tests) → every task's test design explicitly uses our own `email_server` components on loopback. §8 (open questions) → the `Config::to_file` serialization question was answered in the spec itself before this plan was written; version pins are Task 3 Step 2's job.

**Placeholder scan:** Task 1 Step 6's `email_config_from_map` and Step 7's `receive_raw` recipient-key lookup were both initially left unresolved during drafting; both were then closed by reading the actual files (`types.rs`'s full `EmailConfig`/`SmtpConfig`/`ImapConfig` definition, and `email_server::auth`'s `is_authorized_recipient` plus `smtp_server.rs`'s `extract_email_from_rcpt_to`, confirming the message store is keyed by full recipient email address, not by port) — both now have real code, not a description of what to go find. Two genuine gaps remain, stated explicitly rather than papered over: Task 3's IMAP-polling test (Step 1) needs `imap_server.rs`'s `handle_connection` traced before its exact assertions can be written (this plan's research covered `smtp_server.rs` in depth but not `imap_server.rs`'s wire-level detail), and Task 4's adversarial-parsing-factor measurement (Step 1) is inherently a "measure it when the code exists" step, not something a plan can pre-compute. Both are flagged with the exact file to read, not left as silent gaps.

**Type consistency:** `EmailTransportImpl`/`EmailTransportFactory` names and the `Transport`/`TransportReceive`/`TransportFactory` method signatures are held constant from Task 1 through Task 5. `SecretString::expose()` (Task 2) is the exact method Task 1's Step 6 code calls, written before Task 2 in task order but consistent with it — flagged in Task 1's own implementer note as a sequencing consideration.
