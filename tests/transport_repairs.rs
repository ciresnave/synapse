// SPDX-License-Identifier: MIT OR Apache-2.0
//! Transport contract, PR B: every repaired transport carries a verified message end to end.
//! See docs/superpowers/specs/2026-09-18-transport-contract-design.md §4, §7 and §10.
//!
//! The helper here (`node`, `Pair`, `round_trip`) is shared by every transport repair: each one
//! sends a sealed, signed message from Alice to Bob through a real loopback socket and checks what
//! Bob's manager delivers -- the verdict, the certificate that pins Alice, the opened body -- and
//! what Alice's receipt claims.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use synapse::CryptoManager;
use synapse::certificate::{AgentCertificate, Permission};
use synapse::sealing::{self, Payload, SealingKeyPair};
use synapse::sender_auth::{SenderVerdict, TrustStore};
use synapse::transport::{
    DeliveryConfirmation, DeliveryReceipt, ReceivedMessage, TransportFactory, TransportManager,
    TransportManagerBuilder, TransportStatus, TransportTarget, TransportType,
};
use synapse::types::{SecureMessage, SecurityLevel};

const ALICE: &str = "alice@repair.test";
const BOB: &str = "bob@repair.test";

/// Every transport the builder enables by default, plus any a factory could register. `node`
/// disables all of them except the one under test, so a round trip can only cross that transport.
const ALL_TRANSPORTS: [TransportType; 7] = [
    TransportType::Tcp,
    TransportType::Udp,
    TransportType::Http,
    TransportType::WebSocket,
    TransportType::Email,
    TransportType::AutoDiscovery,
    TransportType::Quic,
];

/// The config key each transport reads its listening port from. They differ per transport, and a
/// wrong key is silently ignored (the transport falls back to its default port or to client-only),
/// so every transport a test uses must be listed here.
fn port_key(kind: TransportType) -> &'static str {
    match kind {
        TransportType::Tcp => "listen_port",
        TransportType::Udp => "bind_port",
        // http_unified.rs reads `server_port` (0, the default, means no server is bound).
        TransportType::Http => "server_port",
        // websocket_unified.rs reads `local_port` and binds it in `start`.
        TransportType::WebSocket => "local_port",
        // NAT traversal (`TransportType::Custom(1)`, nat_traversal.rs) has no factory and reads no
        // config key: `NatTraversalTransport::new_with_scope` takes its port as an argument, and
        // providers.rs passes 8080. A test must construct it directly with the port it wants.
        TransportType::Custom(1) => panic!(
            "NAT traversal has no port config key; construct NatTraversalTransport with the port"
        ),
        other => panic!("no port key recorded for {other:?}; add it to port_key"),
    }
}

/// The socket family each transport listens with, so `node` can prove the port is really held.
enum Socket {
    Tcp,
    Udp,
}

fn socket_of(kind: TransportType) -> Socket {
    match kind {
        TransportType::Tcp | TransportType::Http | TransportType::WebSocket => Socket::Tcp,
        // NAT traversal (`Custom(1)`) listens on a `UdpSocket` (nat_traversal.rs, `start`).
        TransportType::Udp | TransportType::Custom(1) => Socket::Udp,
        other => panic!("no socket family recorded for {other:?}; add it to socket_of"),
    }
}

/// Whether something already holds loopback `port` for `kind`'s socket family: a bind of our own
/// must fail. (Binding and releasing is harmless: if the bind succeeds the test fails anyway.)
fn port_is_held(kind: TransportType, port: u16) -> bool {
    match socket_of(kind) {
        Socket::Tcp => std::net::TcpListener::bind(("127.0.0.1", port)).is_err(),
        Socket::Udp => std::net::UdpSocket::bind(("127.0.0.1", port)).is_err(),
    }
}

/// Build a manager with exactly one transport enabled, listening on loopback `port`, and prove it
/// is listening. `manager.start()` returns `Ok` even when a transport fails to start, and a
/// transport whose bind fails may fall back to client-only, so neither `Ok` says anything. The
/// port must be free before the node starts and held after, so the holder can only be the node:
/// with only the "held after" check, a port some other socket already held would pass while the
/// node ran client-only, and the test would fail later, as a message that never arrived.
async fn node(
    kind: TransportType,
    factory: Box<dyn TransportFactory>,
    port: u16,
    store: TrustStore,
    sealing_key: SealingKeyPair,
    extra_config: &HashMap<String, String>,
) -> TransportManager {
    assert!(
        !port_is_held(kind, port),
        "127.0.0.1:{port} is already in use before {kind:?} starts, so the node could not listen there"
    );
    let mut config = HashMap::new();
    config.insert(port_key(kind).to_string(), port.to_string());
    // Loopback is already the default; saying so keeps a changed default from reaching this test.
    config.insert(
        synapse::network_scope::BIND_SCOPE_KEY.to_string(),
        synapse::network_scope::BindScope::Loopback
            .config_value()
            .to_string(),
    );
    config.extend(extra_config.clone());
    let mut builder = TransportManagerBuilder::new();
    for other in ALL_TRANSPORTS {
        if other != kind {
            builder = builder.disable_transport(other);
        }
    }
    let manager = builder
        .enable_transport(kind)
        .transport_config(kind, config)
        .trust_store(store)
        .sealing_key(sealing_key)
        .build();
    manager
        .register_factory(factory)
        .await
        .expect("factory registers");
    tokio::time::timeout(Duration::from_secs(5), manager.start())
        .await
        .expect("start returns")
        .expect("start succeeds");

    let status = manager.get_transport_status().await;
    assert_eq!(
        status.get(&kind),
        Some(&TransportStatus::Running),
        "{kind:?} did not start: the manager reports {status:?}"
    );
    assert!(
        port_is_held(kind, port),
        "{kind:?} reports Running but nothing holds 127.0.0.1:{port}; \
         the transport is not listening"
    );
    manager
}

/// A certificate from `account` for `holder`, valid for an hour. Adapted from
/// tests/agent_certificates.rs, because a test binary cannot import another's helpers.
fn cert_for(
    account: &ed25519_dalek::SigningKey,
    holder: &CryptoManager,
    holder_sealing: &SealingKeyPair,
    id: &str,
) -> AgentCertificate {
    let now = chrono::Utc::now();
    AgentCertificate::sign(
        AgentCertificate {
            version: 1,
            serial: [9u8; 16],
            issuer_key_id: synapse::sender_auth::key_id(&account.verifying_key().to_bytes()),
            subject_label: "repair-test".to_string(),
            subject_global_id: id.to_string(),
            subject_signing_key: holder.public_key_bytes().expect("public key"),
            subject_sealing_key: *holder_sealing.public_key().as_bytes(),
            not_before: now - chrono::Duration::minutes(1),
            not_after: now + chrono::Duration::hours(1),
            permissions: vec![Permission::Send],
            may_delegate: 0,
            signature: [0u8; 64],
        },
        account,
    )
}

/// Alice and Bob, each a node with one transport enabled. Bob pins Alice's account key and holds
/// the sealing key Alice seals to.
struct Pair {
    kind: TransportType,
    alice: CryptoManager,
    alice_signing_key: [u8; 32],
    alice_sealing_key: [u8; 32],
    bob_sealing_key: sealing::SealingPublicKey,
    alice_node: TransportManager,
    bob_node: TransportManager,
    bob_port: u16,
}

impl Pair {
    async fn new(
        kind: TransportType,
        alice_factory: Box<dyn TransportFactory>,
        bob_factory: Box<dyn TransportFactory>,
        alice_port: u16,
        bob_port: u16,
    ) -> Self {
        Self::with_config(
            kind,
            alice_factory,
            bob_factory,
            alice_port,
            bob_port,
            &HashMap::new(),
        )
        .await
    }

    /// As `new`, with `extra_config` added to both nodes' transport config.
    async fn with_config(
        kind: TransportType,
        alice_factory: Box<dyn TransportFactory>,
        bob_factory: Box<dyn TransportFactory>,
        alice_port: u16,
        bob_port: u16,
        extra_config: &HashMap<String, String>,
    ) -> Self {
        assert_ne!(
            alice_port, bob_port,
            "Alice and Bob need different ports, or one node's bind fails"
        );
        let account = ed25519_dalek::SigningKey::from_bytes(&[42u8; 32]);
        let mut alice = CryptoManager::new();
        alice.generate_keypair().expect("keypair");
        let alice_sealing = SealingKeyPair::generate();
        let alice_sealing_key = *alice_sealing.public_key().as_bytes();
        alice.set_certificate_chain(vec![cert_for(&account, &alice, &alice_sealing, ALICE)]);
        let alice_signing_key = alice.public_key_bytes().expect("public key");

        let bob_sealing = SealingKeyPair::generate();
        let bob_sealing_key = bob_sealing.public_key().clone();

        let mut bob_store = TrustStore::new();
        bob_store.pin_account_key("alice-account", account.verifying_key().to_bytes());

        let alice_node = node(
            kind,
            alice_factory,
            alice_port,
            TrustStore::new(),
            alice_sealing,
            extra_config,
        )
        .await;
        let bob_node = node(
            kind,
            bob_factory,
            bob_port,
            bob_store,
            bob_sealing,
            extra_config,
        )
        .await;
        Self {
            kind,
            alice,
            alice_signing_key,
            alice_sealing_key,
            bob_sealing_key,
            alice_node,
            bob_node,
            bob_port,
        }
    }

    /// A message from Alice to Bob, sealed to Bob's sealing key and then signed, so the signature
    /// covers the sealed bytes (tests/sealing.rs does the same).
    fn sealed_signed(&self, payload: &[u8]) -> SecureMessage {
        let mut message = SecureMessage::new(BOB, ALICE, payload.to_vec(), SecurityLevel::Secure);
        sealing::seal(&mut message, &self.bob_sealing_key).expect("seal");
        self.alice.sign_secure_message(&mut message).expect("sign");
        message
    }

    fn signed(&self, payload: &[u8]) -> SecureMessage {
        let mut message =
            SecureMessage::new(BOB, ALICE, payload.to_vec(), SecurityLevel::Authenticated);
        self.alice.sign_secure_message(&mut message).expect("sign");
        message
    }

    fn bob_target(&self) -> TransportTarget {
        TransportTarget::new(BOB.to_string()).with_address(format!("127.0.0.1:{}", self.bob_port))
    }

    /// Everything Bob must conclude about a message from Alice: it crossed `kind`, Alice is
    /// Verified under her own key through her certificate, and the body opened to `payload`.
    fn assert_from_alice(&self, received: &ReceivedMessage, payload: &[u8]) {
        assert_eq!(
            received.incoming.transport_type, self.kind,
            "the message must arrive over the transport under test"
        );
        self.assert_verified_as_alice(received);
        let chain = received
            .certificate
            .as_ref()
            .expect("the verdict came through Alice's certificate, so it must be attached");
        assert_eq!(chain.subject_global_id, ALICE);
        assert_eq!(chain.subject_signing_key, self.alice_signing_key);
        assert_eq!(chain.subject_sealing_key, self.alice_sealing_key);
        assert!(
            received.payload == Payload::Opened(payload.to_vec()),
            "the sealed body must open intact ({} bytes expected): {:?}",
            payload.len(),
            match &received.payload {
                Payload::CouldNotOpen(e) => format!("CouldNotOpen({e:?})"),
                Payload::Plain(b) => format!("Plain({} bytes)", b.len()),
                Payload::Opened(b) => format!("Opened({} bytes)", b.len()),
            }
        );
    }

    /// Bob's verdict on a message from Alice pins Alice's signing key.
    fn assert_verified_as_alice(&self, received: &ReceivedMessage) {
        match &received.sender {
            SenderVerdict::Verified { key_id } => assert_eq!(
                key_id,
                &synapse::sender_auth::key_id(&self.alice_signing_key),
                "the verdict must pin Alice's signing key"
            ),
            other => panic!("the message must arrive Verified: {other:?}"),
        }
    }

    fn assert_receipt(&self, receipt: &DeliveryReceipt, expected: &DeliveryConfirmation) {
        assert_eq!(
            receipt.transport_used, self.kind,
            "the receipt must name the transport under test"
        );
        assert!(
            std::mem::discriminant(&receipt.confirmation) == std::mem::discriminant(expected),
            "{:?} must claim {expected:?} (spec §4), claimed {:?}",
            self.kind,
            receipt.confirmation
        );
    }
}

/// Send one sealed, signed message carrying `payload` from Alice to Bob over `kind`, and return
/// what Bob's manager delivers together with Alice's receipt. Asserts the receipt claims exactly
/// `expected` (the confirmation spec §4 allows this transport), that the message arrives once,
/// from Alice, Verified, opened intact, and that nothing else follows it.
async fn round_trip(
    kind: TransportType,
    alice_factory: Box<dyn TransportFactory>,
    bob_factory: Box<dyn TransportFactory>,
    alice_port: u16,
    bob_port: u16,
    payload: &[u8],
    expected: DeliveryConfirmation,
) -> (ReceivedMessage, DeliveryReceipt) {
    let pair = Pair::new(kind, alice_factory, bob_factory, alice_port, bob_port).await;
    let message = pair.sealed_signed(payload);
    let receipt = pair
        .alice_node
        .send_message(&pair.bob_target(), &message)
        .await
        .expect("the transport sends");
    pair.assert_receipt(&receipt, &expected);

    for _ in 0..30 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        let mut batch = pair.bob_node.receive_messages().await.expect("receive");
        if let Some(received) = batch.pop() {
            assert!(batch.is_empty(), "exactly one message expected");
            assert_eq!(received.incoming.message.message_id.0, message.message_id.0);
            pair.assert_from_alice(&received, payload);
            tokio::time::sleep(Duration::from_millis(200)).await;
            let after = pair.bob_node.receive_messages().await.expect("receive");
            assert!(
                after.is_empty(),
                "a second poll must be empty; {} more arrived",
                after.len()
            );
            return (received, receipt);
        }
    }
    panic!("no message arrived over {kind:?} within 3 s");
}

/// A free loopback port. Bound and released, so another process could take it before the node
/// binds it; `node` then fails with a clear message rather than a false pass.
fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("bind ephemeral")
        .local_addr()
        .expect("local_addr")
        .port()
}

fn tcp() -> Box<dyn TransportFactory> {
    Box::new(synapse::transport::TcpTransportFactory)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tcp_carries_a_verified_message_end_to_end() {
    let (received, receipt) = round_trip(
        TransportType::Tcp,
        tcp(),
        tcp(),
        free_port(),
        free_port(),
        b"repaired",
        DeliveryConfirmation::Sent,
    )
    .await;
    assert_eq!(received.incoming.transport_type, TransportType::Tcp);
    assert_eq!(receipt.transport_used, TransportType::Tcp);
    assert_eq!(received.payload, Payload::Opened(b"repaired".to_vec()));
}

/// A 16 KiB body serialises to far more than one 8 KiB read: `encrypted_content` is a JSON number
/// array. The receiver must read the whole message, not the first segment of it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tcp_carries_a_large_verified_message() {
    let payload: Vec<u8> = (0..16 * 1024).map(|i| (i % 251) as u8).collect();
    let (received, _receipt) = round_trip(
        TransportType::Tcp,
        tcp(),
        tcp(),
        free_port(),
        free_port(),
        &payload,
        DeliveryConfirmation::Sent,
    )
    .await;
    assert!(received.payload == Payload::Opened(payload));
}

/// Many connections at once, while Bob polls: every message must be queued, however busy the
/// queue is when it arrives. The messages are signed but not sealed: the other two tests carry
/// sealing, and sealing plus opening costs tens of milliseconds per message in a debug build
/// (300 sealed messages took 19 s in one run here, against about 5 s signed only).
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn tcp_loses_no_message_under_concurrent_sends() {
    loses_no_message_under_concurrent_sends(TransportType::Tcp, tcp, 400).await;
}

/// Send `n` signed messages from Alice to Bob over `kind` at once, while Bob polls, and check that
/// every one arrives exactly once, Verified, with the body it was sent with.
async fn loses_no_message_under_concurrent_sends(
    kind: TransportType,
    factory: fn() -> Box<dyn TransportFactory>,
    n: usize,
) {
    let pair = Arc::new(Pair::new(kind, factory(), factory(), free_port(), free_port()).await);
    let messages: Vec<SecureMessage> = (0..n)
        .map(|i| pair.signed(format!("concurrent {i}").as_bytes()))
        .collect();
    // Each message id with the body it carries, so every arrival is checked against its own body.
    let sent: HashMap<String, Vec<u8>> = messages
        .iter()
        .map(|m| (m.message_id.0.to_string(), m.encrypted_content.clone()))
        .collect();
    assert_eq!(sent.len(), n, "message ids must be distinct");

    let poller = {
        let pair = Arc::clone(&pair);
        tokio::spawn(async move {
            let mut received = Vec::new();
            // Generous for a slow CI runner: a passing run returns as soon as all n arrive.
            let deadline = Instant::now() + Duration::from_secs(60);
            while received.len() < n && Instant::now() < deadline {
                received.extend(pair.bob_node.receive_messages().await.expect("receive"));
                tokio::task::yield_now().await;
            }
            received
        })
    };

    let sends: Vec<_> = messages
        .into_iter()
        .map(|message| {
            let pair = Arc::clone(&pair);
            tokio::spawn(async move {
                pair.alice_node
                    .send_message(&pair.bob_target(), &message)
                    .await
                    .expect("the transport sends")
            })
        })
        .collect();
    for send in sends {
        let receipt = send.await.expect("send task");
        pair.assert_receipt(&receipt, &DeliveryConfirmation::Sent);
    }

    let received = poller.await.expect("poll task");
    let mut seen = HashSet::new();
    for message in &received {
        let id = message.incoming.message.message_id.0.to_string();
        assert!(seen.insert(id.clone()), "message {id} arrived twice");
        let expected = sent
            .get(&id)
            .unwrap_or_else(|| panic!("message {id} was never sent"));
        pair.assert_verified_as_alice(message);
        assert_eq!(
            message.payload,
            Payload::Plain(expected.clone()),
            "message {id} must arrive with the body it was sent with"
        );
    }
    assert_eq!(
        received.len(),
        n,
        "{} of {n} concurrently sent messages were lost",
        n - received.len()
    );
    tokio::time::sleep(Duration::from_millis(200)).await;
    let after = pair.bob_node.receive_messages().await.expect("receive");
    assert!(after.is_empty(), "{} extra messages arrived", after.len());
}

/// A TCP config naming `key` = `value`.
fn tcp_config(key: &str, value: &str) -> HashMap<String, String> {
    HashMap::from([(key.to_string(), value.to_string())])
}

/// A limit that does not parse, or is zero, must refuse construction: before, a typo silently
/// became the default and `0` was accepted. A valid value is the positive control.
#[tokio::test]
async fn tcp_refuses_an_unparseable_or_zero_limit() {
    for key in ["max_message_size", "max_concurrent_connections"] {
        for bad in ["", "abc", "0", "-1", "1MiB"] {
            match synapse::transport::TcpTransportFactory
                .create_transport(&tcp_config(key, bad))
                .await
            {
                Ok(_) => panic!("{key} = {bad:?} must be refused, not replaced by a default"),
                Err(e) => assert!(
                    e.to_string().contains(key),
                    "the error for {key} = {bad:?} must name the key: {e}"
                ),
            }
        }
        assert!(
            synapse::transport::TcpTransportFactory
                .create_transport(&tcp_config(key, "4096"))
                .await
                .is_ok(),
            "{key} = 4096 must be accepted"
        );
    }
}

/// The queue budget must hold the largest message the receiver reads, or a handler holding such a
/// message would wait forever for budget that never comes; and it must fit the `u32` one
/// semaphore acquire counts in. Construction refuses a budget below `max_message_size`, above
/// `u32::MAX`, unparseable or zero, naming the key; a budget equal to `max_message_size` is the
/// positive control. Before `max_queued_bytes` existed, every one of these was accepted.
#[tokio::test]
async fn tcp_refuses_a_queue_budget_smaller_than_its_largest_message() {
    let config = |budget: &str, message: &str| {
        HashMap::from([
            ("max_queued_bytes".to_string(), budget.to_string()),
            ("max_message_size".to_string(), message.to_string()),
        ])
    };
    let over_u32 = (u64::from(u32::MAX) + 1).to_string();
    for (budget, message) in [
        ("4095", "4096"),
        ("1", "4096"),
        ("1048575", "1048576"),
        (over_u32.as_str(), over_u32.as_str()),
        ("0", "4096"),
        ("abc", "4096"),
    ] {
        match synapse::transport::TcpTransportFactory
            .create_transport(&config(budget, message))
            .await
        {
            Ok(_) => panic!(
                "max_queued_bytes = {budget} with max_message_size = {message} must be refused"
            ),
            Err(e) => assert!(
                e.to_string().contains("max_queued_bytes"),
                "the error for max_queued_bytes = {budget} must name the key: {e}"
            ),
        }
    }
    // Without `max_message_size`, the budget is checked against its default of 1 MiB.
    assert!(
        synapse::transport::TcpTransportFactory
            .create_transport(&tcp_config("max_queued_bytes", "4096"))
            .await
            .is_err(),
        "max_queued_bytes = 4096 is under the default max_message_size and must be refused"
    );
    for (budget, message) in [("4096", "4096"), ("8192", "4096")] {
        assert!(
            synapse::transport::TcpTransportFactory
                .create_transport(&config(budget, message))
                .await
                .is_ok(),
            "max_queued_bytes = {budget} with max_message_size = {message} must be accepted"
        );
    }
    let u32_max = u32::MAX.to_string();
    assert!(
        synapse::transport::TcpTransportFactory
            .create_transport(&config(&u32_max, "4096"))
            .await
            .is_ok(),
        "max_queued_bytes = u32::MAX must be accepted"
    );
}

/// The serialized size of `message`, the unit `max_message_size` counts.
fn wire_size(message: &SecureMessage) -> usize {
    serde_json::to_vec(message).expect("serialize").len()
}

/// A message from Alice whose serialized size is at most `limit` (`under`) or over it, with that
/// size. Serialized size is not a fixed function of body length -- each signature is a fresh 64
/// bytes written as a JSON number array, so it varies by a few characters -- so each candidate is
/// measured rather than predicted. A body of `limit` bytes serializes to about twice `limit`, and a
/// 64-byte body to far less than a 4096-byte limit, so the margins dwarf that variation.
fn measured(pair: &Pair, limit: usize, under: bool) -> (SecureMessage, usize) {
    let body_len = if under { 64 } else { limit };
    let message = pair.signed(&vec![7u8; body_len]);
    let size = wire_size(&message);
    assert_eq!(
        size <= limit,
        under,
        "a {body_len}-byte body serialized to {size} bytes against a {limit}-byte limit; \
         pick another candidate"
    );
    (message, size)
}

/// The sender refuses a message whose serialized form is over its `max_message_size`, instead of
/// writing it and claiming `Sent` for a message a receiver with the same limit drops. A message
/// under the limit still crosses, so both sides accept what the sender sends.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tcp_refuses_at_send_a_message_over_its_limit() {
    const LIMIT: usize = 4096;
    let config = tcp_config("max_message_size", &LIMIT.to_string());
    let pair = Pair::with_config(
        TransportType::Tcp,
        tcp(),
        tcp(),
        free_port(),
        free_port(),
        &config,
    )
    .await;

    let (fits, fits_size) = measured(&pair, LIMIT, true);
    let (over, over_size) = measured(&pair, LIMIT, false);

    // The one that fits crosses, and Bob, with the same limit, accepts it.
    let receipt = pair
        .alice_node
        .send_message(&pair.bob_target(), &fits)
        .await
        .unwrap_or_else(|e| panic!("a message of {fits_size} bytes fits the limit: {e}"));
    pair.assert_receipt(&receipt, &DeliveryConfirmation::Sent);
    let mut arrived = Vec::new();
    for _ in 0..30 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        arrived.extend(pair.bob_node.receive_messages().await.expect("receive"));
        if !arrived.is_empty() {
            break;
        }
    }
    assert_eq!(
        arrived.len(),
        1,
        "the {fits_size}-byte message must arrive once"
    );
    assert_eq!(arrived[0].incoming.message.message_id.0, fits.message_id.0);
    pair.assert_verified_as_alice(&arrived[0]);

    // The one over is refused by the transport itself, with an error that says why.
    let alice_transport = synapse::transport::TcpTransportFactory
        .create_transport(&config)
        .await
        .expect("a client-only transport with the same limit");
    let refusal = match alice_transport
        .send_message(&pair.bob_target(), &over)
        .await
    {
        Ok(receipt) => panic!(
            "a {over_size}-byte message over the {LIMIT}-byte limit must be refused, not {:?}",
            receipt.confirmation
        ),
        Err(e) => e.to_string(),
    };
    assert!(
        refusal.contains("max_message_size")
            && refusal.contains(&over_size.to_string())
            && refusal.contains(&LIMIT.to_string()),
        "the refusal must name the limit and both sizes: {refusal}"
    );
    // And through the manager, the send fails rather than claiming a delivery.
    assert!(
        pair.alice_node
            .send_message(&pair.bob_target(), &over)
            .await
            .is_err(),
        "the manager must not report the over-limit message as sent"
    );
    tokio::time::sleep(Duration::from_millis(500)).await;
    let after = pair.bob_node.receive_messages().await.expect("receive");
    assert!(
        after.is_empty(),
        "nothing over the limit may reach Bob; {} arrived",
        after.len()
    );
}

/// With `max_concurrent_connections` = 2 and two connections held open, a third is not read
/// until one of them closes: the accept loop waits for a permit, so an unauthenticated peer can
/// make the receiver hold at most that many buffers at once.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tcp_reads_no_more_connections_at_once_than_its_cap() {
    let pair = Pair::with_config(
        TransportType::Tcp,
        tcp(),
        tcp(),
        free_port(),
        free_port(),
        &tcp_config("max_concurrent_connections", "2"),
    )
    .await;
    let bob = ("127.0.0.1", pair.bob_port);
    let first = tokio::net::TcpStream::connect(bob).await.expect("connect");
    let second = tokio::net::TcpStream::connect(bob).await.expect("connect");
    tokio::time::sleep(Duration::from_millis(300)).await;

    let message = pair.signed(b"behind the cap");
    let receipt = pair
        .alice_node
        .send_message(&pair.bob_target(), &message)
        .await
        .expect("the kernel accepts the connection into the backlog");
    pair.assert_receipt(&receipt, &DeliveryConfirmation::Sent);

    tokio::time::sleep(Duration::from_secs(1)).await;
    let early = pair.bob_node.receive_messages().await.expect("receive");
    assert!(
        early.is_empty(),
        "with two connections held and a cap of 2, a third was read anyway"
    );

    drop(first);
    let mut arrived = Vec::new();
    for _ in 0..50 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        arrived.extend(pair.bob_node.receive_messages().await.expect("receive"));
        if !arrived.is_empty() {
            break;
        }
    }
    assert_eq!(
        arrived.len(),
        1,
        "once a held connection closes, the waiting message must be read"
    );
    assert_eq!(
        arrived[0].incoming.message.message_id.0,
        message.message_id.0
    );
    pair.assert_verified_as_alice(&arrived[0]);
    assert_eq!(
        arrived[0].payload,
        Payload::Plain(b"behind the cap".to_vec())
    );
    drop(second);
}

/// Poll Bob until `want` messages have arrived or `within` passes, and return what arrived.
async fn poll_bob(pair: &Pair, want: usize, within: Duration) -> Vec<ReceivedMessage> {
    let deadline = Instant::now() + within;
    let mut arrived = Vec::new();
    while arrived.len() < want && Instant::now() < deadline {
        arrived.extend(pair.bob_node.receive_messages().await.expect("receive"));
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    arrived
}

/// The body of the `i`th message the queue test sends: 4 KiB, so each message's serialized size
/// is large beside the few hundred bytes by which two such messages can differ.
fn queued_body(i: usize) -> Vec<u8> {
    let mut body = format!("queued {i} ").into_bytes();
    body.resize(4096, b'.');
    body
}

/// How far the serialized sizes of two messages built alike, with the same body length, can
/// differ: the 64-byte signature serializes as a JSON number array of 127 to 255 characters, the
/// timestamp's fraction takes 0 to 7, and the rest -- certificate chain, key id, ids -- is fixed
/// width or nearly so. 512 bytes covers that with room to spare.
const QUEUED_SIZE_SLACK: usize = 512;

/// With a queue budget that holds two of its messages but not three, and nobody polling, four
/// messages are sent. Only two may enter the queue; the other two handlers wait for budget, holding
/// their connections, rather than being dropped. What shows the budget: the first poll, made long
/// after all four were read, returns exactly two, because a message enters the queue only once it
/// holds budget for its bytes and the first drain happens before any budget is freed. Without the
/// budget all four are already queued and the first poll returns four. The later polls show
/// nothing was lost: all four arrive, each once.
///
/// The budget is set before the messages exist (they are signed by the pair's own key), so it is
/// sized from messages built the same way on a probe pair, `QUEUED_SIZE_SLACK` apart from the real
/// ones at most; the real ones are then measured, as `measured` does, and the test asserts that any
/// two of them fit the budget and no three do before relying on it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tcp_holds_messages_past_its_queue_cap_until_polled_and_loses_none() {
    let probe = Pair::new(TransportType::Tcp, tcp(), tcp(), free_port(), free_port()).await;
    let probe_largest = (0..4)
        .map(|i| wire_size(&probe.signed(&queued_body(i))))
        .max()
        .expect("four probes");
    drop(probe);
    let budget = 2 * (probe_largest + QUEUED_SIZE_SLACK);
    // `max_queued_bytes` may not be below `max_message_size`; every message here is under both.
    let config = HashMap::from([
        ("max_queued_bytes".to_string(), budget.to_string()),
        ("max_message_size".to_string(), budget.to_string()),
    ]);
    let pair = Pair::with_config(
        TransportType::Tcp,
        tcp(),
        tcp(),
        free_port(),
        free_port(),
        &config,
    )
    .await;
    let messages: Vec<SecureMessage> = (0..4).map(|i| pair.signed(&queued_body(i))).collect();
    let sizes: Vec<usize> = messages.iter().map(wire_size).collect();
    let mut sorted = sizes.clone();
    sorted.sort_unstable();
    assert!(
        sorted[2] + sorted[3] <= budget,
        "the two largest messages ({sizes:?} bytes) must fit the {budget}-byte budget"
    );
    assert!(
        sorted[0] + sorted[1] + sorted[2] > budget,
        "no three messages ({sizes:?} bytes) may fit the {budget}-byte budget"
    );
    for message in &messages {
        let receipt = pair
            .alice_node
            .send_message(&pair.bob_target(), message)
            .await
            .expect("the transport sends");
        pair.assert_receipt(&receipt, &DeliveryConfirmation::Sent);
    }
    // Long enough for Bob to read all four; nobody polls meanwhile.
    tokio::time::sleep(Duration::from_millis(1500)).await;

    let first = pair.bob_node.receive_messages().await.expect("receive");
    assert_eq!(
        first.len(),
        2,
        "with a budget for 2 messages, the first poll must find exactly 2 queued messages"
    );
    let mut arrived = first;
    arrived.extend(poll_bob(&pair, 2, Duration::from_secs(5)).await);

    let sent: HashSet<String> = messages
        .iter()
        .map(|m| m.message_id.0.to_string())
        .collect();
    let mut seen = HashSet::new();
    for message in &arrived {
        let id = message.incoming.message.message_id.0.to_string();
        assert!(sent.contains(&id), "message {id} was never sent");
        assert!(seen.insert(id.clone()), "message {id} arrived twice");
        pair.assert_verified_as_alice(message);
    }
    assert_eq!(
        seen, sent,
        "every message held back by the full queue must arrive once polling starts"
    );
    tokio::time::sleep(Duration::from_millis(200)).await;
    let after = pair.bob_node.receive_messages().await.expect("receive");
    assert!(after.is_empty(), "{} extra messages arrived", after.len());
}

/// A message the transport refuses (over its size limit) is a fault of the message, not of the
/// transport: the manager's error must carry the transport's own reason, TCP must stay Running,
/// and the next message must go through. Before, one refusal marked TCP failed for 300 s, both
/// directions, and the caller saw only "All transports failed".
///
/// `REFUSALS` refusals come first, because the manager's circuit breaker must not count them
/// either. With the builder's default config (`TransportManagerConfig::default`, manager.rs) the
/// breaker opens once its 60 s window holds at least `minimum_requests` (10) outcomes of which
/// `failure_threshold` (5) are failures (`CircuitBreaker::should_trip`). So if the manager counted
/// refusals against the breaker, the 10th would open it and the good message would be refused with
/// "Circuit breaker is open". The breaker's own count is not observable through a public API, so
/// what the test observes is that outcome.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_refused_message_does_not_take_tcp_out_of_service() {
    const LIMIT: usize = 4096;
    const REFUSALS: usize = 10;
    let pair = Pair::with_config(
        TransportType::Tcp,
        tcp(),
        tcp(),
        free_port(),
        free_port(),
        &tcp_config("max_message_size", &LIMIT.to_string()),
    )
    .await;
    for attempt in 1..=REFUSALS {
        let (over, over_size) = measured(&pair, LIMIT, false);
        let error = match pair
            .alice_node
            .send_message(&pair.bob_target(), &over)
            .await
        {
            Ok(receipt) => panic!(
                "refusal {attempt}: a {over_size}-byte message over the {LIMIT}-byte limit must be                  refused, not {:?}",
                receipt.confirmation
            ),
            Err(e) => e.to_string(),
        };
        assert!(
            error.contains("Tcp")
                && error.contains("max_message_size")
                && error.contains(&over_size.to_string()),
            "refusal {attempt}: the manager's error must carry TCP's own reason, with the size:              {error}"
        );
        assert_eq!(
            pair.alice_node
                .get_transport_status()
                .await
                .get(&TransportType::Tcp),
            Some(&TransportStatus::Running),
            "refusal {attempt}: a refused message must not mark TCP failed"
        );
    }

    let (fits, _) = measured(&pair, LIMIT, true);
    let receipt = pair
        .alice_node
        .send_message(&pair.bob_target(), &fits)
        .await
        .expect("after the refusals, the next message must still go through TCP");
    pair.assert_receipt(&receipt, &DeliveryConfirmation::Sent);
    let arrived = poll_bob(&pair, 1, Duration::from_secs(3)).await;
    assert_eq!(
        arrived.len(),
        1,
        "the message after the refusals must arrive"
    );
    assert_eq!(arrived[0].incoming.message.message_id.0, fits.message_id.0);
    pair.assert_verified_as_alice(&arrived[0]);
    assert_eq!(
        pair.alice_node
            .get_transport_status()
            .await
            .get(&TransportType::Tcp),
        Some(&TransportStatus::Running),
        "TCP must still be Running after the refusals and the good message"
    );
}

/// Slowloris: with a connection cap of 2, two peers connect and send nothing. The first-byte
/// timeout (500 ms here) closes them, so a legitimate message behind them is read within that
/// timeout plus a margin, not after the 30 s total read timeout. That it takes at least most of the
/// 500 ms shows the idle connections really held both permits.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tcp_closes_silent_connections_so_they_cannot_hold_every_permit() {
    const FIRST_BYTE: Duration = Duration::from_millis(500);
    let config = HashMap::from([
        ("max_concurrent_connections".to_string(), "2".to_string()),
        (
            "first_byte_timeout_ms".to_string(),
            FIRST_BYTE.as_millis().to_string(),
        ),
    ]);
    let pair = Pair::with_config(
        TransportType::Tcp,
        tcp(),
        tcp(),
        free_port(),
        free_port(),
        &config,
    )
    .await;
    let bob = ("127.0.0.1", pair.bob_port);
    let idle_one = tokio::net::TcpStream::connect(bob).await.expect("connect");
    let idle_two = tokio::net::TcpStream::connect(bob).await.expect("connect");

    let message = pair.signed(b"behind two silent peers");
    let start = Instant::now();
    let receipt = pair
        .alice_node
        .send_message(&pair.bob_target(), &message)
        .await
        .expect("the kernel accepts the connection into the backlog");
    pair.assert_receipt(&receipt, &DeliveryConfirmation::Sent);
    let arrived = poll_bob(&pair, 1, FIRST_BYTE + Duration::from_secs(3)).await;
    let waited = start.elapsed();

    assert_eq!(
        arrived.len(),
        1,
        "two silent connections held both permits for {waited:?}; the message was not read \
         within the {FIRST_BYTE:?} first-byte timeout plus 3 s"
    );
    assert_eq!(
        arrived[0].incoming.message.message_id.0,
        message.message_id.0
    );
    pair.assert_verified_as_alice(&arrived[0]);
    assert!(
        waited >= FIRST_BYTE - Duration::from_millis(150),
        "the message arrived after {waited:?}, before the idle peers could have timed out, so \
         they never held the permits and the test shows nothing"
    );
    drop((idle_one, idle_two));
}

// ---------------------------------------------------------------------------------------------
// WebSocket (plan Task 8): one message per connection, over a real handshake.
// ---------------------------------------------------------------------------------------------

fn ws() -> Box<dyn TransportFactory> {
    Box::new(synapse::transport::WebSocketTransportFactory)
}

/// A config naming `key` = `value`, for any transport.
fn one_key(key: &str, value: &str) -> HashMap<String, String> {
    HashMap::from([(key.to_string(), value.to_string())])
}

/// Before the repair, `send_message` refused ("not implemented yet"), and behind the refusal
/// `start` bound twice, so the accept loop never ran, and the handshake was skipped.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn websocket_carries_a_verified_message_end_to_end() {
    let (received, receipt) = round_trip(
        TransportType::WebSocket,
        ws(),
        ws(),
        free_port(),
        free_port(),
        b"repaired",
        DeliveryConfirmation::Sent,
    )
    .await;
    assert_eq!(received.incoming.transport_type, TransportType::WebSocket);
    assert_eq!(receipt.transport_used, TransportType::WebSocket);
    assert_eq!(received.payload, Payload::Opened(b"repaired".to_vec()));
}

/// A 16 KiB body serialises to well over one 8 KiB read buffer.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn websocket_carries_a_large_verified_message() {
    let payload: Vec<u8> = (0..16 * 1024).map(|i| (i % 251) as u8).collect();
    let (received, _receipt) = round_trip(
        TransportType::WebSocket,
        ws(),
        ws(),
        free_port(),
        free_port(),
        &payload,
        DeliveryConfirmation::Sent,
    )
    .await;
    assert!(received.payload == Payload::Opened(payload));
}

/// Each message must arrive exactly once through the manager, however many times Bob polls after.
/// `receive_raw` once put back everything it drained, but the manager's replay record drops a
/// signed message it has seen, so this test passes even with that bug restored (checked: it did).
/// The transport-level check is websocket_unified's `receive_raw_hands_each_message_out_once`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn websocket_delivers_each_message_once_across_polls() {
    const K: usize = 5;
    const EXTRA_POLLS: usize = 10;
    let pair = Pair::new(
        TransportType::WebSocket,
        ws(),
        ws(),
        free_port(),
        free_port(),
    )
    .await;
    let messages: Vec<SecureMessage> = (0..K)
        .map(|i| pair.signed(format!("once {i}").as_bytes()))
        .collect();
    for message in &messages {
        let receipt = pair
            .alice_node
            .send_message(&pair.bob_target(), message)
            .await
            .expect("the transport sends");
        pair.assert_receipt(&receipt, &DeliveryConfirmation::Sent);
    }
    let mut arrived = poll_bob(&pair, K, Duration::from_secs(5)).await;
    let before_extra_polls = arrived.len();
    for _ in 0..EXTRA_POLLS {
        tokio::time::sleep(Duration::from_millis(50)).await;
        arrived.extend(pair.bob_node.receive_messages().await.expect("receive"));
    }
    let mut seen = HashSet::new();
    for message in &arrived {
        let id = message.incoming.message.message_id.0.to_string();
        assert!(seen.insert(id.clone()), "message {id} arrived twice");
        pair.assert_verified_as_alice(message);
    }
    let sent: HashSet<String> = messages
        .iter()
        .map(|m| m.message_id.0.to_string())
        .collect();
    assert_eq!(
        seen, sent,
        "every message must arrive ({before_extra_polls} arrived before the extra polls)"
    );
    assert_eq!(arrived.len(), K, "{EXTRA_POLLS} more polls found repeats");
}

/// N = 200 connections at once, half TCP's 400: every WebSocket send costs two handshakes (the
/// manager's `estimate_metrics` probes once before each send), so 400 would be needlessly slow in
/// a debug build.
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn websocket_loses_no_message_under_concurrent_sends() {
    loses_no_message_under_concurrent_sends(TransportType::WebSocket, ws, 200).await;
}

/// The sender refuses a message whose serialized form is over its `max_message_size` as
/// `MessageRefused` -- a fault of the message, which the manager does not count against the
/// transport -- and does so before connecting: the refusal comes back from a target where nothing
/// listens, where a connect would have failed differently (the control). Ten refusals through the
/// manager leave WebSocket Running (the manager's breaker would open at the 10th failure) and the
/// next message goes through. Before, the refusal was a `TransportError`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn websocket_refuses_an_oversize_message_and_stays_running() {
    const LIMIT: usize = 4096;
    const REFUSALS: usize = 10;
    let config = one_key("max_message_size", &LIMIT.to_string());
    let pair = Pair::with_config(
        TransportType::WebSocket,
        ws(),
        ws(),
        free_port(),
        free_port(),
        &config,
    )
    .await;

    let alice_transport = synapse::transport::WebSocketTransportFactory
        .create_transport(&config)
        .await
        .expect("a transport with the same limit");
    let (over, over_size) = measured(&pair, LIMIT, false);
    let nobody =
        TransportTarget::new(BOB.to_string()).with_address(format!("127.0.0.1:{}", free_port()));
    match alice_transport.send_message(&nobody, &over).await {
        Err(synapse::SynapseError::MessageRefused(reason)) => assert!(
            reason.contains("max_message_size")
                && reason.contains(&over_size.to_string())
                && reason.contains(&LIMIT.to_string()),
            "the refusal must name the limit and both sizes: {reason}"
        ),
        Err(other) => panic!("a {over_size}-byte message must be MessageRefused, not {other:?}"),
        Ok(receipt) => panic!(
            "a {over_size}-byte message over the {LIMIT}-byte limit must be refused, not {:?}",
            receipt.confirmation
        ),
    }
    // Control: a message that fits, to the same nowhere, reaches the connect and fails there.
    let (fits, _) = measured(&pair, LIMIT, true);
    match alice_transport.send_message(&nobody, &fits).await {
        Err(synapse::SynapseError::TransportError(_)) => {}
        other => panic!("a message that fits must reach the connect and fail there: {other:?}"),
    }

    for attempt in 1..=REFUSALS {
        let (over, over_size) = measured(&pair, LIMIT, false);
        let error = match pair
            .alice_node
            .send_message(&pair.bob_target(), &over)
            .await
        {
            Ok(receipt) => panic!(
                "refusal {attempt}: a {over_size}-byte message must be refused, not {:?}",
                receipt.confirmation
            ),
            Err(e) => e.to_string(),
        };
        assert!(
            error.contains("WebSocket")
                && error.contains("max_message_size")
                && error.contains(&over_size.to_string()),
            "refusal {attempt}: the manager's error must carry WebSocket's own reason: {error}"
        );
        assert_eq!(
            pair.alice_node
                .get_transport_status()
                .await
                .get(&TransportType::WebSocket),
            Some(&TransportStatus::Running),
            "refusal {attempt}: a refused message must not mark WebSocket failed"
        );
    }

    let receipt = pair
        .alice_node
        .send_message(&pair.bob_target(), &fits)
        .await
        .expect("after the refusals, the next message must still go through WebSocket");
    pair.assert_receipt(&receipt, &DeliveryConfirmation::Sent);
    let arrived = poll_bob(&pair, 1, Duration::from_secs(3)).await;
    assert_eq!(arrived.len(), 1, "only the message that fits may arrive");
    assert_eq!(arrived[0].incoming.message.message_id.0, fits.message_id.0);
    pair.assert_verified_as_alice(&arrived[0]);
    tokio::time::sleep(Duration::from_millis(300)).await;
    let after = pair.bob_node.receive_messages().await.expect("receive");
    assert!(after.is_empty(), "{} more messages arrived", after.len());
}

/// Refuses `config`, with an error naming `key`.
async fn websocket_refuses(config: HashMap<String, String>, key: &str) {
    match synapse::transport::WebSocketTransportFactory
        .create_transport(&config)
        .await
    {
        Ok(_) => panic!("{config:?} must be refused"),
        Err(e) => assert!(
            e.to_string().contains(key),
            "the error for {config:?} must name {key}: {e}"
        ),
    }
    assert!(
        synapse::transport::WebSocketTransportFactory
            .validate_config(&config)
            .is_err(),
        "validate_config must refuse {config:?} too"
    );
}

/// Every limit, timeout and the port refuse a value that does not parse, or a zero, naming the
/// key, instead of silently becoming the default; the queue budget must hold the largest message
/// and fit a `u32`. Valid values are the positive controls. Before, `max_message_size` fell back
/// to 16 MiB on a typo, `local_port` to 0, and there were no other limits.
#[tokio::test]
async fn websocket_refuses_an_invalid_config() {
    let factory = synapse::transport::WebSocketTransportFactory;
    for key in [
        "max_message_size",
        "max_concurrent_connections",
        "connection_timeout_ms",
        "handshake_timeout_ms",
        "idle_timeout_ms",
    ] {
        for bad in ["", "abc", "0", "-1", "1MiB"] {
            websocket_refuses(one_key(key, bad), key).await;
        }
        assert!(
            factory
                .create_transport(&one_key(key, "4096"))
                .await
                .is_ok(),
            "{key} = 4096 must be accepted"
        );
    }
    for bad in ["", "abc", "-1", "65536"] {
        websocket_refuses(one_key("local_port", bad), "local_port").await;
    }
    assert!(
        factory
            .create_transport(&one_key("local_port", "0"))
            .await
            .is_ok()
    );

    let budget = |budget: &str, message: &str| {
        HashMap::from([
            ("max_queued_bytes".to_string(), budget.to_string()),
            ("max_message_size".to_string(), message.to_string()),
        ])
    };
    let over_u32 = (u64::from(u32::MAX) + 1).to_string();
    for (b, m) in [
        ("4095", "4096"),
        ("0", "4096"),
        ("abc", "4096"),
        (over_u32.as_str(), over_u32.as_str()),
    ] {
        websocket_refuses(budget(b, m), "max_queued_bytes").await;
    }
    // Without `max_message_size`, the budget is checked against its default of 1 MiB.
    websocket_refuses(one_key("max_queued_bytes", "4096"), "max_queued_bytes").await;
    assert!(
        factory
            .create_transport(&budget("4096", "4096"))
            .await
            .is_ok()
    );

    websocket_refuses(one_key("bind_scope", "everywhere"), "bind_scope").await;
    assert!(factory.validate_config(&factory.default_config()).is_ok());
}

/// Connected only after a real handshake: against Bob's node the probe completes and reports a
/// measured round trip, and `estimate_metrics` says available; against a port where nothing
/// listens, neither is claimed. (A listener that accepts TCP but never upgrades is covered by
/// websocket_unified's unit test.)
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn websocket_reports_connectivity_only_after_a_real_handshake() {
    let pair = Pair::new(
        TransportType::WebSocket,
        ws(),
        ws(),
        free_port(),
        free_port(),
    )
    .await;
    let transport = synapse::transport::WebSocketTransportFactory
        .create_transport(&one_key("connection_timeout_ms", "2000"))
        .await
        .expect("construct");
    let live = transport
        .test_connectivity(&pair.bob_target())
        .await
        .expect("connectivity");
    assert!(live.connected, "{live:?}");
    assert!(
        live.rtt.is_some(),
        "a completed handshake is a measured round trip"
    );
    assert!(
        transport
            .estimate_metrics(&pair.bob_target())
            .await
            .expect("estimate")
            .available
    );

    let nobody =
        TransportTarget::new(BOB.to_string()).with_address(format!("127.0.0.1:{}", free_port()));
    let dead = transport
        .test_connectivity(&nobody)
        .await
        .expect("connectivity");
    assert!(!dead.connected, "{dead:?}");
    assert_eq!(dead.rtt, None);
    assert!(
        !transport
            .estimate_metrics(&nobody)
            .await
            .expect("estimate")
            .available
    );
    // The probes are not messages.
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        pair.bob_node
            .receive_messages()
            .await
            .expect("receive")
            .is_empty()
    );
}

/// Slowloris, for the handshake: with a connection cap of 2, two peers connect and never send a
/// handshake. The handshake timeout (500 ms here) closes them, so Alice's handshake, waiting in the
/// backlog behind them, completes and her message is read within that timeout plus a margin. That
/// it takes at least most of the 500 ms shows the silent connections really held both permits.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn websocket_closes_connections_that_never_handshake() {
    const HANDSHAKE: Duration = Duration::from_millis(500);
    let config = HashMap::from([
        ("max_concurrent_connections".to_string(), "2".to_string()),
        (
            "handshake_timeout_ms".to_string(),
            HANDSHAKE.as_millis().to_string(),
        ),
    ]);
    let pair = Pair::with_config(
        TransportType::WebSocket,
        ws(),
        ws(),
        free_port(),
        free_port(),
        &config,
    )
    .await;
    let bob = ("127.0.0.1", pair.bob_port);
    let silent_one = tokio::net::TcpStream::connect(bob).await.expect("connect");
    let silent_two = tokio::net::TcpStream::connect(bob).await.expect("connect");

    let message = pair.signed(b"behind two silent peers");
    let start = Instant::now();
    let receipt = pair
        .alice_node
        .send_message(&pair.bob_target(), &message)
        .await
        .expect("the handshake completes once the silent peers are closed");
    pair.assert_receipt(&receipt, &DeliveryConfirmation::Sent);
    let arrived = poll_bob(&pair, 1, HANDSHAKE + Duration::from_secs(3)).await;
    let waited = start.elapsed();

    assert_eq!(
        arrived.len(),
        1,
        "two silent connections held both permits for {waited:?}; the message was not read \
         within the {HANDSHAKE:?} handshake timeout plus 3 s"
    );
    assert_eq!(
        arrived[0].incoming.message.message_id.0,
        message.message_id.0
    );
    pair.assert_verified_as_alice(&arrived[0]);
    assert!(
        waited >= HANDSHAKE - Duration::from_millis(150),
        "the message arrived after {waited:?}, before the silent peers could have timed out, so \
         they never held the permits and the test shows nothing"
    );
    drop((silent_one, silent_two));
}
