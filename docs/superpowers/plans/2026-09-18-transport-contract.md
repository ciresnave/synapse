# Transport Contract Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** One public receive that only ever yields verified messages; every delivery status a claim
with a precondition; `protocol_version` on the wire; four transports that genuinely send and receive;
and the dead code gone.

**Architecture:** Two pull requests from one plan, because the reviews on this project have
consistently found their serious bugs in large diffs. **PR A (Tasks 1–6)** is the contract itself:
deletions, honesty, the sealed receive, `protocol_version`, and the email validator that turns `main`
green. **PR B (Tasks 7–10)** is the four repairs, each proven by an end-to-end test over a real loopback
socket. Task 11 verifies and opens each PR.

**Tech Stack:** Rust, `tokio`, `tokio-tungstenite` (already a dependency; moved from 0.27 to the latest, 0.30, in Task 8), `reqwest` (already),
and **one new dependency, `axum`**, for a real HTTP server — `Cargo.toml` has none today. Per
CireSnave's standing rule it is added at its latest published version, under the existing `http`
feature.

**Spec:** `docs/superpowers/specs/2026-09-18-transport-contract-design.md` (approved 2026-09-18).

## Global Constraints

- **Branch:** `design/transport-contract` (the spec branch) for PR A; `feat/transport-repairs`,
  stacked on it, for PR B. Held for the PM's pass.
- **Every `DeliveryConfirmation::Delivered` construction carries a comment naming the protocol event
  it rests on.** A transport never constructs `Acknowledged` or `Expired`; only the manager does.
- **`protocol_version`:** `u16`, value `1`, inside `canonical_input`; an unsupported version is
  `Unverifiable { UnsupportedVersion }`, never a parse error.
- **Nothing is `#[ignore]`d.** A test that can pass without reaching its assertion is rewritten.
- **Tests bind loopback only** (`127.0.0.1`). A wildcard bind raises a Windows Firewall prompt on this
  machine and is a hard error.
- **No private key material in any error, log line or `Debug`.**
- Deleting a file is only safe once nothing refers to it: **build after every deletion group.**
- `CARGO_TARGET_DIR=C:/Projects/synapse/target` for every cargo command.
- Checks: `cargo test --no-fail-fast`; `cargo fmt -- --check`; `cargo clippy -- -D warnings`, including
  every new or changed test target. **From Task 2 on, the failing set is EMPTY** — Task 2 fixes the one
  test that has been red on `main` throughout.
- New `.rs` files start with `// SPDX-License-Identifier: MIT OR Apache-2.0`.
- No version bump: `Cargo.toml` already says 2.0.0 and the publish is held.

---

## PR A — the contract

### Task 1: Delete the dead code

**Files:** delete `src/transport/{tcp.rs,tcp_enhanced.rs,udp.rs,quic.rs,nat_traversal_clean.rs,email_enhanced.rs,email_unified.rs,websocket.rs,mdns.rs,production_http.rs}` and `src/wasm/{browser.rs,crypto.rs,storage.rs,webrtc.rs,websocket.rs,worker.rs}`; modify `src/transport/mod.rs` (the old `Transport` trait and its commented-out module lines), `src/transport/abstraction.rs` (`create_standard_factories`), `src/transport/providers.rs` (the `impl super::Transport for MockTransport` block), `tests/loopback_by_default.rs` (its `NOT_COMPILED` list).

**Why these are safe:** a survey verified at `bbd4bf0` that none of the transport files is declared as a module, that `production_http` is never constructed, that `create_standard_factories` is never called, and that none of the six WASM files is declared. The old trait's only live implementor is the mock's second `impl`, which exists solely to satisfy that trait.

**Keep `src/transport/quic.rs` out of the deletion until the QUIC slice?** No — delete it. It cannot
compile (no `quinn` dependency, an old `quinn` API, renamed circuit-breaker methods), so it is not a
foundation. The QUIC slice will read it from git history if it wants the reference.

- [ ] **Step 1: Write the failing test** — a source scan in a new `tests/dead_code_is_gone.rs`:

```rust
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Transport contract, Task 1: the dead transports are deleted, not merely unused.

const DELETED: &[&str] = &[
    "src/transport/tcp.rs", "src/transport/tcp_enhanced.rs", "src/transport/udp.rs",
    "src/transport/quic.rs", "src/transport/nat_traversal_clean.rs", "src/transport/email_enhanced.rs",
    "src/transport/email_unified.rs", "src/transport/websocket.rs", "src/transport/mdns.rs",
    "src/transport/production_http.rs", "src/wasm/browser.rs", "src/wasm/crypto.rs",
    "src/wasm/storage.rs", "src/wasm/webrtc.rs", "src/wasm/websocket.rs", "src/wasm/worker.rs",
];

#[test]
fn the_dead_transport_files_are_deleted() {
    let root = env!("CARGO_MANIFEST_DIR");
    for path in DELETED {
        assert!(!std::path::Path::new(root).join(path).exists(), "{path} should be deleted");
    }
    // Control: a live transport file still exists, so the check is looking in the right place.
    assert!(std::path::Path::new(root).join("src/transport/udp_unified.rs").exists());
}

#[test]
fn there_is_one_transport_trait() {
    let module = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/transport/mod.rs")).unwrap();
    assert!(!module.contains("pub trait Transport"), "the old Transport trait must be deleted");
    let abstraction =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/transport/abstraction.rs")).unwrap();
    assert!(abstraction.contains("pub trait Transport"), "control: the kept trait is still defined");
}
```

- [ ] **Step 2:** Run `cargo test --test dead_code_is_gone`; confirm both fail.
- [ ] **Step 3:** Delete in three groups — the uncompiled transport files, then the old trait with the mock's second impl, then the WASM files and `create_standard_factories` — running `cargo build --all-targets` after each group and fixing any reference the build names. Update `tests/loopback_by_default.rs`'s `NOT_COMPILED` list to drop entries for files that no longer exist, so that test keeps meaning what it says.
- [ ] **Step 4:** `cargo test --no-fail-fast`: the failing set must still be exactly `{test_transport_error_handling}` — this task deletes, it changes no behaviour. Then fmt and clippy.
- [ ] **Step 5:** Commit: `refactor(transport)!: delete the old Transport trait and every transport that never compiled`.

### Task 2: The email validator, honest email, and a green `main`

**Files:** `src/transport/email_simple.rs`, `tests/transport_error_handling_test.rs`

`email_simple` touches no network in either direction, yet reports `Sent`. Until the email slice
builds it properly it must **refuse to operate** rather than pretend. It still *constructs*, because
the address validator is real and is what `test_transport_error_handling` exercises.

- One validator, `fn valid_address(address: &str) -> bool`, used by `can_reach`, `test_connectivity`
  and `send_message`: exactly one `@`, a non-empty local part, and a domain containing a `.` with
  non-empty labels either side.
- For a **malformed** address: `test_connectivity` returns `connected: false`; `send_message` returns
  `Err(SynapseError::TransportError("invalid email address"))`.
- For a **well-formed** address: `send_message`, `receive_messages` and `test_connectivity` return
  `Err(SynapseError::TransportError("email transport is not implemented yet; it arrives in the email slice"))`.
  Never `Sent`, never `connected: true`.

- [ ] **Step 1: Write the failing tests** in `src/email_simple.rs`'s test module, replacing
  `test_email_format_validation` (which only ever tested the strict `can_reach` with addresses that had
  no `@`, and so could never see the bug):

```rust
    #[test]
    fn one_validator_governs_every_path() {
        for bad in ["invalid@", "@example.com", "a@@example.com", "a@example", "a@.com", "a@example."] {
            assert!(!valid_address(bad), "{bad} must be refused");
        }
        for good in ["user@example.com", "first.last@mail.example.org"] {
            assert!(valid_address(good), "{good} must be accepted");
        }
    }

    #[tokio::test]
    async fn a_malformed_address_is_refused_by_connectivity_and_send() {
        let transport = SimpleEmailTransport::new(test_config());
        let target = TransportTarget::new("invalid@".to_string());
        assert!(!transport.test_connectivity(&target).await.unwrap().connected);
        let message = SecureMessage::new("invalid@", "me@example.com", b"hi".to_vec(), SecurityLevel::Public);
        let err = transport.send_message(&target, &message).await.unwrap_err();
        assert!(err.to_string().contains("invalid email address"), "{err}");
    }

    #[tokio::test]
    async fn a_well_formed_address_is_refused_honestly_rather_than_faked() {
        let transport = SimpleEmailTransport::new(test_config());
        let target = TransportTarget::new("user@example.com".to_string());
        let message = SecureMessage::new("user@example.com", "me@example.com", b"hi".to_vec(), SecurityLevel::Public);
        let err = transport.send_message(&target, &message).await.unwrap_err();
        assert!(err.to_string().contains("email slice"), "{err}");
        assert!(transport.receive_messages().await.is_err());
        // The transport must never claim a connection it did not make.
        assert!(transport.test_connectivity(&target).await.is_err());
    }
```

  Use whatever constructor and config helper the file's existing tests use for `test_config()`.

- [ ] **Step 2:** In `tests/transport_error_handling_test.rs`, remove the `Ok(None) | Err(_)` arm that
  treats "the transport failed to construct" as a pass, and the inner `Err(_) => println!(...)` arm that
  treats any error as a pass. Assert instead that construction succeeds and that
  `test_connectivity("invalid@")` returns `connected: false`. A test that can pass without reaching its
  assertion is not a test.
- [ ] **Step 3:** Run; confirm the new tests fail. Implement. Run again.
- [ ] **Step 4: The acceptance bar changes here.** `cargo test --no-fail-fast` must now report an
  **empty failing set** — `test_transport_error_handling` passes for the first time. Say so in the
  commit message.
- [ ] **Step 5:** Commit: `fix(email): one address validator, and refuse rather than fake a send — main goes green`.

### Task 3: QUIC refuses to construct

**Files:** `src/transport/quic_unified.rs` (replaced), `src/transport/abstraction.rs` (`QuicTransportFactory`)

Delete the fabricated `QuicTransportImpl`. `QuicTransportFactory::create_transport` returns
`Err(SynapseError::TransportError("QUIC is not implemented yet; it arrives in the QUIC slice"))`.
`quic_unified.rs` shrinks to nothing and is deleted; keep `TransportType::Quic` so the QUIC slice
has somewhere to land.

- [ ] Test: constructing a QUIC transport through the factory errors, and the message names the QUIC
  slice. Control: the UDP factory, through the same call, succeeds.
- [ ] Commit: `fix(quic)!: refuse to construct instead of fabricating deliveries`.

### Task 4: The sealed receive

**Files:** `src/transport/abstraction.rs`, `src/transport/manager.rs`, every transport implementation

Split `abstraction::Transport`: `receive_messages` moves to a supertrait that is public to
*implement* but not to *call* from outside the crate. The standard sealed pattern:

```rust
mod private {
    /// Only this crate can name this type, so only this crate can call methods that take it.
    pub struct Token(());
    impl Token {
        pub(crate) fn new() -> Self {
            Token(())
        }
    }
}

#[async_trait]
pub trait TransportReceive: Send + Sync {
    /// Raw messages from the wire. Callable only by `TransportManager`, which verifies them;
    /// applications receive through `TransportManager::receive_messages`.
    async fn receive_raw(&self, token: private::Token) -> Result<Vec<IncomingMessage>>;
}
```

`Transport` gains `TransportReceive` as a supertrait; `receive_messages` is removed from `Transport`.
`TransportManager::receive_messages` calls `transport.receive_raw(private::Token::new())`. A
downstream crate can implement a transport (it receives the token as a parameter and ignores it) but
cannot construct a `Token`, so it cannot call `receive_raw`.

- [ ] **Test:** a doc-test on `TransportReceive` marked `compile_fail`, showing that code outside the
  crate cannot call `receive_raw`. Pair it with a doc-test that compiles, showing an external
  implementation IS possible — so the `compile_fail` is failing for the right reason, not because the
  snippet is broken.
- [ ] Commit: `feat(transport)!: only the manager can receive from a transport`.

### Task 5: `protocol_version` on the wire

**Files:** `src/types.rs` (`SecureMessage`), `src/sender_auth.rs` (`canonical_input`, a new
`UnverifiableReason::UnsupportedVersion`, `verify_at`), `src/replay.rs` and `src/mcp_server.rs` (one
match arm each), `src/transport/mdns_enhanced.rs` and `src/transport/discovery.rs` (the TXT record)

- `SecureMessage.protocol_version: u16`, `#[serde(default = "default_protocol_version")]` returning `1`.
  `pub const PROTOCOL_VERSION: u16 = 1;` and `pub const SUPPORTED_PROTOCOL_VERSIONS: &[u16] = &[1];`.
- `canonical_input` puts it **immediately after the domain tag**, as two big-endian bytes, so a relay
  that rewrites it invalidates the signature.
- `verify_at` checks it **first**, before any other interpretation: an unsupported version is
  `Unverifiable { UnsupportedVersion }`, reason string `unsupported_protocol_version`.
- The mDNS TXT key `"version"` becomes `"synapse_protocol"`, value `PROTOCOL_VERSION.to_string()`, in
  both `discovery.rs` and `mdns_enhanced.rs` — ending the `"1.0"` / `"1.1.0"` disagreement.

- [ ] **Tests:** a version-2 message is `Unverifiable { UnsupportedVersion }` and the knock record says
  `unsupported_protocol_version`; a signed version-1 message whose version a relay rewrites to 2 fails
  signature verification; both mDNS paths advertise `synapse_protocol=1`. Control: an unmodified
  version-1 message verifies.
- [ ] Commit: `feat(wire)!: a signed protocol_version, refused by name when unsupported`.

### Task 6: Honest delivery claims

**Files:** `src/transport/websocket_unified.rs`, `src/transport/mdns_enhanced.rs`,
`src/transport/discovery.rs`, `src/transport/email_simple.rs`, `src/transport/providers.rs`, a new
`tests/delivery_claims.rs`

After Tasks 1 and 3, the remaining `Delivered` constructions are WebSocket (claimed off a bare TCP
connect with the handshake skipped), mDNS (a simulated send) and the test mock. WebSocket's
`send_message` **refuses** here, with an error saying WebSocket send is not implemented yet, until
Task 8: `Sent` requires a write to a real socket that returned success (spec §4), and today's path
writes nothing — the connect is a bare TCP connect with no handshake, and
`send_via_existing_connection` ignores its data. mDNS's `send_message`, and auto-discovery's
(`discovery.rs`), return an error — they are discovery, not messaging — and their estimates and
metrics stop advertising them as available or reliable. Email's fake `send_connection_offer` goes, so
the trait default's refusal applies. The mock keeps `Delivered` with a comment saying it is a test
double.

- [ ] **Test:** a source scan over `src/transport/` asserting every `DeliveryConfirmation::Delivered`
  construction has a comment within three lines naming its protocol event (rule 1), that no transport
  file constructs `Acknowledged` or `Expired` (rule 2), and that no file aliases the enum or imports
  its variants, which would hide a construction from rules 1 and 2 (rule 3). Control: the scan finds
  the manager's `Acknowledged` and `Expired` constructions when pointed at `manager.rs`, counting only
  **production** constructions — those outside its `#[cfg(test)] mod` block, so a test's expected
  value cannot satisfy the control; any other `cfg` naming `test` fails the scan rather than being
  counted as production.
- [ ] Commit: `fix(transport): no transport claims a delivery it did not observe`.
- [ ] **Open PR A** after Task 11's verification steps, into `main`.

---

## PR B — the repairs

Each repair ends in the same shape of test, so write the helper once.

### Task 7: TCP, and the shared end-to-end helper

**Files:** `src/transport/mod.rs`, `src/transport/abstraction.rs`, delete `src/transport/tcp_simple.rs`,
new `tests/transport_repairs.rs`

Re-export `tcp_unified::TcpTransportFactory` explicitly, delete `abstraction.rs`'s shadowing
`TcpTransportFactory` and `tcp_simple.rs`. `tcp_unified`'s double bind is already fixed.

The helper every repair test uses, written in full once:

```rust
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Transport contract, PR B: every repaired transport carries a verified message end to end.

use synapse::CryptoManager;
use synapse::transport::{TransportManagerBuilder, TransportType, TransportFactory};
use synapse::types::{SecureMessage, SecurityLevel};

/// Build a manager with exactly one transport enabled, bound to loopback on `port`.
async fn node(
    kind: TransportType,
    factory: Box<dyn TransportFactory>,
    port: u16,
    store: synapse::sender_auth::TrustStore,
) -> synapse::transport::TransportManager {
    let mut config = std::collections::HashMap::new();
    config.insert("bind_port".to_string(), port.to_string());
    let mut builder = TransportManagerBuilder::new();
    for other in [TransportType::Tcp, TransportType::Udp, TransportType::Http,
                  TransportType::WebSocket, TransportType::Email, TransportType::AutoDiscovery] {
        if other != kind {
            builder = builder.disable_transport(other);
        }
    }
    let manager = builder.transport_config(kind, config).trust_store(store).build();
    manager.register_factory(factory).await.expect("factory registers");
    tokio::time::timeout(std::time::Duration::from_secs(5), manager.start())
        .await
        .expect("start returns")
        .expect("start succeeds");
    manager
}

/// A certificate from `account` for `holder`, valid for an hour. Copied from
/// tests/agent_certificates.rs, because a test binary cannot import another's helpers.
fn cert_for(account: &ed25519_dalek::SigningKey, holder: &CryptoManager, id: &str) -> synapse::certificate::AgentCertificate {
    let now = chrono::Utc::now();
    synapse::certificate::AgentCertificate::sign(
        synapse::certificate::AgentCertificate {
            version: 1,
            serial: [9u8; 16],
            issuer_key_id: synapse::sender_auth::key_id(&account.verifying_key().to_bytes()),
            subject_label: "repair-test".to_string(),
            subject_global_id: id.to_string(),
            subject_signing_key: holder.public_key_bytes().expect("public key"),
            subject_sealing_key: [0u8; 32],
            not_before: now - chrono::Duration::minutes(1),
            not_after: now + chrono::Duration::hours(1),
            permissions: vec![synapse::certificate::Permission::Send],
            may_delegate: 0,
            signature: [0u8; 64],
        },
        account,
    )
}

/// Send one signed message from Alice to Bob over `kind`, and return what Bob's manager delivers.
/// Asserts it arrived Verified: the point of every repair is that a real message crosses a real
/// socket and comes out of the manager with a verdict.
async fn round_trip(
    kind: TransportType,
    alice_factory: Box<dyn TransportFactory>,
    bob_factory: Box<dyn TransportFactory>,
    alice_port: u16,
    bob_port: u16,
) -> synapse::transport::ReceivedMessage {
    let account = ed25519_dalek::SigningKey::from_bytes(&[42u8; 32]);
    let mut alice = CryptoManager::new();
    alice.generate_keypair().expect("keypair");
    alice.set_certificate_chain(vec![cert_for(&account, &alice, "alice@repair.test")]);

    let mut bob_store = synapse::sender_auth::TrustStore::new();
    bob_store.pin_account_key("alice-account", account.verifying_key().to_bytes());

    let alice_node = node(kind, alice_factory, alice_port, synapse::sender_auth::TrustStore::new()).await;
    let bob_node = node(kind, bob_factory, bob_port, bob_store).await;

    let mut message = SecureMessage::new("bob@repair.test", "alice@repair.test", b"repaired".to_vec(), SecurityLevel::Authenticated);
    alice.sign_secure_message(&mut message).expect("sign");
    let target = synapse::transport::TransportTarget::new("bob@repair.test".to_string())
        .with_address(format!("127.0.0.1:{bob_port}"));
    alice_node.send_message(&target, &message).await.expect("the transport sends");

    for _ in 0..30 {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let mut batch = bob_node.receive_messages().await.expect("receive");
        if let Some(received) = batch.pop() {
            assert!(batch.is_empty(), "exactly one message expected");
            assert!(received.sender.is_verified(), "the message must arrive Verified: {:?}", received.sender);
            assert_eq!(received.incoming.message.message_id, message.message_id);
            return received;
        }
    }
    panic!("no message arrived over {kind:?} within 3 s");
}

/// A free loopback port.
fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}
```

Check the exact names against the code before relying on them — `TransportTarget`'s re-export
path, `send_message`'s signature on `TransportManager`, and whether `node` needs `transport_config`
keyed differently for TCP. Where they differ, follow the code and say so in the report.

- [ ] **Test:** `tcp_carries_a_verified_message_end_to_end` using the helper. Control: before the fix,
  the public `TcpTransportFactory` builds `tcp_simple`, whose receive is always empty — so this test
  must fail against the unfixed code. Run it against `main` first and record that it fails.
- [ ] Commit: `fix(tcp): the public factory builds the TCP transport that can receive`.

#### Breaking changes (unreleased 2.0.0), from Task 7

Quote this list in PR B's body.

- `SynapseError::MessageRefused` is a new variant, and `SynapseError` is not `#[non_exhaustive]`,
  so an exhaustive `match` on it no longer compiles.
- TCP's error for an oversize message changed from `SynapseError::TransportError` to
  `SynapseError::MessageRefused`, and the manager no longer counts it against TCP.
- The text of the manager's "All transports failed" error now carries each transport's reason.
- TCP has new config keys and new defaults: `max_message_size` 1 MiB of serialized JSON,
  `max_concurrent_connections` 64, `max_queued_bytes` 4 MiB, `first_byte_timeout_ms` and
  `idle_timeout_ms` 5 s. An unparseable or zero value, or a `max_queued_bytes` below
  `max_message_size` or above `u32::MAX`, is refused at construction instead of becoming the default.
- `tcp_simple` and the `TcpTransportFactory` in `abstraction.rs` that shadowed `tcp_unified`'s
  were removed; `synapse::transport::TcpTransportFactory` is now `tcp_unified`'s.

### Task 8: WebSocket

**Files:** `src/transport/websocket_unified.rs`, `src/transport/abstraction.rs` (register the factory
path), `tests/transport_repairs.rs`

Fix the double bind the way `tcp_unified` did: `start()` binds once, stores the listener, and the
spawned accept loop uses that same `Arc<TcpListener>` rather than binding again. Replace the skipped
handshake with `tokio_tungstenite::accept_async` on the server and `connect_async` on the client.
`Delivered` only if the peer returns an application-level frame acknowledging it; otherwise `Sent`.
Task 6 left `send_message` refusing; this task replaces the refusal with the real send, and removes
the `#[allow(dead_code)]` markers Task 6 put on the now-unreachable send helpers.
`send_via_existing_connection` ignores its `data` argument (it sleeps 1 ms and returns `Ok`): the
repair must actually write the frame to the connection's stream, which means keeping the stream, not
just a `WebSocketConnection` record, per connection.

- [ ] **Test:** `websocket_carries_a_verified_message_end_to_end`. Control, as run: at `800d294` the
  test fails on PR A's refusal (`send_message` refused before connecting), so it never reaches the
  accept loop and cannot show the double bind; that control was isolated separately (control B).
- [ ] Commit: `fix(websocket): bind once, perform the real handshake, and receive`.

**As built:** one message per connection, as for TCP, rather than a connection kept open per peer: a
kept-open connection needs a stored write half per peer, reconnects and idle expiry, and a write into
a connection whose peer has silently died still "succeeds" into the local buffer, which would weaken
`Sent`. The receipt is `Sent` once the frame is written and flushed; there is no application-level
ack, so never `Delivered`. The limits and keys are TCP's (the module doc of
`websocket_unified.rs` states them), with a handshake timeout in place of TCP's first-byte timeout.
Fix round 1 (review of `22e68c5`): the
receiver closes a connection that sends a Ping, a Pong or any control frame but Close, and caps its
write buffer at 64 KiB (a peer that pinged and never read grew the heap by 4 GiB in 8.4 s, measured);
the sender's write buffer is capped at one frame of the largest message, header included;
`idle_timeout_ms` is a gap between reads, enforced by a reader around the socket, not a deadline on
a whole message; targets with any scheme but `ws`, an empty host, or a missing, zero or invalid port
are refused; a failed `estimate_metrics` probe reports the connection timeout as latency, with
confidence 0.3. The memory bound's first term is now taken from a measurement, not TCP's formula:
about `C × (2.625M + 75 KiB) + B × f`, about 245 MiB at peak with the defaults and `f` = 18.
The `2.625M` (a fragmented message's transient peak; 2.06 MiB held for one whole frame at `M` =
1 MiB) and 11 KiB per connection after the handshake are measured (counting allocator, release
build, Windows); the 64 KiB write-buffer cap and the scaling to other `M` are derived. `2.625M` is
the largest of the four frame shapes measured, not a proven maximum. As first built it stated
`C × (4M + 80 KiB)`, about 333 MiB, read from tungstenite's code, which was high.
Polish (review of `c604d57`): a target is validated by the parser that dials it (`http::Uri`,
through `IntoClientRequest`), whose host and port must be the ones checked, so what is accepted is
exactly what is dialled; `\`, `%` and non-ASCII characters in the host, a fragment, and a numeric
host that is not a dotted quad (`0x7f.1`, which the `url` crate read as 127.0.0.1) are refused;
the scheme matches in any case; a missing port is reported before a colon in the host. TCP gained
a deterministic unit test for its queue lock, which fails on every run against the old `try_lock`
code, where the concurrency test in `transport_repairs.rs` passed about 1 run in 11.

#### Breaking changes (unreleased 2.0.0), from Task 8

Quote this list in PR B's body, with Task 7's.

- WebSocket's `send_message` sends instead of refusing. Its receipt claims `Sent`, never `Delivered`.
- The wire format is one JSON `SecureMessage` in one binary frame per connection, after a real
  HTTP-upgrade handshake; the receiver drops a text message, and a second message on one connection.
- WebSocket's error for an oversize message changed from `SynapseError::TransportError` to
  `SynapseError::MessageRefused`, returned before connecting, and the manager no longer counts it
  against WebSocket.
- `max_message_size`'s default fell from 16 MiB to 1 MiB of serialized JSON, and it now also sets
  tungstenite's `max_message_size` and `max_frame_size`. New keys: `max_concurrent_connections` 64,
  `max_queued_bytes` 4 MiB, `handshake_timeout_ms` and `idle_timeout_ms` 5 s.
  `connection_timeout_ms` (still 30 s) now bounds the connect and handshake, and then the write.
- An unparseable or zero limit or timeout, a `local_port` that is not a port number, or a
  `max_queued_bytes` below `max_message_size` or above `u32::MAX`, is refused at construction, and by
  `WebSocketTransportFactory::validate_config`, instead of silently becoming the default.
  `validate_config` used to check `local_port` only. `default_config` returns every key.
- A target must carry an address that is a `ws://` URL or `host:port`, and either must name a
  non-empty host and a port from 1 to 65535. An identifier-only target, a missing port (the default
  was 8080; a `ws://` URL without a port is refused too), port 0 or a port over 65535, an empty
  host, an unbracketed IPv6 host, and any scheme but `ws://` -- `http://`, `https://`, `ftp://`, and
  `wss://` -- are refused; `can_reach` says so without touching the network. `wss://` needs TLS, and
  this build has none. (As first built, `22e68c5` accepted `http://` and `https://` addresses and
  dialled `ws://http://...`; fix round 1 refuses them.)
- The receiver closes a connection, logging at `warn`, as soon as it reads a Ping, a Pong or any
  control frame but Close; it used to answer pings. The wire format never sends them.
- `idle_timeout_ms` is the longest gap between two reads on an inbound connection after its
  handshake, not a deadline for each whole frame: a message whose bytes keep arriving is read up to
  the 30 s connection lifetime. A message cut off by any timeout is logged at `warn`.
- `estimate_metrics` on a failed probe reports the connection timeout as `latency` and a
  `confidence` of 0.3; it reported the time the failure took, with confidence 1.0.
- `start()` sets the status to `Failed` when its bind fails (it used to stay `Starting`), and a second
  `start()` on the same instance is an error.
- `test_connectivity` and `estimate_metrics` perform a real handshake with the target. They report
  connected, available and a round-trip time only when it completes. `metrics()` reports real
  counts, and a `reliability_score` of successful sends over attempts (0 before any attempt).
- `capabilities()` reports the configured `max_message_size`, `encrypted: false`, and the features
  `http_upgrade`, `binary_frames` and `one_message_per_connection`.
- `tokio-tungstenite` moved from 0.27.0 to 0.30.0. No public API names its types. The direct
  `tungstenite` dependency is removed: nothing used it but through `tokio_tungstenite::tungstenite`.

### Task 9: HTTP

**Files:** `Cargo.toml` (`axum`, latest, under the `http` feature), `src/transport/http_unified.rs`,
`tests/transport_repairs.rs`

`start_server` binds a real `axum` server on the configured loopback port with one route,
`POST /synapse/message`, whose handler pushes the decoded message into the queue `receive_raw` drains.
The client's `Sent` stays gated on a 2xx; a 2xx is also the protocol event that permits `Delivered`,
since the peer's HTTP stack confirmed receipt — say so in the comment.

- [ ] **Test:** `http_carries_a_verified_message_end_to_end`. Control, as run: at `9f559ed`, in a
  fresh target directory, the test fails in `node` before any message is sent: "Http reports Running but nothing holds 127.0.0.1:57267; the transport is not listening" (`start_server` bound nothing). The `protocol event` comment is enforced: with it removed, `tests/delivery_claims.rs` fails, flagging `http_unified.rs:1041`.
- [ ] Commit: `fix(http)!: bind a real bounded server, so HTTP can receive`.

**As built:** the ruling on the delivery claim overrides the wording above. The server answers `202`
only after the message is read, has queue budget, is parsed and is in the queue, so the receipt is
`Delivered`, with a `// protocol event:` comment naming that `2xx`; any other status is an `Err`,
not `Sent` (the bytes were written, but the peer refused them). The server is `axum` on hyper's
HTTP/1.1 connection builder, driven by an accept loop that is TCP's: a connection permit before
each `accept`, the listener bound once in `start()` (`server_port`, the key `port_key` maps HTTP
to, at the `BindScope` address) and kept, the status `Failed` if the bind fails. Keep-alive is off,
so one connection carries one request, as TCP and WebSocket carry one message: a kept-alive
connection could carry any number of requests under one permit, and pipelined requests after the
first are never read. The limits and keys are TCP's, plus `header_read_timeout_ms` (hyper's, which
also closes a connection that sends nothing), `idle_timeout_ms` (a gap between body reads, `408`)
and `request_timeout_ms` (20 s, the whole connection, including the wait for queue budget; under
the sender's 30 s so a waiting sender sees the `503`). Backpressure: requests wait for queue budget,
holding their permits, until the application polls, and are answered `503` at the deadline or when
`stop()` closes the budget. A body must carry a `Content-Length`: over `max_message_size` it is
`413` (tower-http's `RequestBodyLimitLayer`, and the handler again) and without one `411`, both
before any of it is read; the handler then allocates exactly that length, once, so no reallocation
holds two copies. First built with a buffer that doubled, and accepting chunked bodies; the
measurement below counted the reallocation transient and the design changed before commit. The
memory bound, in TCP's style: about `C × (M + 140 KiB) + B × f`, about 145 MiB at peak with the
defaults and `f` = 18. **Measured** (counting allocator that counts a reallocation's old and new
blocks at once, release build, Windows, 16 connections, each with a whole body read and waiting for
budget): 20.6 KiB per connection once its head is read, and `M` + 137 to 138 KiB at the peak, the
same at `M` = 256 KiB, 1 MiB and 4 MiB and for one write, 64 KiB, 1 KiB and (at 256 KiB) 7-byte
writes. **Derived:** the 140 KiB (a round-up of the largest measured, not a proven maximum), and
that 64 connections hold what 16 did. A target is checked as the `reqwest::Url` that is dialled; its
host and port must be the ones the address names, so `127.1`, `0x7f.1`, `%61` and non-ASCII hosts
are refused, with userinfo, fragments, a missing port and any scheme but `http` and `https`; the
client follows no redirect, uses no proxy and speaks HTTP/1.1 only. `https://` is accepted because
`reqwest`'s default features, kept, include rustls, which a unit test checks (an `https://` send
opens with a TLS handshake record). `abstraction.rs`'s second `HttpTransportFactory`, whose
`validate_config` checked three keys, is deleted, as Task 7 deleted the shadowing TCP factory.
`stop()` also ends the accept loop and releases the listener, which TCP and WebSocket do not yet.
Mutation controls, each in the mutation tree's own target directory, each caught by the named
tests: answering `202` before queuing (the backpressure, 503-deadline, pipelining, `receive_raw`
and `stop` tests); keep-alive on (`http_answers_one_request_per_connection_...`); no header
timeout (`http_closes_connections_that_do_not_finish_their_head`); no body idle timeout
(`http_idle_timeout_is_a_gap_...`); no `413` in the layer or the handler
(`http_server_refuses_an_oversize_body_...`); `receive_raw` handing out copies
(`receive_raw_hands_each_message_out_once`, the backpressure test); `stop` not closing the budget
(`stop_answers_a_waiting_request_with_503`); the cap raised by 100 (the cap and header tests); the
sender not refusing oversize (`http_refuses_an_oversize_message_before_connecting_...`); redirects
followed (`a_non_2xx_answer_is_an_error`); a rewritten host accepted (`target_addresses`,
`http_refuses_targets_it_would_not_dial_as_named`). The unmutated tree passed both runs.

#### Breaking changes (unreleased 2.0.0), from Task 9

Quote this list in PR B's body, with Tasks 7 and 8's.

- HTTP now receives: `start()` binds `server_port` (at the `bind_scope` address, loopback by
  default) and serves `POST /synapse/message`. `start()` sets the status to `Failed` when its bind
  fails, and a second `start()` on the same instance is an error. `stop()` ends the accept loop,
  releases the listener and closes the receive queue.
- HTTP's receipt claims `Delivered` instead of `Sent`, resting on the peer's `2xx`, which this
  server sends only after the message is queued.
- The wire contract: one JSON `SecureMessage` per `POST`, with a `Content-Length` (a chunked body is
  answered `411`), one request per connection (keep-alive off; the server answers with
  `Connection: close`, and a pipelined request is never read), HTTP/1.1 only. The server answers
  `202`, `400`, `408`, `411`, `413` or `503` (module documentation of `http_unified.rs`).
- HTTP's error for an oversize message changed from `SynapseError::TransportError` to
  `SynapseError::MessageRefused`, returned before connecting, and the manager no longer counts it
  against HTTP.
- `max_message_size`'s default fell from 10 MiB to 1 MiB of serialized JSON. New keys:
  `max_concurrent_connections` 64, `max_queued_bytes` 4 MiB, `header_read_timeout_ms` and
  `idle_timeout_ms` 5 s, `request_timeout_ms` 20 s.
- `use_https`'s default changed from `true` to `false`, since this transport's server speaks plain
  HTTP; it now affects only `host:port` targets.
- An unparseable or zero limit or timeout, a `server_port` that is not a port number, a `use_https`
  other than `true` or `false`, a `user_agent` that is not a valid header value, or a
  `max_queued_bytes` below `max_message_size` or above `u32::MAX`, is refused at construction and by
  `HttpTransportFactory::validate_config`, instead of silently becoming the default (a bad
  `server_port` used to disable the server, and a bad `use_https` meant `true`). `validate_config`
  used to check three keys. `default_config` returns every key.
- The keys `server_address` (use `bind_scope`) and `max_connections` (use
  `max_concurrent_connections`) are refused rather than ignored.
- A target must carry an address that is an `http://` or `https://` URL or `host:port`, naming a
  host and a port. Refused, where they used to be turned into some URL: an identifier-only target
  (it became `http(s)://localhost:8080/synapse/message/<identifier>`), a missing port (80, 443, 8080
  or 8443 was guessed), any other scheme, userinfo, a fragment, and a host the URL parser would
  rewrite (`127.1`, `0x7f.1`, a percent-encoded or non-ASCII name). `can_reach` says so without
  touching the network. A URL whose path is empty or `/` is sent to `/synapse/message`.
- The sender follows no redirect (a `3xx` is an error) and ignores `HTTP_PROXY` and the other proxy
  settings of the environment.
- `test_connectivity` sends a `HEAD` to the target URL (it sent one to `/synapse/health`, which
  nothing served) and reports connected on any HTTP response. `estimate_metrics`, which reported
  fixed figures without touching the network (100 or 200 ms, reliability 0.95, 1,000,000 bytes/s,
  available, confidence 0.8), now probes: measured latency and confidence 1 on a response; on a
  failure, unavailable, `timeout_ms` as the latency and confidence 0.3. Bandwidth is measured from
  sends, or 1 until one has been.
- `metrics()`: `reliability_score` is successes over attempts, 0 before any (it was a moving average
  starting at 1.0); `average_latency_ms` is a running mean (it was the last send's);
  `active_connections` counts inbound connections being served.
- `capabilities()` reports the configured `max_message_size`, `encrypted: false`, and the features
  `request_response`, `one_request_per_connection` and `firewall_friendly`.
- The default `User-Agent` is `Synapse-HTTP-Transport/2.0` (was `/1.0`).
- Removed from the public API: `HttpTransportConfig`; `HttpServer`, with its public
  `received_messages` queue; and `synapse::transport::abstraction::HttpTransportFactory`, a second
  factory (`synapse::transport::HttpTransportFactory` is `http_unified`'s, as before). Added:
  `http_unified::validate_config`, `HttpTransportImpl::local_addr`, and the key and default
  constants.
- `reqwest` moved from 0.12.22 to 0.13.5. `axum` 0.8.9, `hyper` 1.11.1, `hyper-util` 0.1.20,
  `tower` 0.5.3, `tower-http` 0.7.1 and `http-body-util` 0.1.5 are new dependencies of the `http`
  feature. No public API names their types; `llm_discovery` and the telemetry error reporter, the
  other users of `reqwest`, compile unchanged against 0.13.

### Task 10: NAT traversal

**Files:** `src/transport/abstraction.rs` (`TransportType::NatTraversal` and a factory),
`src/transport/nat_traversal.rs`, `tests/transport_repairs.rs`

It has no `TransportType` and no factory, so nothing can register it. Add both. Fix its double bind:
`receive_raw` reuses the socket bound into `self.socket` instead of binding a second one on the same
address. Its send also corrupts ciphertext: `send_message` builds its JSON with
`"content": String::from_utf8_lossy(&message.encrypted_content)`, which replaces every invalid UTF-8
sequence with U+FFFD, so sealed bytes do not survive. The repair must send the raw bytes (the
serialized `SecureMessage`, as the other transports do), not a lossy string.

- [ ] **Test:** `nat_traversal_carries_a_verified_message_end_to_end`, over loopback. (The traversal
  itself needs a real NAT to exercise; this test proves the transport sends and receives, which is the
  repair.) Control: record that it fails before the double-bind fix.
- [ ] Commit: `fix(nat): register it, and stop binding twice`.

---

### Task 11: Verification and the PRs (run once after Task 6, once after Task 10)

- [ ] **Mutation check.** Predict in writing, then run. For PR A: removing the version check in
  `verify_at` fails only the `protocol_version` tests. For PR B: re-introducing the second bind in
  `websocket_unified` fails only the WebSocket end-to-end test. Run `--lib` and each `--test` target as
  **separate commands** — a `--lib name::` filter applies to every target and silently runs zero
  integration tests.
- [ ] **Full run in a FRESH target directory** with the UTC window recorded. **The failing set must be
  empty.**
- [ ] **Firewall event 2097 count** for that window: 0, with a positive control.
- [ ] fmt; clippy on the lib and every test target.
- [ ] **Docs:** update `CAPABILITY_INVENTORY.md`'s transport findings — the 26-functions entry, the
  QUIC entry, the email entry, the double-bind entries — each marked fixed or deleted, with the ref.
- [ ] **Open the PR** (A into `main`; B into `design/transport-contract`, retargeted when A merges),
  with the breaking list, the verification numbers and their ref, and the mutation result.
