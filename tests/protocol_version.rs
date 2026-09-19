// SPDX-License-Identifier: MIT OR Apache-2.0
//! `protocol_version` on the wire — Task 5, transport contract.
//!
//! The version is a `u16` on `SecureMessage`, covered by the sender's signature
//! (`sender_auth::canonical_input`), and an unsupported version is refused by name
//! (`UnverifiableReason::UnsupportedVersion`), never as a parse failure.

use synapse::CryptoManager;
use synapse::sender_auth::{TrustStore, UnverifiableReason, canonical_input};
use synapse::transport::{TransportManagerBuilder, TransportType, UdpTransportFactory};
use synapse::types::{PROTOCOL_VERSION, SecureMessage, SecurityLevel};

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

fn free_udp_port() -> u16 {
    let socket = std::net::UdpSocket::bind("127.0.0.1:0").expect("bind ephemeral");
    socket.local_addr().expect("local_addr").port()
}

/// Follows the loopback pattern in `tests/replay_suppression.rs`: bind `127.0.0.1` only, never a
/// wildcard address, which raises a Windows Firewall prompt on this machine.
async fn udp_node(store: TrustStore) -> (synapse::transport::TransportManager, u16) {
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

fn send_raw(port: u16, message: &SecureMessage) {
    let raw = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    raw.send_to(&serde_json::to_vec(message).unwrap(), ("127.0.0.1", port))
        .unwrap();
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

/// Control: an ordinary, unmodified version-1 message verifies.
#[test]
fn a_signed_version_1_message_verifies() {
    let alice = signer();
    let mut message =
        SecureMessage::new(BOB, ALICE, b"hello".to_vec(), SecurityLevel::Authenticated);
    assert_eq!(message.protocol_version, PROTOCOL_VERSION);
    alice.sign_secure_message(&mut message).expect("sign");
    assert!(store_for(&alice).verify(&message).is_verified());
}

/// A message serialised WITHOUT `protocol_version` at all (the shape every message had on the wire
/// before this field existed) still deserialises, with `protocol_version == 1` filled in by
/// `#[serde(default = "default_protocol_version")]`.
#[test]
fn a_message_without_the_field_deserialises_as_version_1() {
    let alice = signer();
    let mut message =
        SecureMessage::new(BOB, ALICE, b"hello".to_vec(), SecurityLevel::Authenticated);
    alice.sign_secure_message(&mut message).expect("sign");

    let mut value = serde_json::to_value(&message).unwrap();
    let removed = value.as_object_mut().unwrap().remove("protocol_version");
    assert!(removed.is_some(), "the field must be present to remove");

    let back: SecureMessage = serde_json::from_value(value).expect("parses without the field");
    assert_eq!(back.protocol_version, 1);
}

/// A version-2 message is refused by name, never as a parse failure, and a `TransportManager`
/// receiving it over loopback UDP records a knock whose reason is `unsupported_protocol_version`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_version_2_message_is_unsupported_and_knocks() {
    let alice = signer();
    let mut message =
        SecureMessage::new(BOB, ALICE, b"hello".to_vec(), SecurityLevel::Authenticated);
    message.protocol_version = 2;
    // Sign AFTER setting the version: this is a genuine version-2 message from Alice, not a
    // relay's rewrite -- that case is `a_relay_cannot_downgrade_the_version_without_detection`
    // below.
    alice.sign_secure_message(&mut message).expect("sign");

    // In-memory: `Unverifiable { UnsupportedVersion }`, not any kind of parse or deserialisation
    // error -- the message parses fine and reaches `verify_at`.
    assert_eq!(
        store_for(&alice).verify(&message),
        synapse::sender_auth::SenderVerdict::Unverifiable {
            reason: UnverifiableReason::UnsupportedVersion
        }
    );

    // Over a real loopback UDP hop: dropped, and the knock names the reason.
    let (bob, port) = udp_node(store_for(&alice)).await;
    send_raw(port, &message);
    assert!(
        drain(&bob).await.is_empty(),
        "an unsupported version must not be delivered"
    );
    let counters = bob.inbound_counters().await;
    assert_eq!(counters.dropped_unverifiable, 1);
    assert_eq!(counters.admitted, 0);
    let knocks = bob.knocks().await;
    assert_eq!(knocks.len(), 1);
    assert_eq!(knocks[0].reason, "unsupported_protocol_version");
}

/// The relay test. A relay that rewrites `protocol_version` without re-signing must invalidate the
/// signature: sign a version-1 message, then change `protocol_version` to 2 without re-signing.
///
/// What each assertion proves:
/// - The verdict is NOT `Verified` -- but on its own this proves nothing about signature coverage,
///   because `verify_at` checks the version FIRST: a rewritten version-2 message is refused as
///   `UnsupportedVersion` whether or not the signature covers the version field at all. A build
///   that forgot to add `protocol_version` to `canonical_input` would still fail this assertion,
///   for the wrong reason.
/// - `canonical_input` differing between the same message at version 1 and at version 2 is what
///   proves the version is actually inside the signed bytes: if a relay's rewrite left the signed
///   bytes unchanged, this would be the assertion that would catch it, since `verify_at`'s
///   version-first check would otherwise mask a `canonical_input` that silently ignored the field.
#[test]
fn a_relay_cannot_downgrade_the_version_without_detection() {
    let alice = signer();
    let mut message =
        SecureMessage::new(BOB, ALICE, b"hello".to_vec(), SecurityLevel::Authenticated);
    alice.sign_secure_message(&mut message).expect("sign");
    assert_eq!(message.protocol_version, 1);

    let canonical_at_v1 = canonical_input(&message);

    // The relay's rewrite: change the version in place, WITHOUT re-signing.
    let mut rewritten = message.clone();
    rewritten.protocol_version = 2;

    // Proves the version is signed over: the bytes a signature would need to cover differ between
    // versions, so a relay's rewrite cannot leave a valid signature behind.
    let canonical_at_v2 = canonical_input(&rewritten);
    assert_ne!(
        canonical_at_v1, canonical_at_v2,
        "protocol_version must be inside the signed bytes"
    );

    // Observed effect: the rewritten message is refused (as UnsupportedVersion, per verify_at's
    // ordering -- see the doc comment above for what this does and does not prove on its own).
    let verdict = store_for(&alice).verify(&rewritten);
    assert_ne!(
        verdict,
        synapse::sender_auth::SenderVerdict::Verified {
            key_id: synapse::sender_auth::key_id(&alice.public_key_bytes().unwrap())
        }
    );
    assert_eq!(
        verdict,
        synapse::sender_auth::SenderVerdict::Unverifiable {
            reason: UnverifiableReason::UnsupportedVersion
        }
    );
}

/// Extracts one function's body (from its `fn NAME` line through the matching closing brace, by
/// simple brace counting) out of `src`, so the check below reads only the TXT-record builders and
/// is not brittle against an unrelated `"1.0"` or `"version"` literal appearing anywhere else in a
/// 1000+ line file.
fn extract_fn<'a>(src: &'a str, fn_name: &str) -> &'a str {
    let needle = format!("fn {fn_name}");
    let start = src
        .find(&needle)
        .unwrap_or_else(|| panic!("fn {fn_name} not found in source"));
    let body_start = src[start..]
        .find('{')
        .map(|i| start + i)
        .expect("fn has a body");
    let mut depth = 0usize;
    for (offset, ch) in src[body_start..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return &src[start..body_start + offset + 1];
                }
            }
            _ => {}
        }
    }
    panic!("fn {fn_name} body never closes");
}

/// Both mDNS TXT-record paths advertise `synapse_protocol=1` and neither advertises the old
/// `version` key.
///
/// Both `src/transport/discovery.rs` and `src/transport/mdns_enhanced.rs` build their TXT
/// key/value pairs inline, inside functions that open a real multicast socket to register or
/// respond to mDNS -- there is no pure, sub-socket function to call from a unit test, and
/// instantiating either live (as `mdns_enhanced.rs`'s own
/// `test_enhanced_mdns_creation_refuses_under_loopback` shows for its default scope) either
/// refuses outright or risks the Windows Firewall prompt this crate's tests are documented to
/// avoid. So this reads the source directly, scoped to the specific TXT-record-building functions
/// (not the whole file, which would make this brittle against any future unrelated `"1.0"` or
/// `"version"` literal elsewhere in either file): a regression that reintroduces the `"version"`
/// key, or drops `"synapse_protocol"`, in one of these functions fails here.
#[test]
fn mdns_txt_records_advertise_synapse_protocol_not_version() {
    let discovery_src = include_str!("../src/transport/discovery.rs");
    let mdns_enhanced_src = include_str!("../src/transport/mdns_enhanced.rs");

    let sites = [
        (
            "discovery.rs::initialize_discovery",
            extract_fn(discovery_src, "initialize_discovery"),
        ),
        (
            "mdns_enhanced.rs::build_txt_records",
            extract_fn(mdns_enhanced_src, "build_txt_records"),
        ),
        (
            "mdns_enhanced.rs::create_service_responder",
            extract_fn(mdns_enhanced_src, "create_service_responder"),
        ),
        (
            "mdns_enhanced.rs::comprehensive_discovery",
            extract_fn(mdns_enhanced_src, "comprehensive_discovery"),
        ),
    ];

    for (name, body) in sites {
        assert!(
            body.contains("\"synapse_protocol\""),
            "{name} must advertise the synapse_protocol TXT key"
        );
        assert!(
            !body.contains("\"version\""),
            "{name} must not advertise the old version TXT key"
        );
        assert!(
            !body.contains("\"1.0\"") && !body.contains("\"1.1.0\""),
            "{name} must not hard-code the old 1.0/1.1.0 disagreement"
        );
    }
}
