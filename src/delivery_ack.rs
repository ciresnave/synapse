// SPDX-License-Identifier: MIT OR Apache-2.0
//! Receiver acknowledgement (P2 slice b).
//!
//! A receiving application acknowledges a message it has processed. The ack is a signed
//! `SecureMessage` sent to the sender's signed `synapse.reply_to` address; it names the original and
//! binds to its canonical input by digest. Design:
//! `docs/superpowers/specs/2026-09-17-receiver-acknowledgement-design.md`.

use crate::crypto::CryptoManager;
use crate::error::Result;
use crate::sender_auth::{canonical_input, to_hex};
use crate::types::{SecureMessage, SecurityLevel};
use sha2::{Digest, Sha256};

/// On a message that wants an ack: where to send it. A signed field.
pub const REPLY_TO_KEY: &str = "synapse.reply_to";
/// On an ack: the `message_id` it acknowledges.
pub const ACK_FOR_KEY: &str = "synapse.ack.for";
/// On an ack: [`message_digest`] of the acknowledged message.
pub const ACK_DIGEST_KEY: &str = "synapse.ack.digest";

/// Lowercase hex SHA-256 of a message's canonical input: exactly what an ack attests to.
pub fn message_digest(message: &SecureMessage) -> String {
    to_hex(&Sha256::digest(canonical_input(message)))
}

pub fn reply_to(message: &SecureMessage) -> Option<&str> {
    message.metadata.get(REPLY_TO_KEY).map(String::as_str)
}

pub fn is_ack(message: &SecureMessage) -> bool {
    message.metadata.contains_key(ACK_FOR_KEY)
}

/// The fields of an ack.
pub struct AckFields<'a> {
    pub for_message_id: &'a str,
    pub digest: &'a str,
}

/// `None` unless `message` carries both ack keys.
pub fn ack_fields(message: &SecureMessage) -> Option<AckFields<'_>> {
    Some(AckFields {
        for_message_id: message.metadata.get(ACK_FOR_KEY)?,
        digest: message.metadata.get(ACK_DIGEST_KEY)?,
    })
}

/// Build and sign the ack for `original`, from its addressee back to its sender.
pub fn build_ack(original: &SecureMessage, signer: &CryptoManager) -> Result<SecureMessage> {
    let mut ack = SecureMessage::new(
        original.from_global_id.clone(),
        original.to_global_id.clone(),
        Vec::new(),
        SecurityLevel::Authenticated,
    );
    ack.add_metadata(ACK_FOR_KEY, original.message_id.0.to_string());
    ack.add_metadata(ACK_DIGEST_KEY, message_digest(original));
    signer.sign_secure_message(&mut ack)?;
    Ok(ack)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sender_auth::TrustStore;

    fn identity() -> CryptoManager {
        let mut c = CryptoManager::new();
        c.generate_keypair().unwrap();
        c
    }

    #[test]
    fn an_ack_swaps_ids_names_the_original_and_is_signed_by_the_acker() {
        let bob = identity();
        let mut original = SecureMessage::new(
            "bob@t",
            "alice@t",
            b"hi".to_vec(),
            SecurityLevel::Authenticated,
        );
        original.request_ack("127.0.0.1:1");
        let ack = build_ack(&original, &bob).unwrap();

        assert_eq!(ack.to_global_id, "alice@t");
        assert_eq!(ack.from_global_id, "bob@t");
        assert!(ack.encrypted_content.is_empty());
        assert!(is_ack(&ack));
        assert_eq!(reply_to(&ack), None, "acks never ask for an ack");
        let fields = ack_fields(&ack).unwrap();
        assert_eq!(fields.for_message_id, original.message_id.0.to_string());
        assert_eq!(fields.digest, message_digest(&original));
        assert_eq!(fields.digest.len(), 64);

        let mut store = TrustStore::new();
        store.pin("bob@t", bob.public_key_bytes().unwrap());
        assert!(store.verify(&ack).is_verified());
    }
}
