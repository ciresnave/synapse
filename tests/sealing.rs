// SPDX-License-Identifier: MIT OR Apache-2.0
//! Sealing (recipient-key encryption) — spec: docs/superpowers/specs/2026-09-17-sealing-design.md
//! Test numbers refer to the spec's §8.

use synapse::CryptoManager;
use synapse::sealing::{self, OpenError, Payload, SEALED_KEY, SealingKeyPair, SealingPublicKey};
use synapse::sender_auth::{ContradictedReason, SenderVerdict, TrustStore};
use synapse::types::{SecureMessage, SecurityLevel};

const ALICE: &str = "alice@synapse.test";
const ALIAS: &str = "alice-alias@synapse.test";
const BOB: &str = "bob@synapse.test";

fn signer() -> CryptoManager {
    let mut crypto = CryptoManager::new();
    crypto.generate_keypair().unwrap();
    crypto
}

/// Alice's signing key pinned under her id and an alias (so a from change re-signs as Verified).
fn store_for(alice: &CryptoManager) -> TrustStore {
    let key = alice.public_key_bytes().unwrap();
    let mut store = TrustStore::new();
    store.pin(ALICE, key);
    store.pin(ALIAS, key);
    store
}

/// A message from Alice to Bob, sealed to `to` and then signed by Alice.
fn sealed_signed(alice: &CryptoManager, to: &SealingPublicKey, text: &[u8]) -> SecureMessage {
    let mut m = SecureMessage::new(BOB, ALICE, text.to_vec(), SecurityLevel::Secure);
    sealing::seal(&mut m, to).expect("seal");
    alice.sign_secure_message(&mut m).expect("sign");
    m
}

fn opened(text: &[u8]) -> Payload {
    Payload::Opened(text.to_vec())
}

// §8 test 2
#[test]
fn a_message_sealed_to_another_key_cannot_be_opened() {
    let alice = signer();
    let (bob, carol) = (SealingKeyPair::generate(), SealingKeyPair::generate());
    let m = sealed_signed(&alice, carol.public_key(), b"for carol");
    assert_eq!(
        sealing::open(&m, Some(&bob)),
        Payload::CouldNotOpen(OpenError::Undecryptable)
    );
    // Control: the right key opens it.
    assert_eq!(sealing::open(&m, Some(&carol)), opened(b"for carol"));
}

// §8 test 3
#[test]
fn tampered_ciphertext_contradicts_the_signature_and_does_not_open() {
    let alice = signer();
    let bob = SealingKeyPair::generate();
    let mut m = sealed_signed(&alice, bob.public_key(), b"hello");
    assert!(store_for(&alice).verify(&m).is_verified());
    *m.encrypted_content.last_mut().unwrap() ^= 1;
    assert_eq!(
        store_for(&alice).verify(&m),
        SenderVerdict::Contradicted {
            reason: ContradictedReason::BadSignature
        }
    );
    assert_eq!(
        sealing::open(&m, Some(&bob)),
        Payload::CouldNotOpen(OpenError::Undecryptable)
    );
}

// §8 test 4: the authenticated data binds the ciphertext to its header. Each change is re-signed,
// so the signature is valid and only the binding can refuse it.
#[test]
fn the_ciphertext_is_bound_to_its_header() {
    let alice = signer();
    let bob = SealingKeyPair::generate();
    let store = store_for(&alice);

    let control = sealed_signed(&alice, bob.public_key(), b"bound");
    assert_eq!(sealing::open(&control, Some(&bob)), opened(b"bound"));

    type Change = (&'static str, fn(&mut SecureMessage));
    let changes: Vec<Change> = vec![
        ("to_global_id", |m| {
            m.to_global_id = "bob2@synapse.test".to_string()
        }),
        ("from_global_id", |m| m.from_global_id = ALIAS.to_string()),
        ("message_id", |m| {
            m.message_id =
                synapse::blockchain::serialization::UuidWrapper::new(uuid::Uuid::new_v4())
        }),
    ];
    let mut wrong = Vec::new();
    for (name, change) in changes {
        let mut m = sealed_signed(&alice, bob.public_key(), b"bound");
        change(&mut m);
        alice.sign_secure_message(&mut m).unwrap();
        assert!(
            store.verify(&m).is_verified(),
            "{name}: the re-signed message should verify"
        );
        let payload = sealing::open(&m, Some(&bob));
        if payload != Payload::CouldNotOpen(OpenError::Undecryptable) {
            wrong.push(format!("{name}: {payload:?}"));
        }
    }
    assert!(wrong.is_empty(), "header fields not bound: {wrong:#?}");
}

// §8 test 5
#[test]
fn level_and_marker_must_agree() {
    let alice = signer();
    let bob = SealingKeyPair::generate();

    let plain = SecureMessage::new(BOB, ALICE, b"plain".to_vec(), SecurityLevel::Authenticated);
    assert_eq!(
        sealing::open(&plain, Some(&bob)),
        Payload::Plain(b"plain".to_vec())
    );

    let secure_but_plain = SecureMessage::new(BOB, ALICE, b"plain".to_vec(), SecurityLevel::Secure);
    assert_eq!(
        sealing::open(&secure_but_plain, Some(&bob)),
        Payload::CouldNotOpen(OpenError::NotSealed)
    );

    let mut sealed_but_plain_level = sealed_signed(&alice, bob.public_key(), b"x");
    sealed_but_plain_level.security_level = SecurityLevel::Authenticated;
    assert_eq!(
        sealing::open(&sealed_but_plain_level, Some(&bob)),
        Payload::CouldNotOpen(OpenError::SealedButPlainLevel)
    );

    let mut truncated = sealed_signed(&alice, bob.public_key(), b"x");
    truncated.encrypted_content.truncate(10);
    assert_eq!(
        sealing::open(&truncated, Some(&bob)),
        Payload::CouldNotOpen(OpenError::Truncated)
    );

    let mut other_version = sealed_signed(&alice, bob.public_key(), b"x");
    other_version.encrypted_content[0] = 2;
    assert_eq!(
        sealing::open(&other_version, Some(&bob)),
        Payload::CouldNotOpen(OpenError::UnsupportedVersion)
    );
    // `Private` is also a sealed level.
    let mut private = SecureMessage::new(BOB, ALICE, b"p".to_vec(), SecurityLevel::Private);
    sealing::seal(&mut private, bob.public_key()).unwrap();
    assert_eq!(sealing::open(&private, Some(&bob)), opened(b"p"));
}

// §8 test 6 (a, b)
#[test]
fn sealing_refuses_plain_levels_and_double_sealing() {
    let bob = SealingKeyPair::generate();
    for level in [SecurityLevel::Public, SecurityLevel::Authenticated] {
        let mut m = SecureMessage::new(BOB, ALICE, b"x".to_vec(), level.clone());
        assert!(
            sealing::seal(&mut m, bob.public_key()).is_err(),
            "{level:?}"
        );
        assert_eq!(
            m.encrypted_content, b"x",
            "a refused seal leaves the body alone"
        );
        assert!(!m.metadata.contains_key(SEALED_KEY));
    }
    let mut m = SecureMessage::new(BOB, ALICE, b"x".to_vec(), SecurityLevel::Secure);
    sealing::seal(&mut m, bob.public_key()).unwrap();
    assert_eq!(m.metadata.get(SEALED_KEY).map(String::as_str), Some("v1"));
    assert!(sealing::seal(&mut m, bob.public_key()).is_err());
}

// §8 test 7 (in memory)
#[test]
fn without_a_sealing_key_nothing_opens() {
    let alice = signer();
    let bob = SealingKeyPair::generate();
    let m = sealed_signed(&alice, bob.public_key(), b"x");
    assert_eq!(
        sealing::open(&m, None),
        Payload::CouldNotOpen(OpenError::NoSealingKey)
    );
}

// §8 test 9 (formats)
#[test]
fn keys_use_the_standard_x25519_formats() {
    let kp = SealingKeyPair::generate();
    let private_pem = kp.to_pkcs8_pem();
    assert!(private_pem.contains("BEGIN PRIVATE KEY"));
    let again = SealingKeyPair::from_pkcs8_pem(&private_pem).unwrap();
    assert_eq!(again.public_key(), kp.public_key());
    let der = pem::parse(&private_pem).unwrap();
    assert_eq!(
        hex(&der.contents()[..16]),
        "302e020100300506032b656e04220420"
    );

    let public_pem = kp.public_key().to_spki_pem();
    let der = pem::parse(&public_pem).unwrap();
    assert_eq!(der.contents().len(), 44);
    assert_eq!(hex(&der.contents()[..12]), "302a300506032b656e032100");
    assert_eq!(
        &SealingPublicKey::from_spki_pem(&public_pem).unwrap(),
        kp.public_key()
    );
    assert_eq!(kp.public_key().key_id().len(), 64);

    // Slice a's Ed25519 public PEM (raw bytes) is not an X25519 SPKI key.
    let mut ed = CryptoManager::new();
    let (_, ed_public) = ed.generate_keypair().unwrap();
    assert!(SealingPublicKey::from_spki_pem(&ed_public).is_err());

    // A private key never appears in Debug output.
    let shown = format!("{kp:?}");
    let raw = der_private_bytes(&private_pem);
    assert!(!shown.contains(&hex(&raw)), "{shown}");
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn der_private_bytes(pem_text: &str) -> Vec<u8> {
    pem::parse(pem_text).unwrap().contents()[16..].to_vec()
}

// §8 test 6c: the email router never produces a sealed or Secure message.
#[tokio::test]
async fn the_router_converts_to_a_plain_authenticated_message() {
    let config = synapse::Config::default_for_entity("router-test", "tool");
    let router = synapse::SynapseRouter::new(config, ALICE.to_string())
        .await
        .expect("router");
    let simple = synapse::types::SimpleMessage {
        to: BOB.to_string(),
        from_entity: ALICE.to_string(),
        content: "hello over email".to_string(),
        message_type: synapse::types::MessageType::Direct,
        metadata: Default::default(),
    };
    let m = router.convert_to_secure_message(&simple).await.unwrap();
    assert_eq!(m.security_level, SecurityLevel::Authenticated);
    assert_eq!(m.encrypted_content, b"hello over email");
    assert!(!m.metadata.contains_key(SEALED_KEY));
}

// §8 test 9 (independence and pinning)
#[test]
fn a_node_has_independent_signing_and_sealing_keys() {
    let mut node = CryptoManager::new();
    node.generate_keypair().unwrap();
    let (sealing_private_pem, sealing_public_pem) = node.generate_sealing_key().unwrap();
    let signing_id = synapse::sender_auth::key_id(&node.public_key_bytes().unwrap());
    let sealing_id = node.sealing_key().unwrap().public_key().key_id();
    assert_ne!(signing_id, sealing_id);
    assert_eq!(node.sealing_public_key_pem().unwrap(), sealing_public_pem);

    let mut other = CryptoManager::new();
    other.load_sealing_key_pem(&sealing_private_pem).unwrap();
    assert_eq!(
        other.sealing_key().unwrap().public_key(),
        node.sealing_key().unwrap().public_key()
    );

    let mut store = TrustStore::new();
    store.pin_sealing_key_pem(BOB, &sealing_public_pem).unwrap();
    assert_eq!(store.sealing_key_id(BOB), Some(sealing_id));
    assert_eq!(
        store.sealing_key_for(BOB),
        Some(node.sealing_key().unwrap().public_key())
    );
    assert!(store.sealing_key_for(ALICE).is_none());
    // Pinning a sealing key does not pin a signing key.
    assert!(!store.is_pinned(BOB));
}

#[test]
fn crypto_manager_seals_and_opens() {
    let mut alice = CryptoManager::new();
    alice.generate_keypair().unwrap();
    let mut bob = CryptoManager::new();
    bob.generate_sealing_key().unwrap();
    let bob_key = bob.sealing_key().unwrap().public_key().clone();

    let mut m = SecureMessage::new(BOB, ALICE, b"via manager".to_vec(), SecurityLevel::Secure);
    alice.seal_secure_message(&mut m, &bob_key).unwrap();
    alice.sign_secure_message(&mut m).unwrap();
    assert_eq!(bob.open_payload(&m), opened(b"via manager"));
    assert_eq!(
        alice.open_payload(&m),
        Payload::CouldNotOpen(OpenError::NoSealingKey)
    );
}

// §8 test 10: the self-decrypting encryption is gone from compiled source.
#[test]
fn the_self_decrypting_encryption_is_gone() {
    fn rust_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                rust_files(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files(&src, &mut files);
    // src/wasm compiles only for wasm32 and has its own, unrelated decrypt_message.
    let files: Vec<_> = files
        .into_iter()
        .filter(|p| !p.components().any(|c| c.as_os_str() == "wasm"))
        .collect();
    let needles = [
        ["fn encrypt", "_message("].concat(),
        ["fn decrypt", "_message("].concat(),
        ["fn encrypt", "_with_aes("].concat(),
    ];
    let control = ["fn seal_secure", "_message("].concat();
    let mut found = Vec::new();
    let mut control_hits = 0;
    for path in &files {
        let text = std::fs::read_to_string(path).unwrap();
        for needle in &needles {
            if text.contains(needle.as_str()) {
                found.push(format!("{}: {needle}", path.display()));
            }
        }
        if text.contains(control.as_str()) {
            control_hits += 1;
        }
    }
    println!(
        "scanned {} files under src/ (src/wasm excluded)",
        files.len()
    );
    assert_eq!(
        control_hits, 1,
        "the scan must find the new sealing entry point"
    );
    assert!(found.is_empty(), "{found:#?}");
}
