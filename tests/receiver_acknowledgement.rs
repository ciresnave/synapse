// SPDX-License-Identifier: MIT OR Apache-2.0
//! Receiver acknowledgement — spec: docs/superpowers/specs/2026-09-17-receiver-acknowledgement-design.md
//! Test numbers refer to the spec's §8. Negative tests prove the bad ack ARRIVED with a canary
//! (spec §9): "status stays Sent" is only asserted once a message sent after it has been received.

use std::collections::HashMap;
use std::time::Duration;

use synapse::CryptoManager;
use synapse::delivery_ack::{self, build_ack, message_digest};
use synapse::error::SynapseError;
use synapse::sender_auth::{ContradictedReason, SenderVerdict, TrustStore, UnverifiableReason};
use synapse::transport::{
    DeliveryConfirmation, IncomingMessage, ReceivedMessage, TransportManager,
    TransportManagerBuilder, TransportTarget, TransportType, UdpTransportFactory,
};
use synapse::types::{SecureMessage, SecurityLevel};

const ALICE: &str = "alice@synapse.test";
const BOB: &str = "bob@synapse.test";
const CAROL: &str = "carol@synapse.test";
const CANARY: &str = "canary@synapse.test";

fn identity() -> CryptoManager {
    let mut crypto = CryptoManager::new();
    crypto.generate_keypair().unwrap();
    crypto
}

fn pinned(entries: &[(&str, &CryptoManager)]) -> TrustStore {
    let mut store = TrustStore::new();
    for (id, crypto) in entries {
        store.pin(*id, crypto.public_key_bytes().unwrap());
    }
    store
}

fn free_udp_port() -> u16 {
    let socket = std::net::UdpSocket::bind("127.0.0.1:0").expect("bind ephemeral");
    socket.local_addr().expect("local_addr").port()
}

async fn node(store: TrustStore) -> (TransportManager, u16) {
    node_with_gate(store, synapse::replay::GateConfig::default()).await
}

/// Like `node`, but with an explicit gate config (P2 slice e). Used only where a test's point is
/// independent of the gate's default-deny behaviour, which `tests/replay_suppression.rs` covers.
async fn node_with_gate(
    store: TrustStore,
    gate: synapse::replay::GateConfig,
) -> (TransportManager, u16) {
    let port = free_udp_port();
    let mut udp = HashMap::new();
    udp.insert("bind_port".to_string(), port.to_string());
    let manager = TransportManagerBuilder::new()
        .disable_transport(TransportType::Tcp)
        .disable_transport(TransportType::Http)
        .disable_transport(TransportType::Email)
        .disable_transport(TransportType::AutoDiscovery)
        .transport_config(TransportType::Udp, udp)
        .trust_store(store)
        .gate_config(gate)
        .build();
    manager
        .register_factory(Box::new(UdpTransportFactory))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), manager.start())
        .await
        .expect("start() returns")
        .expect("start() succeeds");
    (manager, port)
}

fn target(id: &str, port: u16) -> TransportTarget {
    TransportTarget::new(id.to_string()).with_address(format!("127.0.0.1:{port}"))
}

fn id_of(message: &SecureMessage) -> String {
    message.message_id.0.to_string()
}

/// A signed message that asks for an ack at `reply_port`.
fn request(signer: &CryptoManager, from: &str, to: &str, reply_port: u16) -> SecureMessage {
    let mut m = SecureMessage::new(
        to,
        from,
        b"please ack".to_vec(),
        SecurityLevel::Authenticated,
    );
    m.request_ack(format!("127.0.0.1:{reply_port}"));
    signer.sign_secure_message(&mut m).unwrap();
    m
}

/// An ack from `from` for `for_id` carrying `digest`, signed by `signer`.
fn ack_with(from: &str, for_id: &str, digest: &str, signer: &CryptoManager) -> SecureMessage {
    let mut ack = SecureMessage::new(ALICE, from, Vec::new(), SecurityLevel::Authenticated);
    ack.add_metadata(delivery_ack::ACK_FOR_KEY, for_id);
    ack.add_metadata(delivery_ack::ACK_DIGEST_KEY, digest);
    signer.sign_secure_message(&mut ack).unwrap();
    ack
}

/// Send `message` from a throwaway socket, bypassing any manager.
fn send_raw(port: u16, message: &SecureMessage) {
    let raw = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    raw.send_to(&serde_json::to_vec(message).unwrap(), ("127.0.0.1", port))
        .unwrap();
}

/// Poll one reader for up to 2 s. `receive_messages` drains, so there must be a single reader.
async fn receive_one(manager: &TransportManager) -> ReceivedMessage {
    for _ in 0..20 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        let mut batch = manager.receive_messages().await.expect("receive");
        if let Some(first) = batch.pop() {
            assert!(batch.is_empty(), "expected exactly one message");
            return first;
        }
    }
    panic!("no message arrived within 2 s");
}

/// Send a canary to `port` and pump `manager` until it reaches the application (up to 3 s).
/// Returns the OTHER application messages seen meanwhile.
///
/// The canary must be signed and its key pinned under `CANARY` wherever this is called
/// (default-deny would otherwise drop the canary itself as `Unverifiable`, hanging the test), so
/// `canary_signer` is a `CryptoManager` whose key the receiving node's trust store already pins.
async fn pump_past_canary(
    manager: &TransportManager,
    port: u16,
    canary_signer: &CryptoManager,
) -> Vec<ReceivedMessage> {
    let mut canary = SecureMessage::new(ALICE, CANARY, b"canary".to_vec(), SecurityLevel::Public);
    canary_signer.sign_secure_message(&mut canary).unwrap();
    let canary_id = id_of(&canary);
    send_raw(port, &canary);
    let mut others = Vec::new();
    for _ in 0..30 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        for received in manager.receive_messages().await.unwrap() {
            if id_of(&received.incoming.message) == canary_id {
                return others;
            }
            others.push(received);
        }
    }
    panic!("the canary never arrived, so nothing sent before it can be assumed received");
}

/// Pump until `id` is Acknowledged (up to 3 s). Returns the final status and the application
/// messages seen.
async fn pump_until_acknowledged(
    manager: &TransportManager,
    id: &str,
) -> (Option<DeliveryConfirmation>, Vec<ReceivedMessage>) {
    let mut seen = Vec::new();
    for _ in 0..30 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        seen.extend(manager.receive_messages().await.unwrap());
        if manager.delivery_status(id).await == Some(DeliveryConfirmation::Acknowledged) {
            break;
        }
    }
    (manager.delivery_status(id).await, seen)
}

struct Pair {
    alice_c: CryptoManager,
    bob_c: CryptoManager,
    carol_c: CryptoManager,
    /// Signs `pump_past_canary`'s canary; pinned at `alice`'s trust store under `CANARY` so the
    /// default-deny gate does not drop it (see ruling 6 / task-3-report.md).
    canary_c: CryptoManager,
    alice: TransportManager,
    alice_port: u16,
    bob: TransportManager,
    bob_port: u16,
}

/// Alice pins Bob, Carol and the canary; Bob pins Alice.
async fn pair() -> Pair {
    let (alice_c, bob_c, carol_c, canary_c) = (identity(), identity(), identity(), identity());
    let (alice, alice_port) = node(pinned(&[
        (BOB, &bob_c),
        (CAROL, &carol_c),
        (CANARY, &canary_c),
    ]))
    .await;
    let (bob, bob_port) = node(pinned(&[(ALICE, &alice_c)])).await;
    Pair {
        alice_c,
        bob_c,
        carol_c,
        canary_c,
        alice,
        alice_port,
        bob,
        bob_port,
    }
}

/// Alice sends a signed, ack-requesting message to Bob through her manager.
async fn sent_request(p: &Pair) -> SecureMessage {
    let m = request(&p.alice_c, ALICE, BOB, p.alice_port);
    p.alice
        .send_message(&target(BOB, p.bob_port), &m)
        .await
        .unwrap();
    assert_eq!(
        p.alice.delivery_status(&id_of(&m)).await,
        Some(DeliveryConfirmation::Sent)
    );
    m
}

/// Control for the negative tests: Bob's genuine ack is accepted on the same path.
async fn assert_genuine_ack_is_accepted(p: &Pair, m: &SecureMessage) {
    send_raw(p.alice_port, &build_ack(m, &p.bob_c).unwrap());
    let (status, app) = pump_until_acknowledged(&p.alice, &id_of(m)).await;
    assert_eq!(status, Some(DeliveryConfirmation::Acknowledged));
    assert!(app.is_empty());
}

/// For tests 5-8: deliver `bad`, prove it arrived with a canary, check nothing changed, then run
/// the genuine-ack control.
async fn assert_rejected(p: &Pair, m: &SecureMessage, bad: &SecureMessage) {
    send_raw(p.alice_port, bad);
    let app = pump_past_canary(&p.alice, p.alice_port, &p.canary_c).await;
    assert!(app.is_empty(), "an ack must never reach the application");
    assert_eq!(
        p.alice.delivery_status(&id_of(m)).await,
        Some(DeliveryConfirmation::Sent),
        "a rejected ack must not change the status"
    );
    assert_genuine_ack_is_accepted(p, m).await;
}

// §8 test 2
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_acknowledged_message_becomes_acknowledged_at_the_sender() {
    let p = pair().await;
    let m = sent_request(&p).await;

    let got = receive_one(&p.bob).await;
    assert!(got.sender.is_verified(), "{:?}", got.sender);
    p.bob.acknowledge(&got, &p.bob_c).await.unwrap();

    let (status, app) = pump_until_acknowledged(&p.alice, &id_of(&m)).await;
    assert_eq!(status, Some(DeliveryConfirmation::Acknowledged));
    assert!(app.is_empty(), "the ack must not reach the application");

    // A message sent without request_ack is not tracked.
    let mut plain = SecureMessage::new(BOB, ALICE, b"fyi".to_vec(), SecurityLevel::Authenticated);
    p.alice_c.sign_secure_message(&mut plain).unwrap();
    p.alice
        .send_message(&target(BOB, p.bob_port), &plain)
        .await
        .unwrap();
    assert_eq!(p.alice.delivery_status(&id_of(&plain)).await, None);
}

// §8 test 3
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn acknowledging_twice_is_idempotent() {
    let p = pair().await;
    let m = sent_request(&p).await;
    let got = receive_one(&p.bob).await;

    p.bob.acknowledge(&got, &p.bob_c).await.unwrap();
    p.bob.acknowledge(&got, &p.bob_c).await.unwrap();

    let (status, app) = pump_until_acknowledged(&p.alice, &id_of(&m)).await;
    assert_eq!(status, Some(DeliveryConfirmation::Acknowledged));
    assert!(app.is_empty());
    // Drain past the second ack, and resend the original: neither may change anything.
    p.alice
        .send_message(&target(BOB, p.bob_port), &m)
        .await
        .unwrap();
    assert!(
        pump_past_canary(&p.alice, p.alice_port, &p.canary_c)
            .await
            .is_empty()
    );
    assert_eq!(
        p.alice.delivery_status(&id_of(&m)).await,
        Some(DeliveryConfirmation::Acknowledged)
    );
}

// §8 test 4 — the PM's anti-reflector rule, with an in-run control.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unverified_message_is_never_acknowledged_and_nothing_is_sent() {
    let (alice_c, bob_c) = (identity(), identity());
    let listener = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let victim_port = listener.local_addr().unwrap().port();
    // `accept_unverified` so the unverified message still reaches the application here: this test
    // is about `acknowledge()`'s own anti-reflector refusal, not about the gate (which defaults to
    // deny and is covered by tests/replay_suppression.rs).
    let (bob_unpinned, bob_unpinned_port) = node_with_gate(
        TrustStore::new(),
        synapse::replay::GateConfig {
            accept_unverified: true,
            ..synapse::replay::GateConfig::default()
        },
    )
    .await;
    let (bob_pinned, bob_pinned_port) = node(pinned(&[(ALICE, &alice_c)])).await;
    let mut buf = vec![0u8; 65536];

    // The message names the victim as reply_to; a reflector would send the victim an ack.
    let m = request(&alice_c, ALICE, BOB, victim_port);
    send_raw(bob_unpinned_port, &m);
    let got = receive_one(&bob_unpinned).await;
    assert_eq!(
        got.sender,
        SenderVerdict::Unverifiable {
            reason: UnverifiableReason::UnknownSender
        }
    );
    let err = bob_unpinned.acknowledge(&got, &bob_c).await.unwrap_err();
    assert!(
        matches!(err, SynapseError::AuthenticationError(_)),
        "{err:?}"
    );
    let nothing =
        tokio::time::timeout(Duration::from_millis(500), listener.recv_from(&mut buf)).await;
    assert!(nothing.is_err(), "a refused acknowledge must send nothing");

    // Control, same run, same listener: a verified message IS acknowledged to it.
    let m2 = request(&alice_c, ALICE, BOB, victim_port);
    send_raw(bob_pinned_port, &m2);
    let got2 = receive_one(&bob_pinned).await;
    bob_pinned.acknowledge(&got2, &bob_c).await.unwrap();
    let (len, _) = tokio::time::timeout(Duration::from_secs(2), listener.recv_from(&mut buf))
        .await
        .expect("the control ack arrives")
        .unwrap();
    let ack: SecureMessage = serde_json::from_slice(&buf[..len]).unwrap();
    assert_eq!(
        delivery_ack::ack_fields(&ack).unwrap().for_message_id,
        id_of(&m2)
    );
}

// §8 test 5
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_forged_ack_changes_nothing() {
    let p = pair().await;
    let m = sent_request(&p).await;
    let mallory = identity();
    let forged = build_ack(&m, &mallory).unwrap(); // claims Bob, signed by Mallory
    assert_rejected(&p, &m, &forged).await;
}

// §8 test 6
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_wrong_digest_changes_nothing() {
    let p = pair().await;
    let m = sent_request(&p).await;
    let mut other = m.clone();
    other.encrypted_content = b"something else".to_vec();
    let bad = ack_with(BOB, &id_of(&m), &message_digest(&other), &p.bob_c);
    assert_rejected(&p, &m, &bad).await;
}

// §8 test 7
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_ack_from_someone_other_than_the_addressee_changes_nothing() {
    let p = pair().await;
    let m = sent_request(&p).await;
    // Carol is pinned at Alice and signs correctly, but the message was addressed to Bob.
    let bad = ack_with(CAROL, &id_of(&m), &message_digest(&m), &p.carol_c);
    assert_rejected(&p, &m, &bad).await;
}

// §8 test 8
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_ack_for_an_unknown_message_changes_nothing() {
    let p = pair().await;
    let m = sent_request(&p).await;
    let unknown = uuid::Uuid::new_v4().to_string();
    let bad = ack_with(BOB, &unknown, &message_digest(&m), &p.bob_c);
    assert_rejected(&p, &m, &bad).await;
    assert_eq!(p.alice.delivery_status(&unknown).await, None);
}

// §8 test 9, plus refusal 2 (acknowledging an ack)
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn acknowledge_refuses_without_reply_to_and_refuses_acks() {
    let p = pair().await;
    let mut plain = SecureMessage::new(
        BOB,
        ALICE,
        b"no ack wanted".to_vec(),
        SecurityLevel::Authenticated,
    );
    p.alice_c.sign_secure_message(&mut plain).unwrap();
    send_raw(p.bob_port, &plain);
    let got = receive_one(&p.bob).await;
    assert!(got.sender.is_verified());
    let err = p.bob.acknowledge(&got, &p.bob_c).await.unwrap_err();
    assert!(
        matches!(err, SynapseError::InvalidMessageFormat(_)),
        "{err:?}"
    );

    let m = request(&p.alice_c, ALICE, BOB, p.alice_port);
    let an_ack = build_ack(&m, &p.bob_c).unwrap();
    let by_hand = ReceivedMessage {
        incoming: IncomingMessage::new(an_ack, TransportType::Udp, "test".to_string()),
        sender: SenderVerdict::Verified {
            key_id: "irrelevant".to_string(),
        },
        payload: synapse::sealing::Payload::Plain(Vec::new()),
        freshness: synapse::replay::Freshness::Fresh,
    };
    let err = p.alice.acknowledge(&by_hand, &p.alice_c).await.unwrap_err();
    assert!(
        matches!(err, SynapseError::InvalidMessageFormat(_)),
        "{err:?}"
    );
}

// §8 test 10
#[test]
fn request_ack_after_signing_breaks_the_signature() {
    let alice_c = identity();
    let mut m = SecureMessage::new(BOB, ALICE, b"x".to_vec(), SecurityLevel::Authenticated);
    alice_c.sign_secure_message(&mut m).unwrap();
    m.request_ack("127.0.0.1:9");
    assert_eq!(
        pinned(&[(ALICE, &alice_c)]).verify(&m),
        SenderVerdict::Contradicted {
            reason: ContradictedReason::BadSignature
        }
    );
}
