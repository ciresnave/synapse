// SPDX-License-Identifier: MIT OR Apache-2.0
//! INDEPENDENT confirm-or-refute probe for the TCP send/receive silent-drop.
//!
//! Question (from the PM's brief, symptom only — no borrowed diagnosis): does a
//! message that `send_message` receipts `Ok` with `DeliveryConfirmation::Sent`
//! actually ARRIVE at the receiver?
//!
//! Ported for the transport-contract task (2026-09-18): `Transport::receive_raw` (see
//! `synapse::transport::abstraction::TransportReceive`) now takes a `&mut RawInbox` that outside
//! code can push into but cannot construct or read back (see `RawInbox`'s docs) — so a caller
//! outside the crate cannot get a `Vec<IncomingMessage>` out of a raw transport at all; only
//! `TransportManager::receive_messages`, which drains the inbox after verifying every message
//! against a `SenderVerdict`, is public. This probe now drives `TcpTransportImpl` through a real
//! `TransportManager`
//! (built with `TransportManagerBuilder` + `TcpTransportFactory`, exactly the pattern a
//! deployment uses), over a REAL `127.0.0.1` loopback socket, and asserts actual DELIVERY (the
//! payload comes back out of `TransportManager::receive_messages`), never merely that the send
//! returned `Ok`. A green that only proved "send returned Ok" would prove nothing about the bug.
//!
//! The message is intentionally left unsigned: this probe is about whether the TCP transport
//! delivers what was sent, not about sender authentication (that is covered by
//! `sender_authentication.rs`). The receiver's gate is configured to accept unverified senders so
//! delivery is observable, and the verdict is asserted to be the honest `Unverifiable` one rather
//! than silently discarded.
//!
//! The PM's sharpened discriminator is demonstrated explicitly: the probe shows the
//! port ACCEPTS a raw connect ("open") in the same run in which the message is not
//! delivered ("not served"). Those are different facts and a connect test cannot
//! tell them apart.
//!
//! Scope / limitations (stated so a reader knows what this did and did not exercise):
//!   - Single process, loopback, one message, one direction, TCP only.
//!   - Single reader of `receive_messages()`, which DRAINS (destructive) — a probe
//!     with two readers would manufacture a false negative.
//!   - The send crosses a real kernel socket, so the receiver's accept loop is
//!     exercised identically to a two-process test for THIS (receiver-side) bug.

use std::collections::HashMap;
use std::time::Duration;

use synapse::replay::GateConfig;
use synapse::sender_auth::SenderVerdict;
use synapse::transport::abstraction::{DeliveryConfirmation, TransportStatus, TransportTarget};
use synapse::transport::tcp_unified::TcpTransportFactory;
use synapse::transport::{TransportManager, TransportManagerBuilder, TransportType};
use synapse::types::{SecureMessage, SecurityLevel};

/// A free loopback port: bind :0, read the port, drop the listener, reuse it.
fn free_port() -> u16 {
    let l = std::net::TcpListener::bind("127.0.0.1:0").expect("bind ephemeral");
    let p = l.local_addr().expect("local_addr").port();
    drop(l);
    p
}

fn cfg(listen_port: u16) -> HashMap<String, String> {
    let mut m = HashMap::new();
    m.insert("listen_port".to_string(), listen_port.to_string());
    m
}

/// A `TransportManager` with only TCP enabled, bound to `listen_port`, and configured to accept
/// unverified senders (this probe is about delivery, not authentication).
async fn tcp_manager(listen_port: u16) -> TransportManager {
    let manager = TransportManagerBuilder::new()
        .disable_transport(TransportType::Udp)
        .disable_transport(TransportType::Http)
        .disable_transport(TransportType::Email)
        .disable_transport(TransportType::AutoDiscovery)
        .transport_config(TransportType::Tcp, cfg(listen_port))
        .gate_config(GateConfig {
            accept_unverified: true,
            ..GateConfig::default()
        })
        .build();
    manager
        .register_factory(Box::new(TcpTransportFactory))
        .await
        .expect("register TCP factory");
    manager
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_receipted_tcp_message_actually_arrives() {
    let port = free_port();

    // Receiver: a TransportManager with TCP bound to `port`.
    let receiver = tcp_manager(port).await;
    tokio::time::timeout(Duration::from_secs(5), receiver.start())
        .await
        .expect("receiver.start() returns")
        .expect("receiver.start() should succeed");
    // `TransportManager::start()` swallows a per-transport start error (it logs and continues, so
    // that one bad transport doesn't stop the others), so the `expect` above cannot fail even if
    // TCP itself never came up. But the two checks below are not vacuous for TCP specifically:
    // `start_transport` (src/transport/manager.rs) only records `Running` and inserts into the
    // `transports` map AFTER BOTH `factory.create_transport` and `transport.start()` return `Ok`;
    // on an error from either it returns early and the status is left at `Starting`, never
    // `Running`. So both checks below are real evidence that TCP's own `start()` returned `Ok`
    // -- they just don't prove the socket accepts connections, which is FACT 1 below (a real
    // connect), the one check that can actually fail for TCP.
    assert_eq!(
        receiver
            .get_transport_status()
            .await
            .get(&TransportType::Tcp),
        Some(&TransportStatus::Running),
        "receiver should report Running after start()"
    );
    assert!(
        receiver
            .list_available_transports()
            .await
            .contains(&TransportType::Tcp),
        "receiver should have a TCP transport registered"
    );

    // Let the server's accept loop become ready.
    tokio::time::sleep(Duration::from_millis(200)).await;

    // FACT 1 — the port is OPEN: a raw connect completes the TCP handshake.
    // (With the bug this still succeeds, because the constructor's listener owns the
    // port and its backlog absorbs the connection even though nothing accept()s it.)
    let raw = std::net::TcpStream::connect(("127.0.0.1", port));
    assert!(
        raw.is_ok(),
        "raw connect to the receiver port must succeed (port open): {raw:?}"
    );
    drop(raw);

    // Sender: a TransportManager with TCP enabled but not listening on any fixed port of its own.
    let sender = tcp_manager(0).await;
    tokio::time::timeout(Duration::from_secs(5), sender.start())
        .await
        .expect("sender.start() returns")
        .expect("sender.start() should succeed");

    let payload = b"fuel1-probe-PAYLOAD-42".to_vec();
    let msg = SecureMessage::new(
        "receiver-id",
        "sender-id",
        payload.clone(),
        SecurityLevel::Authenticated,
    );
    let target =
        TransportTarget::new("receiver-id".to_string()).with_address(format!("127.0.0.1:{port}"));

    // The "success message": send_message is expected to receipt Ok/Sent EVEN IF the
    // message never arrives — that is the shape under test, not the property we trust.
    let receipt = sender
        .send_message(&target, &msg)
        .await
        .expect("send_message should return Ok");
    assert_eq!(
        receipt.confirmation,
        DeliveryConfirmation::Sent,
        "send receipts Sent"
    );
    eprintln!(
        "SEND RECEIPTED: confirmation={:?} time={:?} target_reached={}",
        receipt.confirmation, receipt.delivery_time, receipt.target_reached
    );

    // FACT 2 — the OBSERVABLE: is the port SERVED? Poll a SINGLE reader up to ~2s.
    let mut delivered: Vec<synapse::transport::ReceivedMessage> = Vec::new();
    for _ in 0..10 {
        tokio::time::sleep(Duration::from_millis(200)).await;
        let batch = receiver
            .receive_messages()
            .await
            .expect("receive_messages ok");
        if !batch.is_empty() {
            delivered = batch;
            break;
        }
    }

    eprintln!("DELIVERED COUNT: {}", delivered.len());
    assert_eq!(
        delivered.len(),
        1,
        "a message the sender receipted as Sent must ARRIVE at receive_messages — \
         Sent is not delivery. Port open ({}) but not served.",
        "raw connect succeeded above",
    );
    assert_eq!(
        delivered[0].incoming.message.encrypted_content, payload,
        "the delivered payload must match what was sent",
    );
    // The message was never signed, so the honest verdict is Unverifiable, not a silently
    // upgraded Verified. Accepting unverified senders (this probe's gate config) is what makes
    // the delivery observable at all; it does not make the verdict verified.
    assert!(
        matches!(delivered[0].sender, SenderVerdict::Unverifiable { .. }),
        "an unsigned message must not be reported as verified: {:?}",
        delivered[0].sender
    );
}
