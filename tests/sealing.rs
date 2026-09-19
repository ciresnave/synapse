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

mod common;
use common::free_udp_port;

async fn udp_node(
    store: TrustStore,
    sealing_key: Option<SealingKeyPair>,
) -> (synapse::transport::TransportManager, u16) {
    use synapse::transport::{TransportManagerBuilder, TransportType, UdpTransportFactory};
    let port = free_udp_port();
    let mut udp = std::collections::HashMap::new();
    udp.insert("bind_port".to_string(), port.to_string());
    let mut builder = TransportManagerBuilder::new()
        .disable_transport(TransportType::Tcp)
        .disable_transport(TransportType::Http)
        .disable_transport(TransportType::Email)
        .disable_transport(TransportType::AutoDiscovery)
        .transport_config(TransportType::Udp, udp)
        .trust_store(store);
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

// §8 test 1, and test 7 through a manager
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_sealed_message_opens_at_its_recipient_over_udp() {
    let alice = signer();
    let bob_key = SealingKeyPair::generate();
    let (sender, _) = udp_node(TrustStore::new(), None).await;
    let (bob, bob_port) = udp_node(store_for(&alice), Some(bob_key.clone())).await;
    let (keyless, keyless_port) = udp_node(store_for(&alice), None).await;

    let m = sealed_signed(&alice, bob_key.public_key(), b"over udp");
    for port in [bob_port, keyless_port] {
        let target = synapse::transport::TransportTarget::new(BOB.to_string())
            .with_address(format!("127.0.0.1:{port}"));
        sender.send_message(&target, &m).await.expect("send");
    }

    let got = receive_one(&bob).await;
    assert!(got.sender.is_verified(), "{:?}", got.sender);
    assert_eq!(got.payload, opened(b"over udp"));
    // Control: what travelled was the sealed body, not the plaintext.
    assert!(
        !got.incoming
            .message
            .encrypted_content
            .windows(8)
            .any(|w| w == b"over udp")
    );

    let got = receive_one(&keyless).await;
    assert!(got.sender.is_verified());
    assert_eq!(got.payload, Payload::CouldNotOpen(OpenError::NoSealingKey));
}

// §8 test 8
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_message_that_could_not_be_opened_is_never_acknowledged() {
    use synapse::error::SynapseError;
    let alice = signer();
    let bob_key = SealingKeyPair::generate();
    let mut bob_signer = CryptoManager::new();
    bob_signer.generate_keypair().unwrap();
    let listener = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let reply_port = listener.local_addr().unwrap().port();
    let (keyless, keyless_port) = udp_node(store_for(&alice), None).await;
    let (keyed, keyed_port) = udp_node(store_for(&alice), Some(bob_key.clone())).await;
    let mut buf = vec![0u8; 65536];

    let request = |text: &[u8]| {
        let mut m = SecureMessage::new(BOB, ALICE, text.to_vec(), SecurityLevel::Secure);
        m.request_ack(format!("127.0.0.1:{reply_port}"));
        sealing::seal(&mut m, bob_key.public_key()).unwrap();
        alice.sign_secure_message(&mut m).unwrap();
        m
    };

    send_raw(keyless_port, &request(b"unopenable here"));
    let got = receive_one(&keyless).await;
    assert!(got.sender.is_verified());
    assert_eq!(got.payload, Payload::CouldNotOpen(OpenError::NoSealingKey));
    let err = keyless.acknowledge(&got, &bob_signer).await.unwrap_err();
    assert!(
        matches!(err, SynapseError::InvalidMessageFormat(_)),
        "{err:?}"
    );
    let nothing = tokio::time::timeout(
        std::time::Duration::from_millis(500),
        listener.recv_from(&mut buf),
    )
    .await;
    assert!(nothing.is_err(), "a refused acknowledge must send nothing");

    // Control, same listener: an opened message is acknowledged.
    send_raw(keyed_port, &request(b"opens here"));
    let got = receive_one(&keyed).await;
    assert_eq!(got.payload, opened(b"opens here"));
    keyed.acknowledge(&got, &bob_signer).await.unwrap();
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        listener.recv_from(&mut buf),
    )
    .await
    .expect("the control ack arrives")
    .unwrap();
}
