# Agent Certificates (f2) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make certificates the only trust path — a peer is configured with one account key and that
peer's certificate, nothing is pinned per agent, and a `synapse-cert` CLI mints, delegates, revokes,
inspects and converts.

**Architecture:** `TrustStore` gains a validated peer certificate store, keyed by global id, from
which both the signing key (for verification) and the sealing key (for encryption) come. The four
direct-pinning methods are deleted and all nineteen call sites convert. A new `src/bin/synapse_cert.rs`
plus `src/cert_cli.rs` implement the tool, with signing behind one trait so hardware-backed signing
can land later without touching the commands.

**Tech Stack:** Rust; `clap` is NOT a dependency of this crate — parse arguments by hand as
`src/bin/synapse_mcp.rs` already does. `ed25519-dalek`, `pem`, `chrono`, `sha2`, `toml` are all
already present. No new dependencies.

**Spec:** `docs/superpowers/specs/2026-09-17-agent-certificates-design.md` — **§11 f2 scope**, plus
the two corrections f1's final review deferred here (§8's summary shape, and `verify_at` returning
the chain it computed).

## Global Constraints

- **Branch:** `feat/agent-certificates-cli`, stacked on `feat/agent-certificates` (#43). Held for the
  PM's daily pass; #43 must merge first.
- **Exact strings:** the PEM labels `SYNAPSE AGENT CERT` and `SYNAPSE REVOCATION`; metadata keys
  `synapse.cert.chain` and `synapse.cert.revocations`; permission names `send`, `request-ack`, `ack`
  and the `x-` prefix. None of these change in f2.
- **A peer entry is `global_id`, `account_public_key`, `certificate_pem`, `address`.**
  `public_key_pem` and `sealing_public_key` are removed. Both the signing and sealing keys come from
  the certificate, validated under the account key.
- **An expired certificate in config refuses the send**, with a message naming the peer and the
  expiry — never an encryption to a possibly-rotated key.
- **No private key material in any error, log line, tool output or `Debug`.** The CLI writes private
  keys to files only, never to stdout, and its file-creation must not widen permissions beyond the
  process default.
- **`Cargo.toml` gains one `[[bin]]` entry** for `synapse-cert` and nothing else. No new dependency.
  No version bump: it is already `2.0.0`, and the crates.io publish is held until the whole P2 set
  plus its breaking changes are ready.
- New `.rs` files start with `// SPDX-License-Identifier: MIT OR Apache-2.0`.
- Tests bind loopback only. `CARGO_TARGET_DIR=C:/Projects/synapse/target` for every cargo command.
- Checks: `cargo test --no-fail-fast` keeps the failing set `{test_transport_error_handling}`;
  `cargo fmt -- --check` exits 0; `cargo clippy -- -D warnings` exits 0, including each new or changed
  test target.

## File Structure

| file | responsibility |
|---|---|
| `src/sender_auth.rs` | the peer certificate store; `sealing_key_for` reads it; the four pinning methods are deleted; `verify_at` returns the chain |
| `src/certificate.rs` | `VerifiedChain` carries key **ids**, not raw key bytes |
| `src/cert_cli.rs` (new) | the CLI's commands as testable functions, with a `CertSigner` trait |
| `src/bin/synapse_cert.rs` (new) | argument parsing and process exit codes only |
| `src/mcp_server.rs` | the new peer entry shape, `revocations_path`, `list`'s account key ids |
| `tests/synapse_cert_cli.rs` (new) | spec §10 tests 13-14, over the real binary |
| every other `tests/*.rs` | converted from pinning to certificates |

---

### Task 1: The peer certificate store, and the two deferred corrections

**Files:**
- Modify: `src/certificate.rs` (`VerifiedChain`), `src/sender_auth.rs` (the store, `verify_at`'s
  return), `src/transport/manager.rs` (the one call site that reads the summary)

**Interfaces:**
- Consumes: `validate_chain`, `chain_from_pem`, `AgentCertificate` from f1.
- Produces:
  - `VerifiedChain { account_key_id: String, subject_label: String, subject_global_id: String, subject_signing_key_id: String, subject_sealing_key: crate::sealing::SealingPublicKey, permissions: Vec<Permission>, links: usize }` — the signing key becomes an **id**, because the summary travels to consumers and §8 says it carries no key bytes. The sealing key stays a key, because `TrustStore` needs it to encrypt, and it is a public key.
  - `TrustStore::pin_peer_certificate(&mut self, chain_pem: &str, now: DateTime<Utc>) -> Result<String>` — validates the chain under the already-pinned account keys, stores it under the leaf's `subject_global_id`, and returns that id. Refuses a chain whose root is unpinned, exactly as `verify_at` does.
  - `TrustStore::peer_certificate(&self, global_id: &str) -> Option<&VerifiedChain>`
  - `TrustStore::sealing_key_for(&self, global_id: &str) -> Option<&SealingPublicKey>` — **unchanged signature**, now reading the peer certificate store instead of the pinned sealing map.
  - `TrustStore::verify_at(&self, message, now) -> (SenderVerdict, Option<VerifiedChain>)` — returns the chain it already validated, so nothing validates twice.

- [ ] **Step 1: Write the failing tests** in `src/sender_auth.rs`'s test module.

```rust
    #[test]
    fn a_peer_certificate_supplies_both_keys_after_validating_under_its_account_key() {
        let account = SigningKey::from_bytes(&[21u8; 32]);
        let agent = CryptoManager::new_with_keypair();
        let sealing = crate::sealing::SealingKeyPair::generate();
        let cert = signed_cert_with_sealing_key(&account, &agent, &sealing, "agent@alice.test", t_now());

        let mut store = TrustStore::new();
        // Without the account key pinned, the certificate is refused.
        assert!(store.pin_peer_certificate(&certificate::chain_to_pem(&[cert.clone()]), t_now()).is_err());
        assert!(store.sealing_key_for("agent@alice.test").is_none());

        store.pin_account_key("alice", account.verifying_key().to_bytes());
        let id = store
            .pin_peer_certificate(&certificate::chain_to_pem(&[cert]), t_now())
            .expect("valid under the pinned account key");
        assert_eq!(id, "agent@alice.test");
        assert_eq!(
            store.sealing_key_for("agent@alice.test").map(|k| k.key_id()),
            Some(sealing.public_key().key_id())
        );
        assert_eq!(
            store.peer_certificate("agent@alice.test").map(|c| c.subject_signing_key_id.clone()),
            Some(key_id(&agent.public_key_bytes().unwrap()))
        );
    }

    #[test]
    fn an_expired_peer_certificate_is_refused_when_it_is_pinned() {
        let account = SigningKey::from_bytes(&[22u8; 32]);
        let agent = CryptoManager::new_with_keypair();
        let sealing = crate::sealing::SealingKeyPair::generate();
        let cert = signed_cert_with_sealing_key(&account, &agent, &sealing, "agent@alice.test", t_now());
        let mut store = TrustStore::new();
        store.pin_account_key("alice", account.verifying_key().to_bytes());
        // A day later the certificate has expired, so it cannot be pinned at all.
        let later = t_now() + Duration::days(2);
        assert!(store.pin_peer_certificate(&certificate::chain_to_pem(&[cert]), later).is_err());
        assert!(store.sealing_key_for("agent@alice.test").is_none());
    }

    #[test]
    fn verify_at_returns_the_chain_it_validated() {
        let account = SigningKey::from_bytes(&[23u8; 32]);
        let agent = CryptoManager::new_with_keypair();
        let message = chain_signed_message(&account, &agent, "agent@alice.test");
        let mut store = TrustStore::new();
        store.pin_account_key("alice", account.verifying_key().to_bytes());
        let (verdict, chain) = store.verify_at(&message, t_now());
        assert!(verdict.is_verified());
        let chain = chain.expect("the chain that produced the verdict");
        assert_eq!(chain.subject_global_id, "agent@alice.test");
        assert_eq!(chain.subject_signing_key_id, key_id(&agent.public_key_bytes().unwrap()));

        // Control: a directly pinned sender verifies with no chain to return.
        let solo = CryptoManager::new_with_keypair();
        let mut plain = SecureMessage::new("bob@test", "solo@test", b"hi".to_vec(), SecurityLevel::Authenticated);
        solo.sign_secure_message(&mut plain).unwrap();
        let mut pinned = TrustStore::new();
        pinned.pin_account_key("nobody", SigningKey::from_bytes(&[24u8; 32]).verifying_key().to_bytes());
        let (verdict, chain) = pinned.verify_at(&plain, t_now());
        assert!(!verdict.is_verified());
        assert!(chain.is_none());
    }
```

Write `signed_cert_with_sealing_key` and `chain_signed_message` beside the existing `signed_cert_for`
helper, reusing it where possible. The sealing key must be a real `SealingKeyPair` rather than the
`[0u8; 32]` placeholder f1's tests used, since this task is what finally reads it.

- [ ] **Step 2: Run them and check that they fail**

Run: `cargo test --lib sender_auth:: 2>&1 | tail -20`
Expected: `pin_peer_certificate`, `peer_certificate` and the tuple return do not exist.

- [ ] **Step 3: Implement**

In `src/certificate.rs`, change `VerifiedChain`'s two key fields:

```rust
    /// The leaf's signing key id — the summary carries no raw key bytes (spec §8).
    pub subject_signing_key_id: String,
    /// The leaf's X25519 sealing key. A public key, and the one a sender encrypts to.
    pub subject_sealing_key: crate::sealing::SealingPublicKey,
```

`validate_chain` builds them with `crate::sender_auth::key_id(&leaf.subject_signing_key)` and
`SealingPublicKey::from_bytes(leaf.subject_sealing_key)`.

In `src/sender_auth.rs`, add `peer_certificates: HashMap<String, VerifiedChain>` to `TrustStore`, and:

```rust
    /// Validate a peer's certificate chain under the account keys already pinned, and remember both
    /// of its keys under the leaf's global id. This is how an outbound sealing key is obtained: a
    /// certificate only arrives on an inbound message, but a node must encrypt to a peer before it
    /// has heard from them (spec §8).
    pub fn pin_peer_certificate(&mut self, chain_pem: &str, now: DateTime<Utc>) -> Result<String> {
        let chain = certificate::chain_from_pem(chain_pem)
            .map_err(|e| CryptoError::InvalidKey(format!("peer certificate: {e}")))?;
        let verified = self
            .validate_chain_now(&chain, now)
            .map_err(|e| CryptoError::InvalidKey(format!("peer certificate: {e}")))?;
        let id = verified.subject_global_id.clone();
        self.peer_certificates.insert(id.clone(), verified);
        Ok(id)
    }
```

where `validate_chain_now` is the private helper `resolve_chain` already uses, so one implementation
validates both inbound chains and configured ones. `sealing_key_for` now reads
`self.peer_certificates.get(global_id).map(|c| &c.subject_sealing_key)`; delete the `sealing` map.

`verify_at` returns `(SenderVerdict, Option<VerifiedChain>)`: the chain route returns the validated
chain alongside `Verified`, every other outcome returns `None`. `verified_chain_at` is **deleted** —
its only caller was `receive_messages`, which now takes the chain from `verify_at`'s tuple. That also
removes the double validation f1's review found, and the two-clock hazard with it.

In `src/transport/manager.rs`, destructure the tuple and keep the existing binding filter, comparing
`chain.subject_signing_key_id` with the verdict's `key_id` (the comparison is now id to id).

- [ ] **Step 4: Run the tests and check that they pass**

Run: `cargo test --lib` then `cargo test --no-fail-fast`.
Expected: the failing set is exactly `{test_transport_error_handling}`. Existing tests that read
`chain.subject_signing_key` must be updated to the id — that is a rename, not a weakening, and the
assertions keep their meaning.

Then `cargo fmt` and `cargo clippy -- -D warnings`.

- [ ] **Step 5: Commit**

```bash
git add src/certificate.rs src/sender_auth.rs src/transport/manager.rs
git commit -m "feat(cert)!: a validated peer certificate supplies both of a peer's keys (P2f2)"
```

---

### Task 2: Remove direct pinning, and convert every call site

**Files:**
- Modify: `src/sender_auth.rs` (delete four methods), `src/delivery_ack.rs`, `src/mcp_server.rs`,
  `tests/agent_certificates.rs`, `tests/receiver_acknowledgement.rs`, `tests/replay_suppression.rs`,
  `tests/sealing.rs`, `tests/sender_authentication.rs`

**Interfaces:**
- Consumes: Task 1's `pin_account_key` + `pin_peer_certificate`.
- Produces: a `TrustStore` whose only trust path is certificates.

**The blast radius, measured:** 19 call sites across 8 files
(`grep -rn "\.pin(\|pin_pem(\|pin_sealing_key(\|pin_sealing_key_pem(" src/ tests/`). Every one becomes
"pin the account key, then pin the peer's certificate".

- [ ] **Step 1: Write the failing test** in `src/sender_auth.rs`'s test module — the one that proves
  the old path is gone rather than merely unused:

```rust
    #[test]
    fn the_direct_pinning_api_is_gone() {
        // A source scan, because a deleted method cannot be called in a compiling test. The point is
        // that no caller anywhere can reintroduce per-agent pinning.
        let source = include_str!("sender_auth.rs");
        for gone in ["pub fn pin(", "pub fn pin_pem(", "pub fn pin_sealing_key(", "pub fn pin_sealing_key_pem("] {
            assert!(!source.contains(gone), "{gone} must be deleted in f2");
        }
        // Control: the replacements are present, so the scan is looking at the right file.
        assert!(source.contains("pub fn pin_account_key("));
        assert!(source.contains("pub fn pin_peer_certificate("));
    }
```

- [ ] **Step 2: Run it and check that it fails**

Run: `cargo test --lib sender_auth::tests::the_direct_pinning_api_is_gone`
Expected: FAIL — the methods still exist.

- [ ] **Step 3: Delete and convert**

Delete `pin`, `pin_pem`, `pin_sealing_key`, `pin_sealing_key_pem` and the `keys`/`sealing` maps they
wrote to. `verify_at`'s direct-pin branch and `verify_against_pinned_key` go with them: **the pinned
route ceases to exist**, so `verify_at` is the chain route plus the timestamp guard. Keep
`ContradictedReason::KeyMismatch` — the chain route still uses it (f1's fix 4).

Then convert each test harness. The shape, once, so every file follows it:

```rust
/// Alice's account key certifies her agent; Bob pins the account key and the certificate.
fn store_for(account: &SigningKey, agent: &CryptoManager, sealing: &SealingKeyPair, id: &str) -> TrustStore {
    let mut store = TrustStore::new();
    store.pin_account_key("alice", account.verifying_key().to_bytes());
    let cert = cert_for(account, agent, sealing, id);
    store
        .pin_peer_certificate(&chain_to_pem(&[cert]), Utc::now())
        .expect("the certificate is valid under the pinned account key");
    store
}
```

and every sender sets its chain with `set_certificate_chain` before signing, as f1's fix wave
established.

**Do not weaken a negative test while converting it.** Several exist specifically to prove a bad
sender is refused — an unpinned sender, a contradicted signature, a sender whose key does not match.
Each must keep its bad input; the only thing that changes is how the *good* side of the test is
configured. If converting a test would remove the property it tests, stop and report it rather than
adjusting the assertion.

- [ ] **Step 4: Run everything**

Run: `cargo test --no-fail-fast`.
Expected: the failing set is exactly `{test_transport_error_handling}`. Report the count of tests that
changed shape, and name any whose meaning you had to think about.

Then `cargo fmt` and `cargo clippy -- -D warnings`.

- [ ] **Step 5: Commit**

```bash
git add -A src/ tests/
git commit -m "feat(cert)!: certificates are the only trust path; direct pinning is removed (P2f2)"
```

---

### Task 3: `synapse-cert init`, `mint` and `inspect`

**Files:**
- Create: `src/cert_cli.rs`, `src/bin/synapse_cert.rs`, `tests/synapse_cert_cli.rs`
- Modify: `Cargo.toml` (one `[[bin]]`), `src/lib.rs` (`pub mod cert_cli;` under the `crypto` feature)

**Interfaces:**
- Produces:
  - `pub trait CertSigner { fn key_id(&self) -> String; fn sign(&self, input: &[u8]) -> [u8; 64]; }` and `pub struct FileSigner` reading a PKCS#8 PEM — the seam that lets hardware-backed signing land later without touching a command.
  - `pub fn init(dir: &Path, global_id: &str, label: &str, hours: i64) -> Result<InitOutput>` where `InitOutput { account_key_path, signing_key_path, sealing_key_path, certificate_path, config_block: String }`.
  - `pub fn mint(signer: &dyn CertSigner, subject: MintArgs) -> Result<AgentCertificate>` with `MintArgs { global_id, label, signing_key: [u8; 32], sealing_key: [u8; 32], hours: i64, permissions: Vec<Permission>, may_delegate: u8 }`.
  - `pub fn inspect(pem: &str, now: DateTime<Utc>) -> Result<String>` — a human-readable summary, **never** private key material.

`init` writes four files and prints the TOML block an operator pastes into a peer's config:

```toml
[[peers]]
global_id = "agent@alice.test"
account_public_key = """
-----BEGIN PUBLIC KEY-----
...
-----END PUBLIC KEY-----
"""
certificate_pem = """
-----BEGIN SYNAPSE AGENT CERT-----
...
-----END SYNAPSE AGENT CERT-----
"""
address = "127.0.0.1:8080"
```

- [ ] **Step 1: Write the failing tests** in `tests/synapse_cert_cli.rs`, driving the real binary with
  `env!("CARGO_BIN_EXE_synapse-cert")`, as `tests/mcp_stdio_process.rs` drives `synapse-mcp`.

```rust
// SPDX-License-Identifier: MIT OR Apache-2.0
//! P2 slice f2: the synapse-cert CLI. Test numbers refer to
//! docs/superpowers/specs/2026-09-17-agent-certificates-design.md §10.

use std::path::Path;
use std::process::Command;

use chrono::Utc;
use synapse::certificate::chain_from_pem;
use synapse::sender_auth::TrustStore;

/// Run the binary and return (exit code, stdout, stderr).
fn run(args: &[&str]) -> (i32, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_synapse-cert"))
        .args(args)
        .output()
        .expect("the synapse-cert binary runs");
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

/// Every private key file `init` writes, so a test can prove none of them reached stdout.
fn private_key_files(dir: &Path) -> Vec<String> {
    ["account.pem", "signing.pem", "sealing.pem"]
        .iter()
        .map(|name| std::fs::read_to_string(dir.join(name)).expect("the key file exists"))
        .collect()
}

// §10 test 13
#[test]
fn init_produces_files_that_let_two_nodes_talk_with_no_hand_editing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().to_str().unwrap();
    let (code, stdout, stderr) = run(&[
        "init",
        "--dir",
        path,
        "--global-id",
        "agent@alice.test",
        "--label",
        "alice-agent",
        "--address",
        "127.0.0.1:9100",
    ]);
    assert_eq!(code, 0, "stderr: {stderr}");

    // The four files exist.
    for name in ["account.pem", "signing.pem", "sealing.pem", "certificate.pem"] {
        assert!(dir.path().join(name).is_file(), "{name} was not written");
    }

    // No private key material reached stdout: not the PEM header, and not any line of any key file.
    assert!(!stdout.contains("PRIVATE KEY"), "stdout leaked a private key header");
    for contents in private_key_files(dir.path()) {
        for line in contents.lines().filter(|l| !l.starts_with("-----") && l.len() > 16) {
            assert!(!stdout.contains(line), "stdout leaked a private key line");
        }
    }

    // The printed block is the peer entry an operator pastes, and it carries all four fields.
    for field in ["[[peers]]", "global_id", "account_public_key", "certificate_pem", "address"] {
        assert!(stdout.contains(field), "the printed block is missing {field}");
    }

    // The certificate it wrote validates under the account key it wrote, with no hand editing.
    let certificate = std::fs::read_to_string(dir.path().join("certificate.pem")).unwrap();
    let account_pem = std::fs::read_to_string(dir.path().join("account.pub.pem"))
        .expect("init writes the account PUBLIC key for pasting");
    let mut store = TrustStore::new();
    store.pin_account_key_pem("alice", &account_pem).expect("the account public key parses");
    let id = store
        .pin_peer_certificate(&certificate, Utc::now())
        .expect("the certificate validates under its own account key");
    assert_eq!(id, "agent@alice.test");
    assert!(store.sealing_key_for("agent@alice.test").is_some(), "both keys come from the certificate");

    // Control: a truncated certificate is refused, so the assertion above is not vacuous.
    let truncated: String = certificate.lines().take(2).collect::<Vec<_>>().join("\n");
    assert!(store.pin_peer_certificate(&truncated, Utc::now()).is_err());
}

#[test]
fn inspect_prints_a_summary_and_never_private_material() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().to_str().unwrap();
    let (code, _, stderr) = run(&[
        "init", "--dir", path, "--global-id", "agent@alice.test", "--label", "alice-agent",
        "--address", "127.0.0.1:9101",
    ]);
    assert_eq!(code, 0, "stderr: {stderr}");

    let certificate_path = dir.path().join("certificate.pem");
    let (code, stdout, stderr) = run(&["inspect", certificate_path.to_str().unwrap()]);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert!(stdout.contains("agent@alice.test"), "the subject id is shown");
    assert!(stdout.contains("alice-agent"), "the label is shown");
    assert!(stdout.contains("send"), "the permissions are shown");
    assert!(!stdout.contains("PRIVATE KEY"), "inspect must never print private material");
    for contents in private_key_files(dir.path()) {
        for line in contents.lines().filter(|l| !l.starts_with("-----") && l.len() > 16) {
            assert!(!stdout.contains(line), "inspect leaked a private key line");
        }
    }

    // Control: a truncated file exits nonzero, and the message names no path.
    let broken = dir.path().join("broken.pem");
    std::fs::write(&broken, "-----BEGIN SYNAPSE AGENT CERT-----\nnonsense\n").unwrap();
    let (code, _, stderr) = run(&["inspect", broken.to_str().unwrap()]);
    assert_ne!(code, 0);
    assert!(!stderr.contains(broken.to_str().unwrap()), "an error must not name a path");
}

#[test]
fn mint_issues_under_an_existing_account_key() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().to_str().unwrap();
    let (code, _, stderr) = run(&[
        "init", "--dir", path, "--global-id", "agent@alice.test", "--label", "alice-agent",
        "--address", "127.0.0.1:9102",
    ]);
    assert_eq!(code, 0, "stderr: {stderr}");

    // A second agent under the same account key, minted from the keys init already wrote.
    let out = dir.path().join("worker.pem");
    let (code, _, stderr) = run(&[
        "mint",
        "--account", dir.path().join("account.pem").to_str().unwrap(),
        "--signing-key", dir.path().join("signing.pem").to_str().unwrap(),
        "--sealing-key", dir.path().join("sealing.pem").to_str().unwrap(),
        "--global-id", "worker@alice.test",
        "--label", "alice-worker",
        "--permissions", "send,ack",
        "--out", out.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "stderr: {stderr}");

    let minted = std::fs::read_to_string(&out).unwrap();
    assert_eq!(chain_from_pem(&minted).expect("parses").len(), 1);

    let account_pem = std::fs::read_to_string(dir.path().join("account.pub.pem")).unwrap();
    let mut store = TrustStore::new();
    store.pin_account_key_pem("alice", &account_pem).unwrap();
    assert_eq!(
        store.pin_peer_certificate(&minted, Utc::now()).expect("valid under its account key"),
        "worker@alice.test"
    );

    // Control: a store pinning a DIFFERENT account key refuses the same certificate.
    let other = tempfile::tempdir().unwrap();
    let (code, _, _) = run(&[
        "init", "--dir", other.path().to_str().unwrap(), "--global-id", "agent@bob.test",
        "--label", "bob-agent", "--address", "127.0.0.1:9103",
    ]);
    assert_eq!(code, 0);
    let other_account = std::fs::read_to_string(other.path().join("account.pub.pem")).unwrap();
    let mut stranger = TrustStore::new();
    stranger.pin_account_key_pem("bob", &other_account).unwrap();
    assert!(stranger.pin_peer_certificate(&minted, Utc::now()).is_err());
}
```

Note the tests require `init` to write `account.pub.pem` (the account **public** key) alongside the
private `account.pem`, because that is what a peer pastes and what `pin_account_key_pem` consumes.

- [ ] **Step 2: Run them and check that they fail**

Run: `cargo test --test synapse_cert_cli 2>&1 | tail -20`
Expected: no such binary target.

- [ ] **Step 3: Implement**

`Cargo.toml` gains exactly:

```toml
[[bin]]
name = "synapse-cert"
path = "src/bin/synapse_cert.rs"
required-features = ["crypto"]
```

`src/bin/synapse_cert.rs` parses `std::env::args_os()` by hand — no `clap` — dispatches to
`cert_cli`, prints errors to stderr with fixed text naming no path, and returns `ExitCode::from(2)`
for a usage error and `ExitCode::FAILURE` for a failed command.

`src/cert_cli.rs` holds the commands. Private key files are written with
`std::fs::write`, and the function returns their paths for the caller to print — the **paths**, never
the contents.

- [ ] **Step 4: Run the tests and check that they pass**, then `cargo fmt`, `cargo clippy -- -D warnings`
  and `cargo clippy --test synapse_cert_cli -- -D warnings`.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml src/lib.rs src/cert_cli.rs src/bin/synapse_cert.rs tests/synapse_cert_cli.rs
git commit -m "feat(cert): synapse-cert init, mint and inspect (P2f2)"
```

---

### Task 4: `synapse-cert delegate`, `revoke` and `import`

**Files:**
- Modify: `src/cert_cli.rs`, `src/bin/synapse_cert.rs`, `tests/synapse_cert_cli.rs`

**Interfaces:**
- Produces:
  - `pub fn delegate(signer: &dyn CertSigner, parent: &AgentCertificate, subject: MintArgs) -> Result<AgentCertificate>` — refuses locally, with a clear message, anything `validate_chain` would refuse: a budget that does not decrease, a widened permission, an id that does not narrow, a window outside the parent's. Failing at mint time beats minting something no receiver will accept.
  - `pub fn revoke(signer: &dyn CertSigner, serial: [u8; 16], reason: &str) -> Result<Revocation>`
  - `pub fn import(config_toml: &str, account_key: &dyn CertSigner, hours: i64) -> Result<String>` — takes a config still using `public_key_pem`/`sealing_public_key`, mints a certificate for each peer under the account key, and returns the converted TOML.

- [ ] **Step 1: Write the failing tests** (append to `tests/synapse_cert_cli.rs`). **Each block below
  is an assertion specification, not code to paste**: write it as real Rust following Task 3's
  `run`/`private_key_files` helpers, which are already in the file. Every named assertion and every
  control must appear. Assert exact values — never an `Err(A) | Err(B)` alternation, which has twice
  let a test in this project pass at a different check than intended.

```rust
#[test]
fn delegate_refuses_what_a_receiver_would_refuse() {
    // Given a parent certificate with permissions [send, ack] and may_delegate 1:
    //   - delegating [send, ack, request-ack] exits nonzero, message mentions permissions;
    //   - delegating with may_delegate 1 (not decreasing) exits nonzero, message mentions delegation;
    //   - delegating to "evil@host" (not narrowing) exits nonzero, message mentions the identity;
    //   - Control: delegating [send] to "worker.agent@alice.test" with may_delegate 0 succeeds AND
    //     the resulting two-link chain validates under the account key in a TrustStore.
}

#[test]
fn revoke_produces_a_statement_that_stops_the_certificate() {
    // `synapse-cert revoke --account <pem> --serial <hex> --reason "key leaked"`:
    //   - the output parses as a Revocation and verifies under the account key;
    //   - a TrustStore that pinned the certificate and then adds this revocation refuses it;
    //   - Control: before the revocation is added, the same store accepts it.
}

// §10 test 14
#[test]
fn import_converts_a_pinned_keys_config_and_the_result_verifies_the_same_peers() {
    // Given a pre-f2 config with two peers using public_key_pem + sealing_public_key:
    //   - `synapse-cert import` emits a config with account_public_key + certificate_pem per peer
    //     and no occurrence of "public_key_pem" or "sealing_public_key";
    //   - the converted config starts a server (McpConfig::from_toml then the store's checks pass);
    //   - each peer's certificate names the SAME global_id and the SAME signing key id as the
    //     original pinned key — the conversion must not silently re-key anyone.
}
```

- [ ] **Step 2: Run them and check that they fail.**

- [ ] **Step 3: Implement.** `delegate` validates locally before signing by constructing the
  two-link chain and calling `validate_chain` with a lookup that returns the account key, mapping each
  `ChainError` to a message naming the rule that failed. `import` reads the old fields, mints one
  certificate per peer carrying that peer's existing signing and sealing keys, and writes the new
  shape.

- [ ] **Step 4: Run the tests and check that they pass**, then fmt and both clippy invocations.

- [ ] **Step 5: Commit**

```bash
git add src/cert_cli.rs src/bin/synapse_cert.rs tests/synapse_cert_cli.rs
git commit -m "feat(cert): synapse-cert delegate, revoke and import (P2f2)"
```

---

### Task 5: The `synapse-mcp` config and surfaces

**Files:**
- Modify: `src/mcp_server.rs`, `tests/mcp_surface.rs`, `tests/mcp_stdio_process.rs`

**Interfaces:**
- Produces: `PeerConfig { global_id, account_public_key, certificate_pem, address }`;
  `McpConfig.revocations_path: Option<PathBuf>`; `list` reporting each peer's `account_key_id` and
  `certificate` summary; `poll` unchanged except that its certificate summary now carries key ids.

`start()` pins each peer's account key, then its certificate, and fails with a fixed message naming
the peer — never a path — if either is rejected. `revocations_path`, when set, is read at startup and
each block offered to `add_revocation`; a file that does not parse is a startup error.

- [ ] **Step 1: Write the failing tests** in `tests/mcp_surface.rs`, extending `server_with` to emit
  the new peer shape. **Each block below is an assertion specification, not code to paste**: write it
  as real Rust following that file's existing `server_with`/`pair` helpers. Every named assertion and
  every control must appear, asserted on exact values.

```rust
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_peer_is_configured_with_an_account_key_and_a_certificate() {
    // Alice and Bob each carry the other's account_public_key + certificate_pem.
    //   - send and poll work end to end, and the delivered message's certificate summary names
    //     the sending agent's global id and its signing key id;
    //   - list reports each peer's account_key_id;
    //   - Control: a config whose certificate_pem is signed by a DIFFERENT account key fails to
    //     start, with a message naming the peer and no path.
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_expired_peer_certificate_refuses_the_send() {
    // Bob's config carries a certificate that expired an hour ago.
    //   - start fails, or send refuses, with a message naming the peer and the expiry;
    //   - Control: the same certificate within its window sends successfully.
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_revocations_file_is_loaded_at_startup() {
    // revocations_path names a file holding a revocation for Alice's certificate.
    //   - Alice's messages are refused after startup;
    //   - Control: without the file, the same messages are delivered.
}
```

- [ ] **Step 2: Run them and check that they fail.**

- [ ] **Step 3: Implement** the config shape, the startup pinning, the revocations file, and the
  `list` fields.

- [ ] **Step 4: Run the tests**, then each of `mcp_surface` and `mcp_stdio_process` 3 more times to
  catch flakes. Then fmt and clippy on both test targets.

- [ ] **Step 5: Commit**

```bash
git add src/mcp_server.rs tests/mcp_surface.rs tests/mcp_stdio_process.rs
git commit -m "feat(mcp)!: peers carry an account key and a certificate (P2f2)"
```

---

### Task 6: Mutation, verification, docs, PR

- [ ] **Step 1: Mutation check.** Predict in writing first: making `pin_peer_certificate` skip its
  validation (accept any chain without checking it under the account keys) should fail **only** the
  tests that assert an unpinned or wrongly-signed certificate is refused — name them before running.
  Run `cargo test --no-fail-fast --lib` and `cargo test --test synapse_cert_cli --test mcp_surface`
  **as separate commands** (a `--lib name::` filter applies to every target and silently runs zero
  integration tests). Record predicted versus actual honestly; a larger actual set is a finding, not
  a prediction to rewrite. Restore and confirm `git status` is clean.

- [ ] **Step 2: Full run in a FRESH target directory**
  (`scratchpad/target-cli-fresh`, which must not already exist). Record the UTC window, the failing
  set by name, the passing count and the doc-test count.

- [ ] **Step 3: Firewall count** for that window: Windows Firewall event 2097 must be 0, reported
  with a positive control showing the query finds an earlier prompt.

- [ ] **Step 4: `cargo fmt -- --check`, `cargo clippy -- -D warnings`**, and clippy on every new test
  target.

- [ ] **Step 5: Docs.** Update `CAPABILITY_INVENTORY.md`'s f1 note: certificates are now the only
  trust path, direct per-agent pinning is gone, and a peer is configured with an account key plus
  that peer's certificate. State plainly that the router's email path is still unauthenticated.
  Add a short `docs/synapse-cert.md` showing the three commands an operator actually runs: `init` on
  each side, paste the printed block, and `mint` again when a certificate expires.

- [ ] **Step 6: Push and open a draft PR into `feat/agent-certificates`** (#43), whose body carries:
  what changed; the breaking list (direct pinning removed, the peer config shape, `VerifiedChain`'s
  key ids, `verify_at`'s tuple); the verification numbers with their ref; the mutation result; and
  the note that publishing 2.0.0 remains CireSnave's call and now waits on the wider breaking-change
  set recorded on his board as item 38.
