// SPDX-License-Identifier: MIT OR Apache-2.0
//! Transport contract, PR B: every repaired transport carries a verified message end to end.
//! See docs/superpowers/specs/2026-09-18-transport-contract-design.md §7.

use std::collections::HashMap;

use synapse::CryptoManager;
use synapse::certificate::{AgentCertificate, Permission};
use synapse::sender_auth::TrustStore;
use synapse::transport::{
    ReceivedMessage, TransportFactory, TransportManager, TransportManagerBuilder, TransportTarget,
    TransportType,
};
use synapse::types::{SecureMessage, SecurityLevel};

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

/// Build a manager with exactly one transport enabled, listening on loopback `port`.
async fn node(
    kind: TransportType,
    factory: Box<dyn TransportFactory>,
    port: u16,
    store: TrustStore,
) -> TransportManager {
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
        .build();
    manager
        .register_factory(factory)
        .await
        .expect("factory registers");
    tokio::time::timeout(std::time::Duration::from_secs(5), manager.start())
        .await
        .expect("start returns")
        .expect("start succeeds");
    manager
}

/// A certificate from `account` for `holder`, valid for an hour. Adapted from
/// tests/agent_certificates.rs, because a test binary cannot import another's helpers.
fn cert_for(
    account: &ed25519_dalek::SigningKey,
    holder: &CryptoManager,
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
            subject_sealing_key: [0u8; 32],
            not_before: now - chrono::Duration::minutes(1),
            not_after: now + chrono::Duration::hours(1),
            permissions: vec![Permission::Send],
            may_delegate: 0,
            signature: [0u8; 64],
        },
        account,
    )
}

/// Send one signed message from Alice to Bob over `kind`, and return what Bob's manager delivers.
/// Asserts it arrived Verified: the point of every repair is that a real message crosses a real
/// socket and comes out of the manager with a verdict.
async fn round_trip(
    kind: TransportType,
    alice_factory: Box<dyn TransportFactory>,
    bob_factory: Box<dyn TransportFactory>,
    alice_port: u16,
    bob_port: u16,
) -> ReceivedMessage {
    let account = ed25519_dalek::SigningKey::from_bytes(&[42u8; 32]);
    let mut alice = CryptoManager::new();
    alice.generate_keypair().expect("keypair");
    alice.set_certificate_chain(vec![cert_for(&account, &alice, "alice@repair.test")]);

    let mut bob_store = TrustStore::new();
    bob_store.pin_account_key("alice-account", account.verifying_key().to_bytes());

    let alice_node = node(kind, alice_factory, alice_port, TrustStore::new()).await;
    let bob_node = node(kind, bob_factory, bob_port, bob_store).await;

    let mut message = SecureMessage::new(
        "bob@repair.test",
        "alice@repair.test",
        b"repaired".to_vec(),
        SecurityLevel::Authenticated,
    );
    alice.sign_secure_message(&mut message).expect("sign");
    let target = TransportTarget::new("bob@repair.test".to_string())
        .with_address(format!("127.0.0.1:{bob_port}"));
    alice_node
        .send_message(&target, &message)
        .await
        .expect("the transport sends");

    for _ in 0..30 {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let mut batch = bob_node.receive_messages().await.expect("receive");
        if let Some(received) = batch.pop() {
            assert!(batch.is_empty(), "exactly one message expected");
            assert!(
                received.sender.is_verified(),
                "the message must arrive Verified: {:?}",
                received.sender
            );
            assert_eq!(received.incoming.message.message_id.0, message.message_id.0);
            return received;
        }
    }
    panic!("no message arrived over {kind:?} within 3 s");
}

/// A free loopback port. Bound and released, so another process could take it before the node
/// binds it; a lost race shows up as "no message arrived", not as a false pass.
fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("bind ephemeral")
        .local_addr()
        .expect("local_addr")
        .port()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tcp_carries_a_verified_message_end_to_end() {
    let received = round_trip(
        TransportType::Tcp,
        Box::new(synapse::transport::TcpTransportFactory),
        Box::new(synapse::transport::TcpTransportFactory),
        free_port(),
        free_port(),
    )
    .await;
    assert_eq!(received.incoming.transport_type, TransportType::Tcp);
    assert!(
        matches!(&received.payload, synapse::sealing::Payload::Plain(body) if body == b"repaired"),
        "the body must arrive intact: {:?}",
        received.payload
    );
}
