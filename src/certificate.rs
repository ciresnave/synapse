// SPDX-License-Identifier: MIT OR Apache-2.0
//! Account keys and agent certificates (P2 slice f1).
//!
//! No I/O and no clock: `now` is passed in. See
//! `docs/superpowers/specs/2026-09-17-agent-certificates-design.md`.

use crate::sender_auth::{put, to_hex};
use chrono::{DateTime, Utc};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use pem::{EncodeConfig, LineEnding, Pem, encode_config};
use std::fmt;

/// Domain-separation tag: the first field of an [`AgentCertificate`]'s signing input.
pub const CERT_DOMAIN_TAG: &[u8] = b"synapse/agent-cert/v1";
/// Domain-separation tag: the first field of a [`Revocation`]'s signing input.
pub const REVOCATION_DOMAIN_TAG: &[u8] = b"synapse/agent-revocation/v1";
/// PEM label for an encoded [`AgentCertificate`].
pub const CERT_PEM_LABEL: &str = "SYNAPSE AGENT CERT";
/// PEM label for an encoded [`Revocation`].
pub const REVOCATION_PEM_LABEL: &str = "SYNAPSE REVOCATION";
/// Signed metadata carrying the sender's chain, leaf first.
pub const CHAIN_KEY: &str = "synapse.cert.chain";
/// Signed metadata carrying relayed revocations.
pub const REVOCATIONS_KEY: &str = "synapse.cert.revocations";

/// A grant carried by an [`AgentCertificate`].
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Permission {
    Send,
    RequestAck,
    Ack,
    /// A consumer-defined grant. Always begins `x-`; Synapse carries it and never interprets it.
    Extension(String),
}

impl Permission {
    #[must_use]
    pub fn as_str(&self) -> String {
        match self {
            Permission::Send => "send".to_string(),
            Permission::RequestAck => "request-ack".to_string(),
            Permission::Ack => "ack".to_string(),
            Permission::Extension(text) => text.clone(),
        }
    }

    /// `None` for an unknown permission that is not `x-` prefixed: a typo must not look like a
    /// grant of nothing.
    #[must_use]
    pub fn parse(text: &str) -> Option<Permission> {
        match text {
            "send" => Some(Permission::Send),
            "request-ack" => Some(Permission::RequestAck),
            "ack" => Some(Permission::Ack),
            other if other.starts_with("x-") => Some(Permission::Extension(other.to_string())),
            _ => None,
        }
    }
}

/// Why a certificate, revocation or chain could not be trusted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChainError {
    Malformed,
    UnknownPermission,
    TooLarge,
    UnknownIssuer,
    BadSignature,
    NotYetValid,
    Expired,
    ValidityNotNested,
    PermissionWidened,
    DelegationNotAllowed,
    IdentityNotNarrowed,
    SubjectMismatch,
    Revoked,
}

impl fmt::Display for ChainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            ChainError::Malformed => "malformed",
            ChainError::UnknownPermission => "unknown_permission",
            ChainError::TooLarge => "too_large",
            ChainError::UnknownIssuer => "unknown_issuer",
            ChainError::BadSignature => "bad_signature",
            ChainError::NotYetValid => "not_yet_valid",
            ChainError::Expired => "expired",
            ChainError::ValidityNotNested => "validity_not_nested",
            ChainError::PermissionWidened => "permission_widened",
            ChainError::DelegationNotAllowed => "delegation_not_allowed",
            ChainError::IdentityNotNarrowed => "identity_not_narrowed",
            ChainError::SubjectMismatch => "subject_mismatch",
            ChainError::Revoked => "revoked",
        })
    }
}

impl std::error::Error for ChainError {}

/// An agent's certificate: an issuer's account (or delegate) key vouching for one agent's signing
/// and sealing keys, for a bounded validity window and a bounded set of permissions (spec §4).
#[derive(Clone)]
pub struct AgentCertificate {
    pub version: u8,
    pub serial: [u8; 16],
    /// Lowercase hex SHA-256 of the issuer's public key (see [`crate::sender_auth::key_id`]).
    pub issuer_key_id: String,
    pub subject_label: String,
    pub subject_global_id: String,
    pub subject_signing_key: [u8; 32],
    pub subject_sealing_key: [u8; 32],
    pub not_before: DateTime<Utc>,
    pub not_after: DateTime<Utc>,
    pub permissions: Vec<Permission>,
    /// How many further links this certificate may delegate through (0 = a leaf).
    pub may_delegate: u8,
    pub signature: [u8; 64],
}

impl fmt::Debug for AgentCertificate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AgentCertificate")
            .field("serial", &to_hex(&self.serial))
            .field("subject_label", &self.subject_label)
            .field("subject_global_id", &self.subject_global_id)
            .field("not_before", &self.not_before)
            .field("not_after", &self.not_after)
            .finish()
    }
}

fn read_field<'a>(data: &'a [u8], pos: &mut usize) -> Result<&'a [u8], ChainError> {
    if data.len() < *pos + 4 {
        return Err(ChainError::Malformed);
    }
    let len_bytes: [u8; 4] = data[*pos..*pos + 4]
        .try_into()
        .map_err(|_| ChainError::Malformed)?;
    let len = u32::from_be_bytes(len_bytes) as usize;
    *pos += 4;
    if data.len() < *pos + len {
        return Err(ChainError::Malformed);
    }
    let field = &data[*pos..*pos + len];
    *pos += len;
    Ok(field)
}

fn field_str(field: &[u8]) -> Result<String, ChainError> {
    std::str::from_utf8(field)
        .map(str::to_string)
        .map_err(|_| ChainError::Malformed)
}

fn field_array<const N: usize>(field: &[u8]) -> Result<[u8; N], ChainError> {
    field.try_into().map_err(|_| ChainError::Malformed)
}

fn field_u8(field: &[u8]) -> Result<u8, ChainError> {
    if field.len() != 1 {
        return Err(ChainError::Malformed);
    }
    Ok(field[0])
}

fn field_timestamp(field: &[u8]) -> Result<DateTime<Utc>, ChainError> {
    let bytes: [u8; 8] = field.try_into().map_err(|_| ChainError::Malformed)?;
    let micros = i64::from_be_bytes(bytes);
    DateTime::<Utc>::from_timestamp_micros(micros).ok_or(ChainError::Malformed)
}

/// Encode `permissions` as a 4-byte big-endian count followed by each `as_str()`, sorted, each
/// length-prefixed. Sorting makes the encoding independent of the caller's insertion order.
fn put_permissions(out: &mut Vec<u8>, permissions: &[Permission]) {
    let mut names: Vec<String> = permissions.iter().map(Permission::as_str).collect();
    names.sort();
    let mut encoded = Vec::new();
    let count = u32::try_from(names.len()).expect("more than 4 GiB permissions");
    encoded.extend_from_slice(&count.to_be_bytes());
    for name in &names {
        put(&mut encoded, name.as_bytes());
    }
    put(out, &encoded);
}

fn read_permissions(field: &[u8]) -> Result<Vec<Permission>, ChainError> {
    if field.len() < 4 {
        return Err(ChainError::Malformed);
    }
    let count_bytes: [u8; 4] = field[..4].try_into().map_err(|_| ChainError::Malformed)?;
    let count = u32::from_be_bytes(count_bytes) as usize;
    let mut pos = 4;
    let mut permissions = Vec::with_capacity(count);
    for _ in 0..count {
        let name_bytes = read_field(field, &mut pos)?;
        let name = field_str(name_bytes)?;
        let permission = Permission::parse(&name).ok_or(ChainError::UnknownPermission)?;
        permissions.push(permission);
    }
    if pos != field.len() {
        return Err(ChainError::Malformed);
    }
    Ok(permissions)
}

fn pem_text(label: &str, der: Vec<u8>) -> String {
    encode_config(
        &Pem::new(label, der),
        EncodeConfig::new().set_line_ending(LineEnding::LF),
    )
}

impl AgentCertificate {
    /// The bytes the issuer signs: the domain tag, then each field in declaration order, each a
    /// 4-byte big-endian length followed by its bytes (see [`crate::sender_auth::put`]).
    /// `permissions` is its own length-prefixed blob: a count, then each sorted name,
    /// length-prefixed in turn. `signature` is never covered by its own input.
    pub fn signing_input(&self) -> Vec<u8> {
        let mut out = Vec::new();
        put(&mut out, CERT_DOMAIN_TAG);
        put(&mut out, &[self.version]);
        put(&mut out, &self.serial);
        put(&mut out, self.issuer_key_id.as_bytes());
        put(&mut out, self.subject_label.as_bytes());
        put(&mut out, self.subject_global_id.as_bytes());
        put(&mut out, &self.subject_signing_key);
        put(&mut out, &self.subject_sealing_key);
        put(&mut out, &self.not_before.timestamp_micros().to_be_bytes());
        put(&mut out, &self.not_after.timestamp_micros().to_be_bytes());
        put_permissions(&mut out, &self.permissions);
        put(&mut out, &[self.may_delegate]);
        out
    }

    /// Sign `unsigned` (whose `signature` field is ignored) with `issuer` and return the signed
    /// certificate.
    #[must_use]
    pub fn sign(mut unsigned: AgentCertificate, issuer: &SigningKey) -> AgentCertificate {
        let input = unsigned.signing_input();
        let signature: Signature = issuer.sign(&input);
        unsigned.signature = signature.to_bytes();
        unsigned
    }

    /// Whether this certificate's signature verifies against `issuer_public`.
    #[must_use]
    pub fn verify_signature(&self, issuer_public: &[u8; 32]) -> bool {
        let Ok(verifying_key) = VerifyingKey::from_bytes(issuer_public) else {
            return false;
        };
        let signature = Signature::from_bytes(&self.signature);
        verifying_key
            .verify(&self.signing_input(), &signature)
            .is_ok()
    }

    /// The PEM body is `signing_input()` followed by the 64 signature bytes: the same canonical
    /// encoding that is signed, so parsing is the inverse of one function.
    pub fn to_pem(&self) -> String {
        let mut body = self.signing_input();
        body.extend_from_slice(&self.signature);
        pem_text(CERT_PEM_LABEL, body)
    }

    /// The inverse of [`AgentCertificate::to_pem`]. Refuses an unknown permission with
    /// [`ChainError::UnknownPermission`] and anything else malformed with
    /// [`ChainError::Malformed`].
    pub fn from_pem(text: &str) -> Result<Self, ChainError> {
        let block = pem::parse(text).map_err(|_| ChainError::Malformed)?;
        if block.tag() != CERT_PEM_LABEL {
            return Err(ChainError::Malformed);
        }
        Self::from_body(block.contents())
    }

    fn from_body(data: &[u8]) -> Result<Self, ChainError> {
        let mut pos = 0usize;
        let domain_tag = read_field(data, &mut pos)?;
        if domain_tag != CERT_DOMAIN_TAG {
            return Err(ChainError::Malformed);
        }
        let version = field_u8(read_field(data, &mut pos)?)?;
        let serial = field_array::<16>(read_field(data, &mut pos)?)?;
        let issuer_key_id = field_str(read_field(data, &mut pos)?)?;
        let subject_label = field_str(read_field(data, &mut pos)?)?;
        let subject_global_id = field_str(read_field(data, &mut pos)?)?;
        let subject_signing_key = field_array::<32>(read_field(data, &mut pos)?)?;
        let subject_sealing_key = field_array::<32>(read_field(data, &mut pos)?)?;
        let not_before = field_timestamp(read_field(data, &mut pos)?)?;
        let not_after = field_timestamp(read_field(data, &mut pos)?)?;
        let permissions = read_permissions(read_field(data, &mut pos)?)?;
        let may_delegate = field_u8(read_field(data, &mut pos)?)?;
        if data.len() != pos + 64 {
            return Err(ChainError::Malformed);
        }
        let signature = field_array::<64>(&data[pos..])?;
        Ok(AgentCertificate {
            version,
            serial,
            issuer_key_id,
            subject_label,
            subject_global_id,
            subject_signing_key,
            subject_sealing_key,
            not_before,
            not_after,
            permissions,
            may_delegate,
            signature,
        })
    }
}

/// A signed statement that a certificate's serial is no longer trusted (spec §6).
#[derive(Debug, Clone)]
pub struct Revocation {
    pub version: u8,
    pub serial: [u8; 16],
    /// Lowercase hex SHA-256 of the issuer's public key.
    pub issuer_key_id: String,
    pub issued_at: DateTime<Utc>,
    pub reason: String,
    pub signature: [u8; 64],
}

impl Revocation {
    /// The bytes the issuer signs: the domain tag, then each field in declaration order (same
    /// length-prefixing discipline as [`AgentCertificate::signing_input`]).
    pub fn signing_input(&self) -> Vec<u8> {
        let mut out = Vec::new();
        put(&mut out, REVOCATION_DOMAIN_TAG);
        put(&mut out, &[self.version]);
        put(&mut out, &self.serial);
        put(&mut out, self.issuer_key_id.as_bytes());
        put(&mut out, &self.issued_at.timestamp_micros().to_be_bytes());
        put(&mut out, self.reason.as_bytes());
        out
    }

    #[must_use]
    pub fn sign(mut unsigned: Revocation, issuer: &SigningKey) -> Revocation {
        let input = unsigned.signing_input();
        let signature: Signature = issuer.sign(&input);
        unsigned.signature = signature.to_bytes();
        unsigned
    }

    #[must_use]
    pub fn verify_signature(&self, issuer_public: &[u8; 32]) -> bool {
        let Ok(verifying_key) = VerifyingKey::from_bytes(issuer_public) else {
            return false;
        };
        let signature = Signature::from_bytes(&self.signature);
        verifying_key
            .verify(&self.signing_input(), &signature)
            .is_ok()
    }

    pub fn to_pem(&self) -> String {
        let mut body = self.signing_input();
        body.extend_from_slice(&self.signature);
        pem_text(REVOCATION_PEM_LABEL, body)
    }

    pub fn from_pem(text: &str) -> Result<Self, ChainError> {
        let block = pem::parse(text).map_err(|_| ChainError::Malformed)?;
        if block.tag() != REVOCATION_PEM_LABEL {
            return Err(ChainError::Malformed);
        }
        Self::from_body(block.contents())
    }

    fn from_body(data: &[u8]) -> Result<Self, ChainError> {
        let mut pos = 0usize;
        let domain_tag = read_field(data, &mut pos)?;
        if domain_tag != REVOCATION_DOMAIN_TAG {
            return Err(ChainError::Malformed);
        }
        let version = field_u8(read_field(data, &mut pos)?)?;
        let serial = field_array::<16>(read_field(data, &mut pos)?)?;
        let issuer_key_id = field_str(read_field(data, &mut pos)?)?;
        let issued_at = field_timestamp(read_field(data, &mut pos)?)?;
        let reason = field_str(read_field(data, &mut pos)?)?;
        if data.len() != pos + 64 {
            return Err(ChainError::Malformed);
        }
        let signature = field_array::<64>(&data[pos..])?;
        Ok(Revocation {
            version,
            serial,
            issuer_key_id,
            issued_at,
            reason,
            signature,
        })
    }
}

/// Encode a chain leaf first: each certificate's own PEM block, concatenated.
pub fn chain_to_pem(chain: &[AgentCertificate]) -> String {
    chain.iter().map(AgentCertificate::to_pem).collect()
}

/// The inverse of [`chain_to_pem`]. Returns [`ChainError::Malformed`] on an empty or unparsable
/// input.
pub fn chain_from_pem(text: &str) -> Result<Vec<AgentCertificate>, ChainError> {
    let blocks = pem::parse_many(text).map_err(|_| ChainError::Malformed)?;
    if blocks.is_empty() {
        return Err(ChainError::Malformed);
    }
    blocks
        .iter()
        .map(|block| {
            if block.tag() != CERT_PEM_LABEL {
                return Err(ChainError::Malformed);
            }
            AgentCertificate::from_body(block.contents())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;

    fn t(secs: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_700_000_000 + secs, 0).expect("valid timestamp")
    }

    fn key(seed: u8) -> SigningKey {
        SigningKey::from_bytes(&[seed; 32])
    }

    fn public(k: &SigningKey) -> [u8; 32] {
        k.verifying_key().to_bytes()
    }

    /// An unsigned certificate with everything filled in; tests tweak one field at a time.
    fn unsigned(issuer: &SigningKey, subject: &SigningKey, id: &str) -> AgentCertificate {
        AgentCertificate {
            version: 1,
            serial: [7u8; 16],
            issuer_key_id: crate::sender_auth::key_id(&public(issuer)),
            subject_label: "test agent".to_string(),
            subject_global_id: id.to_string(),
            subject_signing_key: public(subject),
            subject_sealing_key: [9u8; 32],
            not_before: t(0),
            not_after: t(86_400),
            permissions: vec![Permission::Send, Permission::Ack],
            may_delegate: 2,
            signature: [0u8; 64],
        }
    }

    #[test]
    fn a_signed_certificate_verifies_against_its_issuer() {
        let (issuer, subject) = (key(1), key(2));
        let cert = AgentCertificate::sign(unsigned(&issuer, &subject, "agent@host"), &issuer);
        assert!(cert.verify_signature(&public(&issuer)));
        // Control: another key does not verify it.
        assert!(!cert.verify_signature(&public(&key(3))));
    }

    #[test]
    fn one_flipped_signature_byte_fails() {
        let (issuer, subject) = (key(1), key(2));
        let mut cert = AgentCertificate::sign(unsigned(&issuer, &subject, "agent@host"), &issuer);
        cert.signature[0] ^= 1;
        assert!(!cert.verify_signature(&public(&issuer)));
    }

    #[test]
    fn every_signed_field_is_covered_by_the_signature() {
        let (issuer, subject) = (key(1), key(2));
        let cert = AgentCertificate::sign(unsigned(&issuer, &subject, "agent@host"), &issuer);
        // Changing any one field must break verification: the signing input covers them all.
        let mutations: Vec<Box<dyn Fn(&mut AgentCertificate)>> = vec![
            Box::new(|c| c.version = 2),
            Box::new(|c| c.serial[0] ^= 1),
            Box::new(|c| c.issuer_key_id.push('a')),
            Box::new(|c| c.subject_label.push('a')),
            Box::new(|c| c.subject_global_id.push('a')),
            Box::new(|c| c.subject_signing_key[0] ^= 1),
            Box::new(|c| c.subject_sealing_key[0] ^= 1),
            Box::new(|c| c.not_before = t(1)),
            Box::new(|c| c.not_after = t(2)),
            Box::new(|c| c.permissions.push(Permission::RequestAck)),
            Box::new(|c| c.may_delegate = 1),
        ];
        for (index, mutate) in mutations.iter().enumerate() {
            let mut altered = cert.clone();
            mutate(&mut altered);
            assert!(
                !altered.verify_signature(&public(&issuer)),
                "field {index} is not covered by the signature"
            );
        }
    }

    #[test]
    fn permissions_parse_and_reject_typos() {
        assert_eq!(Permission::parse("send"), Some(Permission::Send));
        assert_eq!(
            Permission::parse("request-ack"),
            Some(Permission::RequestAck)
        );
        assert_eq!(Permission::parse("ack"), Some(Permission::Ack));
        assert_eq!(
            Permission::parse("x-dispatch"),
            Some(Permission::Extension("x-dispatch".to_string()))
        );
        // A typo of a known permission is refused, rather than silently granting nothing.
        assert_eq!(Permission::parse("sned"), None);
        assert_eq!(Permission::parse(""), None);
    }

    #[test]
    fn a_certificate_round_trips_through_pem() {
        let (issuer, subject) = (key(1), key(2));
        let cert = AgentCertificate::sign(unsigned(&issuer, &subject, "agent@host"), &issuer);
        let parsed = AgentCertificate::from_pem(&cert.to_pem()).expect("parses");
        assert_eq!(parsed.signing_input(), cert.signing_input());
        assert_eq!(parsed.signature, cert.signature);
        assert!(parsed.verify_signature(&public(&issuer)));
    }

    #[test]
    fn a_chain_round_trips_leaf_first() {
        let (root, middle, leaf) = (key(1), key(2), key(3));
        let upper = AgentCertificate::sign(unsigned(&root, &middle, "agent@host"), &root);
        let lower = AgentCertificate::sign(unsigned(&middle, &leaf, "worker.agent@host"), &middle);
        let text = chain_to_pem(&[lower.clone(), upper.clone()]);
        let parsed = chain_from_pem(&text).expect("parses");
        assert_eq!(parsed.len(), 2);
        assert_eq!(
            parsed[0].subject_global_id, "worker.agent@host",
            "leaf first"
        );
        assert_eq!(parsed[1].subject_global_id, "agent@host");
    }

    #[test]
    fn a_certificate_with_an_unknown_permission_will_not_parse() {
        let (issuer, subject) = (key(1), key(2));
        let mut cert = unsigned(&issuer, &subject, "agent@host");
        cert.permissions = vec![Permission::Extension("not-prefixed".to_string())];
        let signed = AgentCertificate::sign(cert, &issuer);
        // Encoding is allowed, but parsing refuses it: the wire form is what a receiver trusts.
        assert!(matches!(
            AgentCertificate::from_pem(&signed.to_pem()),
            Err(ChainError::UnknownPermission)
        ));
    }

    #[test]
    fn a_revocation_verifies_and_round_trips() {
        let issuer = key(1);
        let revocation = Revocation::sign(
            Revocation {
                version: 1,
                serial: [7u8; 16],
                issuer_key_id: crate::sender_auth::key_id(&public(&issuer)),
                issued_at: t(10),
                reason: "key leaked".to_string(),
                signature: [0u8; 64],
            },
            &issuer,
        );
        assert!(revocation.verify_signature(&public(&issuer)));
        let parsed = Revocation::from_pem(&revocation.to_pem()).expect("parses");
        assert_eq!(parsed.serial, revocation.serial);
        assert!(parsed.verify_signature(&public(&issuer)));
        // Control: a different key does not verify it.
        assert!(!parsed.verify_signature(&public(&key(4))));
    }
}
