// SPDX-License-Identifier: MIT OR Apache-2.0
//! Transport contract, PR B: every repaired transport carries a verified message end to end.
//! See docs/superpowers/specs/2026-09-18-transport-contract-design.md §4, §7 and §10.
//!
//! The helper here (`node`, `Pair`, `round_trip`) is shared by every transport repair: each one
//! sends a sealed, signed message from Alice to Bob through a real loopback socket and checks what
//! Bob's manager delivers -- the verdict, the certificate that pins Alice, the opened body -- and
//! what Alice's receipt claims.

use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use synapse::CryptoManager;
use synapse::certificate::{AgentCertificate, Permission};
use synapse::sealing::{self, Payload, SealingKeyPair};
use synapse::sender_auth::{SenderVerdict, TrustStore};
use synapse::transport::{
    DeliveryConfirmation, DeliveryReceipt, QuicTransportImpl, ReceivedMessage, Transport,
    TransportFactory, TransportManager, TransportManagerBuilder, TransportStatus, TransportTarget,
    TransportType,
};
use synapse::types::{SecureMessage, SecurityLevel};

const ALICE: &str = "alice@repair.test";
const BOB: &str = "bob@repair.test";

/// Every transport the builder enables by default, plus any a factory could register. `node`
/// disables all of them except the one under test, so a round trip can only cross that transport.
const ALL_TRANSPORTS: [TransportType; 8] = [
    TransportType::Tcp,
    TransportType::Udp,
    TransportType::Http,
    TransportType::WebSocket,
    TransportType::Email,
    TransportType::AutoDiscovery,
    TransportType::Quic,
    TransportType::NatTraversal,
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
        // NatTraversalTransportFactory (abstraction.rs, PR B Task 10) reads the same key name.
        TransportType::NatTraversal => "local_port",
        // quic_unified.rs reads `local_port` and binds it when the endpoint is constructed.
        TransportType::Quic => "local_port",
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
        // NAT traversal listens on a `UdpSocket` (nat_traversal.rs, `start`).
        // QUIC runs over UDP (quic_unified.rs binds a `quinn::Endpoint`, itself a UDP socket).
        TransportType::Udp | TransportType::NatTraversal | TransportType::Quic => Socket::Udp,
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

/// 400 messages, 32 connections at a time, while Bob polls: every message must be queued, however
/// busy the queue is when it arrives. The messages are signed but not sealed: the other two tests carry
/// sealing, and sealing plus opening costs tens of milliseconds per message in a debug build
/// (300 sealed messages took 19 s in one run here, against about 5 s signed only).
///
/// It catches the old `try_lock` bug only by chance: against that code it lost 1 to 3 of the 400
/// messages in a run, and passed about 1 run in 11, so a regressed build has roughly a 9% chance
/// of passing it. The deterministic check is the unit test
/// `a_message_ready_while_the_queue_is_locked_is_queued_once_the_lock_is_released` in
/// `src/transport/tcp_unified.rs`, which holds the queue lock across a send.
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn tcp_loses_no_message_under_concurrent_sends() {
    loses_no_message_under_concurrent_sends(
        TransportType::Tcp,
        tcp,
        400,
        DeliveryConfirmation::Sent,
    )
    .await;
}

/// How many of the concurrent test's sends are in flight at once. Enough to contend for the
/// receiver's queue lock (the old `try_lock` code lost messages under it), and well under any
/// platform's listen backlog: the receiver takes a connection permit before each `accept`, so while
/// its handlers hold their permits new connections wait in the kernel's backlog, and 400 connects
/// at once overflowed it on Windows (os error 10061, 1 run in 3). That refusal is the transport
/// being honest -- the send returns `Err` -- not a lost message; the test was wrong to expect the
/// backlog to absorb every send at once.
const MAX_SENDS_IN_FLIGHT: usize = 32;

/// Send `n` signed messages from Alice to Bob over `kind`, [`MAX_SENDS_IN_FLIGHT`] at a time, while
/// Bob polls, and check that every receipt claims `expected`, and that every message arrives
/// exactly once, Verified, with the body it was sent with.
async fn loses_no_message_under_concurrent_sends(
    kind: TransportType,
    factory: fn() -> Box<dyn TransportFactory>,
    n: usize,
    expected: DeliveryConfirmation,
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

    let in_flight = Arc::new(tokio::sync::Semaphore::new(MAX_SENDS_IN_FLIGHT));
    let sends: Vec<_> = messages
        .into_iter()
        .map(|message| {
            let pair = Arc::clone(&pair);
            let in_flight = Arc::clone(&in_flight);
            tokio::spawn(async move {
                let _slot = in_flight.acquire_owned().await.expect("never closed");
                pair.alice_node
                    .send_message(&pair.bob_target(), &message)
                    .await
                    .expect("the transport sends")
            })
        })
        .collect();
    for send in sends {
        let receipt = send.await.expect("send task");
        pair.assert_receipt(&receipt, &expected);
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

/// N = 200 messages, 32 connections at a time, half TCP's 400: every WebSocket send costs two
/// handshakes (the
/// manager's `estimate_metrics` probes once before each send), so 400 would be needlessly slow in
/// a debug build.
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn websocket_loses_no_message_under_concurrent_sends() {
    loses_no_message_under_concurrent_sends(
        TransportType::WebSocket,
        ws,
        200,
        DeliveryConfirmation::Sent,
    )
    .await;
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

/// A raw WebSocket client: connect to Bob and complete the upgrade by hand, so a test controls
/// every byte that follows.
async fn raw_websocket_client(port: u16) -> tokio::net::TcpStream {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .expect("connect");
    stream
        .write_all(
            b"GET / HTTP/1.1\r\nHost: 127.0.0.1\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
              Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n",
        )
        .await
        .expect("write the upgrade request");
    let mut response = Vec::new();
    let mut byte = [0u8; 1];
    while !response.ends_with(b"\r\n\r\n") {
        stream
            .read_exact(&mut byte)
            .await
            .expect("read the upgrade response");
        response.push(byte[0]);
    }
    assert!(
        response.starts_with(b"HTTP/1.1 101"),
        "the upgrade must be accepted: {}",
        String::from_utf8_lossy(&response)
    );
    stream
}

/// One final, masked client frame with `opcode` carrying `payload`. The mask key is zero, so the
/// payload goes out unchanged.
fn client_frame(opcode: u8, payload: &[u8]) -> Vec<u8> {
    let mut frame = vec![0x80 | opcode];
    match payload.len() {
        len if len < 126 => frame.push(0x80 | len as u8),
        len if len <= usize::from(u16::MAX) => {
            frame.push(0x80 | 126);
            frame.extend_from_slice(&(len as u16).to_be_bytes());
        }
        len => {
            frame.push(0x80 | 127);
            frame.extend_from_slice(&(len as u64).to_be_bytes());
        }
    }
    frame.extend_from_slice(&[0, 0, 0, 0]);
    frame.extend_from_slice(payload);
    frame
}

/// Wait up to `within` for the server to close `stream`: a read that returns 0 bytes or fails.
/// Anything the server writes first is read and ignored. Returns how long the close took, or
/// `None` if the connection was still open.
async fn closed_within(stream: &mut tokio::net::TcpStream, within: Duration) -> Option<Duration> {
    use tokio::io::AsyncReadExt;
    let start = Instant::now();
    let mut sink = [0u8; 1024];
    tokio::time::timeout(within, async {
        loop {
            match stream.read(&mut sink).await {
                Ok(0) | Err(_) => return,
                Ok(_) => {}
            }
        }
    })
    .await
    .ok()
    .map(|()| start.elapsed())
}

/// The wire format never sends Ping or Pong, so the receiver closes a connection that sends one:
/// answering instead let one peer that pinged and never read grow the receiver's heap without
/// limit (4 GiB in 8.4 s, measured), each ping also restarting the idle timer. With a cap of one
/// connection and idle and handshake timeouts of 20 s, a peer that sends a Ping (then, in a second
/// round, a Pong) must be closed within 2 s -- far sooner than any timeout could close it -- and a
/// legitimate message sent next must get the one permit and arrive, while the raw client's socket
/// is still held open on our side.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn websocket_closes_a_peer_that_sends_a_control_frame_and_frees_its_permit() {
    use tokio::io::AsyncWriteExt;
    const PING: u8 = 0x9;
    const PONG: u8 = 0xA;
    let config = HashMap::from([
        ("max_concurrent_connections".to_string(), "1".to_string()),
        ("idle_timeout_ms".to_string(), "20000".to_string()),
        ("handshake_timeout_ms".to_string(), "20000".to_string()),
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
    for (name, opcode) in [("Ping", PING), ("Pong", PONG)] {
        let mut peer = raw_websocket_client(pair.bob_port).await;
        peer.write_all(&client_frame(opcode, b"are you there"))
            .await
            .expect("write the control frame");
        let closed = closed_within(&mut peer, Duration::from_secs(2)).await;
        assert!(
            closed.is_some(),
            "a peer that sent a {name} must be closed at once, not held until a 20 s timeout"
        );

        let message = pair.signed(format!("after a {name}").as_bytes());
        let receipt = pair
            .alice_node
            .send_message(&pair.bob_target(), &message)
            .await
            .unwrap_or_else(|e| panic!("the permit the {name} peer held must be free: {e}"));
        pair.assert_receipt(&receipt, &DeliveryConfirmation::Sent);
        let arrived = poll_bob(&pair, 1, Duration::from_secs(3)).await;
        assert_eq!(
            arrived.len(),
            1,
            "the message after the {name} peer must arrive"
        );
        assert_eq!(
            arrived[0].incoming.message.message_id.0,
            message.message_id.0
        );
        pair.assert_verified_as_alice(&arrived[0]);
        drop(peer);
    }
}

/// Send `frame` to a new raw connection to Bob in `chunks` roughly equal pieces, `gap` apart. Stops
/// at the first write that fails: a receiver that cut the connection off may reset it. Returns the
/// connection and whether every chunk was written.
async fn trickle(
    port: u16,
    frame: &[u8],
    chunks: usize,
    gap: Duration,
) -> (tokio::net::TcpStream, bool) {
    use tokio::io::AsyncWriteExt;
    let mut peer = raw_websocket_client(port).await;
    for (i, chunk) in frame.chunks(frame.len().div_ceil(chunks)).enumerate() {
        if i > 0 {
            tokio::time::sleep(gap).await;
        }
        if peer.write_all(chunk).await.is_err() || peer.flush().await.is_err() {
            return (peer, false);
        }
    }
    (peer, true)
}

/// `idle_timeout_ms` is a gap between reads, not a deadline for a whole message. With it at 300 ms,
/// a message whose frame arrives in 10 chunks 100 ms apart -- about 0.9 s in all, three times the
/// idle timeout -- must be read and delivered. Before, the timeout wrapped the read of a whole
/// message, so a legitimate 1 MiB message on a link slower than about 210 KB/s was always cut off
/// at the default 5 s. The control: the same kind of frame with one gap of 900 ms, three times the
/// idle timeout, must be cut off, so the timeout is really enforced between reads.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn websocket_idle_timeout_is_a_gap_between_reads_not_a_deadline_for_a_message() {
    const IDLE: Duration = Duration::from_millis(300);
    let config = one_key("idle_timeout_ms", &IDLE.as_millis().to_string());
    let pair = Pair::with_config(
        TransportType::WebSocket,
        ws(),
        ws(),
        free_port(),
        free_port(),
        &config,
    )
    .await;

    let slow = pair.signed(b"slow but steady");
    let frame = client_frame(0x2, &serde_json::to_vec(&slow).expect("serialize"));
    let start = Instant::now();
    let (_peer, whole) = trickle(pair.bob_port, &frame, 10, Duration::from_millis(100)).await;
    let took = start.elapsed();
    let arrived = poll_bob(&pair, 1, Duration::from_secs(3)).await;
    assert_eq!(
        arrived.len(),
        1,
        "a frame whose bytes kept coming, never more than 100 ms apart, must not be cut off by a \
         {IDLE:?} idle timeout (every chunk written: {whole}; writing took {took:?})"
    );
    assert!(
        whole && took > IDLE * 2,
        "the frame must take well over the idle timeout to arrive, or this shows nothing: every \
         chunk written: {whole}, in {took:?}"
    );
    assert_eq!(arrived[0].incoming.message.message_id.0, slow.message_id.0);
    pair.assert_verified_as_alice(&arrived[0]);

    // Control: one gap of three idle timeouts inside the frame cuts it off.
    let stalled = pair.signed(b"stalls mid-frame");
    let frame = client_frame(0x2, &serde_json::to_vec(&stalled).expect("serialize"));
    let (_peer, _) = trickle(pair.bob_port, &frame, 2, IDLE * 3).await;
    let arrived = poll_bob(&pair, 1, Duration::from_secs(1)).await;
    assert!(
        arrived.is_empty(),
        "a frame that went silent for {:?} mid-way must be cut off by the {IDLE:?} idle timeout",
        IDLE * 3
    );
}

// ---------------------------------------------------------------------------------------------
// HTTP (plan Task 9): one request per connection to `POST /synapse/message`, answered 202 only
// once the message is queued -- so the sender's receipt is `Delivered`.
// ---------------------------------------------------------------------------------------------

fn http() -> Box<dyn TransportFactory> {
    Box::new(synapse::transport::HttpTransportFactory)
}

/// Before the repair, `start_server` bound nothing: it built an `HttpServer` record whose queue
/// nothing ever filled, so no message could arrive. The server now answers 2xx only after the
/// message is in its queue, so the receipt is `Delivered`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn http_carries_a_verified_message_end_to_end() {
    let (received, receipt) = round_trip(
        TransportType::Http,
        http(),
        http(),
        free_port(),
        free_port(),
        b"repaired",
        DeliveryConfirmation::Delivered,
    )
    .await;
    assert_eq!(received.incoming.transport_type, TransportType::Http);
    assert_eq!(receipt.transport_used, TransportType::Http);
    assert_eq!(
        receipt.metadata.get("status_code").map(String::as_str),
        Some("202")
    );
    assert_eq!(received.payload, Payload::Opened(b"repaired".to_vec()));
}

/// A 16 KiB body serialises to far more than one read: the server must read the whole body.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn http_carries_a_large_verified_message() {
    let payload: Vec<u8> = (0..16 * 1024).map(|i| (i % 251) as u8).collect();
    let (received, _receipt) = round_trip(
        TransportType::Http,
        http(),
        http(),
        free_port(),
        free_port(),
        &payload,
        DeliveryConfirmation::Delivered,
    )
    .await;
    assert!(received.payload == Payload::Opened(payload));
}

/// Each message arrives exactly once through the manager, however many times Bob polls after, and
/// -- because `Delivered` means already queued -- the first poll after the last receipt finds them
/// all. The manager's replay record would hide a transport's repeats, so the transport-level check
/// is http_unified's `receive_raw_hands_each_message_out_once`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn http_delivers_each_message_once_across_polls() {
    const K: usize = 5;
    const EXTRA_POLLS: usize = 10;
    let pair = Pair::new(
        TransportType::Http,
        http(),
        http(),
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
        pair.assert_receipt(&receipt, &DeliveryConfirmation::Delivered);
    }
    let mut arrived = pair.bob_node.receive_messages().await.expect("receive");
    assert_eq!(
        arrived.len(),
        K,
        "every message was receipted Delivered, so every one must already be queued"
    );
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
    assert_eq!(seen, sent);
    assert_eq!(arrived.len(), K, "{EXTRA_POLLS} more polls found repeats");
}

/// N = 200, 32 requests in flight, as for WebSocket: each send is two HTTP exchanges (the
/// manager's `estimate_metrics` probes before each send).
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn http_loses_no_message_under_concurrent_sends() {
    loses_no_message_under_concurrent_sends(
        TransportType::Http,
        http,
        200,
        DeliveryConfirmation::Delivered,
    )
    .await;
}

/// The sender refuses a message whose serialized form is over its `max_message_size` as
/// `MessageRefused`, before connecting: the refusal comes back from a target where nothing
/// listens, where a message that fits fails differently, at the connect (the control). Ten
/// refusals through the manager leave HTTP Running (the manager's breaker would open at the 10th
/// failure) and the next message is delivered. Before, the refusal was a `TransportError`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn http_refuses_an_oversize_message_before_connecting_and_stays_running() {
    const LIMIT: usize = 4096;
    const REFUSALS: usize = 10;
    let config = one_key("max_message_size", &LIMIT.to_string());
    let pair = Pair::with_config(
        TransportType::Http,
        http(),
        http(),
        free_port(),
        free_port(),
        &config,
    )
    .await;

    let alice_transport = synapse::transport::HttpTransportFactory
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
            error.contains("Http")
                && error.contains("max_message_size")
                && error.contains(&over_size.to_string()),
            "refusal {attempt}: the manager's error must carry HTTP's own reason: {error}"
        );
        assert_eq!(
            pair.alice_node
                .get_transport_status()
                .await
                .get(&TransportType::Http),
            Some(&TransportStatus::Running),
            "refusal {attempt}: a refused message must not mark HTTP failed"
        );
    }

    let receipt = pair
        .alice_node
        .send_message(&pair.bob_target(), &fits)
        .await
        .expect("after the refusals, the next message must still go through HTTP");
    pair.assert_receipt(&receipt, &DeliveryConfirmation::Delivered);
    let arrived = poll_bob(&pair, 1, Duration::from_secs(3)).await;
    assert_eq!(arrived.len(), 1, "only the message that fits may arrive");
    assert_eq!(arrived[0].incoming.message.message_id.0, fits.message_id.0);
    pair.assert_verified_as_alice(&arrived[0]);
    tokio::time::sleep(Duration::from_millis(300)).await;
    let after = pair.bob_node.receive_messages().await.expect("receive");
    assert!(after.is_empty(), "{} more messages arrived", after.len());
}

/// The head of a `POST /synapse/message`, with `framing` as its body-framing header lines.
fn post_head(framing: &str) -> Vec<u8> {
    format!(
        "POST /synapse/message HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\n\
         {framing}\r\n"
    )
    .into_bytes()
}

/// A whole `POST /synapse/message` carrying `body`.
fn post_request(body: &[u8]) -> Vec<u8> {
    let mut request = post_head(&format!("Content-Length: {}\r\n", body.len()));
    request.extend_from_slice(body);
    request
}

/// Read one response head from `reader` within `within`, returning its status and head, or `None`
/// if the connection closed or the time passed first.
async fn read_response_head<R: tokio::io::AsyncRead + Unpin>(
    reader: &mut R,
    within: Duration,
) -> Option<(u16, String)> {
    use tokio::io::AsyncReadExt;
    tokio::time::timeout(within, async {
        let mut head = Vec::new();
        let mut byte = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") {
            match reader.read(&mut byte).await {
                Ok(1) => head.push(byte[0]),
                _ => return None,
            }
        }
        let head = String::from_utf8_lossy(&head).to_string();
        let status = head.split(' ').nth(1)?.parse().ok()?;
        Some((status, head))
    })
    .await
    .ok()
    .flatten()
}

/// A body declared over the limit is refused with 413 from its `Content-Length`, before any of it
/// is read: the peer declares 1 GB and then trickles a byte every 100 ms, and the 413 must come
/// within 2 s, after at most a handful of bytes -- with a 20 s idle timeout, so no timeout can be
/// what answers. A body that does not declare its length (chunked) is refused the same way, with
/// 411, before any of it is read. Nothing is queued, and a message that fits still arrives.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn http_server_refuses_an_oversize_body_with_413_without_reading_it() {
    use tokio::io::AsyncWriteExt;
    const LIMIT: usize = 4096;
    let config = HashMap::from([
        ("max_message_size".to_string(), LIMIT.to_string()),
        ("idle_timeout_ms".to_string(), "20000".to_string()),
    ]);
    let pair = Pair::with_config(
        TransportType::Http,
        http(),
        http(),
        free_port(),
        free_port(),
        &config,
    )
    .await;
    let bob = ("127.0.0.1", pair.bob_port);

    let mut peer = tokio::net::TcpStream::connect(bob).await.expect("connect");
    peer.write_all(&post_head("Content-Length: 1000000000\r\n"))
        .await
        .expect("write the head");
    let (mut reader, mut writer) = peer.into_split();
    let trickled = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let trickler = {
        let trickled = Arc::clone(&trickled);
        tokio::spawn(async move {
            for _ in 0..100 {
                if writer.write_all(b"[").await.is_err() {
                    return;
                }
                trickled.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
    };
    let start = Instant::now();
    let answer = read_response_head(&mut reader, Duration::from_secs(2)).await;
    let took = start.elapsed();
    let bytes_sent = trickled.load(std::sync::atomic::Ordering::SeqCst);
    trickler.abort();
    assert_eq!(
        answer.as_ref().map(|(status, _)| *status),
        Some(413),
        "a 1 GB Content-Length must be answered 413 at once; after {took:?}: {answer:?}"
    );
    assert!(
        bytes_sent <= 5,
        "the 413 must come before the body is read, not after {bytes_sent} bytes"
    );

    // Chunked, with no Content-Length: refused at once, whatever follows.
    let mut peer = tokio::net::TcpStream::connect(bob).await.expect("connect");
    peer.write_all(&post_head("Transfer-Encoding: chunked\r\n"))
        .await
        .expect("write the head");
    let _ = peer.write_all(b"400\r\n[").await;
    let answered = read_response_head(&mut peer, Duration::from_secs(2)).await;
    assert_eq!(
        answered.as_ref().map(|(status, _)| *status),
        Some(411),
        "a body without a Content-Length must be refused before it is read: {answered:?}"
    );

    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        pair.bob_node
            .receive_messages()
            .await
            .expect("receive")
            .is_empty(),
        "nothing refused may be queued"
    );
    let (fits, _) = measured(&pair, LIMIT, true);
    let receipt = pair
        .alice_node
        .send_message(&pair.bob_target(), &fits)
        .await
        .expect("a message that fits is delivered");
    pair.assert_receipt(&receipt, &DeliveryConfirmation::Delivered);
    assert_eq!(poll_bob(&pair, 1, Duration::from_secs(3)).await.len(), 1);
}

/// Backpressure: requests wait. With a queue budget that holds two of its messages but not three,
/// and nobody polling, four messages are sent at once. The server answers only once a message is
/// queued, so exactly two sends return `Delivered` and the other two are still waiting -- not
/// answered, not refused, not lost -- when Bob first polls, which finds exactly two. Polling frees
/// the budget, the waiting two are queued and answered `Delivered`, and all four arrive once.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn http_holds_requests_past_its_queue_budget_until_polled() {
    let probe = Pair::new(
        TransportType::Http,
        http(),
        http(),
        free_port(),
        free_port(),
    )
    .await;
    let probe_largest = (0..4)
        .map(|i| wire_size(&probe.signed(&queued_body(i))))
        .max()
        .expect("four probes");
    drop(probe);
    let budget = 2 * (probe_largest + QUEUED_SIZE_SLACK);
    let config = HashMap::from([
        ("max_queued_bytes".to_string(), budget.to_string()),
        ("max_message_size".to_string(), budget.to_string()),
    ]);
    let pair = Arc::new(
        Pair::with_config(
            TransportType::Http,
            http(),
            http(),
            free_port(),
            free_port(),
            &config,
        )
        .await,
    );
    let messages: Vec<SecureMessage> = (0..4).map(|i| pair.signed(&queued_body(i))).collect();
    let mut sorted: Vec<usize> = messages.iter().map(wire_size).collect();
    sorted.sort_unstable();
    assert!(sorted[2] + sorted[3] <= budget, "any two must fit {budget}");
    assert!(
        sorted[0] + sorted[1] + sorted[2] > budget,
        "no three may fit {budget}"
    );

    let sends: Vec<_> = messages
        .iter()
        .cloned()
        .map(|message| {
            let pair = Arc::clone(&pair);
            tokio::spawn(async move {
                pair.alice_node
                    .send_message(&pair.bob_target(), &message)
                    .await
            })
        })
        .collect();
    // Long enough for Bob to read all four; nobody polls meanwhile.
    tokio::time::sleep(Duration::from_millis(2000)).await;
    let finished = sends.iter().filter(|send| send.is_finished()).count();
    assert_eq!(
        finished, 2,
        "with budget for 2, exactly 2 sends may have been answered before Bob polls"
    );

    let first = pair.bob_node.receive_messages().await.expect("receive");
    assert_eq!(first.len(), 2, "the first poll must find exactly 2 queued");
    let mut arrived = first;
    for send in sends {
        let receipt = tokio::time::timeout(Duration::from_secs(5), send)
            .await
            .expect("once polled, every waiting send is answered")
            .expect("send task")
            .expect("a waiting message is delivered, not refused");
        pair.assert_receipt(&receipt, &DeliveryConfirmation::Delivered);
    }
    arrived.extend(poll_bob(&pair, 2, Duration::from_secs(5)).await);
    let sent: HashSet<String> = messages
        .iter()
        .map(|m| m.message_id.0.to_string())
        .collect();
    let mut seen = HashSet::new();
    for message in &arrived {
        let id = message.incoming.message.message_id.0.to_string();
        assert!(seen.insert(id.clone()), "message {id} arrived twice");
        pair.assert_verified_as_alice(message);
    }
    assert_eq!(seen, sent, "every held message must arrive once polled");
}

/// A request that waits for queue budget longer than `request_timeout_ms` is answered 503 and its
/// message dropped: the sender gets an error, never a receipt, and the message never arrives,
/// even once Bob polls. The budget holds one message, which fills it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn http_answers_503_when_no_budget_frees_before_the_deadline() {
    const DEADLINE: Duration = Duration::from_millis(1000);
    let probe = Pair::new(
        TransportType::Http,
        http(),
        http(),
        free_port(),
        free_port(),
    )
    .await;
    let size = wire_size(&probe.signed(b"fills the budget")) + QUEUED_SIZE_SLACK;
    drop(probe);
    let config = HashMap::from([
        ("max_queued_bytes".to_string(), size.to_string()),
        ("max_message_size".to_string(), size.to_string()),
        (
            "request_timeout_ms".to_string(),
            DEADLINE.as_millis().to_string(),
        ),
    ]);
    let pair = Pair::with_config(
        TransportType::Http,
        http(),
        http(),
        free_port(),
        free_port(),
        &config,
    )
    .await;
    let first = pair.signed(b"fills the budget");
    let receipt = pair
        .alice_node
        .send_message(&pair.bob_target(), &first)
        .await
        .expect("the first message fills the budget");
    pair.assert_receipt(&receipt, &DeliveryConfirmation::Delivered);

    let alice = synapse::transport::HttpTransportFactory
        .create_transport(&HashMap::new())
        .await
        .expect("a sender");
    let second = pair.signed(b"finds no room");
    let start = Instant::now();
    match alice.send_message(&pair.bob_target(), &second).await {
        Err(synapse::SynapseError::TransportError(why)) => {
            assert!(why.contains("503"), "{why}")
        }
        other => panic!("a request that found no budget must be a 503 error, not {other:?}"),
    }
    let took = start.elapsed();
    assert!(
        took >= DEADLINE - Duration::from_millis(150) && took < DEADLINE + Duration::from_secs(2),
        "the 503 must come at the {DEADLINE:?} deadline, came after {took:?}"
    );
    let arrived = poll_bob(&pair, 2, Duration::from_millis(500)).await;
    let ids: Vec<_> = arrived
        .iter()
        .map(|m| m.incoming.message.message_id.0)
        .collect();
    assert_eq!(
        ids,
        vec![first.message_id.0],
        "only the first message may arrive"
    );
}

/// With `max_concurrent_connections` = 2 and two connections held open, a third request is not
/// read until one of them closes: the accept loop waits for a permit. The held connections send
/// nothing, under a 20 s header timeout, so only the cap holds Alice back.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn http_reads_no_more_connections_at_once_than_its_cap() {
    let config = HashMap::from([
        ("max_concurrent_connections".to_string(), "2".to_string()),
        ("header_read_timeout_ms".to_string(), "20000".to_string()),
    ]);
    let pair = Arc::new(
        Pair::with_config(
            TransportType::Http,
            http(),
            http(),
            free_port(),
            free_port(),
            &config,
        )
        .await,
    );
    let bob = ("127.0.0.1", pair.bob_port);
    let first = tokio::net::TcpStream::connect(bob).await.expect("connect");
    let second = tokio::net::TcpStream::connect(bob).await.expect("connect");
    tokio::time::sleep(Duration::from_millis(300)).await;

    let message = pair.signed(b"behind the cap");
    let send = {
        let (pair, message) = (Arc::clone(&pair), message.clone());
        tokio::spawn(async move {
            pair.alice_node
                .send_message(&pair.bob_target(), &message)
                .await
        })
    };
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert!(
        !send.is_finished(),
        "with two connections held and a cap of 2, a third request was answered"
    );
    assert!(
        pair.bob_node
            .receive_messages()
            .await
            .expect("receive")
            .is_empty(),
        "with two connections held and a cap of 2, a third was read anyway"
    );

    drop(first);
    let receipt = tokio::time::timeout(Duration::from_secs(10), send)
        .await
        .expect("once a held connection closes, the waiting request is answered")
        .expect("send task")
        .expect("delivered");
    pair.assert_receipt(&receipt, &DeliveryConfirmation::Delivered);
    let arrived = poll_bob(&pair, 1, Duration::from_secs(3)).await;
    assert_eq!(arrived.len(), 1);
    assert_eq!(
        arrived[0].incoming.message.message_id.0,
        message.message_id.0
    );
    pair.assert_verified_as_alice(&arrived[0]);
    drop(second);
}

/// Slowloris, on the request head: with a connection cap of 2, one peer connects and sends nothing
/// and another trickles its head a byte every 100 ms, never finishing it. The header timeout
/// (500 ms here) closes both, so Alice's message, waiting in the backlog behind them, is delivered
/// within that timeout plus a margin -- and not sooner than most of it, which shows the two really
/// held both permits. The trickling peer is cut off however steadily it sends.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn http_closes_connections_that_do_not_finish_their_head() {
    const HEADER: Duration = Duration::from_millis(500);
    let config = HashMap::from([
        ("max_concurrent_connections".to_string(), "2".to_string()),
        (
            "header_read_timeout_ms".to_string(),
            HEADER.as_millis().to_string(),
        ),
    ]);
    let pair = Pair::with_config(
        TransportType::Http,
        http(),
        http(),
        free_port(),
        free_port(),
        &config,
    )
    .await;
    let bob = ("127.0.0.1", pair.bob_port);
    let silent = tokio::net::TcpStream::connect(bob).await.expect("connect");
    let mut trickling = tokio::net::TcpStream::connect(bob).await.expect("connect");
    let head = post_head("Content-Length: 10\r\n");
    let trickler = tokio::spawn(async move {
        use tokio::io::AsyncWriteExt;
        let start = Instant::now();
        // Far more bytes than the head, so the head never ends: a byte every 100 ms for 20 s.
        for byte in head.iter().take(head.len() - 4).cycle().take(200) {
            if trickling.write_all(&[*byte]).await.is_err() {
                return Some(start.elapsed());
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        None
    });

    let message = pair.signed(b"behind two slow heads");
    let start = Instant::now();
    let receipt = pair
        .alice_node
        .send_message(&pair.bob_target(), &message)
        .await
        .expect("delivered once the slow peers are closed");
    let waited = start.elapsed();
    pair.assert_receipt(&receipt, &DeliveryConfirmation::Delivered);
    assert!(
        waited >= HEADER - Duration::from_millis(150),
        "delivered after {waited:?}, before the slow peers could have timed out, so they never \
         held the permits and the test shows nothing"
    );
    assert!(
        waited < HEADER + Duration::from_secs(3),
        "two slow heads held both permits for {waited:?}; the {HEADER:?} header timeout did not \
         close them"
    );
    let cut = tokio::time::timeout(Duration::from_secs(5), trickler)
        .await
        .expect("the trickler stops")
        .expect("trickler task");
    assert!(
        cut.is_some_and(|after| after < HEADER + Duration::from_secs(2)),
        "a head trickled a byte every 100 ms must be cut off near the {HEADER:?} header timeout: \
         {cut:?}"
    );
    assert_eq!(poll_bob(&pair, 1, Duration::from_secs(3)).await.len(), 1);
    drop(silent);
}

/// Slowloris, on the body: `idle_timeout_ms` is a gap between reads, not a deadline for the whole
/// body. With it at 300 ms, a request whose body arrives in 10 pieces 100 ms apart -- about 1 s in
/// all -- is answered 202 and its message delivered. The control: the same kind of request with
/// one gap of 900 ms in its body is answered 408 at about the idle timeout, and its message never
/// arrives.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn http_idle_timeout_is_a_gap_between_body_reads_not_a_deadline() {
    use tokio::io::AsyncWriteExt;
    const IDLE: Duration = Duration::from_millis(300);
    let config = one_key("idle_timeout_ms", &IDLE.as_millis().to_string());
    let pair = Pair::with_config(
        TransportType::Http,
        http(),
        http(),
        free_port(),
        free_port(),
        &config,
    )
    .await;
    let bob = ("127.0.0.1", pair.bob_port);

    let slow = pair.signed(b"slow but steady");
    let json = serde_json::to_vec(&slow).expect("serialize");
    let mut peer = tokio::net::TcpStream::connect(bob).await.expect("connect");
    let start = Instant::now();
    peer.write_all(&post_head(&format!("Content-Length: {}\r\n", json.len())))
        .await
        .expect("the head");
    for piece in json.chunks(json.len().div_ceil(10)) {
        tokio::time::sleep(Duration::from_millis(100)).await;
        peer.write_all(piece).await.expect("a body piece");
    }
    let response = read_response_head(&mut peer, Duration::from_secs(5)).await;
    let took = start.elapsed();
    assert_eq!(
        response.as_ref().map(|(status, _)| *status),
        Some(202),
        "a body whose bytes kept coming, never more than 100 ms apart, must not be cut off by a \
         {IDLE:?} idle timeout (took {took:?}): {response:?}"
    );
    assert!(
        took > IDLE * 3,
        "the body must take well over the idle timeout: {took:?}"
    );
    let arrived = poll_bob(&pair, 1, Duration::from_secs(3)).await;
    assert_eq!(arrived.len(), 1);
    assert_eq!(arrived[0].incoming.message.message_id.0, slow.message_id.0);
    pair.assert_verified_as_alice(&arrived[0]);

    // Control: half the body, then silence for three idle timeouts.
    let stalled = pair.signed(b"stalls mid-body");
    let json = serde_json::to_vec(&stalled).expect("serialize");
    let mut peer = tokio::net::TcpStream::connect(bob).await.expect("connect");
    peer.write_all(&post_head(&format!("Content-Length: {}\r\n", json.len())))
        .await
        .expect("the head");
    peer.write_all(&json[..json.len() / 2])
        .await
        .expect("half the body");
    let start = Instant::now();
    let response = read_response_head(&mut peer, IDLE * 3).await;
    let took = start.elapsed();
    assert_eq!(
        response.as_ref().map(|(status, _)| *status),
        Some(408),
        "a body that went silent must be cut off by the {IDLE:?} idle timeout: {response:?}"
    );
    assert!(
        took >= IDLE - Duration::from_millis(100),
        "the 408 came after {took:?}, before the idle timeout could have passed"
    );
    let _ = peer.write_all(&json[json.len() / 2..]).await;
    let arrived = poll_bob(&pair, 1, Duration::from_secs(1)).await;
    assert!(arrived.is_empty(), "the cut-off message must not arrive");
}

/// One request per connection: two requests written back to back on one connection (HTTP/1.1
/// pipelining) get one answer, 202 with `connection: close`, then the connection ends; only the
/// first message is queued. And a body that is not a `SecureMessage`, or is empty, is answered
/// 400, queuing nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn http_answers_one_request_per_connection_and_refuses_what_does_not_parse() {
    use tokio::io::AsyncWriteExt;
    let pair = Pair::new(
        TransportType::Http,
        http(),
        http(),
        free_port(),
        free_port(),
    )
    .await;
    let bob = ("127.0.0.1", pair.bob_port);
    let first = pair.signed(b"first of two");
    let second = pair.signed(b"pipelined behind it");
    let mut both = post_request(&serde_json::to_vec(&first).expect("serialize"));
    both.extend(post_request(
        &serde_json::to_vec(&second).expect("serialize"),
    ));
    let mut peer = tokio::net::TcpStream::connect(bob).await.expect("connect");
    peer.write_all(&both).await.expect("write both");
    let (status, head) = read_response_head(&mut peer, Duration::from_secs(5))
        .await
        .expect("the first request is answered");
    assert_eq!(status, 202, "{head}");
    assert!(
        head.to_ascii_lowercase().contains("connection: close"),
        "the answer must close the connection: {head}"
    );
    assert!(
        closed_within(&mut peer, Duration::from_secs(2))
            .await
            .is_some(),
        "the connection must end after one answer"
    );
    let arrived = poll_bob(&pair, 2, Duration::from_millis(1000)).await;
    let ids: Vec<_> = arrived
        .iter()
        .map(|m| m.incoming.message.message_id.0)
        .collect();
    assert_eq!(
        ids,
        vec![first.message_id.0],
        "only the first request may be read"
    );

    for body in [&b"not json"[..], b"{\"message_id\": 7}", b""] {
        let mut peer = tokio::net::TcpStream::connect(bob).await.expect("connect");
        peer.write_all(&post_request(body)).await.expect("write");
        let answer = read_response_head(&mut peer, Duration::from_secs(5)).await;
        assert_eq!(
            answer.map(|(status, _)| status),
            Some(400),
            "{:?} must be answered 400",
            String::from_utf8_lossy(body)
        );
    }
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(
        pair.bob_node
            .receive_messages()
            .await
            .expect("receive")
            .is_empty()
    );
}

/// Refuses `config`, with an error naming `key`, at construction and in `validate_config`.
async fn http_refuses(config: HashMap<String, String>, key: &str) {
    match synapse::transport::HttpTransportFactory
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
        synapse::transport::HttpTransportFactory
            .validate_config(&config)
            .is_err(),
        "validate_config must refuse {config:?} too"
    );
}

/// Every limit, timeout and the port refuse a value that does not parse, or a zero, naming the
/// key, instead of silently becoming the default; the queue budget must hold the largest message
/// and fit a `u32`; and the retired keys are refused rather than ignored. Valid values are the
/// positive controls. Before, `server_port` fell back to 0 (no server) and `max_message_size` to
/// 10 MiB on a typo, `use_https` to true, and there were no other limits.
#[tokio::test]
async fn http_refuses_an_invalid_config() {
    let factory = synapse::transport::HttpTransportFactory;
    for key in [
        "max_message_size",
        "max_concurrent_connections",
        "timeout_ms",
        "header_read_timeout_ms",
        "idle_timeout_ms",
        "request_timeout_ms",
    ] {
        for bad in ["", "abc", "0", "-1", "1MiB"] {
            http_refuses(one_key(key, bad), key).await;
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
        http_refuses(one_key("server_port", bad), "server_port").await;
    }
    for bad in ["", "yes", "1", "TRUE"] {
        http_refuses(one_key("use_https", bad), "use_https").await;
    }
    for (key, good) in [
        ("server_port", "0"),
        ("use_https", "true"),
        ("use_https", "false"),
    ] {
        assert!(
            factory.create_transport(&one_key(key, good)).await.is_ok(),
            "{key} = {good}"
        );
    }
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
        http_refuses(budget(b, m), "max_queued_bytes").await;
    }
    http_refuses(one_key("max_queued_bytes", "4096"), "max_queued_bytes").await;
    assert!(
        factory
            .create_transport(&budget("4096", "4096"))
            .await
            .is_ok()
    );
    http_refuses(one_key("server_address", "0.0.0.0"), "server_address").await;
    http_refuses(one_key("max_connections", "10"), "max_connections").await;
    http_refuses(one_key("bind_scope", "everywhere"), "bind_scope").await;
    assert!(factory.validate_config(&factory.default_config()).is_ok());
}

/// A target must be what is dialled: an `http://` or `https://` URL or `host:port`, with a host and
/// a port, no userinfo, no fragment, and a host the URL parser keeps as written. `can_reach` says
/// so without touching the network, and `send_message` refuses the same targets. Bob's own address,
/// in three accepted forms, is the positive control. (The full list is http_unified's unit test
/// `target_addresses`.)
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn http_refuses_targets_it_would_not_dial_as_named() {
    let pair = Pair::new(
        TransportType::Http,
        http(),
        http(),
        free_port(),
        free_port(),
    )
    .await;
    let alice = synapse::transport::HttpTransportFactory
        .create_transport(&HashMap::new())
        .await
        .expect("a sender");
    let message = pair.signed(b"never sent");
    let port = pair.bob_port;
    for bad in [
        format!("ws://127.0.0.1:{port}"),
        format!("ftp://127.0.0.1:{port}"),
        "127.0.0.1".to_string(),
        "http://127.0.0.1/".to_string(),
        format!("http://user@127.0.0.1:{port}/"),
        format!("http://127.0.0.1:{port}/#fragment"),
        format!("http://127.1:{port}/"),
        format!("http://0x7f.1:{port}/"),
        format!("127.0.0.1:{port}/synapse/message"),
        "127.0.0.1:0".to_string(),
    ] {
        let target = TransportTarget::new(BOB.to_string()).with_address(bad.clone());
        assert!(
            !alice.can_reach(&target).await,
            "{bad} must not be reachable"
        );
        assert!(
            alice.send_message(&target, &message).await.is_err(),
            "{bad} must be refused"
        );
    }
    assert!(
        !alice
            .can_reach(&TransportTarget::new(BOB.to_string()))
            .await,
        "a target with no address is refused"
    );
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(
        pair.bob_node
            .receive_messages()
            .await
            .expect("receive")
            .is_empty(),
        "no refused target may have reached Bob"
    );
    for good in [
        format!("127.0.0.1:{port}"),
        format!("http://127.0.0.1:{port}"),
        format!("http://127.0.0.1:{port}/synapse/message"),
    ] {
        let target = TransportTarget::new(BOB.to_string()).with_address(good.clone());
        assert!(alice.can_reach(&target).await, "{good}");
        let receipt = alice
            .send_message(&target, &pair.signed(good.as_bytes()))
            .await
            .unwrap_or_else(|e| panic!("{good} must be delivered: {e}"));
        pair.assert_receipt(&receipt, &DeliveryConfirmation::Delivered);
    }
    assert_eq!(poll_bob(&pair, 3, Duration::from_secs(3)).await.len(), 3);
}

/// Connected only after a real HTTP exchange: against Bob's node the probe is answered (405, since
/// the one route is POST) and reports a measured round trip, and `estimate_metrics` says available;
/// against a port where nothing listens, neither is claimed, and the latency reported is the
/// timeout, not the time the failure took. The probes are not messages.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn http_reports_connectivity_only_after_a_real_exchange() {
    let pair = Pair::new(
        TransportType::Http,
        http(),
        http(),
        free_port(),
        free_port(),
    )
    .await;
    let transport = synapse::transport::HttpTransportFactory
        .create_transport(&one_key("timeout_ms", "2000"))
        .await
        .expect("construct");
    let live = transport
        .test_connectivity(&pair.bob_target())
        .await
        .expect("connectivity");
    assert!(live.connected, "{live:?}");
    assert!(live.rtt.is_some());
    assert_eq!(
        live.details.get("status_code").map(String::as_str),
        Some("405")
    );
    let estimate = transport
        .estimate_metrics(&pair.bob_target())
        .await
        .expect("estimate");
    assert!(estimate.available);
    assert_eq!(estimate.bandwidth, 1, "no send measured, so no bandwidth");

    let nobody =
        TransportTarget::new(BOB.to_string()).with_address(format!("127.0.0.1:{}", free_port()));
    let dead = transport
        .test_connectivity(&nobody)
        .await
        .expect("connectivity");
    assert!(!dead.connected, "{dead:?}");
    assert_eq!(dead.rtt, None);
    let estimate = transport.estimate_metrics(&nobody).await.expect("estimate");
    assert!(!estimate.available);
    assert_eq!(estimate.latency, Duration::from_millis(2000));
    assert!(estimate.confidence < 0.5);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        pair.bob_node
            .receive_messages()
            .await
            .expect("receive")
            .is_empty()
    );
}

fn nat() -> Box<dyn TransportFactory> {
    Box::new(synapse::transport::NatTraversalTransportFactory)
}

/// NAT traversal (`nat_traversal.rs`) had no `TransportType` and no factory, so nothing could
/// register it; `receive_raw` also bound a second UDP socket to the address `start` already held,
/// which the OS refuses, so it never received anything; and `send_message` built its wire format
/// with `String::from_utf8_lossy(&message.encrypted_content)`, which replaces invalid-UTF-8 bytes
/// in sealed ciphertext with U+FFFD, corrupting it. This test proves the repair: the transport
/// registers, binds once, and carries a verified message intact over loopback. Real NAT traversal
/// (STUN/UPnP/ICE) needs an actual NAT to exercise and is not what this test checks.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn nat_traversal_carries_a_verified_message_end_to_end() {
    let (received, receipt) = round_trip(
        TransportType::NatTraversal,
        nat(),
        nat(),
        free_port(),
        free_port(),
        b"repaired",
        DeliveryConfirmation::Sent,
    )
    .await;
    assert_eq!(
        received.incoming.transport_type,
        TransportType::NatTraversal
    );
    assert_eq!(receipt.transport_used, TransportType::NatTraversal);
    assert_eq!(received.payload, Payload::Opened(b"repaired".to_vec()));
}

// ---------------------------------------------------------------------------------------------
// QUIC (plan Task 1): one connection, one stream, one message, over loopback.
// ---------------------------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------------------------
// QUIC (plan Task 2): connection pooling -- reuse, multiplexed streams, idle-timeout eviction.
// ---------------------------------------------------------------------------------------------

/// N = 200, 32 sends in flight, as for WebSocket and HTTP: proves no message is lost under
/// concurrent sends, but -- because Task 1's one-connection-per-message code is already correct,
/// just wasteful -- this alone cannot distinguish pooled from unpooled. See
/// `quic_reuses_one_connection_for_two_sends_to_the_same_peer` for the test that can.
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn quic_loses_no_message_under_concurrent_sends() {
    loses_no_message_under_concurrent_sends(
        TransportType::Quic,
        quic,
        200,
        DeliveryConfirmation::Sent,
    )
    .await;
}

/// Two sends to the same peer must not open two connections: the second reuses the first's, and
/// the pool ends up holding exactly one entry for Bob's address.
///
/// Alice's sends here go through a raw `QuicTransportImpl` (the same production type
/// `TransportManager` wraps) instead of through `pair.alice_node`: `TransportManager` stores
/// transports as `Box<dyn Transport>` with no way to downcast back to the concrete type, so a
/// test that needs to inspect a transport's own pool has to hold that concrete type itself,
/// the way `websocket_closes_a_peer_that_sends_a_control_frame_and_frees_its_permit` drives a raw
/// socket directly rather than going through a manager for the half of the exchange it needs to
/// inspect. Bob's side is still a full node: a real `TransportManager` receiving and verifying
/// exactly as every other transport-repair test checks.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn quic_reuses_one_connection_for_two_sends_to_the_same_peer() {
    let pair = Pair::new(
        TransportType::Quic,
        quic(),
        quic(),
        free_port(),
        free_port(),
    )
    .await;
    let alice_quic = QuicTransportImpl::new(&HashMap::new())
        .await
        .expect("construct a raw QUIC sender");

    let m1 = pair.signed(b"first");
    let m2 = pair.signed(b"second");
    alice_quic
        .send_message(&pair.bob_target(), &m1)
        .await
        .expect("send 1");
    alice_quic
        .send_message(&pair.bob_target(), &m2)
        .await
        .expect("send 2");

    assert_eq!(
        alice_quic.pool_size().await,
        1,
        "two sends to the same peer must reuse one pooled connection, not open two"
    );

    let received = poll_bob(&pair, 2, Duration::from_secs(3)).await;
    assert_eq!(received.len(), 2, "both messages must arrive");
    let mut ids: Vec<_> = received
        .iter()
        .map(|r| r.incoming.message.message_id.0.to_string())
        .collect();
    ids.sort();
    let mut expected = vec![m1.message_id.0.to_string(), m2.message_id.0.to_string()];
    expected.sort();
    assert_eq!(ids, expected);
}

/// An idle connection is evicted and the next send transparently reopens one: both messages must
/// still arrive even though the pool dropped its entry for Bob's address in between.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn quic_reopens_after_the_pool_evicts_an_idle_connection() {
    let config = one_key("idle_timeout_ms", "200");
    let pair = Pair::with_config(
        TransportType::Quic,
        quic(),
        quic(),
        free_port(),
        free_port(),
        &config,
    )
    .await;

    let m1 = pair.signed(b"first");
    pair.alice_node
        .send_message(&pair.bob_target(), &m1)
        .await
        .expect("send 1");

    // Past the idle timeout, plus room for the sweep (woken at half the idle timeout) to run.
    tokio::time::sleep(Duration::from_millis(500)).await;

    let m2 = pair.signed(b"second");
    pair.alice_node
        .send_message(&pair.bob_target(), &m2)
        .await
        .expect("send 2 after reopen");

    let received = poll_bob(&pair, 2, Duration::from_secs(3)).await;
    assert_eq!(
        received.len(),
        2,
        "both messages must arrive even after an idle-timeout reopen"
    );
}

/// `evict()` removes a peer's pooled connection directly (spec §3 condition 3 -- Task 2 only needs
/// to provide the method and test it, not wire it to a verification failure): after a send has
/// populated the pool, evicting Bob's address drops the pool back to empty, and the *next* send to
/// the same address must still succeed by transparently establishing a fresh connection (proven by
/// `pool_size()` going back up to 1, not just by re-checking the old entry).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn quic_evict_drops_the_pooled_connection_and_the_next_send_reopens_one() {
    let pair = Pair::new(
        TransportType::Quic,
        quic(),
        quic(),
        free_port(),
        free_port(),
    )
    .await;
    let alice_quic = QuicTransportImpl::new(&HashMap::new())
        .await
        .expect("construct a raw QUIC sender");
    let bob_addr: SocketAddr = pair
        .bob_target()
        .address
        .as_ref()
        .expect("bob_target has an address")
        .parse()
        .expect("bob_target's address parses");

    let m1 = pair.signed(b"before evict");
    alice_quic
        .send_message(&pair.bob_target(), &m1)
        .await
        .expect("send 1");
    assert_eq!(
        alice_quic.pool_size().await,
        1,
        "the first send must populate the pool"
    );

    alice_quic.evict(bob_addr).await;
    assert_eq!(
        alice_quic.pool_size().await,
        0,
        "evict() must remove the pooled connection"
    );

    let m2 = pair.signed(b"after evict");
    alice_quic
        .send_message(&pair.bob_target(), &m2)
        .await
        .expect("send 2 must transparently reopen a connection after eviction");
    assert_eq!(
        alice_quic.pool_size().await,
        1,
        "the send after evict() must re-establish and re-pool a connection"
    );

    let received = poll_bob(&pair, 2, Duration::from_secs(3)).await;
    assert_eq!(
        received.len(),
        2,
        "both messages must arrive, evicted connection notwithstanding"
    );
}

/// `idle_timeout_ms` = 0 or unparseable must be refused, not silently replaced by the default.
#[tokio::test]
async fn quic_refuses_an_unparseable_or_zero_idle_timeout() {
    for bad in ["", "abc", "0", "-1", "1s"] {
        let config = one_key("idle_timeout_ms", bad);
        assert!(
            synapse::transport::QuicTransportFactory
                .validate_config(&config)
                .is_err(),
            "idle_timeout_ms = {bad:?} must be refused"
        );
        assert!(
            QuicTransportImpl::new(&config).await.is_err(),
            "idle_timeout_ms = {bad:?} must be refused by new(), not just validate_config"
        );
    }
    let good = one_key("idle_timeout_ms", "1000");
    synapse::transport::QuicTransportFactory
        .validate_config(&good)
        .expect("a valid idle_timeout_ms must be accepted");
}
