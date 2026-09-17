// SPDX-License-Identifier: MIT OR Apache-2.0
//! Sealing: encrypting a message body to its recipient's X25519 key (P2 slice d).
//!
//! HPKE (RFC 9180) base mode with DHKEM(X25519, HKDF-SHA256), HKDF-SHA256 and ChaCha20-Poly1305.
//! Only `encrypted_content` is sealed; ids, timestamp, level and metadata stay readable. The
//! authenticated data binds the ciphertext to its header. A signed metadata marker
//! (`synapse.sealed = "v1"`) says a body is sealed; receivers never guess from the bytes.
//! Sealing keys are X25519 keys of their own, never derived from a node's Ed25519 key.
//! Design: `docs/superpowers/specs/2026-09-17-sealing-design.md`.

use crate::error::{Result, SynapseError};
use crate::sender_auth::{key_id, put, security_level_name};
use crate::types::{SecureMessage, SecurityLevel};
use hpke::aead::ChaCha20Poly1305;
use hpke::kdf::HkdfSha256;
use hpke::kem::X25519HkdfSha256;
use hpke::{Deserializable, Kem, OpModeR, OpModeS, Serializable};
use pem::{EncodeConfig, LineEnding, Pem, encode_config, parse};
use std::fmt;

/// HPKE `info` for every seal.
pub const SEAL_INFO: &[u8] = b"synapse/seal/v1";
/// Domain-separation tag: the first field of the authenticated data.
pub const AAD_DOMAIN_TAG: &[u8] = b"synapse/seal-aad/v1";
/// The first byte of a sealed body.
pub const SEALED_FORMAT_VERSION: u8 = 0x01;
/// The signed metadata marker that says a body is sealed.
pub const SEALED_KEY: &str = "synapse.sealed";
const SEALED_MARKER_VALUE: &str = "v1";

const ENC_LEN: usize = 32;
const TAG_LEN: usize = 16;
/// PKCS#8 v1 prefix for an X25519 private key (OID 1.3.101.110).
const PKCS8_PREFIX: [u8; 16] = [
    0x30, 0x2e, 0x02, 0x01, 0x00, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x6e, 0x04, 0x22, 0x04, 0x20,
];
/// SubjectPublicKeyInfo prefix for an X25519 public key.
const SPKI_PREFIX: [u8; 12] = [
    0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x6e, 0x03, 0x21, 0x00,
];

type SuiteKem = X25519HkdfSha256;

fn key_error(message: &str) -> SynapseError {
    SynapseError::EncryptionError(message.to_string())
}

fn pem_text(label: &str, der: Vec<u8>) -> String {
    encode_config(
        &Pem::new(label, der),
        EncodeConfig::new().set_line_ending(LineEnding::LF),
    )
}

/// A peer's X25519 public key.
#[derive(Clone, PartialEq, Eq)]
pub struct SealingPublicKey([u8; 32]);

impl SealingPublicKey {
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Lowercase hex SHA-256 of the key: safe to print.
    pub fn key_id(&self) -> String {
        key_id(&self.0)
    }

    /// The key as a SubjectPublicKeyInfo PEM (`PUBLIC KEY`).
    pub fn to_spki_pem(&self) -> String {
        let mut der = SPKI_PREFIX.to_vec();
        der.extend_from_slice(&self.0);
        pem_text("PUBLIC KEY", der)
    }

    pub fn from_spki_pem(pem: &str) -> Result<Self> {
        let block = parse(pem).map_err(|_| key_error("not a PEM public key"))?;
        let der = block.contents();
        if block.tag() != "PUBLIC KEY"
            || der.len() != SPKI_PREFIX.len() + 32
            || der[..SPKI_PREFIX.len()] != SPKI_PREFIX
        {
            return Err(key_error("not an X25519 SubjectPublicKeyInfo key"));
        }
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(&der[SPKI_PREFIX.len()..]);
        Ok(Self(bytes))
    }
}

impl fmt::Debug for SealingPublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SealingPublicKey({})", self.key_id())
    }
}

/// This node's X25519 key pair. Its `Debug` output never shows the private key.
#[derive(Clone)]
pub struct SealingKeyPair {
    private: <SuiteKem as Kem>::PrivateKey,
    public: SealingPublicKey,
}

impl SealingKeyPair {
    /// A fresh key pair from the system RNG.
    pub fn generate() -> Self {
        let (private, _) = SuiteKem::gen_keypair();
        Self::from_private(private)
    }

    fn from_private(private: <SuiteKem as Kem>::PrivateKey) -> Self {
        let public_bytes = SuiteKem::sk_to_pk(&private).to_bytes();
        let mut public = [0u8; 32];
        public.copy_from_slice(&public_bytes);
        Self {
            private,
            public: SealingPublicKey(public),
        }
    }

    pub fn public_key(&self) -> &SealingPublicKey {
        &self.public
    }

    /// The private key as a PKCS#8 v1 PEM (`PRIVATE KEY`).
    pub fn to_pkcs8_pem(&self) -> String {
        let mut der = PKCS8_PREFIX.to_vec();
        der.extend_from_slice(&self.private.to_bytes());
        pem_text("PRIVATE KEY", der)
    }

    /// Error messages never contain key bytes.
    pub fn from_pkcs8_pem(pem: &str) -> Result<Self> {
        let block = parse(pem).map_err(|_| key_error("not a PEM private key"))?;
        let der = block.contents();
        if block.tag() != "PRIVATE KEY"
            || der.len() != PKCS8_PREFIX.len() + 32
            || der[..PKCS8_PREFIX.len()] != PKCS8_PREFIX
        {
            return Err(key_error("not an X25519 PKCS#8 private key"));
        }
        let private = <SuiteKem as Kem>::PrivateKey::from_bytes(&der[PKCS8_PREFIX.len()..])
            .map_err(|_| key_error("not a usable X25519 private key"))?;
        Ok(Self::from_private(private))
    }
}

impl fmt::Debug for SealingKeyPair {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SealingKeyPair(public {})", self.public.key_id())
    }
}

/// Why a body could not be opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenError {
    /// This node has no sealing key.
    NoSealingKey,
    /// The level says sealed, but there is no sealed marker.
    NotSealed,
    /// The marker or format version is not one this build understands.
    UnsupportedVersion,
    /// The body is too short to be a sealed body.
    Truncated,
    /// Decryption failed: wrong key, tampered bytes or a changed header.
    Undecryptable,
    /// A sealed marker on a plaintext level.
    SealedButPlainLevel,
}

impl fmt::Display for OpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            OpenError::NoSealingKey => "no_sealing_key",
            OpenError::NotSealed => "not_sealed",
            OpenError::UnsupportedVersion => "unsupported_version",
            OpenError::Truncated => "truncated",
            OpenError::Undecryptable => "undecryptable",
            OpenError::SealedButPlainLevel => "sealed_but_plain_level",
        })
    }
}

/// A received message's body, as this node could read it. Never hidden: a body that could not be
/// opened is still delivered, marked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Payload {
    /// A plaintext level with no sealed marker.
    Plain(Vec<u8>),
    /// A sealed body opened with this node's key.
    Opened(Vec<u8>),
    /// Anything else.
    CouldNotOpen(OpenError),
}

impl Payload {
    pub fn bytes(&self) -> Option<&[u8]> {
        match self {
            Payload::Plain(bytes) | Payload::Opened(bytes) => Some(bytes),
            Payload::CouldNotOpen(_) => None,
        }
    }

    pub fn is_open(&self) -> bool {
        !matches!(self, Payload::CouldNotOpen(_))
    }
}

fn is_sealed_level(level: &SecurityLevel) -> bool {
    matches!(level, SecurityLevel::Private | SecurityLevel::Secure)
}

/// The authenticated data (spec §3): tag, message_id, from, to, level, each length-prefixed.
pub(crate) fn seal_aad(message: &SecureMessage) -> Vec<u8> {
    let mut out = Vec::new();
    put(&mut out, AAD_DOMAIN_TAG);
    put(&mut out, message.message_id.0.as_bytes());
    put(&mut out, message.from_global_id.as_bytes());
    put(&mut out, message.to_global_id.as_bytes());
    put(
        &mut out,
        security_level_name(&message.security_level).as_bytes(),
    );
    out
}

/// Seal `message`'s body to `recipient`. Refuses a plaintext level and a body that is already
/// sealed. Sign after sealing, so the signature covers the sealed bytes.
pub fn seal(message: &mut SecureMessage, recipient: &SealingPublicKey) -> Result<()> {
    if !is_sealed_level(&message.security_level) {
        return Err(key_error(
            "only Private and Secure messages are sealed; set the level first",
        ));
    }
    if message.metadata.contains_key(SEALED_KEY) {
        return Err(key_error("the message is already sealed"));
    }
    let recipient_key = <SuiteKem as Kem>::PublicKey::from_bytes(recipient.as_bytes())
        .map_err(|_| key_error("the recipient's sealing key is not a usable X25519 key"))?;
    let aad = seal_aad(message);
    let (encapped, ciphertext) = hpke::single_shot_seal::<ChaCha20Poly1305, HkdfSha256, SuiteKem>(
        &OpModeS::Base,
        &recipient_key,
        SEAL_INFO,
        &message.encrypted_content,
        &aad,
    )
    .map_err(|_| key_error("sealing failed"))?;

    let mut body = Vec::with_capacity(1 + ENC_LEN + ciphertext.len());
    body.push(SEALED_FORMAT_VERSION);
    body.extend_from_slice(&encapped.to_bytes());
    body.extend_from_slice(&ciphertext);
    message.encrypted_content = body;
    message
        .metadata
        .insert(SEALED_KEY.to_string(), SEALED_MARKER_VALUE.to_string());
    Ok(())
}

/// Open `message`'s body with this node's key (spec §6).
pub fn open(message: &SecureMessage, key: Option<&SealingKeyPair>) -> Payload {
    let marker = message.metadata.get(SEALED_KEY);
    match (is_sealed_level(&message.security_level), marker) {
        (false, None) => return Payload::Plain(message.encrypted_content.clone()),
        (false, Some(_)) => return Payload::CouldNotOpen(OpenError::SealedButPlainLevel),
        (true, None) => return Payload::CouldNotOpen(OpenError::NotSealed),
        (true, Some(value)) if value != SEALED_MARKER_VALUE => {
            return Payload::CouldNotOpen(OpenError::UnsupportedVersion);
        }
        (true, Some(_)) => {}
    }
    let Some(key) = key else {
        return Payload::CouldNotOpen(OpenError::NoSealingKey);
    };
    let body = &message.encrypted_content;
    if body.len() < 1 + ENC_LEN + TAG_LEN {
        return Payload::CouldNotOpen(OpenError::Truncated);
    }
    if body[0] != SEALED_FORMAT_VERSION {
        return Payload::CouldNotOpen(OpenError::UnsupportedVersion);
    }
    let Ok(encapped) = <SuiteKem as Kem>::EncappedKey::from_bytes(&body[1..1 + ENC_LEN]) else {
        return Payload::CouldNotOpen(OpenError::Undecryptable);
    };
    match hpke::single_shot_open::<ChaCha20Poly1305, HkdfSha256, SuiteKem>(
        &OpModeR::Base,
        &key.private,
        &encapped,
        SEAL_INFO,
        &body[1 + ENC_LEN..],
        &seal_aad(message),
    ) {
        Ok(plaintext) => Payload::Opened(plaintext),
        Err(_) => Payload::CouldNotOpen(OpenError::Undecryptable),
    }
}
