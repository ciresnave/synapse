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

**Tech Stack:** Rust, `tokio`, `tokio-tungstenite` 0.27 (already a dependency), `reqwest` (already),
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

- [ ] **Test:** `websocket_carries_a_verified_message_end_to_end`. Control: record that it fails
  before the double-bind fix, because the accept loop never starts.
- [ ] Commit: `fix(websocket): bind once, perform the real handshake, and receive`.

### Task 9: HTTP

**Files:** `Cargo.toml` (`axum`, latest, under the `http` feature), `src/transport/http_unified.rs`,
`tests/transport_repairs.rs`

`start_server` binds a real `axum` server on the configured loopback port with one route,
`POST /synapse/message`, whose handler pushes the decoded message into the queue `receive_raw` drains.
The client's `Sent` stays gated on a 2xx; a 2xx is also the protocol event that permits `Delivered`,
since the peer's HTTP stack confirmed receipt — say so in the comment.

- [ ] **Test:** `http_carries_a_verified_message_end_to_end`. Control: record that it fails before the
  server is bound, because the queue is never filled.
- [ ] Commit: `fix(http): bind a real server, so HTTP can receive`.

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
