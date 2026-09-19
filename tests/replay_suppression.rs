// SPDX-License-Identifier: MIT OR Apache-2.0
//! P2 slice e: replays are dropped, unverified senders are denied, and the counters say so.
//! Test numbers refer to docs/superpowers/specs/2026-09-17-replay-suppression-design.md §10.

use synapse::CryptoManager;
use synapse::replay::{Freshness, GateConfig};
use synapse::sender_auth::TrustStore;
use synapse::transport::{
    TransportManagerBuilder, TransportTarget, TransportType, UdpTransportFactory,
};
use synapse::types::{SecureMessage, SecurityLevel};

const ALICE: &str = "alice@synapse.test";
const BOB: &str = "bob@synapse.test";

fn signer() -> CryptoManager {
    let mut crypto = CryptoManager::new();
    crypto.generate_keypair().unwrap();
    crypto
}

fn store_for(alice: &CryptoManager) -> TrustStore {
    let mut store = TrustStore::new();
    store.pin(ALICE, alice.public_key_bytes().unwrap());
    store
}

/// A signed, unsealed message from Alice to Bob at `Authenticated`, so these tests exercise the
/// gate and the guard without involving slice d's sealing.
fn signed(alice: &CryptoManager, text: &[u8]) -> SecureMessage {
    let mut m = SecureMessage::new(BOB, ALICE, text.to_vec(), SecurityLevel::Authenticated);
    alice.sign_secure_message(&mut m).expect("sign");
    m
}

mod common;
use common::free_udp_port;

async fn udp_node_with(
    store: TrustStore,
    sealing_key: Option<synapse::sealing::SealingKeyPair>,
    gate: GateConfig,
) -> (synapse::transport::TransportManager, u16) {
    let port = free_udp_port();
    let mut udp = std::collections::HashMap::new();
    udp.insert("bind_port".to_string(), port.to_string());
    let mut builder = TransportManagerBuilder::new()
        .disable_transport(TransportType::Tcp)
        .disable_transport(TransportType::Http)
        .disable_transport(TransportType::Email)
        .disable_transport(TransportType::AutoDiscovery)
        .transport_config(TransportType::Udp, udp)
        .trust_store(store)
        .gate_config(gate);
    if let Some(key) = sealing_key {
        builder = builder.sealing_key(key);
    }
    let manager = builder.build();
    manager
        .register_factory(Box::new(UdpTransportFactory))
        .await
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), manager.start())
        .await
        .expect("start() returns")
        .expect("start() succeeds");
    (manager, port)
}

/// Thin wrapper over `udp_node_with`, so the file has one builder.
async fn udp_node(
    store: TrustStore,
    sealing_key: Option<synapse::sealing::SealingKeyPair>,
) -> (synapse::transport::TransportManager, u16) {
    udp_node_with(store, sealing_key, GateConfig::default()).await
}

fn send_raw(port: u16, message: &SecureMessage) {
    let raw = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    raw.send_to(&serde_json::to_vec(message).unwrap(), ("127.0.0.1", port))
        .unwrap();
}

async fn receive_one(
    manager: &synapse::transport::TransportManager,
) -> synapse::transport::ReceivedMessage {
    for _ in 0..20 {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let mut batch = manager.receive_messages().await.expect("receive");
        if let Some(first) = batch.pop() {
            assert!(batch.is_empty(), "expected exactly one message");
            return first;
        }
    }
    panic!("no message arrived within 2 s");
}

/// Poll for up to 2 s and return everything that arrived.
async fn drain(
    manager: &synapse::transport::TransportManager,
) -> Vec<synapse::transport::ReceivedMessage> {
    let mut out = Vec::new();
    for _ in 0..20 {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        out.extend(manager.receive_messages().await.expect("receive"));
    }
    out
}

// §10 test 7
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_resent_datagram_is_dropped_the_second_time() {
    let alice = signer();
    let (bob, port) = udp_node(store_for(&alice), None).await;
    let message = signed(&alice, b"once only");

    send_raw(port, &message);
    let first = receive_one(&bob).await;
    assert!(first.sender.is_verified());
    assert_eq!(first.freshness, Freshness::Fresh);

    // The identical datagram again: a verbatim replay.
    send_raw(port, &message);
    assert!(
        drain(&bob).await.is_empty(),
        "the replay must not be delivered"
    );
    assert_eq!(bob.inbound_counters().await.dropped_replay, 1);

    // Control: a genuinely new message from the same sender still arrives.
    send_raw(port, &signed(&alice, b"second message"));
    let second = receive_one(&bob).await;
    assert_eq!(second.freshness, Freshness::Fresh);
    assert_eq!(bob.inbound_counters().await.dropped_replay, 1);
}

// §10 test 8
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unverified_sender_is_dropped_by_default_and_shows_as_a_knock() {
    let alice = signer();
    // Bob pins nobody, so Alice is Unverifiable(UnknownSender).
    let (bob, port) = udp_node(TrustStore::new(), None).await;
    send_raw(port, &signed(&alice, b"who am i"));

    assert!(
        drain(&bob).await.is_empty(),
        "an unpinned sender is denied by default"
    );
    let counters = bob.inbound_counters().await;
    assert_eq!(counters.dropped_unverifiable, 1);
    assert_eq!(counters.admitted, 0);
    let knocks = bob.knocks().await;
    assert_eq!(knocks.len(), 1);
    assert_eq!(knocks[0].claimed_global_id, ALICE);
    assert_eq!(
        knocks[0].key_id,
        synapse::sender_auth::key_id(&alice.public_key_bytes().unwrap())
    );
    assert_eq!(knocks[0].reason, "unknown_sender");
    assert_eq!(knocks[0].count, 1);
}

// §10 test 8, control: the opt-in setting delivers the same message, marked.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn accept_unverified_delivers_it_marked_not_checked() {
    let alice = signer();
    let (bob, port) = udp_node_with(
        TrustStore::new(),
        None,
        GateConfig {
            accept_unverified: true,
            ..GateConfig::default()
        },
    )
    .await;
    send_raw(port, &signed(&alice, b"who am i"));
    let received = receive_one(&bob).await;
    assert_eq!(received.freshness, Freshness::NotChecked);
    assert!(!received.sender.is_verified());
}

// §10 test 9
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_contradicted_message_is_dropped_under_both_settings() {
    for accept_unverified in [false, true] {
        let alice = signer();
        let impostor = signer();
        // Alice's id is pinned to Alice's key, but the message is signed by the impostor's key.
        let mut message =
            SecureMessage::new(BOB, ALICE, b"not me".to_vec(), SecurityLevel::Authenticated);
        impostor.sign_secure_message(&mut message).expect("sign");

        let (bob, port) = udp_node_with(
            store_for(&alice),
            None,
            GateConfig {
                accept_unverified,
                ..GateConfig::default()
            },
        )
        .await;
        send_raw(port, &message);
        assert!(
            drain(&bob).await.is_empty(),
            "a contradicted sender is always denied"
        );
        assert_eq!(bob.inbound_counters().await.dropped_contradicted, 1);
        assert_eq!(bob.knocks().await[0].reason, "key_mismatch");
    }
}

// §10 test 10: the suppression test. An unsigned copy must not reserve a genuine id. This is true
// at the type level, not just by observed behaviour: `Admission::AdmitUnverified` is a unit
// variant carrying no `key_id`, so there is no `key_id` to call `ReplayGuard::check` with on this
// path, and the unsigned copy is never recorded. If `AdmitUnverified` ever gains a `key_id` field,
// that guarantee stops holding and this test — not just the type signature — is what catches it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unsigned_copy_cannot_suppress_a_genuine_message() {
    let alice = signer();
    let (bob, port) = udp_node_with(
        store_for(&alice),
        None,
        GateConfig {
            accept_unverified: true,
            ..GateConfig::default()
        },
    )
    .await;

    let genuine = signed(&alice, b"the real thing");
    // The same message_id, unsigned, sent first.
    let mut forged = SecureMessage::new(
        BOB,
        ALICE,
        b"the forgery".to_vec(),
        SecurityLevel::Authenticated,
    );
    forged.message_id = genuine.message_id.clone();

    send_raw(port, &forged);
    let first = receive_one(&bob).await;
    assert_eq!(first.freshness, Freshness::NotChecked);

    send_raw(port, &genuine);
    let second = receive_one(&bob).await;
    assert_eq!(
        second.freshness,
        Freshness::Fresh,
        "the genuine message must not be suppressed"
    );
    assert!(second.sender.is_verified());
    assert_eq!(bob.inbound_counters().await.dropped_replay, 0);
}

// §10 test 11
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_replayed_ack_is_dropped_before_it_is_applied() {
    use synapse::transport::abstraction::DeliveryConfirmation;

    let alice = signer();
    let bob_signer = signer();
    let mut alice_store = TrustStore::new();
    alice_store.pin(BOB, bob_signer.public_key_bytes().unwrap());
    let (alice_node, alice_port) = udp_node(alice_store, None).await;
    let (bob_node, bob_port) = udp_node(store_for(&alice), None).await;

    // A raw listener stands in for the reply address, so we can capture the ack's exact bytes:
    // DeliveryReceipt (src/transport/abstraction.rs) carries only ids and metadata, never the ack
    // message itself.
    let listener = tokio::net::UdpSocket::bind("127.0.0.1:0")
        .await
        .expect("bind listener");
    let listener_port = listener.local_addr().expect("local_addr").port();

    // Alice sends, asking for an ack at the raw listener. request_ack changes signed metadata, so
    // the message is signed afterwards (tests/receiver_acknowledgement.rs:81-82).
    let mut request = SecureMessage::new(
        BOB,
        ALICE,
        b"please ack".to_vec(),
        SecurityLevel::Authenticated,
    );
    request.request_ack(format!("127.0.0.1:{listener_port}"));
    alice.sign_secure_message(&mut request).expect("sign");
    let message_id = request.message_id.0.to_string();

    let target =
        TransportTarget::new(BOB.to_string()).with_address(format!("127.0.0.1:{bob_port}"));
    alice_node
        .send_message(&target, &request)
        .await
        .expect("send");

    let received = receive_one(&bob_node).await;
    bob_node
        .acknowledge(&received, &bob_signer)
        .await
        .expect("ack");

    // Capture the ack's exact bytes from the raw listener.
    let mut buf = vec![0u8; 65536];
    let (len, _) = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        listener.recv_from(&mut buf),
    )
    .await
    .expect("the ack arrives")
    .expect("recv_from");
    let ack_bytes = buf[..len].to_vec();
    let ack: SecureMessage = serde_json::from_slice(&ack_bytes).expect("parse ack");
    assert_eq!(
        synapse::delivery_ack::ack_fields(&ack)
            .unwrap()
            .for_message_id,
        message_id
    );

    // Send the same bytes to Alice's real manager port twice: the first is applied, the second is
    // the replay that must be dropped before it reaches apply_ack.
    let raw = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    raw.send_to(&ack_bytes, ("127.0.0.1", alice_port)).unwrap();
    let _ = drain(&alice_node).await;
    assert_eq!(
        alice_node.delivery_status(&message_id).await,
        Some(DeliveryConfirmation::Acknowledged)
    );

    raw.send_to(&ack_bytes, ("127.0.0.1", alice_port)).unwrap();
    let _ = drain(&alice_node).await;
    assert_eq!(alice_node.inbound_counters().await.dropped_replay, 1);
    assert_eq!(
        alice_node.delivery_status(&message_id).await,
        Some(DeliveryConfirmation::Acknowledged),
        "the status is unchanged, and the replayed ack never reached apply_ack"
    );
}

/// A manager whose ack tracking uses the given limits instead of the 1 h / 10_000 default, so the
/// tests below can drive expiry and the capacity bound directly.
async fn udp_node_with_tracking(
    store: TrustStore,
    ttl: chrono::Duration,
    capacity: usize,
) -> (synapse::transport::TransportManager, u16) {
    let port = free_udp_port();
    let mut udp = std::collections::HashMap::new();
    udp.insert("bind_port".to_string(), port.to_string());
    let manager = TransportManagerBuilder::new()
        .disable_transport(TransportType::Tcp)
        .disable_transport(TransportType::Http)
        .disable_transport(TransportType::Email)
        .disable_transport(TransportType::AutoDiscovery)
        .transport_config(TransportType::Udp, udp)
        .trust_store(store)
        .tracking_limits(ttl, capacity)
        .build();
    manager
        .register_factory(Box::new(UdpTransportFactory))
        .await
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), manager.start())
        .await
        .expect("start() returns")
        .expect("start() succeeds");
    (manager, port)
}

/// A message that asks for an ack at a port nobody listens on, so no ack will ever arrive.
fn ack_requesting_message(alice: &CryptoManager) -> SecureMessage {
    let mut message = SecureMessage::new(
        BOB,
        ALICE,
        b"please ack".to_vec(),
        SecurityLevel::Authenticated,
    );
    // Nobody listens on this port: no ack ever arrives.
    message.request_ack("127.0.0.1:1".to_string());
    alice.sign_secure_message(&mut message).expect("sign");
    message
}

// §10 test 12
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unacknowledged_tracked_message_expires() {
    let alice = signer();
    // A port nobody listens on: freed immediately after binding, so the send is fire-and-forget
    // UDP into the void, and no ack will ever come back.
    let dead_port = free_udp_port();
    let target =
        TransportTarget::new(BOB.to_string()).with_address(format!("127.0.0.1:{dead_port}"));

    let (expiring, _expiring_port) =
        udp_node_with_tracking(store_for(&alice), chrono::Duration::seconds(0), 10).await;
    let message = ack_requesting_message(&alice);
    let message_id = message.message_id.0.to_string();
    expiring
        .send_message(&target, &message)
        .await
        .expect("send");
    assert_eq!(
        expiring.delivery_status(&message_id).await,
        Some(synapse::transport::abstraction::DeliveryConfirmation::Expired)
    );
    // Spec §6: Expired is reported for as long as the entry remains, so a SECOND call must not
    // see it swept away by the first.
    assert_eq!(
        expiring.delivery_status(&message_id).await,
        Some(synapse::transport::abstraction::DeliveryConfirmation::Expired),
        "a second delivery_status call on an expired id must still report Expired"
    );

    // Control: with the default 1 h ttl, the same send reports Sent.
    let (default_ttl, _default_port) =
        udp_node_with_tracking(store_for(&alice), chrono::Duration::hours(1), 10_000).await;
    let control_message = ack_requesting_message(&alice);
    let control_id = control_message.message_id.0.to_string();
    default_ttl
        .send_message(&target, &control_message)
        .await
        .expect("send");
    assert_eq!(
        default_ttl.delivery_status(&control_id).await,
        Some(synapse::transport::abstraction::DeliveryConfirmation::Sent)
    );
}

// §10 test 13
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ack_tracking_is_capped() {
    let alice = signer();
    let (bob, _port) =
        udp_node_with_tracking(store_for(&alice), chrono::Duration::hours(1), 2).await;
    let dead_port = free_udp_port();
    let target =
        TransportTarget::new(BOB.to_string()).with_address(format!("127.0.0.1:{dead_port}"));

    let mut ids = Vec::new();
    for _ in 0..4 {
        let message = ack_requesting_message(&alice);
        ids.push(message.message_id.0.to_string());
        bob.send_message(&target, &message).await.expect("send");
    }

    assert_eq!(bob.tracked_count().await, 2);
    assert_eq!(bob.delivery_status(&ids[0]).await, None);
    assert_eq!(
        bob.delivery_status(&ids[3]).await,
        Some(synapse::transport::abstraction::DeliveryConfirmation::Sent)
    );
}
