# Sender Authentication Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every `SecureMessage` carries a mandatory sender proof. A receiver computes exactly one of
`Verified`, `Unverifiable` or `Contradicted` from the message plus keys it pinned itself.

**Architecture:** A new `src/sender_auth.rs` holds the wire type (`SenderProof`), the canonical signing
input (v1), the pinned `TrustStore` and the `SenderVerdict` rules. `CryptoManager` signs.
`TransportManager` pairs every received message with a verdict.

**Tech Stack:** Rust 2024 (rustc 1.100.0-nightly), `ring` 0.17.14 (Ed25519), `sha2` 0.10,
`chrono` 0.4.45, `serde`/`serde_json`, `tokio`. Python 3.14 with `cryptography` 50.0.1 is used only
for the independent cross-check in Task 3. It is not committed.

**Spec:** `docs/superpowers/specs/2026-09-17-sender-authentication-design.md`

## Global Constraints

- Branch `feat/sender-authentication`. **Its PR is held** until CireSnave answers #33 §11 Q1 or reviews #33.
- **Prerequisite:** PR #36 (the `TransportManager::start` deadlock fix) must be merged, and this branch
  rebased onto it, before Task 4. Task 4's end-to-end test cannot run otherwise.
- Every new `.rs` file starts with `// SPDX-License-Identifier: MIT OR Apache-2.0`. The CI gate checks this.
- Build and test with `CARGO_TARGET_DIR=C:/Projects/synapse/target` to reuse the cache.
- **The failing-test set must stay `{test_transport_error_handling}`.** Check it with
  `cargo test --no-fail-fast`, and read failures by name, not by count.
- CI's lint form is `cargo clippy -- -D warnings`. Also lint every new test target with
  `cargo clippy --test <name> -- -D warnings`.
- `cargo fmt -- --check` must exit 0.
- Do not edit `src/auth_integration.rs` (not in the build), `src/auth_integration_enhanced.rs` or
  `examples/auth_framework_v3_demo.rs` (both need `enhanced-auth`, which is known broken), or any
  `*.disabled` file.
- The domain tag is exactly `synapse/sender-proof/v1`. `alg` values are exactly `"ed25519"` and `"none"`.
- **No version bump.** How to apply §6.6 day to day is still waiting on CireSnave.

## File map

| file | responsibility |
|---|---|
| `src/sender_auth.rs` (new) | `ProofAlg`, `SenderProof`, `key_id`, `canonical_input`, `truncate_timestamp_to_micros`, `TrustStore`, `SenderVerdict` and its reasons |
| `src/lib.rs` | `pub mod sender_auth;` and re-exports |
| `src/types.rs` | `SecureMessage.signature` becomes `sender_proof`; `SecureMessage::new` drops its signature parameter |
| `src/crypto.rs` | `public_key_bytes`, `sign_secure_message` |
| `src/transport/manager.rs` | trust store on the manager and builder; `ReceivedMessage`; verdicts from `receive_messages` |
| `src/router.rs` | signs on `send_message`; marks `receive_messages` as raw |
| `src/transport/abstraction.rs` | marks the two raw receive paths |
| construction sites | `src/email.rs`, `src/email_server/smtp_server.rs`, `src/router_enhanced.rs` (2), `src/transport/nat_traversal.rs` (2), 8 example sites, `tests/tcp_delivery_probe.rs` |
| `tests/sender_authentication.rs` (new) | spec §10, tests 1–9 |

---

### Task 1: The wire change — a mandatory `sender_proof`

**Files:**
- Create: `src/sender_auth.rs`
- Modify: `src/lib.rs:200` (module list), `src/types.rs:476-510`, `src/router.rs:60-88,207-217`,
  `src/email.rs:62-73`, `src/email_server/smtp_server.rs:383-395`, `src/router_enhanced.rs:670-712`,
  `src/transport/nat_traversal.rs:566-620`, `tests/tcp_delivery_probe.rs:86-92`,
  `examples/basic_unified_transport_test.rs:98-104`, `examples/email_server_demo.rs:77-87`,
  `examples/http_transport_demo.rs:219-233`, `examples/real_nat_traversal_test.rs:115-129`,
  `examples/unified_transport_demo.rs:76,105,220`, `examples/unified_transport_test.rs:79-85`
- Test: `tests/sender_authentication.rs` (created here with the parse-failure tests)

**Interfaces:**
- Produces: `synapse::sender_auth::{ProofAlg, SenderProof}`, where `ProofAlg::{None, Ed25519}`,
  `SenderProof { alg: ProofAlg, key_id: String, sig: Vec<u8> }` and `SenderProof::unsigned() -> SenderProof`.
  `SecureMessage.sender_proof: SenderProof`.
  `SecureMessage::new(to, from, encrypted_content: Vec<u8>, security_level: SecurityLevel)`, now with 4 parameters.

- [ ] **Step 1: Write the failing test.** Create `tests/sender_authentication.rs`:

```rust
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
```

- [ ] **Step 2: Run it and check that it fails.**
Run: `cargo test --test sender_authentication`
Expected: a compile error. `synapse::sender_auth` is unresolved, and `SecureMessage::new` takes 5 arguments.

- [ ] **Step 3: Create `src/sender_auth.rs`** with the wire types only (later tasks append to it):

```rust
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Sender authentication for [`SecureMessage`](crate::types::SecureMessage).
//!
//! Every message carries a mandatory [`SenderProof`]. A receiver turns the proof plus keys it
//! pinned itself into exactly one [`SenderVerdict`]; the verdict is never read from the wire.
//! Design: `docs/superpowers/specs/2026-09-17-sender-authentication-design.md`.

use serde::{Deserialize, Serialize};

/// The signature algorithm a sender used. Any other value fails to parse.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, bincode::Encode, bincode::Decode,
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
```

(The `[`canonical_input`]` doc link resolves once Task 2 adds the function. Until then, write it as
plain backticks, `` `canonical_input` ``, and switch it to a link in Task 2.)

- [ ] **Step 4: Register the module.** In `src/lib.rs`, after `pub mod router_enhanced;` (line 206), add:

```rust
pub mod sender_auth;
```

- [ ] **Step 5: Change `SecureMessage`** in `src/types.rs`:
  - Replace the field `pub signature: Vec<u8>,` with `pub sender_proof: crate::sender_auth::SenderProof,`.
    Add **no** `#[serde(default)]`.
  - In `SecureMessage::new`, delete the `signature: Vec<u8>,` parameter. In the body, replace
    `signature,` with `sender_proof: crate::sender_auth::SenderProof::unsigned(),`.
  - Replace the doc comment `/// Create a new secure message` with
    `/// Create a new, explicitly unsigned message. Sign it with \`CryptoManager::sign_secure_message\`.`

- [ ] **Step 6: Update every compiled construction site.** In each struct literal below, replace the
  `signature: …,` line with `sender_proof: synapse::sender_auth::SenderProof::unsigned(),`. Inside
  `src/`, use `crate::` instead of `synapse::`.
  - `src/email.rs:67` (`signature: vec![],`)
  - `src/email_server/smtp_server.rs:388` (`signature: Vec::new(),`)
  - `src/router.rs:67` and `:211` (`signature: Vec::new(),`)
  - `src/router_enhanced.rs:677` and `:701` (`signature: Vec::new(),`)
  - `src/transport/nat_traversal.rs:571` and `:609` (`signature: Vec::new(),`)
  - `examples/email_server_demo.rs:82`
  - `examples/real_nat_traversal_test.rs:120`
  - `examples/http_transport_demo.rs:224`. This one is `signature: vec![0u8; 64], // Placeholder
    signature`: replace the whole line, comment included.

  In each `SecureMessage::new(...)` call below, delete the signature argument line:
  - `examples/basic_unified_transport_test.rs:102` (`Vec::new(), // empty signature for testing`)
  - `examples/unified_transport_demo.rs:80`, `:110`, `:224` (`vec![], // signature placeholder`)
  - `examples/unified_transport_test.rs:83` (`Vec::new(), // empty signature for now`)
  - `tests/tcp_delivery_probe.rs:90` (`Vec::new(),`)

  In `src/router.rs`, delete the whole block at lines 82-88:

```rust
        // Sign the message
        let signature = {
            let crypto = self.crypto.read().await;
            crypto.sign_message(&simple_msg.content).unwrap_or_default()
        };
        secure_msg.signature = signature;
```

  Task 3 replaces it with real signing. Until then the router's messages go out with `alg: none`,
  which is explicit, rather than an unread content-only signature. The line numbers above are at
  `8edce9c1` and may shift slightly.

- [ ] **Step 7: Check that no other compiled code uses the old field.**
Run: `grep -rn -E 'signature: *(vec!|Vec::)|\.signature\b' src tests examples --include=*.rs | grep -v -E 'blockchain|auth_integration|auth_framework_v3_demo|block\.signature|signature: None'`
Expected: no output. Control: without the final `grep -v`, the same command lists `src/synapse/blockchain/` hits, so the query can find matches.

- [ ] **Step 8: Run the tests and check that they pass.**
Run: `cargo test --test sender_authentication`
Expected: 4 passed.
Run: `cargo build --all-targets 2>&1 | grep -E '^error' ; cargo fmt -- --check`
Expected: no errors; fmt exits 0.

- [ ] **Step 9: Commit.**

```bash
git add src/sender_auth.rs src/lib.rs src/types.rs src/router.rs src/email.rs src/email_server/smtp_server.rs src/router_enhanced.rs src/transport/nat_traversal.rs tests/tcp_delivery_probe.rs tests/sender_authentication.rs examples/basic_unified_transport_test.rs examples/email_server_demo.rs examples/http_transport_demo.rs examples/real_nat_traversal_test.rs examples/unified_transport_demo.rs examples/unified_transport_test.rs
git commit -m "feat(wire)!: SecureMessage carries a mandatory sender_proof"
```

---

### Task 2: Canonical signing input v1

**Files:**
- Modify: `src/sender_auth.rs` (append)
- Test: unit tests inside `src/sender_auth.rs`

**Interfaces:**
- Consumes: `SenderProof`, `SecureMessage.sender_proof` (Task 1).
- Produces:
  - `pub const CANONICAL_DOMAIN_TAG: &[u8]`
  - `pub fn key_id(public_key: &[u8; 32]) -> String`
  - `pub fn canonical_input(message: &SecureMessage) -> Vec<u8>`
  - `pub fn truncate_timestamp_to_micros(message: &mut SecureMessage)`
  - `pub(crate) fn has_sub_micro_digits(message: &SecureMessage) -> bool`

- [ ] **Step 1: Write the failing unit tests.** Append to `src/sender_auth.rs`:

```rust
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
        assert!(id.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
    }

    #[test]
    fn canonical_input_starts_with_the_length_prefixed_domain_tag() {
        let m = SecureMessage::new("b", "a", vec![], SecurityLevel::Public);
        let input = canonical_input(&m);
        assert_eq!(&input[..4], &(CANONICAL_DOMAIN_TAG.len() as u32).to_be_bytes());
        assert_eq!(&input[4..4 + CANONICAL_DOMAIN_TAG.len()], CANONICAL_DOMAIN_TAG);
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
```

- [ ] **Step 2: Run them and check that they fail.**
Run: `cargo test --lib sender_auth`
Expected: a compile error; `security_level_name`, `key_id`, `canonical_input` and the other functions are not found.

- [ ] **Step 3: Implement.** Insert this above the `#[cfg(test)]` module in `src/sender_auth.rs`, and
add the imports at the top:

```rust
use crate::types::{SecureMessage, SecurityLevel};
use chrono::Timelike;
use sha2::{Digest, Sha256};
```

```rust
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

/// The bytes a sender signs (spec §4, v1). Each field is a 4-byte big-endian length followed by its
/// bytes, in this order: domain tag, message_id (16 raw bytes), from_global_id, to_global_id,
/// timestamp (Unix microseconds, i64 BE), security_level (serde name), sender_proof.key_id,
/// SHA-256(encrypted_content), metadata. The metadata field's bytes are a 4-byte count followed
/// by each (key, value) pair, each length-prefixed, sorted by key bytes.
/// `routing_path` is NOT covered: relays append to it.
pub fn canonical_input(message: &SecureMessage) -> Vec<u8> {
    let mut out = Vec::new();
    put(&mut out, CANONICAL_DOMAIN_TAG);
    put(&mut out, message.message_id.0.as_bytes());
    put(&mut out, message.from_global_id.as_bytes());
    put(&mut out, message.to_global_id.as_bytes());
    put(&mut out, &message.timestamp.0.timestamp_micros().to_be_bytes());
    put(&mut out, security_level_name(&message.security_level).as_bytes());
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
    message.timestamp.0.nanosecond() % 1_000 != 0
}
```

Also update the spec's §4 row 9 so it matches the doc comment: *"the field's bytes are a 4-byte count,
then each pair as length-prefixed key and value, sorted by key bytes"*.

(`pairs.sort()` orders `(&String, &String)` tuples by key first, and `String` orders by bytes.
Keys in a `HashMap` are unique, so the value never decides the order.)

- [ ] **Step 4: Run the tests and check that they pass.**
Run: `cargo test --lib sender_auth`
Expected: 5 passed.

- [ ] **Step 5: Commit.**

```bash
git add src/sender_auth.rs docs/superpowers/specs/2026-09-17-sender-authentication-design.md
git commit -m "feat(sender-auth): canonical signing input v1"
```

---

### Task 3: Signing, the trust store and the verdict

**Files:**
- Modify: `src/sender_auth.rs` (append), `src/crypto.rs` (add two methods after `verify_signature`),
  `src/router.rs:82` (signing), `src/lib.rs` (re-exports)
- Test: `tests/sender_authentication.rs` (append tests 1, 2, 4, 5, 7, 8)

**Interfaces:**
- Consumes: `key_id`, `canonical_input`, `truncate_timestamp_to_micros`, `has_sub_micro_digits` (Task 2).
- Produces:
  - `SenderVerdict::{Verified { key_id: String }, Unverifiable { reason: UnverifiableReason }, Contradicted { reason: ContradictedReason }}`
  - `UnverifiableReason::{Unsigned, UnknownSender}`
  - `ContradictedReason::{NonCanonicalTimestamp, KeyMismatch, BadSignature}`
  - `TrustStore::{new, pin(&mut self, impl Into<String>, [u8; 32]), pin_pem(&mut self, &str, &str) -> Result<()>, is_pinned(&self, &str) -> bool, verify(&self, &SecureMessage) -> SenderVerdict}`
  - `CryptoManager::public_key_bytes(&self) -> Result<[u8; 32]>`
  - `CryptoManager::sign_secure_message(&self, &mut SecureMessage) -> Result<()>`

- [ ] **Step 1: Make the fixed test key.** Run a throwaway test once that prints a PKCS#8 v2 PEM from
`CryptoManager::generate_keypair()`, and do the same for a second key. Paste both into
`tests/sender_authentication.rs` as `const ALICE_PEM: &str` and `const BOB_PEM: &str`. Delete the
throwaway test. (Why: ring's `from_pkcs8` needs v2, which Python's `cryptography` does not emit.)

- [ ] **Step 2: Compute the test vector independently, in Python.** In the scratchpad (not committed),
write `vector.py`. It must:
  - take the 32-byte seed from ALICE_PEM's DER at offset 16, after asserting that the 16-byte prefix is
    ring's v2 prefix `3051020101300506032b657004220420`. (Corrected during execution: an earlier draft
    said `3053…`, but the keys ring actually generated begin `3051…`.) Also check that the public key
    after the `812100` tag matches the one derived from the seed;
  - build canonical input v1 **from the spec text alone**, for the vector message in Step 3;
  - print `canonical_hex` and `sig_hex` using
    `cryptography.hazmat.primitives.asymmetric.ed25519.Ed25519PrivateKey.from_private_bytes(seed).sign(...)`;
  - print `alice_key_id` as the SHA-256 hex of the raw public key.

  Paste the three values into the test as `EXPECTED_CANONICAL_HEX`, `EXPECTED_SIG_HEX` and
  `EXPECTED_ALICE_KEY_ID`. **They come from Python, not from the Rust code under test.** The Rust
  result agreeing with them is agreement between two independent implementations.

- [ ] **Step 3: Write the failing tests.** Append to `tests/sender_authentication.rs` (and extend its
`use` lines):

```rust
use synapse::blockchain::serialization::{DateTimeWrapper, UuidWrapper};
use synapse::sender_auth::{
    canonical_input, key_id, ContradictedReason, SenderVerdict, TrustStore, UnverifiableReason,
};
use synapse::CryptoManager;

const ALICE_PEM: &str = "<pasted in Step 1>";
const BOB_PEM: &str = "<pasted in Step 1>";
const EXPECTED_CANONICAL_HEX: &str = "<pasted in Step 2>";
const EXPECTED_SIG_HEX: &str = "<pasted in Step 2>";
const EXPECTED_ALICE_KEY_ID: &str = "<pasted in Step 2>";

fn signer(pem: &str) -> CryptoManager {
    let mut crypto = CryptoManager::new();
    crypto.load_private_key(pem).expect("fixed test key loads");
    crypto
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The fixed message of spec §4's test vector.
fn vector_message() -> SecureMessage {
    let mut m = unsigned_message();
    m.message_id = UuidWrapper::new(
        uuid::Uuid::parse_str("00112233-4455-6677-8899-aabbccddeeff").unwrap(),
    );
    m.timestamp = DateTimeWrapper::new(
        chrono::DateTime::parse_from_rfc3339("2026-09-17T04:00:00.123456Z")
            .unwrap()
            .with_timezone(&chrono::Utc),
    );
    m.add_metadata("topic", "greeting");
    m.add_metadata("lane", "synapse");
    m
}

/// Alice's key, pinned under her id and under an alias, so a from_global_id change is caught by
/// the signature alone (spec §10 test 2).
fn store_with_alice() -> TrustStore {
    let alice = signer(ALICE_PEM).public_key_bytes().unwrap();
    let mut store = TrustStore::new();
    store.pin("alice@synapse.test", alice);
    store.pin("alice-alias@synapse.test", alice);
    store
}

fn signed_by(pem: &str) -> SecureMessage {
    let mut m = vector_message();
    signer(pem).sign_secure_message(&mut m).expect("signs");
    m
}

// §10 test 1
#[test]
fn a_signed_message_verifies() {
    assert_eq!(
        store_with_alice().verify(&signed_by(ALICE_PEM)),
        SenderVerdict::Verified { key_id: EXPECTED_ALICE_KEY_ID.to_string() }
    );
}

// §10 test 2: every covered field is covered; the reason must be BadSignature, not another rule.
#[test]
fn changing_any_covered_field_contradicts_the_signature() {
    type Mutation = (&'static str, fn(&mut SecureMessage));
    let rows: Vec<Mutation> = vec![
        ("message_id", |m| m.message_id = UuidWrapper::new(uuid::Uuid::new_v4())),
        ("to_global_id", |m| m.to_global_id.push('x')),
        ("from_global_id (same-key alias)", |m| {
            m.from_global_id = "alice-alias@synapse.test".to_string()
        }),
        ("timestamp +1us", |m| {
            m.timestamp = DateTimeWrapper::new(m.timestamp.0 + chrono::TimeDelta::microseconds(1))
        }),
        ("security_level", |m| m.security_level = SecurityLevel::Public),
        ("encrypted_content", |m| m.encrypted_content[0] ^= 1),
        ("metadata add", |m| {
            m.metadata.insert("extra".to_string(), "1".to_string());
        }),
        ("metadata change", |m| {
            m.metadata.insert("topic".to_string(), "other".to_string());
        }),
        ("metadata remove", |m| {
            m.metadata.remove("topic");
        }),
    ];
    let store = store_with_alice();
    let mut wrong = Vec::new();
    for (name, mutate) in rows {
        let mut m = signed_by(ALICE_PEM);
        mutate(&mut m);
        let verdict = store.verify(&m);
        if verdict != (SenderVerdict::Contradicted { reason: ContradictedReason::BadSignature }) {
            wrong.push(format!("{name}: {verdict:?}"));
        }
    }
    assert!(wrong.is_empty(), "fields not covered by the signature: {wrong:#?}");
}

// §10 test 3 (routing_path) lives in Task 4 with the transport test, and here:
#[test]
fn appending_to_routing_path_keeps_the_signature_valid() {
    let mut m = signed_by(ALICE_PEM);
    m.add_routing_hop("relay-1@synapse.test");
    assert!(store_with_alice().verify(&m).is_verified());
}

// §10 test 4
#[test]
fn unsigned_and_unknown_senders_are_unverifiable() {
    let store = store_with_alice();
    assert_eq!(
        store.verify(&vector_message()),
        SenderVerdict::Unverifiable { reason: UnverifiableReason::Unsigned }
    );
    let mut m = vector_message();
    m.from_global_id = "carol@synapse.test".to_string();
    signer(ALICE_PEM).sign_secure_message(&mut m).unwrap();
    assert_eq!(
        store.verify(&m),
        SenderVerdict::Unverifiable { reason: UnverifiableReason::UnknownSender }
    );
}

// §10 test 5
#[test]
fn the_contradicted_cases() {
    let store = store_with_alice();
    let contradicted = |reason| SenderVerdict::Contradicted { reason };

    // Bob's key presented for Alice's pinned id.
    assert_eq!(store.verify(&signed_by(BOB_PEM)), contradicted(ContradictedReason::KeyMismatch));

    // Alice's key_id claimed, Bob's signature.
    let mut m = signed_by(BOB_PEM);
    m.sender_proof.key_id = EXPECTED_ALICE_KEY_ID.to_string();
    assert_eq!(store.verify(&m), contradicted(ContradictedReason::BadSignature));

    // A 63-byte signature.
    let mut m = signed_by(ALICE_PEM);
    m.sender_proof.sig.pop();
    assert_eq!(store.verify(&m), contradicted(ContradictedReason::BadSignature));

    // A sub-microsecond timestamp.
    let mut m = signed_by(ALICE_PEM);
    m.timestamp = DateTimeWrapper::new(m.timestamp.0 + chrono::TimeDelta::nanoseconds(1));
    assert_eq!(store.verify(&m), contradicted(ContradictedReason::NonCanonicalTimestamp));
}

// §10 test 7
#[test]
fn a_json_round_trip_keeps_the_verdict() {
    let json = serde_json::to_string(&signed_by(ALICE_PEM)).unwrap();
    let back: SecureMessage = serde_json::from_str(&json).unwrap();
    assert!(store_with_alice().verify(&back).is_verified());
}

// §10 test 8: the values were computed by an independent Python implementation of spec §4.
#[test]
fn the_test_vector_matches_an_independent_implementation() {
    let alice = signer(ALICE_PEM);
    assert_eq!(key_id(&alice.public_key_bytes().unwrap()), EXPECTED_ALICE_KEY_ID);
    let m = signed_by(ALICE_PEM);
    assert_eq!(hex(&canonical_input(&m)), EXPECTED_CANONICAL_HEX);
    assert_eq!(hex(&m.sender_proof.sig), EXPECTED_SIG_HEX);
}

#[test]
fn signing_without_a_key_is_an_error_not_an_empty_signature() {
    let mut m = vector_message();
    assert!(CryptoManager::new().sign_secure_message(&mut m).is_err());
    assert_eq!(m.sender_proof, SenderProof::unsigned());
}
```

Add `uuid` and `chrono` to the test's reach. They are normal dependencies of the crate, so an
integration test can use them directly as `uuid::` and `chrono::`.

- [ ] **Step 4: Run the tests and check that they fail.**
Run: `cargo test --test sender_authentication`
Expected: a compile error; `TrustStore`, `SenderVerdict`, `sign_secure_message` and `public_key_bytes` are unresolved.

- [ ] **Step 5: Implement the verdict and the trust store.** Append to `src/sender_auth.rs`, above
the tests module, and add `use ring::signature::{UnparsedPublicKey, ED25519};` and
`use std::collections::HashMap;` to the imports:

```rust
/// What a receiver concluded about a message's sender. Computed locally; never read from the wire.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SenderVerdict {
    Verified { key_id: String },
    Unverifiable { reason: UnverifiableReason },
    Contradicted { reason: ContradictedReason },
}

impl SenderVerdict {
    pub fn is_verified(&self) -> bool {
        matches!(self, SenderVerdict::Verified { .. })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnverifiableReason {
    /// The sender declared `alg: none`.
    Unsigned,
    /// No key is pinned for `from_global_id`.
    UnknownSender,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContradictedReason {
    /// The timestamp has sub-microsecond digits, which v1 never signs.
    NonCanonicalTimestamp,
    /// The proof names a key other than the one pinned for this sender.
    KeyMismatch,
    /// The signature is malformed or does not verify against the pinned key.
    BadSignature,
}

/// Ed25519 public keys pinned out of band, by global id. Nothing in a received message can add or
/// change an entry.
#[derive(Debug, Clone, Default)]
pub struct TrustStore {
    keys: HashMap<String, [u8; 32]>,
}

impl TrustStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn pin(&mut self, global_id: impl Into<String>, public_key: [u8; 32]) {
        self.keys.insert(global_id.into(), public_key);
    }

    /// Pin a key in the PEM form `CryptoManager::get_public_key_pem` produces.
    pub fn pin_pem(&mut self, global_id: &str, pem: &str) -> crate::error::Result<()> {
        let block = pem::parse(pem)
            .map_err(|e| crate::error::CryptoError::InvalidKey(e.to_string()))?;
        let key: [u8; 32] = block.contents().try_into().map_err(|_| {
            crate::error::CryptoError::InvalidKey(
                "Ed25519 public key must be exactly 32 bytes".to_string(),
            )
        })?;
        self.pin(global_id, key);
        Ok(())
    }

    pub fn is_pinned(&self, global_id: &str) -> bool {
        self.keys.contains_key(global_id)
    }

    /// Spec §5: the rules apply in order and the first match wins.
    pub fn verify(&self, message: &SecureMessage) -> SenderVerdict {
        let proof = &message.sender_proof;
        match proof.alg {
            ProofAlg::None => {
                return SenderVerdict::Unverifiable { reason: UnverifiableReason::Unsigned };
            }
            ProofAlg::Ed25519 => {}
        }
        let Some(pinned) = self.keys.get(&message.from_global_id) else {
            return SenderVerdict::Unverifiable { reason: UnverifiableReason::UnknownSender };
        };
        if has_sub_micro_digits(message) {
            return SenderVerdict::Contradicted {
                reason: ContradictedReason::NonCanonicalTimestamp,
            };
        }
        let pinned_id = key_id(pinned);
        if proof.key_id != pinned_id {
            return SenderVerdict::Contradicted { reason: ContradictedReason::KeyMismatch };
        }
        if proof.sig.len() != 64 {
            return SenderVerdict::Contradicted { reason: ContradictedReason::BadSignature };
        }
        match UnparsedPublicKey::new(&ED25519, pinned).verify(&canonical_input(message), &proof.sig) {
            Ok(()) => SenderVerdict::Verified { key_id: pinned_id },
            Err(_) => SenderVerdict::Contradicted { reason: ContradictedReason::BadSignature },
        }
    }
}
```

Check that `crate::error::CryptoError` converts into `crate::error::SynapseError` with `?`.
`crypto.rs` already does `.map_err(|e| CryptoError::InvalidKey(..))?` in a function returning
`crate::error::Result`, so the conversion exists.

- [ ] **Step 6: Implement signing.** In `src/crypto.rs`, after `verify_signature`, add:

```rust
    /// Our Ed25519 public key as raw bytes.
    pub fn public_key_bytes(&self) -> Result<[u8; 32]> {
        let key_pair = self
            .key_pair
            .as_ref()
            .ok_or_else(|| CryptoError::KeyNotFound("No private key loaded".to_string()))?;
        let mut out = [0u8; 32];
        out.copy_from_slice(key_pair.public_key().as_ref());
        Ok(out)
    }

    /// Sign `message` as its sender (spec §7). Truncates the timestamp to whole microseconds, sets
    /// the proof's `key_id`, and signs canonical input v1. Errors if no key pair is loaded; never
    /// leaves an empty signature behind.
    pub fn sign_secure_message(&self, message: &mut crate::types::SecureMessage) -> Result<()> {
        use crate::sender_auth::{canonical_input, key_id, truncate_timestamp_to_micros};
        use crate::sender_auth::{ProofAlg, SenderProof};

        let key_pair = self
            .key_pair
            .as_ref()
            .ok_or_else(|| CryptoError::KeyNotFound("No private key loaded".to_string()))?;
        let public = self.public_key_bytes()?;
        truncate_timestamp_to_micros(message);
        message.sender_proof = SenderProof {
            alg: ProofAlg::Ed25519,
            key_id: key_id(&public),
            sig: Vec::new(),
        };
        let signature = key_pair.sign(&canonical_input(message));
        message.sender_proof.sig = signature.as_ref().to_vec();
        Ok(())
    }
```

- [ ] **Step 7: Sign on the router's send path.** In `src/router.rs`, where Task 1 deleted the old
block (just after the encryption block in `send_message`), insert:

```rust
        // Sign as the sender. Without a key pair the message goes out explicitly unsigned
        // (alg "none"), which receivers mark Unverifiable — never silently.
        {
            let crypto = self.crypto.read().await;
            if let Err(e) = crypto.sign_secure_message(&mut secure_msg) {
                warn!("Sending {} unsigned: {}", secure_msg.message_id, e);
            }
        }
```

Signing comes after encryption, so the signature covers the bytes that are actually sent. This path
sends through email and has no test. It is covered by compilation only, and the PR says so.

- [ ] **Step 8: Re-export.** In `src/lib.rs`, next to `pub use crypto::CryptoManager;`, add:

```rust
pub use sender_auth::{SenderProof, SenderVerdict, TrustStore};
```

- [ ] **Step 9: Run the tests and check that they pass.**
Run: `cargo test --test sender_authentication && cargo test --lib sender_auth`
Expected: all pass: 12 in the integration file (4 from Task 1, 8 from this task) and 5 unit tests.
If the vector test fails while the others pass, **do not change the expected values to match Rust.**
Find which implementation departs from spec §4, because a disagreement is exactly what the vector
exists to catch.

- [ ] **Step 10: Commit.**

```bash
git add src/sender_auth.rs src/crypto.rs src/router.rs src/lib.rs tests/sender_authentication.rs
git commit -m "feat(sender-auth): sign as the sender, verify against pinned keys"
```

---

### Task 4: Verdicts from `TransportManager`, end to end over UDP

**Prerequisite:** PR #36 merged, then `git fetch && git rebase origin/main` on this branch.

**Files:**
- Modify: `src/transport/manager.rs` (struct at `:139`, `new` at `:233`, `receive_messages` at `:445`,
  builder at `:932-990`), `src/transport/abstraction.rs:40-41` and `:1094-1095` (doc comments),
  `src/router.rs:105-106` (doc comment), `examples/unified_transport_demo.rs:192-203`
- Test: `tests/sender_authentication.rs` (append test 9)

**Interfaces:**
- Consumes: `TrustStore`, `SenderVerdict` (Task 3); `CryptoManager::sign_secure_message` (Task 3).
- Produces:
  - `synapse::transport::ReceivedMessage { pub incoming: IncomingMessage, pub sender: SenderVerdict }`
  - `TransportManager::receive_messages(&self) -> Result<Vec<ReceivedMessage>>`
  - `TransportManager::trust_store(&self) -> TrustStore` (async)
  - `TransportManager::set_trust_store(&self, TrustStore)` (async)
  - `TransportManagerBuilder::trust_store(self, TrustStore) -> Self`

- [ ] **Step 1: Write the failing end-to-end test.** Append to `tests/sender_authentication.rs`:

```rust
use std::collections::HashMap;
use std::time::Duration;
use synapse::transport::{
    ReceivedMessage, TransportManager, TransportManagerBuilder, TransportTarget, TransportType,
    UdpTransportFactory,
};

fn free_udp_port() -> u16 {
    let socket = std::net::UdpSocket::bind("127.0.0.1:0").expect("bind ephemeral");
    socket.local_addr().expect("local_addr").port()
}

async fn udp_manager(port: u16, store: TrustStore) -> TransportManager {
    let mut udp = HashMap::new();
    udp.insert("bind_port".to_string(), port.to_string());
    let manager = TransportManagerBuilder::new()
        .disable_transport(TransportType::Tcp)
        .disable_transport(TransportType::Http)
        .disable_transport(TransportType::Email)
        .disable_transport(TransportType::AutoDiscovery)
        .transport_config(TransportType::Udp, udp)
        .trust_store(store)
        .build();
    manager.register_factory(Box::new(UdpTransportFactory)).await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), manager.start())
        .await
        .expect("start() returns (needs PR #36)")
        .expect("start() succeeds");
    manager
}

/// Poll one reader for up to 2 s. `receive_messages` drains, so there must be a single reader.
async fn receive_one(manager: &TransportManager) -> ReceivedMessage {
    for _ in 0..20 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        let mut batch = manager.receive_messages().await.expect("receive");
        if let Some(first) = batch.pop() {
            assert!(batch.is_empty(), "expected exactly one message");
            return first;
        }
    }
    panic!("no message arrived within 2 s");
}

// §10 test 9 (and test 3 over a real socket: the transport path does not break the signature)
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn verdicts_survive_a_real_udp_hop() {
    let (sender_port, trusting_port, empty_port) = (free_udp_port(), free_udp_port(), free_udp_port());
    let sender = udp_manager(sender_port, TrustStore::new()).await;
    let trusting = udp_manager(trusting_port, store_with_alice()).await;
    let empty = udp_manager(empty_port, TrustStore::new()).await;

    let message = signed_by(ALICE_PEM);
    for port in [trusting_port, empty_port] {
        let target = TransportTarget::new("bob@synapse.test".to_string())
            .with_address(format!("127.0.0.1:{port}"));
        sender.send_message(&target, &message).await.expect("send");
    }

    let got = receive_one(&trusting).await;
    assert_eq!(got.incoming.message.encrypted_content, message.encrypted_content);
    assert!(got.sender.is_verified(), "pinned sender should verify: {:?}", got.sender);

    assert_eq!(
        receive_one(&empty).await.sender,
        SenderVerdict::Unverifiable { reason: UnverifiableReason::UnknownSender }
    );

    // A forged datagram: a correctly shaped proof whose signature bytes were altered.
    let mut forged = signed_by(ALICE_PEM);
    forged.sender_proof.sig[0] ^= 1;
    let raw = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    raw.send_to(&serde_json::to_vec(&forged).unwrap(), ("127.0.0.1", trusting_port))
        .unwrap();
    assert_eq!(
        receive_one(&trusting).await.sender,
        SenderVerdict::Contradicted { reason: ContradictedReason::BadSignature }
    );
}
```

- [ ] **Step 2: Run it and check that it fails.**
Run: `cargo test --test sender_authentication verdicts_survive`
Expected: a compile error; `ReceivedMessage` and `TransportManagerBuilder::trust_store` are unresolved.

- [ ] **Step 3: Implement in `src/transport/manager.rs`.**
  - Add the import: `use crate::sender_auth::{SenderVerdict, TrustStore};`
  - Above `pub struct TransportManager`, add:

```rust
/// A received message together with what this node concluded about its sender.
#[derive(Debug, Clone)]
pub struct ReceivedMessage {
    pub incoming: IncomingMessage,
    pub sender: SenderVerdict,
}
```

  - Add a field to `TransportManager`:
    `/// Pinned sender keys; see crate::sender_auth.` and `trust_store: TokioRwLock<TrustStore>,`
  - In `new`, add `trust_store: TokioRwLock::new(TrustStore::default()),`
  - Add these methods next to `receive_messages`:

```rust
    /// A snapshot of the pinned sender keys.
    pub async fn trust_store(&self) -> TrustStore {
        self.trust_store.read().await.clone()
    }

    /// Replace the pinned sender keys.
    pub async fn set_trust_store(&self, store: TrustStore) {
        *self.trust_store.write().await = store;
    }
```

  - Change `receive_messages` to return `Result<Vec<ReceivedMessage>>`. Replace its final
    `Ok(all_messages)` with:

```rust
        let store = self.trust_store.read().await;
        Ok(all_messages
            .into_iter()
            .map(|incoming| {
                let sender = store.verify(&incoming.message);
                ReceivedMessage { incoming, sender }
            })
            .collect())
```

  and change its doc comment to:
  `/// Receive messages from all active transports, each paired with a sender verdict.`

  - Builder: add the field `trust_store: TrustStore` to `TransportManagerBuilder`, and
    `trust_store: TrustStore::default(),` to `TransportManagerBuilder::new`. Add:

```rust
    pub fn trust_store(mut self, store: TrustStore) -> Self {
        self.trust_store = store;
        self
    }
```

  and change `build` to:

```rust
    pub fn build(self) -> TransportManager {
        let mut manager = TransportManager::new(self.config);
        manager.trust_store = TokioRwLock::new(self.trust_store);
        manager
    }
```

- [ ] **Step 4: Mark the raw receive paths.** Put this doc line directly above each of
`Transport::receive_messages` (`abstraction.rs:41`), `UnifiedTransportManager::receive_messages`
(`abstraction.rs:1095`) and `SynapseRouter::receive_messages` (`router.rs:106`), keeping each
existing doc line:

```rust
    /// ⚠️ Unverified: senders are not authenticated here. `TransportManager::receive_messages`
    /// pairs each message with a `SenderVerdict`.
```

- [ ] **Step 5: Update the demo.** In `examples/unified_transport_demo.rs`, replace the loop body at
`:196-202` with:

```rust
                for received in messages {
                    let msg = &received.incoming;
                    let content_str = String::from_utf8_lossy(&msg.message.encrypted_content);
                    info!(
                        "  Message from {} via {:?} (sender: {:?}): {}",
                        msg.source, msg.transport_type, received.sender, content_str
                    );
                }
```

- [ ] **Step 6: Run the tests and check that they pass.**
Run: `cargo test --test sender_authentication`
Expected: 13 passed.
Known flake source, not fixed here: the UDP receive loop uses `try_lock` and silently drops a
datagram that arrives while `receive_messages` holds the queue lock (`udp_unified.rs`). If a run
fails with "no message arrived", rerun once and record it in the PR.

- [ ] **Step 7: Commit.**

```bash
git add src/transport/manager.rs src/transport/abstraction.rs src/router.rs examples/unified_transport_demo.rs tests/sender_authentication.rs
git commit -m "feat(transport): TransportManager pairs every received message with a sender verdict"
```

---

### Task 5: Mutation check, full verification, inventory, branch

**Files:**
- Modify: `CAPABILITY_INVENTORY.md` (§2.2 sender-authentication entry: add a dated "built on branch" note)

- [ ] **Step 1: Mutation check (spec §10 test 10).** Predict the result before running it:
**exactly** the `to_global_id` row of `changing_any_covered_field_contradicts_the_signature` fails,
and so does `the_test_vector_matches_an_independent_implementation`. Nothing else fails. In
`canonical_input`, comment out `put(&mut out, message.to_global_id.as_bytes());`.
Before running, assert with `grep -c` that exactly one line matches.
Run: `cargo test --test sender_authentication 2>&1 | grep -E 'FAILED|to_global_id'`
Record the output verbatim for the PR. Restore the line, and check with `git diff --stat` that
nothing changed.

- [ ] **Step 2: Full verification.**

```bash
cargo fmt -- --check
cargo clippy -- -D warnings
cargo clippy --test sender_authentication -- -D warnings
cargo test --no-fail-fast 2>&1 | tee ../p2a-full.log
grep -oE 'test [A-Za-z0-9_:]+ \.\.\. FAILED' ../p2a-full.log | sort -u
```

Expected:
- fmt and both clippy runs exit 0;
- the failing set is exactly `test test_transport_error_handling ... FAILED`;
- the named `... ok` count equals the #36 baseline (108) plus the new tests (13 integration + 5 unit = 126);
- doc-tests still pass.

- [ ] **Step 3: Inventory.** Under §2.2's "NO MESSAGE'S SENDER IS EVER AUTHENTICATED" heading, add a
dated note. It must say that the fix is **built on branch `feat/sender-authentication`, not on
`main`**, and that the heading stays true of `main` until the held PR merges.

- [ ] **Step 4: Push, and report to the PM.** Push the branch. Open the PR as a **draft**, titled
`feat!: sender authentication (P2 slice a) — HELD for #33 §11 Q1`. The body must include the
mutation output, the test-vector method, and the untested router path. Send the PM a `[READY]`
with `note: HELD per PM until CireSnave answers #33 §11 Q1`.

```bash
git add CAPABILITY_INVENTORY.md
git commit -m "docs(inventory): sender authentication built on branch (held)"
git push -u origin feat/sender-authentication
```
