# Remove bincode (RUSTSEC-2025-0141, unmaintained) (plan)

**Why:** `cargo audit` flags `bincode 2.0.1` as unmaintained (RUSTSEC-2025-0141). It is one of the three IDs
keeping Security Audit red on `main`. Unlike the other two (2026-0258 h2 and 2025-0134 rustls-pemfile,
which both arrive through auth-framework 0.3.0 → reqwest 0.11 and clear with auth-framework 0.6.0 stable,
board 102), bincode is **our own direct dependency**: optional, pulled in by the `minimal` feature, and
therefore by `default`. The synapse lane owns it (PM ruling, 2026-10-07).

**Measured at:** `origin/main@80e4e66` (6.0.0-rc.8). Every count below is `git grep` at that ref, over `src`,
`crates` and `tests`.

## 1. Where bincode is used

| file | refs | what |
|---|---|---|
| `src/synapse/blockchain/block.rs` | 138 | hand-written `Encode`/`Decode` for blocks and transactions; `to_bytes`/`from_bytes`; the trust-report signature check |
| `src/synapse/models/trust/trust_bincode_impls.rs` | 79 | hand-written impls for the trust models |
| `src/types.rs` | 23 | derives on `MessageType`, `SecurityLevel`, `SimpleMessage`, `SecureMessage`, `StreamPriority`, `StreamType`, `StreamChunk`, `StreamMetadata`; **four** public `to_bytes`/`from_bytes` pairs |
| `src/synapse/blockchain/serialization.rs` | 20 | `Encode`/`Decode` for `DateTimeWrapper` and `UuidWrapper` |
| `src/error.rs` | 4 | `From<bincode::error::{EncodeError, DecodeError}> for SynapseError` |
| `src/sender_auth.rs` | 3 | derives on `ProofAlg` and `SenderProof`, which `SecureMessage` carries |
| `src/synapse/models/trust/mod.rs`, `models/participant.rs` | 4 | the module declaration and comments |

**bincode is not confined to the legacy trees.** The core message types derive it, and
`DateTimeWrapper`/`UuidWrapper` live in `synapse::blockchain::serialization`.

- **In `src`, outside the blockchain tree,** 12 files use the wrappers, 78 times in all.
  - 7 are core: `types.rs`, `crypto.rs`, `identity.rs`, `sender_auth.rs`, `streaming.rs`,
    `connectivity.rs` and `auth_integration_enhanced.rs`.
  - 5 are themselves legacy: `models/trust/mod.rs`, `services/trust_manager.rs`, `api/errors.rs`,
    `storage/database.rs` and `telemetry/error_reporting.rs`.
- **Including `tests`,** it is 91 uses across 16 files. That adds `key_manager_properties`, `sealing`,
  `security_test` and `sender_authentication`.
- `types.rs` already re-exports them as `synapse::types::{DateTimeWrapper, UuidWrapper}`.

**So deleting the legacy trees alone would not clear the advisory.**

## 2. (a) Does anything persist or transmit bincode bytes today?

**No.** Each answer below comes with its control.

- **The wire is JSON.** No transport, router or crate encodes with bincode: 0 hits for
  `blockchain|\bBlock\b` in `src/transport`, `src/router_merged.rs` and `crates`. The transport contract
  sends `serde_json` (`smtp_server.rs`'s `store_message_round_trips_a_json_secure_message_byte_identical`).
- **The four `types.rs` `to_bytes`/`from_bytes` have no in-tree callers.** 0 hits for
  `(SecureMessage|SimpleMessage|StreamChunk|StreamMetadata)::from_bytes` or `(message|msg|chunk|metadata|meta)\.to_bytes\(\)`.
  Control: `git grep -c 'fn to_bytes' src/types.rs` finds the 4 definitions.
- **Blocks are not persisted.** `SynapseBlockchain` holds `chain: Arc<RwLock<Vec<Block>>>` in memory. There
  are 0 hits for `sqlx|Database|std::fs|tokio::fs|File::` under `src/synapse/blockchain`. Control: the same
  pattern finds `storage/database.rs`, `migrations.rs` and `mod.rs`.
- **Block hashes and block signatures do not use bincode.** `Block::calculate_hash` feeds fields into
  SHA-256 directly. `sign_block` signs `serde_json::to_vec` of the block without its signature.
  `cache_block_hash` stores that hex string in Redis.
- **What is persisted holds no bincode bytes.**
  - The `trust_reports` table stores `reporter_id, subject_id, score, category, transaction_id, timestamp`
    (`database.rs` `INSERT INTO trust_reports`). There is no signature and no encoded blob.
- **The one bincode-dependent check** is `TrustReport::verify` (`block.rs` ~441). It verifies an Ed25519
  signature over `bincode::encode_to_vec` of the report's fields. Reports live only in the in-memory chain, so
  changing that encoding orphans no stored signature.
- **No shipped binary reaches the legacy trees.**
  - `synapsed`, `synapsectl`, `synapse-mcp-server` and `synapse-security` have 0 hits for
    `SynapseNode|synapse::synapse::|blockchain`. Control: 3 `synapsed` files mention `synapse::`.
  - `SynapseNode::new`, the only constructor of the blockchain plus Postgres stack, has no in-tree caller. It
    is re-exported (`lib.rs:255`), so an outside user of the published crate could call it.

**Who confirms that the legacy trees may go:** CireSnave, as part of the FAM/Synapse merge
(`CIRESNAVE-EXPECTATIONS.md` §5.2; spec `2026-10-07-brute-force-hardening-design.md` §6: "whether to delete
them is the FAM/Synapse merge's call"). The PM routes that question to the board once. Step B2 below forks on
the answer. Step B1 does not depend on it.

## 3. Steps

Two PRs, each red-first. Both are **breaking** and land inside the 6.0.0 rc series, before 6.0.0 final. The PM
allocates each version at gate time.

### B1: the core stops using bincode (independent of the FAM decision)

1. **Move `DateTimeWrapper` and `UuidWrapper`** into a new core module, `src/wire.rs` (`pub mod wire` in
   `lib.rs`).
   - **What moves:** the two structs, with their `Serialize`/`Deserialize` derives unchanged, their
     `Default`/`Display` impls, and their bincode-free inherent methods (`new`, `into_inner`).
   - **What stays:** their bincode `Encode`/`Decode`/`BorrowDecode` impls, and the `to_bincode`/`from_bincode`
     methods (no callers outside that file), stay in `blockchain/serialization.rs` as impls on the moved types. A trait impl may live anywhere in
     the defining crate, so `wire.rs` contains no bincode and B1's guard can cover it.
   - **`serialization.rs`** deletes the moved definitions and adds `pub use crate::wire::{DateTimeWrapper, UuidWrapper};`,
     so `synapse::blockchain::serialization::…` keeps resolving until B2.
   - **`types.rs:233`** becomes `pub use crate::wire::{DateTimeWrapper, UuidWrapper};`.
   - **The comment at `models/participant.rs:215`** ("Manual bincode impls are provided elsewhere") goes:
     it is the only bincode mention under `src/synapse/models` outside `trust/`.
2. **Remove the bincode derives** from the eight `types.rs` types and the two `sender_auth.rs` types (§1).
3. **Remove the four public `to_bytes`/`from_bytes` pairs** on `SimpleMessage`, `SecureMessage`,
   `StreamChunk` and `StreamMetadata`. **(b) Explicit breaking change:** these are public API with no
   in-tree caller. The changelog entry names each of the 8 removed functions and the replacement:
   `serde_json::to_vec` / `from_slice`, which is the wire format.
4. **Keep `From<bincode::…>` for `SynapseError`** until B2. The legacy trees still return bincode errors
   through it.

### B2: the legacy trees (forks on CireSnave's answer)

- **If they go:** delete `synapse::blockchain`, `models/trust/trust_bincode_impls.rs` and whatever depends
  only on them: `api/trust_api.rs`, `services/trust_manager.rs`, `SynapseNode`, and the block tests in
  `tests/key_manager_properties.rs` and `tests/security_test.rs`. The non-block tests in those files stay.
  - **First repoint every import of `crate::synapse::blockchain::serialization` / `crate::blockchain::…`**
    (about 19 sites) to `crate::wire`. They include `auth_integration_enhanced`, `connectivity`,
    `crypto`, `identity` (and its doctest at `identity.rs:121`), `sender_auth` and `streaming`, plus
    `tests/sealing.rs`, `tests/sender_authentication.rs`, `tests/security_test.rs` and
    `tests/key_manager_properties.rs`.
  - **Breaking public API, named in the changelog:**
    - `synapse::blockchain` (`lib.rs:259`);
    - `synapse::synapse::blockchain::serialization::*`, now at `synapse::wire`;
    - `SynapseNode` and `SynapseConfig` (`lib.rs:255`);
    - and whatever else the deletion removes, measured at the time.
  - The exact set is measured with the compiler and `git grep` at the time, not taken from this list.
- **If they stay:** port the hand-written impls to serde. `TrustReport::verify` then signs and checks
  `serde_json::to_vec` of the same field tuple, the format `sign_block` already uses. No stored signature
  exists to migrate (§2).
- **Either way:**
  - remove `From<bincode::…>` from `error.rs` (breaking);
  - remove the `bincode` dependency and `"dep:bincode"` from `minimal`;
  - confirm `cargo audit` no longer lists RUSTSEC-2025-0141.

## 4. (c) Guards, red first

**`tests/bincode_is_gone.rs`** enumerates tracked sources with `git ls-files` (CLAUDE.md §5b), never by
walking the disk.

- **`no_tracked_source_mentions_bincode`:**
  - In B1 it is scoped to the core: everything under `src/` except `src/synapse/blockchain/**`,
    `src/synapse/models/trust/**` and `src/error.rs`, plus `crates/**` and `tests/**` except this guard.
    In B2 it widens to the whole tree and to `Cargo.toml`.
  - **Red first:** it fails at `80e4e66` (`types.rs` has 23 hits).
  - Positive control in the same test: the same scan finds `serde_json` in `src/transport/email_unified.rs`.
- **`removed_byte_codecs_are_gone`:** type-qualified, because block.rs keeps `Block`, `Transaction` and
  `TrustReport` codecs until B2.
  - (i) `src/types.rs` contains no `fn to_bytes` and no `fn from_bytes`.
  - (ii) No tracked file matches `(SimpleMessage|SecureMessage|StreamChunk|StreamMetadata)::(to|from)_bytes`.
  - **Red first:** (i) fails at `80e4e66`, where the 4 `to_bytes` and 4 `from_bytes` are defined.
  - **Control for (ii):** the same pattern shape with another type, `Signature::from_bytes`, finds
    `src/synapse/blockchain/block.rs`. So the regex form works.
- **`secure_message_json_is_unchanged`** (a characterization test, added in B1 before any code moves):
  - It serializes a fixed `SecureMessage`, with a fixed UUID, timestamp and payload, using `serde_json`, and
    compares the result byte for byte against a literal captured at `80e4e66`.
  - This proves moving the wrappers did not change the wire.
  - Its negative control is a mutated copy of the literal, which must compare unequal.

The existing JSON round-trip tests (`store_message_round_trips_a_json_secure_message_byte_identical` and
the transport suites) keep running unchanged and are the second witness.

## 5. Out of scope

- Any change to the wire format: it stays `serde_json`.
- Deciding the FAM merge. This plan only asks the one question in §2.
- RUSTSEC-2026-0258 and 2025-0134 (auth-framework 0.6.0 stable, board 102).
