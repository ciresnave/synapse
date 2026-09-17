# Agent Certificates (f1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A receiver that has pinned one **account key** can verify any agent that account holder
runs, from a certificate chain the agent attaches to every message — with no per-agent configuration.

**Architecture:** A new `src/certificate.rs` holds the certificate, the revocation, their canonical
signed bytes, PEM encoding and chain validation — no I/O, no clock, `now` passed in. `TrustStore`
gains account keys and a revocation store, and `TrustStore::verify` gains a second route to
`Verified`: a chain rooted in a pinned account key. Because every consumer already reads that
verdict, the delivery gate, the knock record and `synapse-mcp` inherit certificates with no changes.

**Tech Stack:** Rust, `ed25519-dalek` and `sha2` (already dependencies, used by `src/sender_auth.rs`),
`pem` 3 (already used by `src/sealing.rs`), `chrono`. No new dependencies.

**Spec:** `docs/superpowers/specs/2026-09-17-agent-certificates-design.md` — **§11 f1 scope only.**
The CLI, the `synapse-mcp` config changes and removing direct pinning are f2, not this plan.

## Global Constraints

- **Branch:** `feat/agent-certificates`, stacked on `feat/replay-suppression` (#42). Held for the
  PM's daily merge pass.
- **Direct pinning keeps working.** `TrustStore::pin`, `pin_pem`, `pin_sealing_key`,
  `pin_sealing_key_pem` are untouched in f1; a node may be configured either way. Every existing
  test must still pass unchanged.
- **Exact strings:** certificate domain tag `synapse/agent-cert/v1`; revocation domain tag
  `synapse/agent-revocation/v1`; PEM labels `SYNAPSE AGENT CERT` and `SYNAPSE REVOCATION`; metadata
  keys `synapse.cert.chain` and `synapse.cert.revocations`.
- **Canonical bytes** follow `sender_auth::canonical_input`'s discipline exactly: a domain tag first,
  then each field as a 4-byte big-endian length followed by its bytes, in the order the spec's struct
  declares. Timestamps are i64 microseconds, big-endian. Reuse `sender_auth::put`.
- **Permissions:** the known set is `send`, `request-ack`, `ack`. A permission starting `x-` is
  carried and reported, never interpreted. Any other unknown string makes the certificate invalid.
- **Never validate a chain whose claimed root is not a pinned account key**, and never parse a chain
  field larger than `max_chain_bytes` (default 16 KiB). Both refusals happen before any signature in
  the chain is verified.
- **A chain longer than `max_chain_links` (default 64) is refused**, even when its root is pinned.
  This is a resource guard, not a trust rule: it exists so a compromised pinned account cannot wedge
  a node with an enormous chain.
- **Identity narrows:** a child's `subject_global_id` is its parent's with one label prepended
  (`worker.agent@host` under `agent@host`).
- **Secret hygiene:** no private key bytes in any `Debug`, error, log line or tool output.
- New `.rs` files start with `// SPDX-License-Identifier: MIT OR Apache-2.0`.
- Tests bind loopback only. `CARGO_TARGET_DIR=C:/Projects/synapse/target` for every cargo command.
- Checks: `cargo test --no-fail-fast` keeps the failing set `{test_transport_error_handling}`;
  `cargo fmt -- --check` exits 0; `cargo clippy -- -D warnings` exits 0, including on each new or
  changed test target.
- No new dependency. No version bump (P2 bumps once, when the set is complete).

## File Structure

| file | responsibility |
|---|---|
| `src/certificate.rs` (new) | `Permission`, `AgentCertificate`, `Revocation`, canonical bytes, PEM, chain parsing and validation, `ChainError`, `VerifiedChain`; all unit tests |
| `src/lib.rs` | `pub mod certificate;` |
| `src/sender_auth.rs` | account keys and the revocation store in `TrustStore`; the chain route in `verify`; new `UnverifiableReason` variants |
| `src/crypto.rs` | hold this node's own chain, and attach it when signing |
| `src/transport/manager.rs` | `ReceivedMessage.certificate` summary |
| `tests/agent_certificates.rs` (new) | spec §10 tests 7-11 over loopback UDP |

---

### Task 1: The certificate and revocation types, canonical bytes and PEM

**Files:**
- Create: `src/certificate.rs`
- Modify: `src/lib.rs` (add `pub mod certificate;` beside `pub mod replay;`), `src/sender_auth.rs`
  (make `put` visible to the new module — it is already `pub(crate)`, so no change is expected;
  confirm)

**Interfaces:**
- Consumes: `sender_auth::{put, key_id}`.
- Produces:
  - `pub enum Permission { Send, RequestAck, Ack, Extension(String) }` with
    `as_str(&self) -> String` (`"send"`, `"request-ack"`, `"ack"`, or the `x-` string) and
    `Permission::parse(&str) -> Option<Permission>` returning `None` for an unknown non-`x-` string.
  - `pub struct AgentCertificate` with the spec §4 fields, `Clone`, and a `Debug` that shows the
    serial, label, subject id and validity — never key bytes.
  - `pub struct Revocation` with the spec §6 fields.
  - `AgentCertificate::signing_input(&self) -> Vec<u8>` and `Revocation::signing_input(&self) -> Vec<u8>`.
  - `AgentCertificate::sign(unsigned, issuer: &SigningKey) -> AgentCertificate` and
    `verify_signature(&self, issuer_public: &[u8; 32]) -> bool`; the same pair on `Revocation`.
  - `to_pem(&self) -> String` and `from_pem(&str) -> Result<Self, ChainError>` for both.
  - `pub fn chain_to_pem(chain: &[AgentCertificate]) -> String` and
    `pub fn chain_from_pem(text: &str) -> Result<Vec<AgentCertificate>, ChainError>` (leaf first).
  - `pub enum ChainError { Malformed, UnknownPermission, TooLarge, UnknownIssuer, BadSignature, NotYetValid, Expired, ValidityNotNested, PermissionWidened, DelegationNotAllowed, IdentityNotNarrowed, SubjectMismatch, Revoked }`, with `Display` giving snake_case names.

- [ ] **Step 1: Write the failing tests** in `src/certificate.rs`'s `#[cfg(test)] mod tests`.

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{SigningKey, VerifyingKey};

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
        assert_eq!(Permission::parse("request-ack"), Some(Permission::RequestAck));
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
        assert_eq!(parsed[0].subject_global_id, "worker.agent@host", "leaf first");
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
```

- [ ] **Step 2: Run the tests and check that they fail**

Run: `cargo test --lib certificate:: 2>&1 | tail -20`
Expected: the module does not exist, then the types do not exist.

- [ ] **Step 3: Implement `src/certificate.rs`**

Follow `src/sealing.rs` for PEM handling and `src/sender_auth.rs` for signing. The essentials:

```rust
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Account keys and agent certificates (P2 slice f1).
//!
//! No I/O and no clock: `now` is passed in. See
//! `docs/superpowers/specs/2026-09-17-agent-certificates-design.md`.

use chrono::{DateTime, Utc};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};

pub const CERT_DOMAIN_TAG: &[u8] = b"synapse/agent-cert/v1";
pub const REVOCATION_DOMAIN_TAG: &[u8] = b"synapse/agent-revocation/v1";
pub const CERT_PEM_LABEL: &str = "SYNAPSE AGENT CERT";
pub const REVOCATION_PEM_LABEL: &str = "SYNAPSE REVOCATION";
/// Signed metadata carrying the sender's chain, leaf first.
pub const CHAIN_KEY: &str = "synapse.cert.chain";
/// Signed metadata carrying relayed revocations.
pub const REVOCATIONS_KEY: &str = "synapse.cert.revocations";

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
```

`AgentCertificate` and `Revocation` carry the spec §4 and §6 fields exactly. `signing_input`
starts with the domain tag and then `put`s each field in declaration order, using
`timestamp_micros().to_be_bytes()` for times, `[value]` for the `u8` fields, and for `permissions` a
4-byte big-endian count followed by each `as_str()` sorted. `sign` computes the input, signs with
`ed25519_dalek::Signer`, and returns the value with `signature` filled in. `verify_signature`
rebuilds the input and checks it with `VerifyingKey::verify`.

The wire form inside the PEM body is the same canonical encoding as `signing_input`, followed by the
64 signature bytes, so parsing is the inverse of one function rather than two encodings that can
drift. `from_pem` refuses a body whose permissions do not all `Permission::parse`, returning
`ChainError::UnknownPermission`, and refuses a wrong PEM label or trailing bytes with
`ChainError::Malformed`. Write a `Debug` for `AgentCertificate` that prints the serial (hex), label,
subject id and validity only.

`chain_to_pem` concatenates blocks leaf first; `chain_from_pem` parses every block in order and
returns `ChainError::Malformed` on an empty input.

Add `pub mod certificate;` to `src/lib.rs`.

- [ ] **Step 4: Run the tests and check that they pass**

Run: `cargo test --lib certificate:: 2>&1 | grep -E "^test |test result"`
Expected: 8 tests, all ok. Then `cargo fmt` and `cargo clippy -- -D warnings`.

- [ ] **Step 5: Commit**

```bash
git add src/certificate.rs src/lib.rs
git commit -m "feat(cert): the agent certificate and revocation formats (P2f1)"
```

---

### Task 2: Chain validation

**Files:**
- Modify: `src/certificate.rs` (validation plus its tests)

**Interfaces:**
- Consumes: everything from Task 1.
- Produces:
  - `pub struct VerifiedChain { pub account_key_id: String, pub subject_label: String, pub subject_global_id: String, pub subject_signing_key: [u8; 32], pub subject_sealing_key: [u8; 32], pub permissions: Vec<Permission>, pub links: usize }`
  - `pub trait RevocationLookup { fn is_revoked(&self, serial: &[u8; 16]) -> bool; }`
  - `pub fn validate_chain(chain: &[AgentCertificate], account_keys: &dyn Fn(&str) -> Option<[u8; 32]>, revoked: &dyn RevocationLookup, now: DateTime<Utc>) -> Result<VerifiedChain, ChainError>`
  - `pub fn identity_narrows(parent: &str, child: &str) -> bool`

Validation order, which is also the order the tests check, mirrors spec §5:

1. The chain is non-empty (`Malformed`).
2. The **root**'s `issuer_key_id` resolves through `account_keys` (`UnknownIssuer`) and its signature
   verifies against that key (`BadSignature`).
3. Walking from root to leaf, each certificate's signature verifies against its parent's
   `subject_signing_key` (`BadSignature`).
4. For every certificate: `now` within `[not_before, not_after]` (`NotYetValid` / `Expired`), and a
   child's window inside its parent's (`ValidityNotNested`).
5. Every child permission appears in its parent (`PermissionWidened`).
6. A parent with `may_delegate == 0` may not issue (`DelegationNotAllowed`), and a child's
   `may_delegate` is strictly less than its parent's (`DelegationNotAllowed`).
7. A child's `subject_global_id` is its parent's with one label prepended (`IdentityNotNarrowed`).
8. No certificate's serial is revoked (`Revoked`).

- [ ] **Step 1: Write the failing tests** (append to the same `mod tests`)

```rust
    struct NoRevocations;
    impl RevocationLookup for NoRevocations {
        fn is_revoked(&self, _serial: &[u8; 16]) -> bool {
            false
        }
    }

    struct Revoked([u8; 16]);
    impl RevocationLookup for Revoked {
        fn is_revoked(&self, serial: &[u8; 16]) -> bool {
            serial == &self.0
        }
    }

    /// A two-link chain: account -> agent -> worker. Returns (leaf-first chain, account public key).
    fn two_link_chain() -> (Vec<AgentCertificate>, [u8; 32]) {
        let (account, agent, worker) = (key(1), key(2), key(3));
        let upper = AgentCertificate::sign(unsigned(&account, &agent, "agent@host"), &account);
        let mut lower_unsigned = unsigned(&agent, &worker, "worker.agent@host");
        lower_unsigned.issuer_key_id = crate::sender_auth::key_id(&public(&agent));
        lower_unsigned.may_delegate = 1;
        let lower = AgentCertificate::sign(lower_unsigned, &agent);
        (vec![lower, upper], public(&account))
    }

    fn pinned(account_key: [u8; 32]) -> impl Fn(&str) -> Option<[u8; 32]> {
        let id = crate::sender_auth::key_id(&account_key);
        move |key_id: &str| (key_id == id).then_some(account_key)
    }

    #[test]
    fn a_valid_two_link_chain_verifies() {
        let (chain, account) = two_link_chain();
        let verified =
            validate_chain(&chain, &pinned(account), &NoRevocations, t(10)).expect("valid");
        assert_eq!(verified.subject_global_id, "worker.agent@host");
        assert_eq!(verified.links, 2);
        assert_eq!(verified.account_key_id, crate::sender_auth::key_id(&account));
    }

    #[test]
    fn a_chain_whose_root_is_not_pinned_is_refused() {
        let (chain, _account) = two_link_chain();
        let nobody = |_key_id: &str| None;
        assert_eq!(
            validate_chain(&chain, &nobody, &NoRevocations, t(10)),
            Err(ChainError::UnknownIssuer)
        );
    }

    #[test]
    fn validity_must_nest_inside_the_parents() {
        let (account, agent) = (key(1), key(2));
        let mut child = unsigned(&account, &agent, "worker.agent@host");
        child.not_after = t(999_999); // outside the parent's window
        let (mut chain, account_key) = two_link_chain();
        child.issuer_key_id = chain[1].issuer_key_id.clone();
        chain[0] = AgentCertificate::sign(child, &account);
        assert!(matches!(
            validate_chain(&chain, &pinned(account_key), &NoRevocations, t(10)),
            Err(ChainError::ValidityNotNested) | Err(ChainError::BadSignature)
        ));
    }

    #[test]
    fn an_expired_or_not_yet_valid_certificate_is_refused() {
        let (chain, account) = two_link_chain();
        assert_eq!(
            validate_chain(&chain, &pinned(account), &NoRevocations, t(-1)),
            Err(ChainError::NotYetValid)
        );
        assert_eq!(
            validate_chain(&chain, &pinned(account), &NoRevocations, t(86_401)),
            Err(ChainError::Expired)
        );
        // Control: exactly on each boundary is valid.
        assert!(validate_chain(&chain, &pinned(account), &NoRevocations, t(0)).is_ok());
        assert!(validate_chain(&chain, &pinned(account), &NoRevocations, t(86_400)).is_ok());
    }

    #[test]
    fn a_child_cannot_widen_its_permissions() {
        let (account, agent, worker) = (key(1), key(2), key(3));
        let upper_unsigned = {
            let mut c = unsigned(&account, &agent, "agent@host");
            c.permissions = vec![Permission::Send];
            c
        };
        let upper = AgentCertificate::sign(upper_unsigned, &account);
        let lower = {
            let mut c = unsigned(&agent, &worker, "worker.agent@host");
            c.issuer_key_id = crate::sender_auth::key_id(&public(&agent));
            c.may_delegate = 1;
            c.permissions = vec![Permission::Send, Permission::Ack]; // Ack was never granted
            AgentCertificate::sign(c, &agent)
        };
        assert_eq!(
            validate_chain(&[lower, upper], &pinned(public(&account)), &NoRevocations, t(10)),
            Err(ChainError::PermissionWidened)
        );
    }

    #[test]
    fn delegation_budgets_must_decrease_and_zero_cannot_issue() {
        let (account, agent, worker) = (key(1), key(2), key(3));
        // A parent with may_delegate 0 may not issue at all.
        let upper = {
            let mut c = unsigned(&account, &agent, "agent@host");
            c.may_delegate = 0;
            AgentCertificate::sign(c, &account)
        };
        let lower = {
            let mut c = unsigned(&agent, &worker, "worker.agent@host");
            c.issuer_key_id = crate::sender_auth::key_id(&public(&agent));
            c.may_delegate = 0;
            AgentCertificate::sign(c, &agent)
        };
        assert_eq!(
            validate_chain(&[lower, upper], &pinned(public(&account)), &NoRevocations, t(10)),
            Err(ChainError::DelegationNotAllowed)
        );
    }

    #[test]
    fn a_child_cannot_claim_an_id_outside_its_parents_namespace() {
        // The impersonation case: a delegate mints itself a certificate for its account holder's id.
        let (account, agent, worker) = (key(1), key(2), key(3));
        let upper = AgentCertificate::sign(unsigned(&account, &agent, "agent@host"), &account);
        let lower = {
            let mut c = unsigned(&agent, &worker, "ciresnave@host"); // not under agent@host
            c.issuer_key_id = crate::sender_auth::key_id(&public(&agent));
            c.may_delegate = 1;
            AgentCertificate::sign(c, &agent)
        };
        assert_eq!(
            validate_chain(&[lower, upper], &pinned(public(&account)), &NoRevocations, t(10)),
            Err(ChainError::IdentityNotNarrowed)
        );
    }

    #[test]
    fn identity_narrowing_accepts_one_prepended_label_only() {
        assert!(identity_narrows("agent@host", "worker.agent@host"));
        assert!(identity_narrows("worker.agent@host", "task.worker.agent@host"));
        assert!(!identity_narrows("agent@host", "agent@host"));
        assert!(!identity_narrows("agent@host", "a.b.agent@host"), "one label at a time");
        assert!(!identity_narrows("agent@host", "evil@host"));
        assert!(!identity_narrows("agent@host", "workeragent@host"), "the dot is required");
        assert!(!identity_narrows("agent@host", "worker.agent@other"));
    }

    #[test]
    fn a_revoked_certificate_invalidates_the_chain() {
        let (chain, account) = two_link_chain();
        let leaf_serial = chain[0].serial;
        assert_eq!(
            validate_chain(&chain, &pinned(account), &Revoked(leaf_serial), t(10)),
            Err(ChainError::Revoked)
        );
        // Control: revoking an unrelated serial leaves the chain valid.
        assert!(validate_chain(&chain, &pinned(account), &Revoked([1u8; 16]), t(10)).is_ok());
    }

    #[test]
    fn a_revoked_parent_invalidates_everything_below_it() {
        let (chain, account) = two_link_chain();
        let root_serial = chain[1].serial;
        assert_eq!(
            validate_chain(&chain, &pinned(account), &Revoked(root_serial), t(10)),
            Err(ChainError::Revoked)
        );
    }
```

Note: `two_link_chain`'s two certificates share a serial (`[7u8; 16]`) because `unsigned` sets it.
Give the lower certificate a different serial (`[8u8; 16]`) inside `two_link_chain` so the revocation
tests distinguish them; adjust the helper when you write it, and make
`a_revoked_parent_invalidates_everything_below_it` revoke the root's serial specifically.

- [ ] **Step 2: Run them and check that they fail**

Run: `cargo test --lib certificate:: 2>&1 | tail -20`
Expected: `validate_chain`, `VerifiedChain`, `RevocationLookup` and `identity_narrows` do not exist.

- [ ] **Step 3: Implement validation**

```rust
/// A child's id is its parent's with exactly one label prepended: `worker.agent@host` under
/// `agent@host`. Without this, a delegate could mint itself a certificate for any id, including its
/// own account holder's (spec §5 rule 8).
#[must_use]
pub fn identity_narrows(parent: &str, child: &str) -> bool {
    let Some(prefix) = child.strip_suffix(parent) else {
        return false;
    };
    let Some(label) = prefix.strip_suffix('.') else {
        return false;
    };
    !label.is_empty() && !label.contains('.')
}
```

`validate_chain` walks the slice from its last element (the root) to its first (the leaf), carrying
the parent's public key, validity window, permission set and delegation budget. Each step applies the
rules in the order listed above and returns the first error. On success it builds `VerifiedChain`
from the leaf, with `links = chain.len()`. Revocation is checked for every certificate, so a revoked
parent fails the whole chain.

- [ ] **Step 4: Run the tests and check that they pass**

Run: `cargo test --lib certificate:: 2>&1 | grep -E "^test |test result"`
Expected: 18 tests, all ok. Then `cargo fmt` and `cargo clippy -- -D warnings`.

- [ ] **Step 5: Commit**

```bash
git add src/certificate.rs
git commit -m "feat(cert): chain validation, narrowing identity, permissions and delegation (P2f1)"
```

---

### Task 3: `TrustStore` — account keys, revocations, and the chain route to `Verified`

**Files:**
- Modify: `src/sender_auth.rs`
- Test: the same file's `#[cfg(test)] mod tests`

**Interfaces:**
- Consumes: `certificate::{AgentCertificate, ChainError, Revocation, RevocationLookup, VerifiedChain, chain_from_pem, validate_chain, CHAIN_KEY, REVOCATIONS_KEY}`.
- Produces:
  - `TrustStore::{pin_account_key(global_id_or_label: impl Into<String>, key: [u8; 32]), pin_account_key_pem(&str, &str) -> Result<()>, account_key_ids() -> Vec<String>, add_revocation(Revocation) -> bool, is_revoked(&[u8; 16]) -> bool, revocation_count() -> usize}`, plus the fields `max_chain_bytes: usize` (default `16 * 1024`) and `max_chain_links: usize` (default 64).
  - `UnverifiableReason` gains `UnknownIssuer`, `InvalidChain`, `ChainTooLarge`.
  - `SenderVerdict::Verified` is unchanged, so every existing consumer keeps working. The chain
    summary is returned separately by `TrustStore::verified_chain(&SecureMessage) -> Option<VerifiedChain>`.

**How `verify` changes.** Today it looks up a pinned key for `from_global_id` and checks the
signature. After this task:

1. If a key is pinned for `from_global_id`, behave exactly as today. Direct pinning wins, so no
   existing deployment changes behaviour.
2. Otherwise, if the message has no `CHAIN_KEY` metadata, return today's
   `Unverifiable { UnknownSender }`.
3. Otherwise: if the chain text is longer than `max_chain_bytes`, return
   `Unverifiable { ChainTooLarge }` **without parsing**.
4. Parse it. On a parse error, `Unverifiable { InvalidChain }`. A chain with more than
   `max_chain_links` certificates is `Unverifiable { ChainTooLarge }`, checked immediately after
   parsing and before any signature is verified.
5. Read the root's `issuer_key_id`. If it is not a pinned account key, return
   `Unverifiable { UnknownIssuer }` **without verifying any signature in the chain**.
6. Validate the chain. On error, `Unverifiable { InvalidChain }`.
7. Check the message's own signature against the leaf's `subject_signing_key`. On failure,
   `Contradicted { BadSignature }` — the sender presented a chain and then signed with a different
   key, which has no innocent reading.
8. Otherwise `Verified { key_id: key_id(&leaf.subject_signing_key) }`.

- [ ] **Step 1: Write the failing tests**

```rust
    #[test]
    fn a_pinned_account_key_verifies_an_agent_it_has_never_seen() {
        // Alice's account key signs a certificate for an agent key; Bob pins only the account key.
        let account = SigningKey::from_bytes(&[11u8; 32]);
        let agent = CryptoManager::new_with_keypair();
        let cert = signed_cert_for(&account, &agent, "agent@alice.test", t_now());
        let mut message = SecureMessage::new("bob@test", "agent@alice.test", b"hi".to_vec(), SecurityLevel::Authenticated);
        message.add_metadata(certificate::CHAIN_KEY, &certificate::chain_to_pem(&[cert]));
        agent.sign_secure_message(&mut message).unwrap();

        let mut store = TrustStore::new();
        store.pin_account_key("alice", account.verifying_key().to_bytes());
        assert!(store.verify(&message).is_verified());
        let chain = store.verified_chain(&message).expect("a summary");
        assert_eq!(chain.subject_global_id, "agent@alice.test");
        assert_eq!(chain.links, 1);

        // Control: with the account key unpinned, the same message is unverifiable.
        assert_eq!(
            TrustStore::new().verify(&message),
            SenderVerdict::Unverifiable { reason: UnverifiableReason::UnknownIssuer }
        );
    }

    #[test]
    fn a_chain_with_too_many_links_is_refused() {
        // A resource guard: 65 links exceeds the default max_chain_links of 64. The certificates do
        // not need to be valid — the refusal happens before any of them is verified.
        let account = SigningKey::from_bytes(&[11u8; 32]);
        let agent = CryptoManager::new_with_keypair();
        let one = signed_cert_for(&account, &agent, "agent@alice.test", t_now());
        let chain: Vec<_> = std::iter::repeat_with(|| one.clone()).take(65).collect();
        let mut message = SecureMessage::new("bob@test", "agent@alice.test", b"hi".to_vec(), SecurityLevel::Authenticated);
        message.add_metadata(certificate::CHAIN_KEY, &certificate::chain_to_pem(&chain));
        agent.sign_secure_message(&mut message).unwrap();
        let mut store = TrustStore::new();
        store.pin_account_key("alice", account.verifying_key().to_bytes());
        assert_eq!(
            store.verify(&message),
            SenderVerdict::Unverifiable { reason: UnverifiableReason::ChainTooLarge }
        );
    }

    #[test]
    fn an_oversized_chain_is_refused_without_parsing() {
        let mut message = SecureMessage::new("bob@test", "agent@alice.test", b"hi".to_vec(), SecurityLevel::Authenticated);
        message.add_metadata(certificate::CHAIN_KEY, &"A".repeat(17 * 1024));
        let store = TrustStore::new();
        assert_eq!(
            store.verify(&message),
            SenderVerdict::Unverifiable { reason: UnverifiableReason::ChainTooLarge }
        );
    }

    #[test]
    fn a_chain_that_does_not_match_the_signing_key_is_contradicted() {
        // The chain is valid, but the message is signed by a different key than the leaf names.
        let account = SigningKey::from_bytes(&[11u8; 32]);
        let agent = CryptoManager::new_with_keypair();
        let impostor = CryptoManager::new_with_keypair();
        let cert = signed_cert_for(&account, &agent, "agent@alice.test", t_now());
        let mut message = SecureMessage::new("bob@test", "agent@alice.test", b"hi".to_vec(), SecurityLevel::Authenticated);
        message.add_metadata(certificate::CHAIN_KEY, &certificate::chain_to_pem(&[cert]));
        impostor.sign_secure_message(&mut message).unwrap();

        let mut store = TrustStore::new();
        store.pin_account_key("alice", account.verifying_key().to_bytes());
        assert_eq!(
            store.verify(&message),
            SenderVerdict::Contradicted { reason: ContradictedReason::BadSignature }
        );
    }

    #[test]
    fn direct_pinning_still_wins_and_still_works() {
        // An existing deployment: no certificate anywhere, a pinned agent key.
        let agent = CryptoManager::new_with_keypair();
        let mut message = SecureMessage::new("bob@test", "alice@test", b"hi".to_vec(), SecurityLevel::Authenticated);
        agent.sign_secure_message(&mut message).unwrap();
        let mut store = TrustStore::new();
        store.pin("alice@test", agent.public_key_bytes().unwrap());
        assert!(store.verify(&message).is_verified());
    }

    #[test]
    fn a_revocation_is_stored_once_and_refuses_an_unknown_issuer() {
        let account = SigningKey::from_bytes(&[11u8; 32]);
        let mut store = TrustStore::new();
        let revocation = signed_revocation(&account, [7u8; 16]);
        // Not pinned yet: refused, and nothing stored.
        assert!(!store.add_revocation(revocation.clone()));
        assert_eq!(store.revocation_count(), 0);
        // Pinned: accepted once, and the duplicate is ignored.
        store.pin_account_key("alice", account.verifying_key().to_bytes());
        assert!(store.add_revocation(revocation.clone()));
        assert!(!store.add_revocation(revocation));
        assert_eq!(store.revocation_count(), 1);
        assert!(store.is_revoked(&[7u8; 16]));
        assert!(!store.is_revoked(&[8u8; 16]));
    }

    #[test]
    fn a_revoked_certificate_stops_verifying() {
        let account = SigningKey::from_bytes(&[11u8; 32]);
        let agent = CryptoManager::new_with_keypair();
        let cert = signed_cert_for(&account, &agent, "agent@alice.test", t_now());
        let serial = cert.serial;
        let mut message = SecureMessage::new("bob@test", "agent@alice.test", b"hi".to_vec(), SecurityLevel::Authenticated);
        message.add_metadata(certificate::CHAIN_KEY, &certificate::chain_to_pem(&[cert]));
        agent.sign_secure_message(&mut message).unwrap();

        let mut store = TrustStore::new();
        store.pin_account_key("alice", account.verifying_key().to_bytes());
        assert!(store.verify(&message).is_verified(), "valid before revocation");
        assert!(store.add_revocation(signed_revocation(&account, serial)));
        assert_eq!(
            store.verify(&message),
            SenderVerdict::Unverifiable { reason: UnverifiableReason::InvalidChain }
        );
    }
```

Write the three helpers (`signed_cert_for`, `signed_revocation`, `t_now`) in the test module, and a
`CryptoManager::new_with_keypair` test helper if the existing tests do not already have one — check
first and reuse whatever `src/sender_auth.rs`'s tests already do for making a signer.

- [ ] **Step 2: Run them and check that they fail**

Run: `cargo test --lib sender_auth:: 2>&1 | tail -20`

- [ ] **Step 3: Implement**

Add to `TrustStore`: `account_keys: HashMap<String, [u8; 32]>` (keyed by the account key's `key_id`,
with the label kept for reporting), `revocations: HashMap<[u8; 16], Revocation>`, and
`max_chain_bytes: usize`. `add_revocation` verifies the revocation's signature against a pinned
account key and returns `false` if the issuer is not pinned, the signature fails, or the serial is
already held; it evicts the oldest `issued_at` beyond 4096 entries. Implement `RevocationLookup` for
`TrustStore`. `verify` follows the seven steps above; `verified_chain` repeats steps 3-6 and returns
the summary.

- [ ] **Step 4: Run the tests and check that they pass**

Run: `cargo test --lib sender_auth:: certificate:: 2>&1 | grep -E "test result"` and then the full
`cargo test --no-fail-fast`, which must keep the failing set `{test_transport_error_handling}` —
every earlier slice's test still passes because direct pinning is untouched.

- [ ] **Step 5: Commit**

```bash
git add src/sender_auth.rs
git commit -m "feat(cert): verify a sender from a chain rooted in a pinned account key (P2f1)"
```

---

### Task 4: Sending with a chain, and the received summary

**Files:**
- Modify: `src/crypto.rs`, `src/transport/manager.rs`
- Test: `tests/agent_certificates.rs` (new)

**Interfaces:**
- Consumes: Tasks 1-3.
- Produces:
  - `CryptoManager::{set_certificate_chain(Vec<AgentCertificate>), certificate_chain() -> &[AgentCertificate]}`. `sign_secure_message` inserts `CHAIN_KEY` metadata **before** signing when a chain is set, so the chain is covered by the signature.
  - `ReceivedMessage.certificate: Option<VerifiedChain>`, filled in `receive_messages` from `store.verified_chain(message)` for a verified sender.

- [ ] **Step 1: Write the failing tests** in `tests/agent_certificates.rs`, over loopback UDP. Copy
  the `free_udp_port`, `udp_node`, `send_raw` and `receive_one` helpers from
  `tests/replay_suppression.rs` (a test binary cannot import another's helpers), and add the
  certificate helpers below.

```rust
// SPDX-License-Identifier: MIT OR Apache-2.0
//! P2 slice f1: an account key vouches for agents a receiver has never pinned.
//! Test numbers refer to docs/superpowers/specs/2026-09-17-agent-certificates-design.md §10.

use chrono::{Duration, Utc};
use ed25519_dalek::SigningKey;
use synapse::CryptoManager;
use synapse::certificate::{
    AgentCertificate, Permission, Revocation, REVOCATIONS_KEY, chain_to_pem,
};
use synapse::sender_auth::{TrustStore, UnverifiableReason};
use synapse::types::{SecureMessage, SecurityLevel};

const BOB: &str = "bob@synapse.test";

fn account_key(seed: u8) -> SigningKey {
    SigningKey::from_bytes(&[seed; 32])
}

fn agent() -> CryptoManager {
    let mut crypto = CryptoManager::new();
    crypto.generate_keypair().expect("keypair");
    crypto
}

/// A certificate from `account` for `holder`, valid for an hour around now.
fn cert_for(
    account: &SigningKey,
    holder: &CryptoManager,
    global_id: &str,
    permissions: Vec<Permission>,
    serial: u8,
) -> AgentCertificate {
    let now = Utc::now();
    AgentCertificate::sign(
        AgentCertificate {
            version: 1,
            serial: [serial; 16],
            issuer_key_id: synapse::sender_auth::key_id(&account.verifying_key().to_bytes()),
            subject_label: "agent".to_string(),
            subject_global_id: global_id.to_string(),
            subject_signing_key: holder.public_key_bytes().expect("public key"),
            subject_sealing_key: [0u8; 32],
            not_before: now - Duration::minutes(1),
            not_after: now + Duration::hours(1),
            permissions,
            may_delegate: 1,
            signature: [0u8; 64],
        },
        account,
    )
}

/// A signed message from `holder` carrying `chain`, sent to Bob.
fn message_with_chain(
    holder: &CryptoManager,
    from: &str,
    chain: &[AgentCertificate],
    text: &[u8],
) -> SecureMessage {
    let mut m = SecureMessage::new(BOB, from, text.to_vec(), SecurityLevel::Authenticated);
    m.add_metadata(synapse::certificate::CHAIN_KEY, &chain_to_pem(chain));
    holder.sign_secure_message(&mut m).expect("sign");
    m
}

fn store_pinning(account: &SigningKey) -> TrustStore {
    let mut store = TrustStore::new();
    store.pin_account_key("alice", account.verifying_key().to_bytes());
    store
}

/// Poll for up to 2 s and return everything that arrived.
async fn drain(
    manager: &synapse::transport::TransportManager,
) -> Vec<synapse::transport::ReceivedMessage> {
    let mut out = Vec::new();
    for _ in 0..20 {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        out.extend(manager.receive_messages().await.expect("receive"));
    }
    out
}

// §10 test 7
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_agent_is_verified_from_its_account_keys_certificate() {
    let account = account_key(11);
    let alice = agent();
    let cert = cert_for(&account, &alice, "agent@alice.test", vec![Permission::Send], 1);
    let message = message_with_chain(&alice, "agent@alice.test", &[cert], b"hello");

    // Bob pins the ACCOUNT key only; he has never seen this agent's key.
    let (bob, port) = udp_node(store_pinning(&account), None).await;
    send_raw(port, &message);
    let received = receive_one(&bob).await;
    assert!(received.sender.is_verified());
    let chain = received.certificate.expect("a certificate summary");
    assert_eq!(chain.subject_global_id, "agent@alice.test");
    assert_eq!(chain.subject_label, "agent");
    assert_eq!(chain.permissions, vec![Permission::Send]);
    assert_eq!(chain.links, 1);
    assert_eq!(
        chain.account_key_id,
        synapse::sender_auth::key_id(&account.verifying_key().to_bytes())
    );

    // Control: a receiver that pins nothing denies the same message and records why.
    let (stranger, stranger_port) = udp_node(TrustStore::new(), None).await;
    send_raw(stranger_port, &message);
    assert!(drain(&stranger).await.is_empty());
    let knocks = stranger.knocks().await;
    assert_eq!(knocks.len(), 1);
    assert_eq!(knocks[0].reason, "unknown_issuer");
}

// §10 test 8: a stranger's chain costs no signature verification.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_chain_rooting_in_an_unpinned_key_costs_no_signature_work() {
    let account = account_key(12);
    let alice = agent();
    // The certificate's signature is deliberately invalid.
    let mut cert = cert_for(&account, &alice, "agent@alice.test", vec![Permission::Send], 2);
    cert.signature = [0u8; 64];
    let message = message_with_chain(&alice, "agent@alice.test", &[cert.clone()], b"hello");

    // Unpinned root: the reason must be unknown_issuer, NOT invalid_chain. Reporting
    // invalid_chain would prove the node verified a signature on a stranger's behalf.
    let (bob, port) = udp_node(TrustStore::new(), None).await;
    send_raw(port, &message);
    assert!(drain(&bob).await.is_empty());
    assert_eq!(bob.knocks().await[0].reason, "unknown_issuer");

    // Control: with the root pinned, the same chain IS validated, and fails on its signature.
    let (pinned, pinned_port) = udp_node(store_pinning(&account), None).await;
    send_raw(pinned_port, &message);
    assert!(drain(&pinned).await.is_empty());
    assert_eq!(pinned.knocks().await[0].reason, "invalid_chain");
}

// §10 test 9
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_revoked_certificate_stops_being_accepted() {
    let account = account_key(13);
    let alice = agent();
    let cert = cert_for(&account, &alice, "agent@alice.test", vec![Permission::Send], 3);
    let serial = cert.serial;

    let (bob, port) = udp_node(store_pinning(&account), None).await;
    send_raw(port, &message_with_chain(&alice, "agent@alice.test", &[cert.clone()], b"first"));
    let first = receive_one(&bob).await;
    assert!(first.sender.is_verified(), "valid before the revocation");

    let mut store = bob.trust_store().await;
    assert!(store.add_revocation(signed_revocation(&account, serial)));
    bob.set_trust_store(store).await;

    send_raw(port, &message_with_chain(&alice, "agent@alice.test", &[cert], b"second"));
    assert!(drain(&bob).await.is_empty(), "denied after the revocation");
    assert!(bob.knocks().await.iter().any(|k| k.reason == "invalid_chain"));
}

// §10 test 11: rotation is the whole point of the slice.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rotating_an_agent_key_needs_no_change_at_the_receiver() {
    let account = account_key(14);
    let (old_agent, new_agent) = (agent(), agent());
    let (bob, port) = udp_node(store_pinning(&account), None).await;

    let old_cert = cert_for(&account, &old_agent, "agent@alice.test", vec![Permission::Send], 4);
    send_raw(port, &message_with_chain(&old_agent, "agent@alice.test", &[old_cert], b"before"));
    assert!(receive_one(&bob).await.sender.is_verified());

    // A brand new agent key, a new certificate from the SAME account key, and no config change.
    let new_cert = cert_for(&account, &new_agent, "agent@alice.test", vec![Permission::Send], 5);
    send_raw(port, &message_with_chain(&new_agent, "agent@alice.test", &[new_cert], b"after"));
    let rotated = receive_one(&bob).await;
    assert!(rotated.sender.is_verified());
    assert_eq!(
        rotated.certificate.expect("summary").subject_global_id,
        "agent@alice.test"
    );

    // Control: an agent with no certificate from that account is denied.
    let outsider = agent();
    let mut plain = SecureMessage::new(BOB, "agent@alice.test", b"nope".to_vec(), SecurityLevel::Authenticated);
    outsider.sign_secure_message(&mut plain).expect("sign");
    send_raw(port, &plain);
    assert!(drain(&bob).await.is_empty());
}

/// A revocation of `serial`, signed by `account`.
fn signed_revocation(account: &SigningKey, serial: [u8; 16]) -> Revocation {
    Revocation::sign(
        Revocation {
            version: 1,
            serial,
            issuer_key_id: synapse::sender_auth::key_id(&account.verifying_key().to_bytes()),
            issued_at: Utc::now(),
            reason: "test".to_string(),
            signature: [0u8; 64],
        },
        account,
    )
}
```

- [ ] **Step 2: Run them and check that they fail**

Run: `cargo test --test agent_certificates 2>&1 | tail -20`
Expected: `set_certificate_chain`, `certificate` on `ReceivedMessage` and `pin_account_key` on the
builder path do not exist yet.

- [ ] **Step 3: Implement**

In `src/crypto.rs`, add `chain: Vec<AgentCertificate>` with its setter and getter, and in
`sign_secure_message`, before computing the signature:

```rust
        if !self.chain.is_empty() {
            message.add_metadata(
                crate::certificate::CHAIN_KEY,
                &crate::certificate::chain_to_pem(&self.chain),
            );
        }
```

In `src/transport/manager.rs`, add `pub certificate: Option<crate::certificate::VerifiedChain>` to
`ReceivedMessage` after `freshness`, and fill it in `receive_messages` from
`store.verified_chain(message)` on the `Admit` path. Update every construction site the compiler
names, including `tests/receiver_acknowledgement.rs`'s hand-built value.

- [ ] **Step 4: Run the tests and check that they pass**

Run: `cargo test --test agent_certificates --test replay_suppression --test receiver_acknowledgement --test sealing --test sender_authentication --test mcp_surface 2>&1 | grep -E "test result"`
Expected: all pass. Then fmt and `cargo clippy --test agent_certificates -- -D warnings`.

- [ ] **Step 5: Commit**

```bash
git add src/crypto.rs src/transport/manager.rs tests/agent_certificates.rs tests/receiver_acknowledgement.rs
git commit -m "feat(cert): attach the chain when signing, and report it on receive (P2f1)"
```

---

### Task 5: Enforce `send`, relay revocations, then verify and open the PR

**Files:**
- Modify: `src/transport/manager.rs`, `src/sender_auth.rs`, `tests/agent_certificates.rs`,
  `CAPABILITY_INVENTORY.md`

**Interfaces:**
- Consumes: Tasks 1-4.
- Produces: the `send` enforcement in `receive_messages`, and revocation relay through
  `REVOCATIONS_KEY` metadata.

- [ ] **Step 1: Write the failing tests** (append to `tests/agent_certificates.rs`, reusing its
  helpers)

```rust
// §10 test 10
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_leaf_without_send_is_refused_at_the_transport() {
    let account = account_key(15);
    let alice = agent();
    // Granted `ack` only: this agent may acknowledge, but may not originate messages.
    let narrowed = cert_for(&account, &alice, "agent@alice.test", vec![Permission::Ack], 6);
    let (bob, port) = udp_node(store_pinning(&account), None).await;
    send_raw(port, &message_with_chain(&alice, "agent@alice.test", &[narrowed], b"denied"));
    assert!(drain(&bob).await.is_empty());
    assert_eq!(bob.knocks().await[0].reason, "no_send_permission");

    // Control: the same agent, same account key, with `send` granted, is delivered.
    let allowed = cert_for(
        &account,
        &alice,
        "agent@alice.test",
        vec![Permission::Send, Permission::Ack],
        7,
    );
    send_raw(port, &message_with_chain(&alice, "agent@alice.test", &[allowed], b"allowed"));
    assert!(receive_one(&bob).await.sender.is_verified());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_relayed_revocation_is_accepted_only_for_a_pinned_account_key() {
    let account = account_key(16);
    let alice = agent();
    let doomed = cert_for(&account, &alice, "agent@alice.test", vec![Permission::Send], 8);
    let serial = doomed.serial;
    let revocation = signed_revocation(&account, serial);

    // Bob pins Alice's account key, so a relayed revocation signed by it is stored.
    let (bob, port) = udp_node(store_pinning(&account), None).await;
    let mut carrier = SecureMessage::new(BOB, "agent@alice.test", b"carrier".to_vec(), SecurityLevel::Authenticated);
    carrier.add_metadata(
        synapse::certificate::CHAIN_KEY,
        &chain_to_pem(&[cert_for(&account, &alice, "agent@alice.test", vec![Permission::Send], 9)]),
    );
    carrier.add_metadata(REVOCATIONS_KEY, &revocation.to_pem());
    alice.sign_secure_message(&mut carrier).expect("sign");
    send_raw(port, &carrier);
    assert!(receive_one(&bob).await.sender.is_verified(), "the carrier itself is fine");
    assert!(bob.trust_store().await.is_revoked(&serial), "the relayed revocation was stored");

    // The revoked certificate is now refused.
    send_raw(port, &message_with_chain(&alice, "agent@alice.test", &[doomed.clone()], b"revoked"));
    assert!(drain(&bob).await.is_empty());

    // Control: a receiver that has NOT pinned Alice stores nothing from the same relay, and the
    // certificate the revocation names keeps verifying under a receiver that pins its own root.
    let other_account = account_key(17);
    let (stranger, stranger_port) = udp_node(store_pinning(&other_account), None).await;
    send_raw(stranger_port, &carrier);
    let _ = drain(&stranger).await;
    assert_eq!(stranger.trust_store().await.revocation_count(), 0);
}
```

- [ ] **Step 2: Run them and check that they fail**

- [ ] **Step 3: Implement**

- `UnverifiableReason` gains `NoSendPermission`, and `verify` returns it when the validated leaf's
  permissions do not contain `Permission::Send`. Putting it in `verify` rather than the transport
  keeps one verdict path, so `synapse-mcp` and every other consumer inherit it.
- In `receive_messages`, after a verified sender, read `REVOCATIONS_KEY` metadata if present, parse
  each block, and offer it to the trust store via a new
  `TransportManager::add_revocations(&[Revocation]) -> usize`, which takes the write lock on the
  trust store. `TrustStore::add_revocation` already refuses anything not signed by a pinned account
  key, so a relay can deliver but never forge.
- Cap the relayed field at `max_chain_bytes` too, before parsing.

- [ ] **Step 4: Run the tests and check that they pass**, then run `agent_certificates` 3 more times.

- [ ] **Step 5: Verification, docs and the PR**

  - **Mutation check.** Predict first: removing the identity-narrowing rule (`identity_narrows`
    always returning `true`) makes ONLY
    `a_child_cannot_claim_an_id_outside_its_parents_namespace` and
    `identity_narrowing_accepts_one_prepended_label_only` fail. Run
    `cargo test --no-fail-fast --lib certificate:: --test agent_certificates`, record the actual
    failing set, compare it with the prediction honestly, restore, and confirm `git status` is clean.
    **Do not filter a `--lib` test name across `--test` targets in one command** — the filter applies
    to every target and silently runs zero integration tests.
  - **Full run** in a **fresh** target directory, with the UTC window recorded, then the Windows
    Firewall event 2097 count for that window and a positive control showing the query finds an
    earlier prompt.
  - `cargo fmt -- --check`, `cargo clippy -- -D warnings`, and clippy on the new test target.
  - **Inventory:** add a dated note beside the slice a, d and e entries: account keys and agent
    certificates are built on branch `feat/agent-certificates` (held); a receiver pins one account
    key instead of every agent; direct pinning still works and is removed in f2; the router's email
    path remains unauthenticated.
  - **Push** and open a **draft** PR into `feat/replay-suppression`, whose body carries: what
    changed, the breaking change (`ReceivedMessage.certificate`), the verification numbers with
    their ref, the mutation result, the stack position, and the f2 scope that follows.
