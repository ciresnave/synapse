// SPDX-License-Identifier: MIT OR Apache-2.0
//! Hardening P6: the transport receive path records knocks, replays, unopenable bodies and refused
//! revocations as security events (audit rows 8–11). Plan:
//! docs/superpowers/plans/2026-10-07-hardening-p6-transports.md.
//!
//! Every test pairs a negative control (the event is written) with a positive control (a legitimate
//! message writes none and is still delivered). Alert delivery awaits board 131.

use std::sync::{Arc, Mutex};

use chrono::{Duration, Utc};
use ed25519_dalek::SigningKey;
use synapse::CryptoManager;
use synapse::certificate::Revocation;
use synapse::security_events::{LimiterConfig, SecurityEvent, SecurityEventKind, SecuritySink};
use synapse::sender_auth::TrustStore;
use synapse::transport::{
    KnockLimits, TransportManager, TransportManagerBuilder, TransportType, UdpTransportFactory,
};
use synapse::types::{SecureMessage, SecurityLevel};

mod common;
use common::free_udp_port;

const ALICE: &str = "alice@synapse.test";
const BOB: &str = "bob@synapse.test";
const MALLORY: &str = "mallory@synapse.test";
const PAYLOAD: &[u8] = b"PAYLOAD-MUST-NOT-APPEAR";

#[derive(Default)]
struct Capture(Mutex<Vec<SecurityEvent>>);

impl SecuritySink for Capture {
    fn record(&self, event: &SecurityEvent) {
        self.0.lock().unwrap().push(event.clone());
    }
}

impl Capture {
    fn all(&self) -> Vec<SecurityEvent> {
        self.0.lock().unwrap().clone()
    }
    fn on(&self, surface: &str) -> Vec<SecurityEvent> {
        self.all()
            .into_iter()
            .filter(|e| e.surface == surface)
            .collect()
    }
}

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

fn signed(alice: &CryptoManager, level: SecurityLevel) -> SecureMessage {
    let mut m = SecureMessage::new(BOB, ALICE, PAYLOAD.to_vec(), level);
    alice.sign_secure_message(&mut m).expect("sign");
    m
}

fn unsigned_from(claimed: &str) -> SecureMessage {
    SecureMessage::new(BOB, claimed, PAYLOAD.to_vec(), SecurityLevel::Authenticated)
}

/// No delay ever runs out inside a test: an over-budget key stays over budget.
fn limiter(free_failures: u32) -> LimiterConfig {
    LimiterConfig {
        free_failures,
        window: Duration::minutes(5),
        base_delay: Duration::hours(1),
        max_delay: Duration::hours(1),
        lockout_after: u32::MAX,
        lockout: Duration::zero(),
        max_keys: 100,
    }
}

fn limits(per_sender: u32, per_source: u32) -> KnockLimits {
    KnockLimits {
        per_sender: limiter(per_sender),
        per_source: limiter(per_source),
        event_interval: Duration::zero(),
    }
}

async fn node(
    store: TrustStore,
    sealing_key: Option<synapse::sealing::SealingKeyPair>,
    knock_limits: KnockLimits,
) -> (TransportManager, u16, Arc<Capture>) {
    let port = free_udp_port();
    let mut udp = std::collections::HashMap::new();
    udp.insert("bind_port".to_string(), port.to_string());
    let capture = Arc::new(Capture::default());
    let mut builder = TransportManagerBuilder::new()
        .disable_transport(TransportType::Tcp)
        .disable_transport(TransportType::Http)
        .disable_transport(TransportType::Email)
        .disable_transport(TransportType::AutoDiscovery)
        .transport_config(TransportType::Udp, udp)
        .trust_store(store)
        .security_sink(capture.clone())
        .knock_limits(knock_limits);
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
    (manager, port, capture)
}

fn sender() -> std::net::UdpSocket {
    std::net::UdpSocket::bind("127.0.0.1:0").unwrap()
}

fn send(from: &std::net::UdpSocket, port: u16, message: &SecureMessage) {
    from.send_to(&serde_json::to_vec(message).unwrap(), ("127.0.0.1", port))
        .unwrap();
}

/// Polls until `n` raw messages have been through the receive path, returning what was delivered.
async fn pump(manager: &TransportManager, n: u64) -> Vec<synapse::transport::ReceivedMessage> {
    let mut delivered = Vec::new();
    for _ in 0..40 {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        delivered.extend(manager.receive_messages().await.expect("receive"));
        let c = manager.inbound_counters().await;
        let seen = c.admitted + c.dropped_contradicted + c.dropped_unverifiable;
        if seen >= n {
            return delivered;
        }
    }
    panic!("only some of {n} messages arrived");
}

fn kinds(events: &[SecurityEvent]) -> Vec<SecurityEventKind> {
    events.iter().map(|e| e.kind).collect()
}

// Row 9.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_knock_is_an_event_with_its_source_and_a_pinned_sender_writes_none() {
    let alice = signer();
    let (bob, port, capture) = node(store_for(&alice), None, limits(10, 50)).await;
    let from = sender();

    send(&from, port, &signed(&alice, SecurityLevel::Authenticated));
    let delivered = pump(&bob, 1).await;
    assert_eq!(delivered.len(), 1, "positive control: delivered");
    assert!(capture.all().is_empty(), "{:?}", capture.all());

    send(&from, port, &unsigned_from(MALLORY));
    pump(&bob, 2).await;
    let knocks = capture.on("transport/knock");
    assert_eq!(
        kinds(&knocks),
        [SecurityEventKind::UnverifiedSender],
        "{knocks:?}"
    );
    assert_eq!(knocks[0].subject, MALLORY);
    let source = knocks[0].source.as_deref().expect("source recorded");
    assert!(source.starts_with("127.0.0.1:"), "{source}");
    assert!(
        knocks[0].detail.contains("unsigned"),
        "{}",
        knocks[0].detail
    );
    assert!(
        knocks[0].detail.starts_with("new_source"),
        "the first knock from a source is marked: {}",
        knocks[0].detail
    );
}

// Row 9: a flood folds into one RateLimited per trip, so it cannot rotate the event file.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_knock_flood_trips_once_and_another_sender_from_the_source_still_counts() {
    let alice = signer();
    let (bob, port, capture) = node(store_for(&alice), None, limits(2, 100)).await;
    let from = sender();
    for _ in 0..5 {
        send(&from, port, &unsigned_from(MALLORY));
    }
    pump(&bob, 5).await;
    let knocks = capture.on("transport/knock");
    assert_eq!(
        kinds(&knocks),
        [
            SecurityEventKind::UnverifiedSender,
            SecurityEventKind::UnverifiedSender,
            SecurityEventKind::RateLimited
        ],
        "{knocks:?}"
    );
    // Only the first knock from the source carries the marker.
    assert!(knocks[0].detail.starts_with("new_source"));
    assert!(!knocks[1].detail.starts_with("new_source"), "{knocks:?}");

    // Positive control: a different claimed id from the same source has its own budget.
    send(&from, port, &unsigned_from("eve@synapse.test"));
    pump(&bob, 6).await;
    let last = capture.on("transport/knock").pop().unwrap();
    assert_eq!(last.kind, SecurityEventKind::UnverifiedSender);
    assert_eq!(last.subject, "eve@synapse.test");
}

// Row 9: the per-source backstop trips across claimed ids.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_per_source_backstop_trips_across_claimed_ids() {
    let alice = signer();
    let (bob, port, capture) = node(store_for(&alice), None, limits(100, 2)).await;
    let from = sender();
    for i in 0..4 {
        send(
            &from,
            port,
            &unsigned_from(&format!("spray{i}@synapse.test")),
        );
    }
    pump(&bob, 4).await;
    let knocks = capture.on("transport/knock");
    assert_eq!(
        kinds(&knocks),
        [
            SecurityEventKind::UnverifiedSender,
            SecurityEventKind::UnverifiedSender,
            SecurityEventKind::RateLimited
        ],
        "{knocks:?}"
    );
    assert_eq!(
        knocks[2].subject, "127.0.0.1",
        "the backstop names the source"
    );
}

// Row 9, the ruling's guarantee (option A): an over-budget key is still verified, never dropped
// unread, because a spoofed source address must not silence a real peer.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn after_a_trip_a_valid_message_from_the_same_source_is_still_delivered() {
    let alice = signer();
    let (bob, port, _capture) = node(store_for(&alice), None, limits(1, 1)).await;
    let from = sender();
    for _ in 0..3 {
        let mut forged = unsigned_from(ALICE);
        forged.encrypted_content = b"forged".to_vec();
        send(&from, port, &forged);
    }
    pump(&bob, 3).await;
    send(&from, port, &signed(&alice, SecurityLevel::Authenticated));
    let delivered = pump(&bob, 4).await;
    assert_eq!(delivered.len(), 1);
    assert!(delivered[0].sender.is_verified());
}

// Row 10.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_replay_is_an_event_and_the_first_copy_is_not() {
    let alice = signer();
    let (bob, port, capture) = node(store_for(&alice), None, limits(10, 50)).await;
    let from = sender();
    let message = signed(&alice, SecurityLevel::Authenticated);
    send(&from, port, &message);
    assert_eq!(pump(&bob, 1).await.len(), 1);
    assert!(
        capture.all().is_empty(),
        "positive control: {:?}",
        capture.all()
    );

    send(&from, port, &message);
    for _ in 0..40 {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        bob.receive_messages().await.unwrap();
        if bob.inbound_counters().await.dropped_replay == 1 {
            break;
        }
    }
    let replays = capture.on("transport/replay");
    assert_eq!(
        kinds(&replays),
        [SecurityEventKind::ReplayRefused],
        "{replays:?}"
    );
    assert_eq!(
        replays[0].subject,
        synapse::sender_auth::key_id(&alice.public_key_bytes().unwrap())
    );
    assert!(replays[0].source.is_some());
}

// Row 11.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unopenable_body_is_an_event_and_a_sealed_one_is_not() {
    let alice = signer();
    let key = synapse::sealing::SealingKeyPair::generate();
    let public = key.public_key().clone();
    let (bob, port, capture) = node(store_for(&alice), Some(key), limits(10, 50)).await;
    let from = sender();

    let mut sealed = SecureMessage::new(BOB, ALICE, PAYLOAD.to_vec(), SecurityLevel::Private);
    synapse::sealing::seal(&mut sealed, &public).unwrap();
    alice.sign_secure_message(&mut sealed).unwrap();
    send(&from, port, &sealed);
    assert_eq!(pump(&bob, 1).await.len(), 1);
    assert!(
        capture.all().is_empty(),
        "positive control: {:?}",
        capture.all()
    );

    // Private level, signed, but never sealed.
    send(&from, port, &signed(&alice, SecurityLevel::Private));
    assert_eq!(pump(&bob, 2).await.len(), 1, "still delivered, marked");
    let sealing = capture.on("transport/sealing");
    assert_eq!(
        kinds(&sealing),
        [SecurityEventKind::UnverifiedSender],
        "{sealing:?}"
    );
    assert_eq!(sealing[0].subject, ALICE);
    assert_eq!(sealing[0].detail, "not_sealed");
}

fn account_key(seed: u8) -> SigningKey {
    SigningKey::from_bytes(&[seed; 32])
}

fn revocation(signer: &SigningKey, serial: u8) -> Revocation {
    Revocation::sign(
        Revocation {
            version: 1,
            serial: [serial; 16],
            issuer_key_id: synapse::sender_auth::key_id(&signer.verifying_key().to_bytes()),
            issued_at: Utc::now(),
            reason: "test".to_string(),
            signature: [0u8; 64],
        },
        signer,
    )
}

// Row 8.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_refused_revocation_is_an_event_and_a_valid_or_duplicate_one_is_not() {
    let account = account_key(7);
    let mut store = TrustStore::new();
    store.pin_account_key("alice", account.verifying_key().to_bytes());
    let (bob, _port, capture) = node(store, None, limits(10, 50)).await;

    let valid = revocation(&account, 1);
    assert_eq!(bob.add_revocations(std::slice::from_ref(&valid)).await, 1);
    assert_eq!(bob.add_revocations(&[valid]).await, 0, "duplicate");
    assert!(
        capture.all().is_empty(),
        "positive control: {:?}",
        capture.all()
    );

    let unpinned = revocation(&account_key(9), 2);
    let mut tampered = revocation(&account, 3);
    tampered.reason = "changed after signing".to_string();
    assert_eq!(bob.add_revocations(&[unpinned.clone(), tampered]).await, 0);
    let refused = capture.on("transport/revocation");
    assert_eq!(
        refused
            .iter()
            .map(|e| e.detail.as_str())
            .collect::<Vec<_>>(),
        ["unknown_issuer", "bad_signature"],
        "{refused:?}"
    );
    assert_eq!(refused[0].subject, unpinned.issuer_key_id);
    assert!(
        refused
            .iter()
            .all(|e| e.kind == SecurityEventKind::UnverifiedSender)
    );
}

// Every row: no payload, signature or key bytes in any event.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn no_event_carries_a_payload_or_a_signature() {
    let alice = signer();
    let (bob, port, capture) = node(store_for(&alice), None, limits(10, 50)).await;
    let from = sender();
    let message = signed(&alice, SecurityLevel::Authenticated);
    let mut bad = message.clone();
    bad.encrypted_content = PAYLOAD.iter().rev().copied().collect();
    send(&from, port, &message);
    send(&from, port, &message);
    send(&from, port, &bad);
    send(&from, port, &unsigned_from(MALLORY));
    for _ in 0..40 {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        bob.receive_messages().await.unwrap();
        let c = bob.inbound_counters().await;
        if c.admitted + c.dropped_contradicted + c.dropped_unverifiable >= 4 {
            break;
        }
    }
    let events = capture.all();
    let text = serde_json::to_string(&events).unwrap();
    // Positive control: the scan sees a field that is meant to be there.
    assert!(text.contains(MALLORY), "{text}");
    assert!(events.len() >= 3, "{events:?}");
    let payload = String::from_utf8_lossy(PAYLOAD).to_string();
    let sig_hex: String = message
        .sender_proof
        .sig
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let public_hex: String = alice
        .public_key_bytes()
        .unwrap()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    for secret in [payload.as_str(), &sig_hex, &public_hex] {
        assert!(!text.contains(secret), "leaked {secret}: {text}");
    }
}

// Without a sink nothing changes: no event plumbing is required to receive.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_manager_with_no_sink_receives_as_before() {
    let alice = signer();
    let port = free_udp_port();
    let mut udp = std::collections::HashMap::new();
    udp.insert("bind_port".to_string(), port.to_string());
    let bob = TransportManagerBuilder::new()
        .disable_transport(TransportType::Tcp)
        .disable_transport(TransportType::Http)
        .disable_transport(TransportType::Email)
        .disable_transport(TransportType::AutoDiscovery)
        .transport_config(TransportType::Udp, udp)
        .trust_store(store_for(&alice))
        .build();
    bob.register_factory(Box::new(UdpTransportFactory))
        .await
        .unwrap();
    bob.start().await.unwrap();
    let from = sender();
    send(&from, port, &unsigned_from(MALLORY));
    send(&from, port, &signed(&alice, SecurityLevel::Authenticated));
    assert_eq!(pump(&bob, 2).await.len(), 1);
    assert_eq!(bob.knocks().await.len(), 1);
}
