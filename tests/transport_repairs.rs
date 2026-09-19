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
        TransportType::Udp => Socket::Udp,
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
        )
        .await;
        let bob_node = node(kind, bob_factory, bob_port, bob_store, bob_sealing).await;
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
        match &received.sender {
            SenderVerdict::Verified { key_id } => assert_eq!(
                key_id,
                &synapse::sender_auth::key_id(&self.alice_signing_key),
                "the verdict must pin Alice's signing key"
            ),
            other => panic!("the message must arrive Verified: {other:?}"),
        }
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
    const N: usize = 400;
    let pair =
        Arc::new(Pair::new(TransportType::Tcp, tcp(), tcp(), free_port(), free_port()).await);
    let messages: Vec<SecureMessage> = (0..N)
        .map(|i| pair.signed(format!("concurrent {i}").as_bytes()))
        .collect();
    let sent: HashSet<String> = messages
        .iter()
        .map(|m| m.message_id.0.to_string())
        .collect();
    assert_eq!(sent.len(), N, "message ids must be distinct");

    let poller = {
        let pair = Arc::clone(&pair);
        tokio::spawn(async move {
            let mut received = Vec::new();
            let deadline = Instant::now() + Duration::from_secs(10);
            while received.len() < N && Instant::now() < deadline {
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
        assert!(sent.contains(&id), "message {id} was never sent");
        assert!(message.sender.is_verified(), "{:?}", message.sender);
        assert!(message.payload.is_open(), "{:?}", message.payload);
    }
    assert_eq!(
        received.len(),
        N,
        "{} of {N} concurrently sent messages were lost",
        N - received.len()
    );
    tokio::time::sleep(Duration::from_millis(200)).await;
    let after = pair.bob_node.receive_messages().await.expect("receive");
    assert!(after.is_empty(), "{} extra messages arrived", after.len());
}
