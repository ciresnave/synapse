# Sealing (recipient-key encryption) — design (P2 slice d)

**Status:** design for CireSnave's review, written 2026-09-17. It is stacked on `feat/mcp-surface`
(#39). **Publishing 2.0.0 is blocked until this slice lands**; the finding that caused the block is
recorded in `CAPABILITY_INVENTORY.md` §2.2, "ENCRYPTION ON `main` IS NOT CONFIDENTIAL".

**Decided by CireSnave (2026-09-17):**
- Sealing hides the **payload only**.
- Encryption keys are **pinned in config**.
- Failures are **closed and marked**: no silent fallback to plaintext, and a message that can't be
  opened is delivered marked, never hidden.
- The design in §2–§8 below was approved in chat before this spec was written.

---

## 1. The problem

`CryptoManager::encrypt_message` writes the AES key into its own output, and `decrypt_message` reads
it back without using any private key. A `CryptoManager` with no keys decrypted another manager's
ciphertext; this was measured, with a control. So `SecurityLevel::Secure` has provided no
confidentiality since `f0f570c`.

The fix must use a real recipient key. It **must never derive an X25519 key from an Ed25519 key**:
that produces ciphertext the recipient cannot open (OverMind MEASUREMENTS §20).

## 2. Algorithm

**HPKE** (RFC 9180), in **base mode**, via the `hpke` crate 0.14.1 (rozbb/rust-hpke, updated
2026-09-06):

| part | choice |
|---|---|
| KEM | DHKEM(X25519, HKDF-SHA256) |
| KDF | HKDF-SHA256 |
| AEAD | ChaCha20-Poly1305 |
| `info` | ASCII `synapse/seal/v1` |

**Why base mode is enough.** HPKE's auth modes would authenticate the sender with a second X25519
key. Slice a's Ed25519 signature already does that, and it covers the ciphertext (§4), so base mode
is sufficient.

## 3. What is sealed, and the wire format

Only `encrypted_content` is sealed. Ids, the timestamp, the security level, metadata (including
`synapse.reply_to` and the `synapse.ack.*` keys) and `routing_path` **stay readable**. This is
documented: metadata is not confidential.

The sealed `encrypted_content` is:

```
0x01 ‖ enc (32 bytes, the HPKE encapsulated key) ‖ ciphertext (plaintext length + 16-byte tag)
```

The first byte is the format version. Any other value means the body can't be opened.

**Authenticated data** ties the ciphertext to its header, so a sealed body can't be moved into
another message. It is built from these fields, each length-prefixed the same way as slice a's
canonical input:

```
"synapse/seal-aad/v1", message_id (16 raw bytes), from_global_id, to_global_id, security_level name
```

**What the security levels mean from now on:**

| level | body | signed |
|---|---|---|
| `Public` | plaintext | optional |
| `Authenticated` | plaintext | yes |
| `Private` | **sealed** | optional |
| `Secure` | **sealed** | yes |

A receiver treats a sealed level whose body doesn't parse as sealed, or a plaintext level whose body
starts with the sealed format, exactly as it treats any other body it can't open (§5).

## 4. Order of operations (sender)

1. Build the message and call `request_ack` if wanted.
2. **Seal.** This replaces the plaintext body with the sealed body.
3. **Sign** (slice a). The signature covers SHA-256 of the sealed bytes, so a tampered ciphertext
   comes out as `Contradicted(BadSignature)` before anything tries to open it.
4. Send.

## 5. Keys

**This node's key.** A `SealingKeyPair` holds a node's X25519 key pair. It is **generated separately**
and never derived from the Ed25519 key.

The key is stored in the standard formats, so other languages can read it:
- the private key as **PKCS#8 v1** PEM (`PRIVATE KEY`, OID 1.3.101.110);
- the public key as a **SubjectPublicKeyInfo** PEM (`PUBLIC KEY`).

Note that slice a's Ed25519 public PEM is raw bytes under the same label; that isn't changed here.

**Peers' keys.** `TrustStore` gains:
- `pin_sealing_key(global_id, public_key)`
- `pin_sealing_key_pem(global_id, pem)`
- `sealing_key_for(global_id) -> Option<SealingPublicKey>`
- `sealing_key_id(global_id) -> Option<String>`, the lowercase hex SHA-256 of the 32-byte key

These pins sit beside the signing-key pins and are just as independent of the relay.

**`CryptoManager`** gains:
- `generate_sealing_key()`, `load_sealing_key_pem(pem)` and `sealing_public_key_pem()`;
- `seal_secure_message(&self, message: &mut SecureMessage, recipient: &SealingPublicKey) -> Result<()>`.
  It refuses unless the message's level is `Private` or `Secure`, and refuses if the body is already
  sealed.
- `open_payload(&self, message: &SecureMessage) -> Result<Vec<u8>, OpenError>`.

**`CryptoManager::encrypt_message` and `decrypt_message` are deleted**, along with their two unit
tests. The two unit tests are replaced by the sealing tests. This is a breaking change, which 2.0.0
allows.

**The router** (`SynapseRouter`, the untested email path) holds no `TrustStore`, so it has nowhere to
look up a recipient's sealing key. In this slice it **refuses** `Secure` and `Private` sends with an
error, rather than falling back to plaintext. `convert_to_secure_message` stops trying to encrypt and
leaves the body plain at level `Authenticated`. Sealing on that path can be wired later, once it has a
key store and a test.

## 6. Receiving

**Payload.** `ReceivedMessage` gains `payload: Payload`:

```rust
pub enum Payload {
    Plain(Vec<u8>),                  // level Public/Authenticated, body not in the sealed format
    Opened(Vec<u8>),                 // level Private/Secure, opened with this node's key
    CouldNotOpen(OpenError),         // anything else: marked, never hidden
}
pub enum OpenError { NoSealingKey, NotSealed, UnsupportedVersion, Truncated, Undecryptable, SealedButPlainLevel }
```

**Where opening happens.** `TransportManager` holds this node's `SealingKeyPair`, set through the
builder with `.sealing_key(..)` or at runtime with `set_sealing_key`.

`receive_messages` opens every message and computes `payload` after the verdict. It opens messages
whatever their verdict: an unverified message still shows its content, marked by its verdict.

A manager with no sealing key reports every sealed message as `CouldNotOpen(NoSealingKey)`.

**Acknowledging.** `acknowledge` also refuses a message whose payload is `CouldNotOpen`, and sends
nothing. The ack digest is unchanged: it covers the canonical input, which includes the hash of the
sealed body.

## 7. The MCP server (`synapse-mcp`)

**Config.**
- `sealing_key_path` is **required**. It has the same hygiene rules as `private_key_pem_path`: fixed
  error text that never names the path.
- Each `[[peers]]` entry gains `sealing_public_key`, optional in the TOML. A peer without one can't be
  sent to.

**Tools.**
- **`send`** always seals, using `SecurityLevel::Secure`. It refuses, with a tool error, a peer that has
  no pinned sealing key. Its description changes to: *"Messages are signed and encrypted to the
  recipient's pinned key; metadata (ids, timestamps) is not encrypted."*
- **`poll`** changes each message as follows:
  - `text` comes from `Plain` or `Opened`;
  - a new field `sealed` is true or false;
  - for `CouldNotOpen`, `text` is null and `open_error` names the reason.

  The UNTRUSTED paragraph is unchanged.
- **`list`** adds `sealing_key_id` for self and for each peer, and null where a peer has no key.
- **`ack`** refuses unopened messages, through the manager.

## 8. Tests

**In-library, `tests/sealing.rs`:**
1. **Round trip.** Seal, sign, send over UDP, and receive `Opened`, with the right text and `Verified`.
2. **Wrong recipient.** A message sealed to another key gives `CouldNotOpen(Undecryptable)`.
3. **Tampered ciphertext.** One changed byte gives `Contradicted(BadSignature)` **and**
   `CouldNotOpen(Undecryptable)`.
4. **Header binding.** Change `to_global_id` or `from_global_id` after sealing and re-sign with the same
   key, so the signature is valid. Opening then fails with `Undecryptable`. This test isolates the
   authenticated data from the signature.
5. **Level/body mismatch.** A `Secure` message with a plain body gives `CouldNotOpen(NotSealed)`. An
   `Authenticated` message with a sealed body gives `CouldNotOpen(SealedButPlainLevel)`.
6. **Refusals.**
   - Sealing an `Authenticated`-level message is an error.
   - Sealing twice is an error.
   - `SynapseRouter::send_message` at level `Secure` returns an error before touching the transport.
7. **No key at the receiver.** A manager with no sealing key gives `CouldNotOpen(NoSealingKey)`.
8. **Ack refusal.** `acknowledge` on a message that couldn't be opened returns an error and sends
   nothing. Control: acknowledging an opened message works.
9. **Key format.**
   - A generated key round-trips through its PEM.
   - The public key's SubjectPublicKeyInfo has the X25519 OID.
   - The sealing key id differs from the Ed25519 `key_id` of the same node, because the keys are
     independent.
10. **The broken code is gone.** `encrypt_message` and `decrypt_message` no longer exist; a source
    grep finds them nowhere in compiled `src/`. **Control:** the grep finds `seal_secure_message`.

**MCP (`tests/mcp_surface.rs` and `tests/mcp_stdio_process.rs`):**
- The existing tests are updated for sealing.
- New: `send` to a peer without a sealing key is refused.
- New: the text in `poll` round-trips. **Control for "encrypted on the wire":** a raw UDP listener that
  captures what `send` sends does **not** contain the plaintext bytes.
- The no-leak search also covers the sealing key path and its file name.

**Independent check (not committed).** A from-scratch Python implementation of RFC 9180 base-mode
*open* opens a Rust-sealed message given the recipient's private key and the authenticated-data
bytes. It uses `cryptography` 50.0.1's X25519, HKDF and ChaCha20-Poly1305. Agreement checks the wire
format, the `info` string and the authenticated-data construction by a different method. The PR
reports the result.

**Mutation check, run once and reported.** Seal with empty authenticated data. Test 4 must fail, and
no other test may fail.

The failing-test set must stay `{test_transport_error_handling}`.

## 9. Out of scope

- Encrypting metadata.
- Forward secrecy beyond HPKE's per-message ephemeral key.
- Key rotation, which is slice f.
- Replay suppression, which is slice e.
- Bumping the `aes-gcm` dependency (0.10 → 0.11.1 is available); that is a separate dependency task.
- WASM (`src/wasm/crypto.rs`), which compiles only for `wasm32`.

## 10. Risks

- **The `hpke` crate is a new dependency.** Its 0.14.1 API is checked with the compiler, and the
  independent Python check guards the construction built on it.
- **Two PEM conventions now exist side by side:** raw bytes for Ed25519 (slice a) and standard for
  X25519 (this slice). This is documented, and unifying them belongs to slice f.
