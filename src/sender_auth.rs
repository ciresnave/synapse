// SPDX-License-Identifier: MIT OR Apache-2.0
//! Sender authentication for [`SecureMessage`](crate::types::SecureMessage).
//!
//! Every message carries a mandatory [`SenderProof`]. A receiver turns the proof plus keys it
//! pinned itself into exactly one verdict; the verdict is never read from the wire.
//! Design: `docs/superpowers/specs/2026-09-17-sender-authentication-design.md`.

use crate::types::{SecureMessage, SecurityLevel};
use chrono::Timelike;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The signature algorithm a sender used. Any other value fails to parse.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Default,
    Serialize,
    Deserialize,
    bincode::Encode,
    bincode::Decode,
)]
#[serde(rename_all = "lowercase")]
pub enum ProofAlg {
    /// The sender had no key. Receivers mark the message `Unverifiable(Unsigned)`.
    #[default]
    None,
    /// Ed25519 over [`canonical_input`] v1.
    Ed25519,
}

/// The sender's claim of authorship. Mandatory on the wire: a message without it does not parse.
#[derive(
    Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, bincode::Encode, bincode::Decode,
)]
pub struct SenderProof {
    pub alg: ProofAlg,
    /// Lowercase hex SHA-256 of the signer's 32-byte public key; empty when `alg` is `None`.
    pub key_id: String,
    /// 64 bytes for Ed25519; empty when `alg` is `None`.
    pub sig: Vec<u8>,
}

impl SenderProof {
    /// An explicit "no signature" proof.
    pub fn unsigned() -> Self {
        Self::default()
    }
}

/// Domain-separation tag: the first field of canonical input v1.
pub const CANONICAL_DOMAIN_TAG: &[u8] = b"synapse/sender-proof/v1";

/// Lowercase hex SHA-256 of an Ed25519 public key.
pub fn key_id(public_key: &[u8; 32]) -> String {
    Sha256::digest(public_key)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The serde name of a security level. The match is exhaustive, so a new variant fails to compile
/// here instead of silently signing a wrong name.
fn security_level_name(level: &SecurityLevel) -> &'static str {
    match level {
        SecurityLevel::Public => "public",
        SecurityLevel::Private => "private",
        SecurityLevel::Authenticated => "authenticated",
        SecurityLevel::Secure => "secure",
    }
}

fn put(out: &mut Vec<u8>, bytes: &[u8]) {
    let len = u32::try_from(bytes.len()).expect("a signed field longer than 4 GiB");
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(bytes);
}

/// The bytes a sender signs (spec §4, v1). Each field is a 4-byte big-endian length followed by
/// its bytes, in this order: domain tag, `message_id` (16 raw bytes), `from_global_id`,
/// `to_global_id`, timestamp (Unix microseconds, i64 BE), `security_level` (serde name),
/// `sender_proof.key_id`, SHA-256(`encrypted_content`), metadata. The metadata field's bytes are
/// a 4-byte count followed by each (key, value) pair, each length-prefixed, sorted by key bytes.
///
/// `routing_path` is NOT covered: relays append to it.
pub fn canonical_input(message: &SecureMessage) -> Vec<u8> {
    let mut out = Vec::new();
    put(&mut out, CANONICAL_DOMAIN_TAG);
    put(&mut out, message.message_id.0.as_bytes());
    put(&mut out, message.from_global_id.as_bytes());
    put(&mut out, message.to_global_id.as_bytes());
    put(
        &mut out,
        &message.timestamp.0.timestamp_micros().to_be_bytes(),
    );
    put(
        &mut out,
        security_level_name(&message.security_level).as_bytes(),
    );
    put(&mut out, message.sender_proof.key_id.as_bytes());
    put(&mut out, &Sha256::digest(&message.encrypted_content));

    let mut pairs: Vec<(&String, &String)> = message.metadata.iter().collect();
    pairs.sort();
    let count = u32::try_from(pairs.len()).expect("more than 4 GiB metadata pairs");
    let mut metadata = count.to_be_bytes().to_vec();
    for (key, value) in pairs {
        put(&mut metadata, key.as_bytes());
        put(&mut metadata, value.as_bytes());
    }
    put(&mut out, &metadata);
    out
}

/// Drop sub-microsecond digits so the wire timestamp and the signed timestamp agree exactly.
/// Python's `datetime` holds only microseconds (spec §4).
pub fn truncate_timestamp_to_micros(message: &mut SecureMessage) {
    let ts = message.timestamp.0;
    let nanos = ts.nanosecond();
    message.timestamp.0 = ts
        .with_nanosecond(nanos - nanos % 1_000)
        .expect("a smaller nanosecond value is always valid");
}

pub(crate) fn has_sub_micro_digits(message: &SecureMessage) -> bool {
    !message.timestamp.0.nanosecond().is_multiple_of(1_000)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::SecurityLevel;

    #[test]
    fn security_level_names_match_their_serde_names() {
        for level in [
            SecurityLevel::Public,
            SecurityLevel::Private,
            SecurityLevel::Authenticated,
            SecurityLevel::Secure,
        ] {
            let serde_name = serde_json::to_string(&level).unwrap();
            assert_eq!(serde_name, format!("\"{}\"", security_level_name(&level)));
        }
    }

    #[test]
    fn key_id_is_64_lowercase_hex() {
        let id = key_id(&[7u8; 32]);
        assert_eq!(id.len(), 64);
        assert!(
            id.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        );
    }

    #[test]
    fn canonical_input_starts_with_the_length_prefixed_domain_tag() {
        let m = SecureMessage::new("b", "a", vec![], SecurityLevel::Public);
        let input = canonical_input(&m);
        assert_eq!(
            &input[..4],
            &(CANONICAL_DOMAIN_TAG.len() as u32).to_be_bytes()
        );
        assert_eq!(
            &input[4..4 + CANONICAL_DOMAIN_TAG.len()],
            CANONICAL_DOMAIN_TAG
        );
    }

    #[test]
    fn canonical_input_ignores_metadata_insertion_order_and_routing_path() {
        let mut a = SecureMessage::new("b", "a", b"x".to_vec(), SecurityLevel::Public);
        let mut b = a.clone();
        a.add_metadata("k1", "v1");
        a.add_metadata("k2", "v2");
        b.add_metadata("k2", "v2");
        b.add_metadata("k1", "v1");
        b.add_routing_hop("relay-1");
        assert_eq!(canonical_input(&a), canonical_input(&b));
    }

    #[test]
    fn truncation_leaves_whole_microseconds() {
        let mut m = SecureMessage::new("b", "a", vec![], SecurityLevel::Public);
        m.timestamp = crate::synapse::blockchain::serialization::DateTimeWrapper::new(
            chrono::DateTime::parse_from_rfc3339("2026-09-17T04:00:00.123456789Z")
                .unwrap()
                .with_timezone(&chrono::Utc),
        );
        assert!(has_sub_micro_digits(&m));
        truncate_timestamp_to_micros(&mut m);
        assert!(!has_sub_micro_digits(&m));
        assert_eq!(m.timestamp.0.timestamp_subsec_nanos(), 123_456_000);
    }
}
