// SPDX-License-Identifier: MIT OR Apache-2.0
//! Sender authentication — spec: docs/superpowers/specs/2026-09-17-sender-authentication-design.md
//! Test numbers refer to the spec's §10.

use synapse::sender_auth::{ProofAlg, SenderProof};
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
