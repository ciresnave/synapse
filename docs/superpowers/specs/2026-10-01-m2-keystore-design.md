# M2: persistent keystore (design)

**Status:** DRAFT for PM approval. Nothing is built.
**Plan:** `docs/superpowers/plans/2026-09-29-claude-peers-replacement.md`, milestone M2. PM [TASK] 2026-10-01;
the scope was confirmed by the PM against the plan's table.
**Measured at:** `origin/main@9a2359f` (5.0.0).

## 1. Goal

Any process can load a **stable identity** from disk. That has three parts:
- an **account key** created once per user;
- per-**role** signing and sealing keys;
- a role certificate that the account key signs locally, using f1's `AgentCertificate`.

A restarted session that names the same role gets the same keys and the same `global_id`. That is
the foundation for M3 (role takeover) and for the PM's #1 pain, rediscovery after a restart.

This also closes PR #56's disclosed gap: neither `synapse-router` nor `synapse-client` can persist a
signing keypair across runs.

## 2. What exists (measured at 9a2359f)

- **f1 certificates** are in `src/certificate.rs`:
  - `AgentCertificate { serial, issuer_key_id, subject_label, subject_global_id, subject_signing_key,
    subject_sealing_key, not_before, not_after, permissions, may_delegate, signature }`;
  - `AgentCertificate::sign(unsigned, &ed25519_dalek::SigningKey)`, plus `to_pem`/`from_pem` and
    `chain_to_pem`/`chain_from_pem`;
  - `validate_chain` resolves the root's issuer **by `issuer_key_id`** through the receiver's pinned
    account keys.
  - Permissions: `Send`, `RequestAck`, `Ack`, and `Extension("x-…")`.
- **Key formats in use:**
  - agent signing: ring `Ed25519KeyPair`, as PKCS#8 v2 PEM, via
    `CryptoManager::generate_keypair`/`load_private_key`. This is the format `synapse-mcp` loads
    today.
  - sealing: `SealingKeyPair::to_pkcs8_pem`/`from_pkcs8_pem` (X25519).
  - the account key, for signing certificates: `ed25519_dalek::SigningKey`. The f1 tests mint it from
    seed bytes, and **no on-disk format exists yet**.
- **Nothing in the crate stores keys on disk** for later use.
- **Overlap to note:** memory records a planned "f2" (a `synapse-cert` CLI, a config change, and
  removing direct pinning), but no f2 plan or spec is on `main`. **M2 covers f2's key-and-certificate
  issuance half.** Removing direct pinning stays separate. M2 doesn't change how receivers pin.

## 3. Design

### 3.1 Placement (model-agnostic, per §5.1d)

- **`src/keystore.rs` in the core** (feature `crypto`). It does file I/O only: no network, no clock
  (`now` is passed in, as in `certificate.rs`/`replay.rs`), and no vendor names. The M0 guard
  enforces the last point.
- **A new crate `crates/synapse-cli`** with the binary `synapse`. This is the seed of the plan's
  adapter (c), and M5 adds the daemon commands to it. M2 adds only `synapse id init` and
  `synapse id show`.

### 3.2 Layout on disk

```
<home>/                              home = $SYNAPSE_HOME, else the platform data dir + "synapse"
  account/account.key.pem            Ed25519 account key, PKCS#8 (ed25519-dalek `pkcs8` feature)
  account/account.toml               account = "<name>", key_id = "<hex>"   (no secrets)
  roles/<role>/signing.key.pem       ring PKCS#8 v2, the same format synapse-mcp loads today
  roles/<role>/sealing.key.pem       SealingKeyPair PKCS#8
  roles/<role>/cert.pem              the role's certificate chain (one link: the account signs the role)
```

- **The platform data dir** is `%LOCALAPPDATA%` on Windows and `$XDG_DATA_HOME`, else
  `~/.local/share`, elsewhere. It uses `std::env` only, with no new dependency.
- **Identity:** `global_id = "<role>@<account>"`, for example `synapse@ciresnave`.
- **Role and account names** follow `identity_narrows`'s label rule: ASCII alphanumerics, `-` and
  `_`, non-empty, at most 64 characters. Anything else is refused, which also makes a name safe to
  use as a path component.

### 3.3 Behaviour

- **`Keystore::init_account(home, account)`**
  - It creates the account key.
  - It **refuses if one already exists**. Overwriting an account key silently loses the identity of
    every role.
- **`Keystore::open(home)`** fails when there is no account. It never creates one implicitly.
- **`keystore.role(role, now) -> RoleIdentity`**
  - It loads the role's keys, or creates them on first use.
  - It issues a certificate when none exists or less than half its window remains (see Q3), signed
    by the account key.
  - `RoleIdentity` gives:
    - `global_id`;
    - a `CryptoManager` with `load_private_key`, `load_sealing_key_pem` and `set_certificate_chain` applied
      (`set_certificate_chain`);
    - a summary: `key_id`s, the validity window, the permissions.
- **Every write is atomic:** write a temp file in the same directory, `sync_all`, then `rename`. A
  crash can never leave a half-written key. A key file that exists is never rewritten. Only
  `cert.pem` is replaced, on renewal.
- **Permissions on Unix:** directories `0700`, files `0600`. They are set at creation and **checked
  on every load**, and a key readable by group or others is refused, as OpenSSH does. Windows is Q2.
- **Secret hygiene, carried over from `synapse-mcp` §6:**
  - no `Debug` that prints key bytes;
  - no key bytes or **key file paths** in any error, log line or CLI output;
  - output may carry only `global_id`, `key_id`s, public keys and validity windows.
- **The CLI:**
  - `synapse id init --account <name>` creates the account, refusing if one exists, and prints the
    account and its `key_id`.
  - `synapse id show [--role <r>]` prints the account and `key_id`, and with `--role` also the role's
    `global_id`, `key_id`s and validity. `show --role` creates the role on first use, the same as
    any loader.

## 4. Questions for the PM (or CireSnave)

- **Q1. Home location.** Recommended: `$SYNAPSE_HOME`, else `%LOCALAPPDATA%\synapse` on Windows, else
  `$XDG_DATA_HOME/synapse` or `~/.local/share/synapse`.
- **Q2. Owner-only on Windows.** Options:
  - **(a) Recommended.** Create the home under `%LOCALAPPDATA%`, where inheritance gives the user,
    SYSTEM and Administrators access. On load, **check** the ACL (`GetNamedSecurityInfoW`, through
    `windows-sys`) and refuse if any other principal can read: `Everyone`, `Users` or
    `Authenticated Users`. This is the same rule OpenSSH for Windows applies to private keys.
  - **(b)** Set an explicit owner-only DACL at creation. This is stricter, but it shuts out
    SYSTEM-run backup tools.
  - **(c)** Rely on inheritance and don't check.

  (a) or (b) adds one Windows-only dependency, `windows-sys`.
- **Q3. Certificate validity.** CireSnave asked for short validity windows instead of revocation
  lists (identity roadmap). Recommended: **24 h windows, renewed on load once under 12 h remain.**
  The account key is local, so renewal never needs a human.
- **Q4. Default role permissions.** Recommended: `send`, `request-ack` and `ack`, which is what every
  lane does today. `x-` grants are left to a later milestone.

## 5. Acceptance (the plan tests this; listed here so the PM can approve the bar)

- **The same role on a second load** gives the same `global_id` and the same signing and sealing
  public keys, and the certificate is not reissued while it's valid.
- **A fresh certificate passes f1's `validate_chain`** against the account key, used as the pinned
  account key: issuer by `key_id`, signature, window and permissions.
- **Refusals:**
  - `init` when an account already exists;
  - an invalid name;
  - `open` with no account;
  - a key whose permissions are too wide (on Unix, and on Windows per Q2);
  - a truncated or garbage key file, with an error that names neither the path nor any bytes.
- **Renewal:** with `now` near expiry, the certificate is reissued and the keys are unchanged.
  With `now` mid-window, nothing is rewritten.
- **The crash window:** a temp file left by an interrupted write never becomes a key, and the next
  load ignores it or cleans it up.
- **Secret hygiene:** a test scans every error string and all CLI output for the home path and for
  the PEM bodies.
- **The M0 guard stays green:** no vendor names in core.

## 6. Size

Estimated at 1–2 PRs and about 1 lane-day, as in the plan. The Windows ACL check in Q2(a) is the
part most likely to run long.
