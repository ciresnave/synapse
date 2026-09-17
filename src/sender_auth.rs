// SPDX-License-Identifier: MIT OR Apache-2.0
//! Sender authentication for [`SecureMessage`](crate::types::SecureMessage).
//!
//! Every message carries a mandatory [`SenderProof`]. A receiver turns the proof plus keys it
//! pinned itself into exactly one verdict; the verdict is never read from the wire.
//! Design: `docs/superpowers/specs/2026-09-17-sender-authentication-design.md`.

use serde::{Deserialize, Serialize};

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
    /// Ed25519 over `canonical_input` v1.
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
