// SPDX-License-Identifier: MIT OR Apache-2.0
//! P2 slice f1: an account key vouches for agents a receiver has never pinned.
//! Test numbers refer to docs/superpowers/specs/2026-09-17-agent-certificates-design.md §10.

use chrono::{Duration, Utc};
use ed25519_dalek::SigningKey;
use synapse::CryptoManager;
use synapse::certificate::{AgentCertificate, Permission, REVOCATIONS_KEY, Revocation};
use synapse::sender_auth::TrustStore;
use synapse::types::{SecureMessage, SecurityLevel};

const BOB: &str = "bob@synapse.test";

fn account_key(seed: u8) -> SigningKey {
    SigningKey::from_bytes(&[seed; 32])
}

fn agent() -> CryptoManager {
    let mut crypto = CryptoManager::new();
    crypto.generate_keypair().expect("keypair");
    crypto
}

fn free_udp_port() -> u16 {
    let socket = std::net::UdpSocket::bind("127.0.0.1:0").expect("bind ephemeral");
    socket.local_addr().expect("local_addr").port()
}

async fn udp_node(
    store: TrustStore,
    sealing_key: Option<synapse::sealing::SealingKeyPair>,
) -> (synapse::transport::TransportManager, u16) {
    let port = free_udp_port();
    let mut udp = std::collections::HashMap::new();
    udp.insert("bind_port".to_string(), port.to_string());
    let mut builder = synapse::transport::TransportManagerBuilder::new()
        .disable_transport(synapse::transport::TransportType::Tcp)
        .disable_transport(synapse::transport::TransportType::Http)
        .disable_transport(synapse::transport::TransportType::Email)
        .disable_transport(synapse::transport::TransportType::AutoDiscovery)
        .transport_config(synapse::transport::TransportType::Udp, udp)
        .trust_store(store)
        .gate_config(synapse::replay::GateConfig::default());
    if let Some(key) = sealing_key {
        builder = builder.sealing_key(key);
    }
    let manager = builder.build();
    manager
        .register_factory(Box::new(synapse::transport::UdpTransportFactory))
        .await
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), manager.start())
        .await
        .expect("start() returns")
        .expect("start() succeeds");
    (manager, port)
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

/// A certificate from `account` for `holder`, valid for an hour around now.
fn cert_for(
    account: &SigningKey,
    holder: &CryptoManager,
    global_id: &str,
    permissions: Vec<Permission>,
    serial: u8,
) -> AgentCertificate {
    let now = Utc::now();
    AgentCertificate::sign(
        AgentCertificate {
            version: 1,
            serial: [serial; 16],
            issuer_key_id: synapse::sender_auth::key_id(&account.verifying_key().to_bytes()),
            subject_label: "agent".to_string(),
            subject_global_id: global_id.to_string(),
            subject_signing_key: holder.public_key_bytes().expect("public key"),
            subject_sealing_key: [0u8; 32],
            not_before: now - Duration::minutes(1),
            not_after: now + Duration::hours(1),
            permissions,
            may_delegate: 1,
            signature: [0u8; 64],
        },
        account,
    )
}

/// A signed message from `holder` carrying `chain`, sent to Bob. Goes through the real send-side
/// mechanism (`set_certificate_chain` + `sign_secure_message`, exercised directly by FIX 1's test
/// below) rather than hand-adding `CHAIN_KEY` metadata: after FIX 2, a manager with no chain set
/// strips any `CHAIN_KEY` entry on sign, so hand-added metadata would not survive signing here.
fn message_with_chain(
    holder: &mut CryptoManager,
    from: &str,
    chain: &[AgentCertificate],
    text: &[u8],
) -> SecureMessage {
    holder.set_certificate_chain(chain.to_vec());
    let mut m = SecureMessage::new(BOB, from, text.to_vec(), SecurityLevel::Authenticated);
    holder.sign_secure_message(&mut m).expect("sign");
    m
}

fn store_pinning(account: &SigningKey) -> TrustStore {
    let mut store = TrustStore::new();
    store.pin_account_key("alice", account.verifying_key().to_bytes());
    store
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

// FIX 1 (P2f1 fix wave): the send-side mechanism (`CryptoManager::set_certificate_chain` +
// `sign_secure_message` attaching it) had zero callers and zero tests before this -- every other
// test in this file attaches CHAIN_KEY by hand. This exercises the actual send path, then proves
// the chain is signature-covered: stripping CHAIN_KEY out of the received message's metadata and
// re-verifying must NOT verify, because the metadata is inside the signed bytes.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_send_side_chain_mechanism_signs_and_verifies_with_no_hand_added_metadata() {
    let account = account_key(21);
    let mut alice = agent();
    let cert = cert_for(
        &account,
        &alice,
        "agent@alice.test",
        vec![Permission::Send],
        1,
    );
    // The mechanism under test: tell the manager its chain, then sign normally. No
    // `add_metadata(CHAIN_KEY, ...)` anywhere in this test.
    alice.set_certificate_chain(vec![cert]);

    let mut message = SecureMessage::new(
        BOB,
        "agent@alice.test",
        b"hello".to_vec(),
        SecurityLevel::Authenticated,
    );
    alice.sign_secure_message(&mut message).expect("sign");
    assert!(
        message
            .metadata
            .contains_key(synapse::certificate::CHAIN_KEY),
        "signing with a chain set must attach CHAIN_KEY on its own"
    );

    let (bob, port) = udp_node(store_pinning(&account), None).await;
    send_raw(port, &message);
    let received = receive_one(&bob).await;
    assert!(received.sender.is_verified());
    let summary = received.certificate.expect("a certificate summary");
    assert_eq!(summary.subject_global_id, "agent@alice.test");

    // Control: strip CHAIN_KEY from the message's metadata (as a relay stripping the chain would)
    // and re-run verify_at directly. Pin Alice's own key directly here too (spec: direct pinning
    // always wins when both apply), so the recomputed canonical input -- which folds in metadata
    // -- is checked against the ORIGINAL signature, which was computed over metadata that included
    // the chain. Because the chain is inside the signed bytes, removing it changes the recomputed
    // input and must break the signature outright, not merely fall back to "no chain": that is what
    // proves a relay cannot strip the chain undetected.
    let mut stripped = message.clone();
    stripped
        .metadata
        .remove(synapse::certificate::CHAIN_KEY)
        .expect("the chain key was present");
    let mut store = store_pinning(&account);
    store.pin(
        "agent@alice.test",
        alice.public_key_bytes().expect("public key"),
    );
    let verdict = store.verify(&stripped);
    assert_eq!(
        verdict,
        synapse::sender_auth::SenderVerdict::Contradicted {
            reason: synapse::sender_auth::ContradictedReason::BadSignature
        },
        "stripping the signed-in chain must break verification, not just lose the chain summary"
    );
}

// §10 test 7
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_agent_is_verified_from_its_account_keys_certificate() {
    let account = account_key(11);
    let mut alice = agent();
    let cert = cert_for(
        &account,
        &alice,
        "agent@alice.test",
        vec![Permission::Send],
        1,
    );
    let message = message_with_chain(&mut alice, "agent@alice.test", &[cert], b"hello");

    // Bob pins the ACCOUNT key only; he has never seen this agent's key.
    let (bob, port) = udp_node(store_pinning(&account), None).await;
    send_raw(port, &message);
    let received = receive_one(&bob).await;
    assert!(received.sender.is_verified());
    let chain = received.certificate.expect("a certificate summary");
    assert_eq!(chain.subject_global_id, "agent@alice.test");
    assert_eq!(chain.subject_label, "agent");
    assert_eq!(chain.permissions, vec![Permission::Send]);
    assert_eq!(chain.links, 1);
    assert_eq!(
        chain.account_key_id,
        synapse::sender_auth::key_id(&account.verifying_key().to_bytes())
    );

    // Control: a receiver that pins nothing denies the same message and records why.
    let (stranger, stranger_port) = udp_node(TrustStore::new(), None).await;
    send_raw(stranger_port, &message);
    assert!(drain(&stranger).await.is_empty());
    let knocks = stranger.knocks().await;
    assert_eq!(knocks.len(), 1);
    assert_eq!(knocks[0].reason, "unknown_issuer");
}

// §10 test 8: a stranger's chain costs no signature verification.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_chain_rooting_in_an_unpinned_key_costs_no_signature_work() {
    let account = account_key(12);
    let mut alice = agent();
    // The certificate's signature is deliberately invalid.
    let mut cert = cert_for(
        &account,
        &alice,
        "agent@alice.test",
        vec![Permission::Send],
        2,
    );
    cert.signature = [0u8; 64];
    let message = message_with_chain(&mut alice, "agent@alice.test", &[cert], b"hello");

    // Unpinned root: the reason must be unknown_issuer, NOT invalid_chain. Reporting
    // invalid_chain would prove the node verified a signature on a stranger's behalf.
    let (bob, port) = udp_node(TrustStore::new(), None).await;
    send_raw(port, &message);
    assert!(drain(&bob).await.is_empty());
    assert_eq!(bob.knocks().await[0].reason, "unknown_issuer");

    // Control: with the root pinned, the same chain IS validated, and fails on its signature.
    let (pinned, pinned_port) = udp_node(store_pinning(&account), None).await;
    send_raw(pinned_port, &message);
    assert!(drain(&pinned).await.is_empty());
    assert_eq!(pinned.knocks().await[0].reason, "invalid_chain");
}

// §10 test 9
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_revoked_certificate_stops_being_accepted() {
    let account = account_key(13);
    let mut alice = agent();
    let cert = cert_for(
        &account,
        &alice,
        "agent@alice.test",
        vec![Permission::Send],
        3,
    );
    let serial = cert.serial;

    let (bob, port) = udp_node(store_pinning(&account), None).await;
    send_raw(
        port,
        &message_with_chain(
            &mut alice,
            "agent@alice.test",
            std::slice::from_ref(&cert),
            b"first",
        ),
    );
    let first = receive_one(&bob).await;
    assert!(first.sender.is_verified(), "valid before the revocation");

    let mut store = bob.trust_store().await;
    assert!(store.add_revocation(signed_revocation(&account, serial)));
    bob.set_trust_store(store).await;

    send_raw(
        port,
        &message_with_chain(&mut alice, "agent@alice.test", &[cert], b"second"),
    );
    assert!(drain(&bob).await.is_empty(), "denied after the revocation");
    assert!(
        bob.knocks()
            .await
            .iter()
            .any(|k| k.reason == "invalid_chain")
    );
}

// §10 test 11: rotation is the whole point of the slice.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rotating_an_agent_key_needs_no_change_at_the_receiver() {
    let account = account_key(14);
    let (mut old_agent, mut new_agent) = (agent(), agent());
    let (bob, port) = udp_node(store_pinning(&account), None).await;

    let old_cert = cert_for(
        &account,
        &old_agent,
        "agent@alice.test",
        vec![Permission::Send],
        4,
    );
    send_raw(
        port,
        &message_with_chain(&mut old_agent, "agent@alice.test", &[old_cert], b"before"),
    );
    assert!(receive_one(&bob).await.sender.is_verified());

    // A brand new agent key, a new certificate from the SAME account key, and no config change.
    let new_cert = cert_for(
        &account,
        &new_agent,
        "agent@alice.test",
        vec![Permission::Send],
        5,
    );
    send_raw(
        port,
        &message_with_chain(&mut new_agent, "agent@alice.test", &[new_cert], b"after"),
    );
    let rotated = receive_one(&bob).await;
    assert!(rotated.sender.is_verified());
    assert_eq!(
        rotated.certificate.expect("summary").subject_global_id,
        "agent@alice.test"
    );

    // Control: an agent with no certificate from that account is denied.
    let outsider = agent();
    let mut plain = SecureMessage::new(
        BOB,
        "agent@alice.test",
        b"nope".to_vec(),
        SecurityLevel::Authenticated,
    );
    outsider.sign_secure_message(&mut plain).expect("sign");
    send_raw(port, &plain);
    assert!(drain(&bob).await.is_empty());
}

// Fix round 1: a directly pinned sender must not inherit a chain's summary unless that chain is
// the one that authenticated this message -- not merely one naming the same `global_id`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_directly_pinned_sender_does_not_inherit_an_unrelated_chains_summary() {
    let account = account_key(15);
    let mut alice = agent();
    // A different key than Alice's -- standing in for a key that has since rotated away, whose
    // certificate is stale but still sitting in metadata somewhere upstream.
    let stale_agent = agent();

    let stale_cert = cert_for(
        &account,
        &stale_agent,
        "agent@alice.test",
        vec![Permission::Send],
        6,
    );
    // Signed by ALICE's key (the one Bob pins directly), but carrying a chain for the SAME
    // global id naming the STALE key.
    let message = message_with_chain(
        &mut alice,
        "agent@alice.test",
        std::slice::from_ref(&stale_cert),
        b"hi",
    );

    // Positive control: the chain is well-formed and resolves entirely on its own -- a receiver
    // that pins only the account key (no direct pin) accepts it, naming the stale key. This rules
    // out the failure mode where the test would pass merely because the chain was malformed.
    let account_only = store_pinning(&account);
    let resolved = account_only
        .verified_chain(&message)
        .expect("the chain resolves on its own");
    assert_eq!(
        synapse::sender_auth::key_id(&resolved.subject_signing_key),
        synapse::sender_auth::key_id(&stale_agent.public_key_bytes().expect("public key"))
    );

    // Bob pins Alice's key directly AND the account key, so both routes are live at once.
    let mut store = store_pinning(&account);
    store.pin(
        "agent@alice.test",
        alice.public_key_bytes().expect("public key"),
    );
    let (bob, port) = udp_node(store, None).await;
    send_raw(port, &message);
    let received = receive_one(&bob).await;

    // The direct pin authenticates the message -- Alice's own signature verifies.
    assert!(received.sender.is_verified());
    assert_eq!(
        received.sender,
        synapse::sender_auth::SenderVerdict::Verified {
            key_id: synapse::sender_auth::key_id(&alice.public_key_bytes().expect("public key"))
        }
    );
    // But the chain names a different key than the one that signed this message, so no summary
    // is attached.
    assert!(received.certificate.is_none());
}

/// A revocation of `serial`, signed by `account`.
fn signed_revocation(account: &SigningKey, serial: [u8; 16]) -> Revocation {
    Revocation::sign(
        Revocation {
            version: 1,
            serial,
            issuer_key_id: synapse::sender_auth::key_id(&account.verifying_key().to_bytes()),
            issued_at: Utc::now(),
            reason: "test".to_string(),
            signature: [0u8; 64],
        },
        account,
    )
}

// §10 test 10
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_leaf_without_send_is_refused_at_the_transport() {
    let account = account_key(15);
    let mut alice = agent();
    // Granted `ack` only: this agent may acknowledge, but may not originate messages.
    let narrowed = cert_for(
        &account,
        &alice,
        "agent@alice.test",
        vec![Permission::Ack],
        6,
    );
    let (bob, port) = udp_node(store_pinning(&account), None).await;
    send_raw(
        port,
        &message_with_chain(&mut alice, "agent@alice.test", &[narrowed], b"denied"),
    );
    assert!(drain(&bob).await.is_empty());
    assert_eq!(bob.knocks().await[0].reason, "no_send_permission");

    // Control: the same agent, same account key, with `send` granted, is delivered.
    let allowed = cert_for(
        &account,
        &alice,
        "agent@alice.test",
        vec![Permission::Send, Permission::Ack],
        7,
    );
    send_raw(
        port,
        &message_with_chain(&mut alice, "agent@alice.test", &[allowed], b"allowed"),
    );
    assert!(receive_one(&bob).await.sender.is_verified());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_relayed_revocation_is_accepted_only_for_a_pinned_account_key() {
    let account = account_key(16);
    let mut alice = agent();
    let doomed = cert_for(
        &account,
        &alice,
        "agent@alice.test",
        vec![Permission::Send],
        8,
    );
    let serial = doomed.serial;
    let revocation = signed_revocation(&account, serial);

    // Bob pins Alice's account key, so a relayed revocation signed by it is stored.
    let (bob, port) = udp_node(store_pinning(&account), None).await;
    alice.set_certificate_chain(vec![cert_for(
        &account,
        &alice,
        "agent@alice.test",
        vec![Permission::Send],
        9,
    )]);
    let mut carrier = SecureMessage::new(
        BOB,
        "agent@alice.test",
        b"carrier".to_vec(),
        SecurityLevel::Authenticated,
    );
    carrier.add_metadata(REVOCATIONS_KEY, revocation.to_pem());
    alice.sign_secure_message(&mut carrier).expect("sign");
    send_raw(port, &carrier);
    assert!(
        receive_one(&bob).await.sender.is_verified(),
        "the carrier itself is fine"
    );
    assert!(
        bob.trust_store().await.is_revoked(
            &synapse::sender_auth::key_id(&account.verifying_key().to_bytes()),
            &serial
        ),
        "the relayed revocation was stored"
    );

    // The revoked certificate is now refused.
    send_raw(
        port,
        &message_with_chain(
            &mut alice,
            "agent@alice.test",
            std::slice::from_ref(&doomed),
            b"revoked",
        ),
    );
    assert!(drain(&bob).await.is_empty());

    // Control: a receiver that has NOT pinned Alice stores nothing from the same relay, and the
    // certificate the revocation names keeps verifying under a receiver that pins its own root.
    let other_account = account_key(17);
    let (stranger, stranger_port) = udp_node(store_pinning(&other_account), None).await;
    send_raw(stranger_port, &carrier);
    let _ = drain(&stranger).await;
    assert_eq!(stranger.trust_store().await.revocation_count(), 0);
}
