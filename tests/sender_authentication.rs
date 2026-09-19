// SPDX-License-Identifier: MIT OR Apache-2.0
//! Sender authentication — spec: docs/superpowers/specs/2026-09-17-sender-authentication-design.md
//! Test numbers refer to the spec's §10.

use std::collections::HashMap;
use std::time::Duration;

use synapse::CryptoManager;
use synapse::blockchain::serialization::{DateTimeWrapper, UuidWrapper};
use synapse::sender_auth::{
    ContradictedReason, ProofAlg, SenderProof, SenderVerdict, TrustStore, UnverifiableReason,
    canonical_input, key_id,
};
use synapse::transport::{
    ReceivedMessage, TransportManager, TransportManagerBuilder, TransportTarget, TransportType,
    UdpTransportFactory,
};
use synapse::types::{SecureMessage, SecurityLevel};

fn unsigned_message() -> SecureMessage {
    SecureMessage::new(
        "bob@synapse.test",
        "alice@synapse.test",
        b"hello, bob".to_vec(),
        SecurityLevel::Authenticated,
    )
}

// §10 test 6 — a message without a sender proof must fail to parse.
#[test]
fn a_message_without_sender_proof_does_not_parse() {
    let mut value = serde_json::to_value(unsigned_message()).unwrap();
    value.as_object_mut().unwrap().remove("sender_proof");
    assert!(serde_json::from_value::<SecureMessage>(value).is_err());
}

#[test]
fn an_unknown_alg_does_not_parse() {
    let mut value = serde_json::to_value(unsigned_message()).unwrap();
    value["sender_proof"]["alg"] = serde_json::json!("rsa");
    assert!(serde_json::from_value::<SecureMessage>(value).is_err());
}

/// Probe D's forged message (inventory §2.2): the pre-2.0 shape, `"signature": []`, with a
/// made-up sender. It was delivered and believed. It must no longer parse.
#[test]
fn probe_d_forged_shape_does_not_parse() {
    let forged = serde_json::json!({
        "message_id": "00112233-4455-6677-8899-aabbccddeeff",
        "to_global_id": "bob@synapse.test",
        "from_global_id": "python-agent@openai.example",
        "encrypted_content": [104, 105],
        "signature": [],
        "timestamp": "2026-09-09T14:13:31.517436100Z",
        "security_level": "public",
        "routing_path": [],
        "metadata": {}
    });
    assert!(serde_json::from_value::<SecureMessage>(forged).is_err());
}

#[test]
fn an_unsigned_message_says_so_on_the_wire() {
    let json = serde_json::to_value(unsigned_message()).unwrap();
    assert_eq!(
        json["sender_proof"],
        serde_json::json!({"alg": "none", "key_id": "", "sig": []})
    );
    let proof: SenderProof = serde_json::from_value(json["sender_proof"].clone()).unwrap();
    assert_eq!(proof, SenderProof::unsigned());
    assert_eq!(proof.alg, ProofAlg::None);
}

// Fixed test keys: PKCS#8 v2 PEMs as produced by `CryptoManager::generate_keypair` (ring).
const ALICE_PEM: &str = "-----BEGIN PRIVATE KEY-----\nMFECAQEwBQYDK2VwBCIEIHlT5KuliCfGPc4fzz4RYnp56ey9Wn9QjoTuBmXPg1AN\ngSEAPmcRS3zxT2qYvEeRdfsivbOtzmj3jQ/99r7gtN+8jlk=\n-----END PRIVATE KEY-----\n";
const BOB_PEM: &str = "-----BEGIN PRIVATE KEY-----\nMFECAQEwBQYDK2VwBCIEIJX5WiDidmrYDazpqo0+x6kKYJK2ijrpLVs1n9Gm7mPs\ngSEAH/TcFlwaeWvcTABgiHMIKMZVImEkF1z/WAFySDJWQH8=\n-----END PRIVATE KEY-----\n";

// §10 test 8 values. Computed by an independent Python implementation of spec §4 (written from the
// spec text, using `cryptography` 50.0.1's Ed25519), NOT by the Rust code under test. Python took
// the seed from ALICE_PEM's DER at offset 16 and checked that the public key it derived matches
// the one embedded in the PEM.
const EXPECTED_ALICE_KEY_ID: &str =
    "9acdd3973cd0929292a45aa964fc9847efb06b0ef0fee8fe5642096b025eb338";
// Recomputed for spec §4's v1 canonical input as AMENDED (Task 5, transport contract) to add
// `protocol_version` (2 raw bytes, big-endian, length-prefixed like every other field) immediately
// after the domain tag. Same Python script and `cryptography` version as the original test 8
// values, but this is NOT independent confirmation of the new layout the way the original vector
// was of spec §4 before this amendment: the script was written by, and the amendment reviewed by,
// the same person implementing this change. Six bytes are new, right after the domain tag:
// `00000002` (the 4-byte length, 2) followed by `0001` (the value, 1).
const EXPECTED_CANONICAL_HEX: &str = "0000001773796e617073652f73656e6465722d70726f6f662f76310000000200010000001000112233445566778899aabbccddeeff00000012616c6963654073796e617073652e7465737400000010626f624073796e617073652e746573740000000800065ba5d156d2400000000d61757468656e746963617465640000004039616364643339373363643039323932393261343561613936346663393834376566623036623065663066656538666535363432303936623032356562333338000000203ca0d02d916ddbc62d938be706be3b9049079fc69763fe62421958bb1629b59d0000002c00000002000000046c616e650000000773796e6170736500000005746f706963000000086772656574696e67";
const EXPECTED_SIG_HEX: &str = "b4126ae459fcdd9da7e80b23c0246ad2b3584a0e86ab4070d817071b898a951972792eb04bba015cf5d981929441f011aedffacac9e75e70f28e1d6d5b647d0a";

fn signer(pem: &str) -> CryptoManager {
    let mut crypto = CryptoManager::new();
    crypto.load_private_key(pem).expect("fixed test key loads");
    crypto
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The fixed message of spec §4's test vector.
fn vector_message() -> SecureMessage {
    let mut m = unsigned_message();
    m.message_id =
        UuidWrapper::new(uuid::Uuid::parse_str("00112233-4455-6677-8899-aabbccddeeff").unwrap());
    m.timestamp = DateTimeWrapper::new(
        chrono::DateTime::parse_from_rfc3339("2026-09-17T04:00:00.123456Z")
            .unwrap()
            .with_timezone(&chrono::Utc),
    );
    m.add_metadata("topic", "greeting");
    m.add_metadata("lane", "synapse");
    m
}

/// Alice's key, pinned under her id and under an alias, so a from_global_id change is caught by
/// the signature alone (spec §10 test 2).
fn store_with_alice() -> TrustStore {
    let alice = signer(ALICE_PEM).public_key_bytes().unwrap();
    let mut store = TrustStore::new();
    store.pin("alice@synapse.test", alice);
    store.pin("alice-alias@synapse.test", alice);
    store
}

fn signed_by(pem: &str) -> SecureMessage {
    let mut m = vector_message();
    signer(pem).sign_secure_message(&mut m).expect("signs");
    m
}

// §10 test 1
#[test]
fn a_signed_message_verifies() {
    assert_eq!(
        store_with_alice().verify(&signed_by(ALICE_PEM)),
        SenderVerdict::Verified {
            key_id: EXPECTED_ALICE_KEY_ID.to_string()
        }
    );
}

// §10 test 2: every covered field is covered; the reason must be BadSignature, not another rule.
#[test]
fn changing_any_covered_field_contradicts_the_signature() {
    type Mutation = (&'static str, fn(&mut SecureMessage));
    let rows: Vec<Mutation> = vec![
        ("message_id", |m| {
            m.message_id = UuidWrapper::new(uuid::Uuid::new_v4())
        }),
        ("to_global_id", |m| m.to_global_id.push('x')),
        ("from_global_id (same-key alias)", |m| {
            m.from_global_id = "alice-alias@synapse.test".to_string()
        }),
        ("timestamp +1us", |m| {
            m.timestamp = DateTimeWrapper::new(m.timestamp.0 + chrono::TimeDelta::microseconds(1))
        }),
        ("security_level", |m| {
            m.security_level = SecurityLevel::Public
        }),
        ("encrypted_content", |m| m.encrypted_content[0] ^= 1),
        ("metadata add", |m| {
            m.metadata.insert("extra".to_string(), "1".to_string());
        }),
        ("metadata change", |m| {
            m.metadata.insert("topic".to_string(), "other".to_string());
        }),
        ("metadata remove", |m| {
            m.metadata.remove("topic");
        }),
    ];
    let store = store_with_alice();
    let expected = SenderVerdict::Contradicted {
        reason: ContradictedReason::BadSignature,
    };
    let mut wrong = Vec::new();
    for (name, mutate) in rows {
        let mut m = signed_by(ALICE_PEM);
        mutate(&mut m);
        let verdict = store.verify(&m);
        if verdict != expected {
            wrong.push(format!("{name}: {verdict:?}"));
        }
    }
    assert!(
        wrong.is_empty(),
        "fields not covered by the signature: {wrong:#?}"
    );
}

// §10 test 3, in memory (Task 4 repeats the check over a real socket).
#[test]
fn appending_to_routing_path_keeps_the_signature_valid() {
    let mut m = signed_by(ALICE_PEM);
    m.add_routing_hop("relay-1@synapse.test");
    assert!(store_with_alice().verify(&m).is_verified());
}

// §10 test 4
#[test]
fn unsigned_and_unknown_senders_are_unverifiable() {
    let store = store_with_alice();
    assert_eq!(
        store.verify(&vector_message()),
        SenderVerdict::Unverifiable {
            reason: UnverifiableReason::Unsigned
        }
    );
    let mut m = vector_message();
    m.from_global_id = "carol@synapse.test".to_string();
    signer(ALICE_PEM).sign_secure_message(&mut m).unwrap();
    assert_eq!(
        store.verify(&m),
        SenderVerdict::Unverifiable {
            reason: UnverifiableReason::UnknownSender
        }
    );
}

// §10 test 5
#[test]
fn the_contradicted_cases() {
    let store = store_with_alice();
    let contradicted = |reason| SenderVerdict::Contradicted { reason };

    // Bob's key presented for Alice's pinned id.
    assert_eq!(
        store.verify(&signed_by(BOB_PEM)),
        contradicted(ContradictedReason::KeyMismatch)
    );

    // Alice's key_id claimed, Bob's signature.
    let mut m = signed_by(BOB_PEM);
    m.sender_proof.key_id = EXPECTED_ALICE_KEY_ID.to_string();
    assert_eq!(
        store.verify(&m),
        contradicted(ContradictedReason::BadSignature)
    );

    // A 63-byte signature.
    let mut m = signed_by(ALICE_PEM);
    m.sender_proof.sig.pop();
    assert_eq!(
        store.verify(&m),
        contradicted(ContradictedReason::BadSignature)
    );

    // A sub-microsecond timestamp.
    let mut m = signed_by(ALICE_PEM);
    m.timestamp = DateTimeWrapper::new(m.timestamp.0 + chrono::TimeDelta::nanoseconds(1));
    assert_eq!(
        store.verify(&m),
        contradicted(ContradictedReason::NonCanonicalTimestamp)
    );
}

// §10 test 7
#[test]
fn a_json_round_trip_keeps_the_verdict() {
    let json = serde_json::to_string(&signed_by(ALICE_PEM)).unwrap();
    let back: SecureMessage = serde_json::from_str(&json).unwrap();
    assert!(store_with_alice().verify(&back).is_verified());
}

// §10 test 8
#[test]
fn the_test_vector_matches_an_independent_implementation() {
    let alice = signer(ALICE_PEM);
    assert_eq!(
        key_id(&alice.public_key_bytes().unwrap()),
        EXPECTED_ALICE_KEY_ID
    );
    let m = signed_by(ALICE_PEM);
    assert_eq!(hex(&canonical_input(&m)), EXPECTED_CANONICAL_HEX);
    assert_eq!(hex(&m.sender_proof.sig), EXPECTED_SIG_HEX);
}

#[test]
fn signing_without_a_key_is_an_error_not_an_empty_signature() {
    let mut m = vector_message();
    assert!(CryptoManager::new().sign_secure_message(&mut m).is_err());
    assert_eq!(m.sender_proof, SenderProof::unsigned());
}

mod common;
use common::free_udp_port;

async fn udp_manager(port: u16, store: TrustStore) -> TransportManager {
    udp_manager_with_gate(port, store, synapse::replay::GateConfig::default()).await
}

/// Like `udp_manager`, but with an explicit gate config (P2 slice e). Used only where a test's
/// point is independent of the gate's default-deny behaviour, which
/// `tests/replay_suppression.rs` covers.
async fn udp_manager_with_gate(
    port: u16,
    store: TrustStore,
    gate: synapse::replay::GateConfig,
) -> TransportManager {
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
    manager
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

// §10 test 9: verdicts through TransportManager over a real loopback UDP socket.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn verdicts_survive_a_real_udp_hop() {
    let (sender_port, trusting_port, empty_port) =
        (free_udp_port(), free_udp_port(), free_udp_port());
    let sender = udp_manager(sender_port, TrustStore::new()).await;
    let trusting = udp_manager(trusting_port, store_with_alice()).await;
    // accept_unverified: this assertion is about the SenderVerdict computed for an unknown
    // sender surviving a real UDP hop, not about the gate's default-deny admission (which
    // tests/replay_suppression.rs covers); without it the message never reaches receive_messages.
    let empty = udp_manager_with_gate(
        empty_port,
        TrustStore::new(),
        synapse::replay::GateConfig {
            accept_unverified: true,
            ..synapse::replay::GateConfig::default()
        },
    )
    .await;

    let message = signed_by(ALICE_PEM);
    for port in [trusting_port, empty_port] {
        let target = TransportTarget::new("bob@synapse.test".to_string())
            .with_address(format!("127.0.0.1:{port}"));
        sender.send_message(&target, &message).await.expect("send");
    }

    let got = receive_one(&trusting).await;
    assert_eq!(
        got.incoming.message.encrypted_content,
        message.encrypted_content
    );
    assert!(
        got.sender.is_verified(),
        "pinned sender should verify: {:?}",
        got.sender
    );

    assert_eq!(
        receive_one(&empty).await.sender,
        SenderVerdict::Unverifiable {
            reason: UnverifiableReason::UnknownSender
        }
    );

    // A forged datagram: a correctly shaped proof whose signature bytes were altered. The gate
    // (P2 slice e) always rejects `Contradicted` senders — see
    // `tests/replay_suppression.rs::a_contradicted_message_is_dropped_under_both_settings` — so
    // this can no longer be observed through `trusting.receive_messages()`; instead it is read off
    // a raw loopback listener (still a real UDP hop) and verified against `trusting`'s trust store
    // directly, which is what the gate itself calls internally.
    let mut forged = signed_by(ALICE_PEM);
    forged.sender_proof.sig[0] ^= 1;
    let raw_listener = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let raw_listener_port = raw_listener.local_addr().unwrap().port();
    let raw = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    raw.send_to(
        &serde_json::to_vec(&forged).unwrap(),
        ("127.0.0.1", raw_listener_port),
    )
    .unwrap();
    let mut buf = vec![0u8; 65536];
    let (len, _) = tokio::time::timeout(Duration::from_secs(2), raw_listener.recv_from(&mut buf))
        .await
        .expect("the forged datagram arrives")
        .unwrap();
    let received: SecureMessage = serde_json::from_slice(&buf[..len]).unwrap();
    let trusting_store = trusting.trust_store().await;
    assert_eq!(
        trusting_store.verify(&received),
        SenderVerdict::Contradicted {
            reason: ContradictedReason::BadSignature
        }
    );
}
