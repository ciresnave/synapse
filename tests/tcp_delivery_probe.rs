//! INDEPENDENT confirm-or-refute probe for the TCP send/receive silent-drop.
//!
//! Question (from the PM's brief, symptom only — no borrowed diagnosis): does a
//! message that `send_message` receipts `Ok` with `DeliveryConfirmation::Sent`
//! actually ARRIVE at the receiver's `receive_messages()`?
//!
//! This drives the REAL `Transport` trait (`TcpTransportImpl` → `start` /
//! `send_message` / `receive_messages`) over a REAL `127.0.0.1` loopback socket —
//! the same entry points a deployment uses, not a shortcut. It asserts actual
//! DELIVERY (the payload comes back out of `receive_messages`), never merely that
//! the send returned `Ok`. A green that only proved "send returned Ok" would prove
//! nothing about the bug.
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

use synapse::transport::TcpTransportImpl;
use synapse::transport::abstraction::{
    DeliveryConfirmation, Transport, TransportStatus, TransportTarget,
};
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_receipted_tcp_message_actually_arrives() {
    let port = free_port();

    // Receiver: binds `listen_port` in its constructor, then start() spins the server.
    let receiver = TcpTransportImpl::new(&cfg(port))
        .await
        .expect("construct receiver");
    receiver.start().await.expect("receiver.start() should succeed");
    assert!(
        matches!(receiver.status().await, TransportStatus::Running),
        "receiver should report Running after start()"
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

    // Sender: client-only (no listener of its own).
    let sender = TcpTransportImpl::new(&cfg(0)).await.expect("construct sender");

    let payload = b"fuel1-probe-PAYLOAD-42".to_vec();
    let msg = SecureMessage::new(
        "receiver-id",
        "sender-id",
        payload.clone(),
        Vec::new(),
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
    let mut delivered: Vec<synapse::transport::abstraction::IncomingMessage> = Vec::new();
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
        delivered[0].message.encrypted_content, payload,
        "the delivered payload must match what was sent",
    );
}
