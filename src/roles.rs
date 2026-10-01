// SPDX-License-Identifier: MIT OR Apache-2.0
//! Roles (M3): a role is a stable identity that a restarted session takes over.
//!
//! A session claims `role@account` by signing a short, domain-separated text with the role's key.
//! The role certificate (M2's keystore, f1's [`AgentCertificate`]) binds that key to the role, so
//! holding the key is the authorization. The newest valid claim always wins and gets a strictly
//! increasing **epoch**; any call made under an older epoch gets [`Superseded`].
//!
//! No I/O and no clock: `now` is passed in. M4 persists [`RolesState`]; M5's daemon calls this.
//! Design: `docs/superpowers/specs/2026-10-01-m3-roles-design.md`.

use std::collections::BTreeMap;
use std::fmt;

use chrono::{DateTime, SecondsFormat, Utc};
use ring::signature::{ED25519, UnparsedPublicKey};
use serde::{Deserialize, Serialize};

use crate::certificate::{AgentCertificate, ChainError, RevocationLookup, validate_chain};
use crate::keystore::RoleIdentity;
use crate::replay::{Decision, Freshness, ReplayConfig, ReplayGuard};

/// The first line of every signed claim, so a claim signature can never be mistaken for (or
/// replayed as) a signature over anything else.
pub const CLAIM_DOMAIN_TAG: &str = "synapse/role-claim/v1";

/// What a session sends to take a role.
#[derive(Debug, Clone)]
pub struct ClaimRequest {
    /// The role's certificate chain, leaf first. The claimed role is the leaf's subject.
    pub chain: Vec<AgentCertificate>,
    /// Fresh for every claim; a nonce is accepted once.
    pub nonce: [u8; 16],
    pub signed_at: DateTime<Utc>,
    /// Ed25519 by the leaf's signing key over [`claim_signing_input`]. A `Vec`, not `[u8; 64]`,
    /// because a request off the wire may carry any length and a wrong one must be refused.
    pub signature: Vec<u8>,
}

/// The exact text a claim signs.
#[must_use]
pub fn claim_signing_input(global_id: &str, nonce: &[u8; 16], signed_at: DateTime<Utc>) -> String {
    format!(
        "{CLAIM_DOMAIN_TAG}\n{global_id}\n{}\n{}",
        hex(nonce),
        signed_at.to_rfc3339_opts(SecondsFormat::Secs, true)
    )
}

/// The client half: sign a claim for `identity`'s role.
pub fn sign_claim(
    identity: &RoleIdentity,
    nonce: [u8; 16],
    now: DateTime<Utc>,
) -> Result<ClaimRequest, ClaimError> {
    let signature = identity
        .crypto
        .sign_message(&claim_signing_input(&identity.global_id, &nonce, now))
        .map_err(|_| ClaimError::NoSigningKey)?;
    Ok(ClaimRequest {
        chain: identity.chain.clone(),
        nonce,
        signed_at: now,
        signature,
    })
}

/// A granted claim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grant {
    pub global_id: String,
    pub epoch: u64,
    /// The epoch this claim displaced, if the role was held.
    pub superseded: Option<u64>,
}

/// Why a claim was refused. A refused claim changes nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaimError {
    /// The certificate chain does not validate against the pinned account keys.
    Chain(ChainError),
    /// The signature is not the role key's over this role's claim text.
    BadSignature,
    /// `signed_at` is outside the freshness window (too old, or ahead of `now`), or at or before
    /// this table started: after a restart the replay record is empty, so such a claim could be a
    /// captured one. The client signs a fresh claim.
    Stale,
    /// This nonce was already used by this key.
    Replayed,
    /// The identity has no signing key loaded (client side only).
    NoSigningKey,
}

impl fmt::Display for ClaimError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ClaimError::Chain(e) => write!(f, "the role certificate is not valid: {e:?}"),
            ClaimError::BadSignature => write!(f, "the claim is not signed by the role's key"),
            ClaimError::Stale => write!(f, "the claim is outside the freshness window"),
            ClaimError::Replayed => write!(f, "the claim was already used"),
            ClaimError::NoSigningKey => write!(f, "no signing key is loaded for the role"),
        }
    }
}

impl std::error::Error for ClaimError {}

/// A call made under an epoch that is not the role's current one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Superseded {
    /// The role's current epoch; 0 if it was never claimed.
    pub current: u64,
}

impl fmt::Display for Superseded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "superseded: the role's current epoch is {}",
            self.current
        )
    }
}

impl std::error::Error for Superseded {}

/// One role's persisted state: its current epoch and when it was claimed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoleRecord {
    pub epoch: u64,
    pub claimed_at: DateTime<Utc>,
}

/// Everything that must survive a restart (spec rule 6): every role's epoch, so a restarted table
/// never reissues an old one. The replay record is deliberately absent; see [`Roles::restore`].
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct RolesState {
    pub roles: BTreeMap<String, RoleRecord>,
}

/// The role table: the current epoch of every claimed role.
pub struct Roles {
    roles: BTreeMap<String, RoleRecord>,
    claims: ReplayGuard,
}

impl Roles {
    #[must_use]
    pub fn new(now: DateTime<Utc>) -> Self {
        Roles {
            roles: BTreeMap::new(),
            claims: ReplayGuard::new(ReplayConfig::default(), now),
        }
    }

    /// Verify `req` and, if it holds, make it the role's newest epoch.
    ///
    /// Order matters: the chain and the signature are checked before the replay guard records the
    /// nonce, so a request that fails verification cannot burn a legitimate client's nonce.
    pub fn claim(
        &mut self,
        req: &ClaimRequest,
        account_keys: &dyn Fn(&str) -> Option<[u8; 32]>,
        revoked: &dyn RevocationLookup,
        now: DateTime<Utc>,
    ) -> Result<Grant, ClaimError> {
        let leaf =
            validate_chain(&req.chain, account_keys, revoked, now).map_err(ClaimError::Chain)?;
        let global_id = leaf.subject_global_id;
        let text = claim_signing_input(&global_id, &req.nonce, req.signed_at);
        UnparsedPublicKey::new(&ED25519, &leaf.subject_signing_key)
            .verify(text.as_bytes(), &req.signature)
            .map_err(|_| ClaimError::BadSignature)?;

        let key_id = crate::sender_auth::key_id(&leaf.subject_signing_key);
        match self
            .claims
            .check(&key_id, &hex(&req.nonce), req.signed_at, now)
        {
            Decision::Deliver(Freshness::Fresh) => {}
            Decision::Deliver(_) => return Err(ClaimError::Stale),
            Decision::Drop => return Err(ClaimError::Replayed),
        }

        let previous = self.roles.get(&global_id).map(|r| r.epoch);
        let epoch = previous.unwrap_or(0) + 1;
        self.roles.insert(
            global_id.clone(),
            RoleRecord {
                epoch,
                claimed_at: now,
            },
        );
        Ok(Grant {
            global_id,
            epoch,
            superseded: previous,
        })
    }

    /// `Ok` only for the role's current epoch. A never-claimed role has current epoch 0, which no
    /// claim is ever granted, so no epoch can be presumed valid.
    pub fn check(&self, global_id: &str, epoch: u64) -> Result<(), Superseded> {
        match self.roles.get(global_id) {
            Some(record) if record.epoch == epoch => Ok(()),
            Some(record) => Err(Superseded {
                current: record.epoch,
            }),
            None => Err(Superseded { current: 0 }),
        }
    }

    /// The state to persist (M4).
    #[must_use]
    pub fn snapshot(&self) -> RolesState {
        RolesState {
            roles: self.roles.clone(),
        }
    }

    /// Rebuild a table from persisted state. Epochs continue from where they were. The replay
    /// record starts empty with its horizon at `now`, so any claim signed at or before `now` is
    /// refused as [`ClaimError::Stale`]: a claim captured before the restart cannot be replayed
    /// after it.
    #[must_use]
    pub fn restore(state: RolesState, now: DateTime<Utc>) -> Roles {
        Roles {
            roles: state.roles,
            claims: ReplayGuard::new(ReplayConfig::default(), now),
        }
    }

    #[must_use]
    pub fn current(&self, global_id: &str) -> Option<u64> {
        self.roles.get(global_id).map(|r| r.epoch)
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
