// SPDX-License-Identifier: MIT OR Apache-2.0
//! Sender authentication — spec: docs/superpowers/specs/2026-09-17-sender-authentication-design.md
//! Test numbers refer to the spec's §10.

use synapse::CryptoManager;
use synapse::blockchain::serialization::{DateTimeWrapper, UuidWrapper};
use synapse::sender_auth::{
    ContradictedReason, ProofAlg, SenderProof, SenderVerdict, TrustStore, UnverifiableReason,
    canonical_input, key_id,
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
const EXPECTED_CANONICAL_HEX: &str = "0000001773796e617073652f73656e6465722d70726f6f662f76310000001000112233445566778899aabbccddeeff00000012616c6963654073796e617073652e7465737400000010626f624073796e617073652e746573740000000800065ba5d156d2400000000d61757468656e746963617465640000004039616364643339373363643039323932393261343561613936346663393834376566623036623065663066656538666535363432303936623032356562333338000000203ca0d02d916ddbc62d938be706be3b9049079fc69763fe62421958bb1629b59d0000002c00000002000000046c616e650000000773796e6170736500000005746f706963000000086772656574696e67";
const EXPECTED_SIG_HEX: &str = "353040783df25d25a253d284811347c3f22d8314874392d2c1a63fb81b3cad71d0b29ca05d2baed4191a9aaf8612b0bfa88f09a040b15c3e5618deffbe70ef0a";

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
