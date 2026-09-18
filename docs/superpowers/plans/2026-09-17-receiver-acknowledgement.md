# Receiver Acknowledgement Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A receiving application acknowledges a processed message with a signed ack. The sender's
`TransportManager` upgrades that message to `Acknowledged` only when the ack is verified and matches
what was sent.

**Architecture:** A new `src/delivery_ack.rs` holds the reserved metadata keys, `message_digest`,
`build_ack` and the parsing helpers. `SecureMessage::request_ack` sets the signed reply-to address.
`TransportManager` tracks messages that request an ack, takes acks out of `receive_messages`, and
exposes `acknowledge` and `delivery_status`.

**Tech Stack:** Rust 2024, `sha2`, `tokio`, slice a's `sender_auth` (branch `feat/sender-authentication`).

**Spec:** `docs/superpowers/specs/2026-09-17-receiver-acknowledgement-design.md`

## Global Constraints

- Branch `feat/receiver-ack`, stacked on `feat/sender-authentication`. **Draft PR, held** with #37.
- New `.rs` files start with `// SPDX-License-Identifier: MIT OR Apache-2.0`.
- `CARGO_TARGET_DIR=C:/Projects/synapse/target`.
- The failing-test set must stay `{test_transport_error_handling}` under `cargo test --no-fail-fast`.
- `cargo fmt -- --check`, `cargo clippy -- -D warnings` and
  `cargo clippy --test receiver_acknowledgement -- -D warnings` must all exit 0.
- The metadata keys are exactly `synapse.reply_to`, `synapse.ack.for` and `synapse.ack.digest`.
- No version bump: one bump when P2 completes.

---

### Task 1: `delivery_ack` module and `request_ack`

**Files:**
- Create: `src/delivery_ack.rs`
- Modify: `src/lib.rs` (add `pub mod delivery_ack;` after `pub mod crypto;`),
  `src/types.rs` (add `request_ack` to `impl SecureMessage`),
  `src/sender_auth.rs` (extract `pub(crate) fn to_hex`)

**Interfaces — produces:**
- `delivery_ack::{REPLY_TO_KEY, ACK_FOR_KEY, ACK_DIGEST_KEY}`, all `&str`
- `message_digest(&SecureMessage) -> String`
- `reply_to(&SecureMessage) -> Option<&str>`
- `is_ack(&SecureMessage) -> bool`
- `AckFields<'a> { for_message_id: &'a str, digest: &'a str }` and
  `ack_fields(&SecureMessage) -> Option<AckFields<'_>>`
- `build_ack(&SecureMessage, &CryptoManager) -> Result<SecureMessage>`
- `SecureMessage::request_ack(&mut self, impl Into<String>)`

- [ ] **Step 1: Write the failing unit test**, at the bottom of the new `src/delivery_ack.rs`
  (spec §8 test 1):

```rust
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
        let mut original = SecureMessage::new("bob@t", "alice@t", b"hi".to_vec(), SecurityLevel::Authenticated);
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
```

- [ ] **Step 2: Run it and check that it fails.**
  Run: `cargo test --lib delivery_ack`. Expected: a compile error; the module and its functions don't exist yet.

- [ ] **Step 3: Implement.** Put this at the top of `src/delivery_ack.rs`:

```rust
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

/// Lowercase hex SHA-256 of a message's canonical input — exactly what an ack attests to.
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
```

In `src/sender_auth.rs`, replace the body of `key_id` with `to_hex(&Sha256::digest(public_key))`,
and add:

```rust
pub(crate) fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
```

In `src/types.rs`, inside `impl SecureMessage`, after `add_metadata`:

```rust
    /// Ask the receiver to acknowledge this message at `reply_to` (P2 slice b). Call this BEFORE
    /// signing: the address is a signed field.
    pub fn request_ack(&mut self, reply_to: impl Into<String>) {
        self.add_metadata(crate::delivery_ack::REPLY_TO_KEY, reply_to);
    }
```

- [ ] **Step 4: Run the tests and check that they pass.**
  Run: `cargo test --lib delivery_ack && cargo test --lib sender_auth`. Expected: 1 and 5 tests pass.
  Then run `cargo test --test sender_authentication`: the test vector must still pass, since `to_hex`
  must not change `key_id`.

- [ ] **Step 5: Commit** — `feat(ack): delivery_ack module and SecureMessage::request_ack`.

---

### Task 2: `TransportManager` — tracking, taking acks out, `acknowledge`, `delivery_status`

**Files:**
- Modify: `src/transport/manager.rs`, `src/transport/abstraction.rs:223-226` (remove `Received`, re-document `Acknowledged`)
- Test: `tests/receiver_acknowledgement.rs` (new)

**Interfaces:**
- Consumes: everything from Task 1; `ReceivedMessage`, `TrustStore` and `SenderVerdict` from slice a.
- Produces:
  - `TransportManager::acknowledge(&self, &ReceivedMessage, &CryptoManager) -> Result<DeliveryReceipt>`
  - `TransportManager::delivery_status(&self, &str) -> Option<DeliveryConfirmation>`

- [ ] **Step 1: Write the failing tests.** Create `tests/receiver_acknowledgement.rs` with spec §8
  tests 2–10 (the full code is in the file). Its helpers:
  - `identity()`
  - `node(store) -> (TransportManager, port)`, a UDP manager built as in slice a's test
  - `request(from_crypto, from, to, reply_port) -> SecureMessage`, which calls `request_ack` and then signs
  - `receive_one`
  - `settle(manager, id, canary) -> (status, app_messages)`, which pumps `receive_messages` until the
    canary message id appears among the application messages (spec §9), up to 3 s
  - `send_raw(port, &SecureMessage)`

- [ ] **Step 2: Run them and check that they fail.**
  Expected: a compile error; `request_ack` exists after Task 1, but `acknowledge` and
  `delivery_status` don't yet.

- [ ] **Step 3: Implement in `manager.rs`:**
  - Imports: `use crate::crypto::CryptoManager;`, `use crate::delivery_ack;`,
    `use crate::error::SynapseError;`.
  - A private type `struct Outbound { to_global_id: String, digest: String, status: DeliveryConfirmation }`.
  - A field on `TransportManager`: `outbound: TokioRwLock<HashMap<String, Outbound>>`. Initialize it
    in `new`.
  - In `send_message`, in the `Ok(receipt)` arm before `return Ok(receipt);`, add
    `self.track_if_ack_requested(message).await;`.
  - Add `track_if_ack_requested`. It uses `entry(id).or_insert(..)`, so resending a message never
    downgrades an `Acknowledged` entry.
  - Add `delivery_status`, documented as spec §6 says ("Pumping").
  - At the end of `receive_messages`: verify each message. If `delivery_ack::is_ack`, call
    `self.apply_ack(&msg, &verdict).await` and `continue`. Otherwise push the `ReceivedMessage`.
  - Add `apply_ack`, which applies spec §6's conditions 1–4 in order. Each failure is `debug!` plus
    `return`. On success, set `entry.status = DeliveryConfirmation::Acknowledged`.
  - Add `acknowledge`, which applies spec §5's refusals 1–3 in order with the errors the spec names.
    Then it calls `delivery_ack::build_ack(original, signer)?`, builds
    `TransportTarget::new(original.from_global_id.clone()).with_address(reply_to.to_string())`, and
    calls `self.send_message(&target, &ack).await`.

  In `abstraction.rs`, delete the `Received` variant and its doc line. Change `Acknowledged`'s doc to
  the spec §7 text.

- [ ] **Step 4: Run the tests and check that they pass.**
  Run: `cargo test --test receiver_acknowledgement` (all pass), then `cargo test --test sender_authentication`
  (still 13), then `grep -rn 'DeliveryConfirmation::Received' src tests examples` (no output).

- [ ] **Step 5: Commit** — `feat(transport)!: receiver acknowledgement through TransportManager`.

---

### Task 3: Mutation check, full verification, branch

- [ ] **Step 1: Mutation check (spec §8 test 11).** Predict first: **only**
  `a_wrong_digest_changes_nothing` fails. In `apply_ack`, make the digest comparison inert, after
  asserting with `grep -c` that the anchor matches exactly once. Run
  `cargo test --test receiver_acknowledgement`. Record the output, restore the line, and check that
  `git status` is clean.
- [ ] **Step 2: Full verification** with the Global Constraints commands. Expected: named `... ok` =
  126 + 1 unit test + the new integration tests; failing set unchanged; doc-tests still 25.
- [ ] **Step 3: Inventory.** Add a dated note under §2.2's "EVERY DELIVERY CONFIRMATION SYNAPSE
  PRODUCES IS SENDER-SIDE" heading. It must say that a receiver-derived ack is built on branch
  `feat/receiver-ack` (held with #37), and that the heading stays true of `main` until then.
- [ ] **Step 4: Push and report.** Push, then open a draft PR against `feat/sender-authentication`
  (stacked), titled `feat!: receiver acknowledgement (P2 slice b) — HELD with #37`. Send the PM a
  `[READY]` with the HELD note.
