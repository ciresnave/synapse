# Sender authentication for `SecureMessage` — design (P2 slice a)

**Status:** design for review. Written 2026-09-17 against `main` `8edce9c1`. The PR that implements
it is held until CireSnave answers #33 §11 question 1 or reviews #33 (the PM's ruling, 2026-09-17).

**Decisions already made by the PM (2026-09-17):**
- keys come from a pinned trust store;
- a message without a sender proof fails to parse;
- the signature covers a canonical byte string that excludes `routing_path`;
- the receiver computes the verdict, and it never travels on the wire;
- `TransportManager::receive_messages` returns each message paired with its verdict;
- replay suppression is a later slice.

---

## 1. The requirement

From `CAPABILITY_INVENTORY.md` §2.2, contributed by the OverMind lane:

> a recipient must be able to compute, from the message alone plus keys obtained independently of the
> relay, exactly one of — `VERIFIED` / `UNVERIFIABLE` / `CONTRADICTED`.

> the fourth state is eliminated by the field being MANDATORY in the wire format, not optional. A
> message without it must fail to parse rather than arrive looking ordinary.

**What `main` does today** (measured, inventory §2.2):
- `SecureMessage.signature` is written in one place (`router.rs:88`), over the message content only,
  and read nowhere.
- `from_global_id` is an unchecked string.
- A Python process sent `"signature": []` with a made-up sender, and it was delivered as if genuine.

## 2. Scope

**In this slice:**
- a mandatory `sender_proof` field on `SecureMessage`, replacing the unread `signature` field;
- a canonical byte string for signing, version 1;
- a `TrustStore` of pinned Ed25519 public keys;
- `SenderVerdict`, computed at the receiver;
- `TransportManager::receive_messages` returns `ReceivedMessage { incoming, sender }`;
- signing on the router's send path;
- updating every compiled construction site.

**Not in this slice** (the P2 slice list, in order, as set by the PM):

| slice | what |
|---|---|
| **a** | sender authentication — this document |
| b | receiver acknowledgement (`DeliveryConfirmation::Received` is never constructed today) |
| c | a Rust MCP stdio surface an agent can attach to (send, poll, list) |
| d | **sealing**: real recipient-key encryption (X25519/HPKE, never a key derived from Ed25519). **Publishing 2.0.0 is blocked until this lands.** |
| e | replay suppression: the signed `message_id` and `timestamp` make it possible; nothing checks them yet |
| f | key rotation and revocation for the trust store |

**Left raw on purpose.** These paths still hand out messages without a verdict:
- `Transport::receive_messages` on each transport, which `TransportManager` builds on;
- `UnifiedTransportManager::receive_messages`, whose only user is a test target that does not compile;
- `SynapseRouter::receive_messages`, which reads from email.

Each gets a doc comment saying its output is unverified and naming the verified path.
`src/auth_integration.rs` (not in the build) and `src/auth_integration_enhanced.rs` (known broken)
are not edited.

## 3. Wire format

`SecureMessage` loses `signature: Vec<u8>` and gains a mandatory field:

```rust
pub struct SecureMessage {
    // ...existing fields, unchanged...
    pub sender_proof: SenderProof,   // no #[serde(default)]: absent => parse error
}

pub struct SenderProof {
    pub alg: ProofAlg,     // serde: "ed25519" | "none"; any other value => parse error
    pub key_id: String,    // lowercase hex SHA-256 of the 32-byte public key; "" when alg = none
    pub sig: Vec<u8>,      // 64 bytes for ed25519; empty when alg = none
}
```

In JSON:

```json
"sender_proof": {"alg": "ed25519", "key_id": "3f1c…", "sig": [12, 200, …]}
"sender_proof": {"alg": "none", "key_id": "", "sig": []}
```

`alg: "none"` is how a sender with no key says so. The receiver marks such a message
`UNVERIFIABLE`, so an unsigned message is labelled, never silent. `sig` stays a JSON integer array,
matching every other byte field in `SecureMessage`. This is a breaking wire change, which 2.0.0
already allows.

## 4. What gets signed — canonical input v1

The signature is Ed25519 over the byte string below. Every field is written as a **4-byte big-endian
length followed by its bytes**, in this order:

| # | field | bytes |
|---|---|---|
| 1 | domain tag | ASCII `synapse/sender-proof/v1` |
| 2 | `message_id` | the UUID's 16 raw bytes |
| 3 | `from_global_id` | UTF-8 |
| 4 | `to_global_id` | UTF-8 |
| 5 | `timestamp` | Unix time in **microseconds**, as an 8-byte big-endian signed integer |
| 6 | `security_level` | its serde name in UTF-8 (`public`, `private`, `authenticated`, `secure`) |
| 7 | `sender_proof.key_id` | UTF-8 |
| 8 | payload | SHA-256 of `encrypted_content` (32 bytes) |
| 9 | `metadata` | the field's bytes are a 4-byte count, then each pair as a length-prefixed key and a length-prefixed value, **sorted by key bytes** |

**Not covered: `routing_path`.** Relays append to it, so a signature over it would break at the first
hop. A consumer must treat `routing_path` as the relays' own claim.

**Why microseconds.** The wire timestamp is chrono's RFC 3339 string with nanosecond precision.
Python's `datetime` holds only microseconds, and OverMind's runner is Python. So:
- `sign` truncates the message's timestamp to whole microseconds **before** signing, so the wire value
  and the signed value agree exactly;
- a received timestamp with nonzero sub-microsecond digits is `CONTRADICTED` ("non-canonical
  timestamp"). This keeps "every covered field is exact" true.

**Test vector.** The spec's tests fix an Ed25519 seed and a message, and check the exact canonical
bytes and signature in hex. Ed25519 is deterministic, so a non-Rust implementation can check itself
against the same vector.

## 5. The verdict

```rust
#[must_use]
pub enum SenderVerdict {
    Verified { key_id: String },
    Unverifiable { reason: UnverifiableReason },   // Unsigned | UnknownSender
    Contradicted { reason: ContradictedReason },   // KeyMismatch | BadSignature | NonCanonicalTimestamp
}
```

The receiver applies these rules in order; the first match wins:

| # | condition | verdict |
|---|---|---|
| 1 | `alg` is `none` | `Unverifiable(Unsigned)` |
| 2 | no pinned key for `from_global_id` | `Unverifiable(UnknownSender)` |
| 3 | timestamp has sub-microsecond digits | `Contradicted(NonCanonicalTimestamp)` |
| 4 | `key_id` differs from the pinned key's id | `Contradicted(KeyMismatch)` |
| 5 | `sig` is not 64 bytes, or fails Ed25519 verification against the pinned key | `Contradicted(BadSignature)` |
| 6 | otherwise | `Verified { key_id }` |

**Why rule 4 is a contradiction, not "unverifiable".** With a pinned store and no rotation (slice f),
a different key claiming a pinned identity is exactly what an impersonation looks like. Slice f will
revisit this rule.

**Rule 2 comes before rule 3 on purpose.** An unknown sender's message is labelled unverifiable
whatever else is wrong with it. Nothing about an unknown sender can be contradicted.

## 6. Key source — the pinned trust store

```rust
#[derive(Clone, Default)]
pub struct TrustStore { /* global_id -> [u8; 32] */ }

impl TrustStore {
    pub fn pin(&mut self, global_id: &str, public_key: [u8; 32]);
    pub fn pin_pem(&mut self, global_id: &str, pem: &str) -> Result<()>;  // same PEM as CryptoManager
    pub fn verify(&self, message: &SecureMessage) -> SenderVerdict;
}

pub fn key_id(public_key: &[u8; 32]) -> String;   // lowercase hex SHA-256
```

- Keys are obtained **out of band**: from config, or pinned through the API by the consumer.
- Nothing in a received message can add or change a pinned key.
- `TrustStore` is separate from `CryptoManager` on purpose. `CryptoManager.known_keys` also feeds the
  broken `encrypt_message` (inventory §2.2), and slice d will replace that.
- `TrustStore` has one job, and it is cheap to clone into a manager.

## 7. APIs

**Signing.** On `CryptoManager`, which holds the Ed25519 key pair:

```rust
pub fn sign_secure_message(&self, message: &mut SecureMessage) -> Result<()>;
pub fn public_key_bytes(&self) -> Result<[u8; 32]>;
```

`sign_secure_message` truncates the timestamp to microseconds, sets `key_id`, builds the canonical
input and sets `sig`. It returns an error when no key pair is loaded. It never returns an empty
signature.

**Constructing.** `SecureMessage::new` loses its `signature` parameter and sets `alg: none`.
`SenderProof::unsigned()` exists for struct literals.

**Receiving.**

```rust
pub struct ReceivedMessage {
    pub incoming: IncomingMessage,
    pub sender: SenderVerdict,
}

impl TransportManager {
    pub async fn receive_messages(&self) -> Result<Vec<ReceivedMessage>>;   // was Vec<IncomingMessage>
    pub async fn trust_store(&self) -> TrustStore;                          // snapshot
    pub async fn set_trust_store(&self, store: TrustStore);
}
impl TransportManagerBuilder {
    pub fn trust_store(self, store: TrustStore) -> Self;
}
```

A manager built without a trust store has an empty one, so every message comes back
`Unverifiable(UnknownSender)`: marked, not silent.

**The router.** `SynapseRouter::send_message` currently signs the content alone and falls back to an
empty signature. It will instead:
- call `sign_secure_message`;
- if no key pair is loaded, send `alg: none` and log a warning.

## 8. Code that changes

| file | change |
|---|---|
| `src/sender_auth.rs` (new) | `SenderProof`, `ProofAlg`, canonical input, `TrustStore`, `SenderVerdict`, `key_id` |
| `src/lib.rs` | `pub mod sender_auth;` plus re-exports |
| `src/types.rs` | `signature` becomes `sender_proof`; `SecureMessage::new` loses a parameter |
| `src/crypto.rs` | `sign_secure_message`, `public_key_bytes` |
| `src/transport/manager.rs` | trust store field, builder method, `ReceivedMessage` return |
| `src/router.rs`, `src/router_enhanced.rs`, `src/email.rs`, `src/email_server/smtp_server.rs`, `src/transport/nat_traversal.rs` | construction sites use `SenderProof::unsigned()` or sign |
| `src/transport/abstraction.rs` | doc comments marking the raw receive paths |
| `examples/*` (8 sites in 6 files), `tests/tcp_delivery_probe.rs` | follow the constructor change; `unified_transport_demo.rs` follows the new return type |

## 9. Errors

- **Parse failures.** A missing `sender_proof`, an unknown `alg`, or wrong JSON types are
  deserialization errors. The transport already drops messages it cannot parse; that behaviour does
  not change.
- **Verification never errors.** Every outcome is a verdict. A bad key, signature or timestamp is
  data, not an exception.
- **Signing without a key pair** is an error returned to the caller. It never produces an empty
  signature.

## 10. Tests

All tests go in a new `tests/sender_authentication.rs`, unless noted.

1. **Round trip:** sign, then verify with the sender's key pinned: `Verified`, with the right `key_id`.
2. **Every covered field is covered** (PM requirement). Change each of the following after signing,
   one at a time, and check the verdict is exactly `Contradicted(BadSignature)`:
   - `message_id`, `to_global_id`, `timestamp` (±1 µs), `security_level`;
   - `from_global_id`, changed to a second id **pinned to the same key**. Otherwise rule 4 would
     reject it for `KeyMismatch`, and the row would pass even if `from_global_id` were missing from
     the signed bytes;
   - `encrypted_content`, one byte;
   - metadata: add a pair, change a value, remove a pair.

   Written as a table test that checks the *reason* as well as the verdict. So it fails if the
   canonical input omits any of these fields, and a row cannot pass through a different rule.
   `from_global_id` changed to an *unpinned* id is `Unverifiable(UnknownSender)` by rule 2, and has
   its own case.
3. **`routing_path` is not covered** (PM requirement): append a hop after signing; the verdict stays
   `Verified`.
4. **The unverifiable cases:** `alg: none`, and a correctly signed message from an unpinned sender.
5. **The contradicted cases:**
   - a message signed by key B whose sender is pinned to key A (`KeyMismatch`);
   - a message that claims key A's `key_id` but is signed by B (`BadSignature`);
   - a 63-byte `sig`;
   - a sub-microsecond timestamp.
6. **Parse failures:** JSON with no `sender_proof`, and JSON with `"alg": "rsa"`, both fail to
   deserialize. The old shape, `"signature": []` with no proof, also fails. **This is Probe D's forged
   message, and it must no longer parse.**
7. **JSON round trip keeps the verdict:** serialize, deserialize, still `Verified`.
8. **Test vector:** a fixed seed and message give the exact canonical bytes and signature in hex (§4).
9. **End to end, through a real socket:** two `TransportManager`s over UDP on loopback, built through
   the factory with `bind_port` (the path measured in inventory §2.2). Three checks:
   - the receiver with the sender's key pinned gets `Verified`;
   - a receiver with an empty store gets `Unverifiable(UnknownSender)`;
   - a raw-socket datagram carrying a correctly shaped `sender_proof` with a forged `sig` gets
     `Contradicted(BadSignature)`.
10. **Mutation check, run once and reported in the PR.** Change the canonical input to drop
    `to_global_id`. Test 2's `to_global_id` row must fail and the other rows must pass. This checks
    that test 2 fails for the reason it exists.

CI must keep the failing-test set at `main`'s, `{test_transport_error_handling}`. The new tests all
pass.

## 11. Risks

- **The UDP payload cap.** `sig` as a JSON integer array adds about 250 bytes per message, against a
  measured cap of about 14 KB (inventory §2.2). This is acceptable for this slice; the binary frame
  proposed in #33 §8 removes the cost.
- **Foreign implementations.** The canonical input must be reproduced byte for byte. The test vector
  (§4) is what makes that checkable. The spec, not the Rust code, is the authority.
- **Relays re-serializing messages.** The signature covers field values, not JSON text, so key order
  and whitespace changes are harmless. Changing any covered value is not, and should not be.
