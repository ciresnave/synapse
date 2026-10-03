# M5c: daemon authentication (#74) and rejection-branch tests (#77) (design)

**Status:** DRAFT for PM approval. Nothing is built.
**Tasked:** PM, 2026-10-03 05:38Z: spec first as [READY], then one PR per item; nothing publishes and no
lane relies on `synapsed` until both land; no M6.
**Measured at:** `origin/main@274cfed` (5.6.2). Every `file:line` below is at that ref. For `src/` and
`crates/`, `git diff 0afe40a 274cfed` touches only Cargo.toml version lines, so #77's line numbers
(measured at 0afe40a) are still current.
**Versions (PM allocates at gate):** 5.7.0 for #74, since it adds to the daemon protocol; 5.7.1 for #77.

---

## Item 1: #74, the CLI authenticates the daemon before sending credentials

### 1.1 Threat and scope

- **In scope:** any local process, another user's included, that binds the port named in a stale
  `<home>/synapsed.json`. Today it receives the cached Bearer token, or a fresh claim, and can forge
  replies (#74 repro).
- **Out of scope:** a process running as the same user. It can read the keystore and the session cache
  directly, so no protocol stops it.
- **What the attacker cannot do:** read `synapsed.json`. It is owner-only, and the client already
  refuses a looser one (`crates/synapse-cli/src/mail.rs:308`). So `instance_id` is a secret shared by
  the daemon and this user's clients, and is the key for the proof.

### 1.2 Proof of possession on `/v1/health`

**Protocol.** `GET /v1/health?challenge=<32 lowercase hex>`, a 16-byte random challenge from the client.

- **The daemon** answers its existing fields plus
  `"proof": hex(HMAC-SHA256(key = instance_id's 16 raw bytes, msg))`, where
  `msg = "synapsed-health-v1\n" || <bound addr as "ip:port"> || "\n" || <the challenge's 16 raw bytes>`.
- **The client** recomputes the HMAC from the announce file's `instance_id` and `addr` and the challenge
  it sent. It checks with `ring::hmac::verify` (constant time). Anything else counts as "no daemon":
  no `proof`, the wrong proof, or a malformed proof.
- **`ring` 0.17 is already a dependency of both crates.** No new dependency.

**Edge cases.**
- **A malformed challenge** gets 400 `malformed`, through the same strict `unhex` (`lib.rs:369`).
- **Health with no challenge** still answers `ok`, without `proof`. It stays a liveness probe for
  anything that only wants that, but it proves nothing.
- **The domain tag** separates this MAC from any other use of the key. `instance_id` is also the claim
  audience (`lib.rs:451`), but there it is a signed string, not a key.

**Why the address is in the MAC.** The health endpoint is an oracle: anyone on loopback can get a MAC
of any challenge. Binding the bound address into the MAC makes a relay useless. A squatter on a stale
port A could forward the client's challenge to a live daemon on port B and return its answer, but that
daemon's MAC covers B and the client expects A.

### 1.3 Every credential waits for the proof

- **Construction is the gate.** `Daemon::connect` returns a `Daemon` only through `answering()`
  (`mail.rs:146,156`), and `answering()` is where the proof is checked. `claim`, `call` and `session`
  are methods on `Daemon`, so no path can send a Bearer token or a claim (chain and signature) to an
  unproven listener.
- **The test asserts this on the wire, not by reasoning.** See §1.6.
- **The pid-liveness check stays** (`mail.rs:303–366`) as defence in depth. Its test,
  `a_dead_daemons_port_is_not_trusted`, keeps passing unchanged. The module doc comment
  (`mail.rs:17–20`) and `daemon_alive`'s comment are rewritten: the pid check is no longer the interim
  stand-in.

### 1.4 What the client does when the proof fails — **PM decision**

A failed proof means the listener is not this home's daemon.

- **(A) Recommended:** ignore it, as for a dead daemon, and print one stderr line:
  `synapse: <addr> answered health but could not prove it is this home's synapsed; ignoring it`.
  Then start a daemon as today.
  - With the default fixed port 127.0.0.1:7920, the squatter still holds the port. The new daemon fails
    to bind and the command fails with `NoDaemon`. The error text gains the warning, so the user learns
    why.
  - With `SYNAPSE_ADDR=127.0.0.1:0` (the tests), the new daemon binds a fresh port and the command
    succeeds.
  - Either way, no credential reaches the squatter. That is the property #74 asks for.
- **(B):** fail the command outright with an "impostor" error, and never auto-start.
  - Stricter, but a stale file whose port an unrelated service now uses would then block the CLI until
    the user deletes the file by hand.

#74's test text says "`send` must fail". That was written for the fixed port. Under (A) the test asserts
instead:
- no `Authorization` header and no claim body reached the squatter;
- the squatter did receive a challenged health request (positive control: the squatter was really asked);
- the warning line appeared;
- a fresh daemon took over.

### 1.5 `synapsed` removes its announce file when it stops

- **Where:** inside `Daemon::serve`, as a guard created right after `write_announce` (`lib.rs:186`).
  - It runs on graceful shutdown, on a `serve` error, and on unwind.
  - The lib tests drive it in process through their oneshot shutdown (`crates/synapsed/tests/api.rs`).
- **Only its own file:** the guard reads the file and removes it only if `instance_id` is its own, so it
  never deletes a newer daemon's announcement.
  - Today a second daemon on the home exits at `Daemon::open` on redb's lock, before it writes.
  - The check costs one read, and it keeps the guard correct if that ever changes.
- **Signals (`main.rs:53–55`).** Today shutdown is `ctrl_c` only.
  - **Unix:** add SIGTERM, the default `kill`, and SIGHUP. The auto-started daemon runs under `setsid`,
    so SIGHUP reaches it only when it is sent on purpose.
  - **Windows:** add `ctrl_break`, `ctrl_close`, `ctrl_logoff` and `ctrl_shutdown` (tokio's
    `signal::windows`, already enabled by the `signal` feature).
- **Honest limits.**
  - An auto-started daemon on Windows is `DETACHED_PROCESS` (`mail.rs:409`). It has no console, so no
    console event reaches it. Ending it with `taskkill /F`, Task Manager, `TerminateProcess` or logoff
    runs no handler.
  - On Unix, SIGKILL, a crash or power loss runs no handler either.
  - **In all those cases the file stays behind.** Removing the file is hygiene. The proof in §1.2 is the
    fix, and it holds whether or not the file was removed.

### 1.6 Tests (TDD: the first test is red on main for the right reason)

1. **`a_squatter_on_a_live_pids_port_gets_no_credential`** (`crates/synapse-cli/tests/mail.rs`).
   The new failing test. It must defeat the pid check, which `a_dead_daemons_port_is_not_trusted` does
   not.
   - **Setup:** home H claims `alpha`. Kill H's daemon and bind the squatter on its port, as the existing
     test does. Start a second daemon in another home H2 with `synapse claim` there. Rewrite only H's
     announce `pid` to H2's live synapsed. The file's `addr` and `instance_id` stay genuine.
   - **What it simulates:** pid reuse by a live `synapsed`, which passes `daemon_alive` on both
     platforms.
   - **Red on main:** the squatter logs `authorization: bearer`. Confirm it is red for that reason,
     not for a setup error, by reading the failure message.
   - **Green after:** the §1.4 (A) assertions.
2. **`health_proves_the_instance_id`** (`crates/synapsed/tests/api.rs`).
   - **Positive:** the proof for a random challenge equals the HMAC computed from the announce file.
   - **Negative:** the same proof fails against a different challenge, and against a different address.
     A health request with no challenge carries no `proof`. A malformed challenge gets 400.
3. **`stopping_removes_only_its_own_announce_file`** (`api.rs`).
   - **Positive control:** the file exists while serving.
   - After a graceful stop it is gone.
   - **Negative:** if the file was overwritten with another `instance_id` before the stop, it is left in
     place.
4. **`sigterm_removes_the_announce_file`** (`mail.rs`, `#[cfg(unix)]`, so CI-only). Send SIGTERM to the
   announced pid. The file is gone and the process has exited. The control is the file being present
   before the signal.
5. **The whole existing CLI suite** is the positive control that real daemons pass the proof. Every
   command goes through it.

**Review:** a fresh Opus review of #74's diff before [READY] (PM, security-relevant).

### 1.7 Compatibility and residual risk

- **A new CLI against an old daemon:** there is no proof, so it is ignored. The new daemon the CLI then
  starts cannot open the locked store, and the command fails with `NoDaemon`. Restart `synapsed` after
  upgrading. No lane relies on synapsed yet (PM condition), so nobody is broken.
- **The window between proof and use.** The proof covers the listener at health time, and the
  credential is sent in a later request within the same command (ms, under `CALL_TIMEOUT`). To exploit
  that window an attacker must make the real daemon die and bind its port inside it.
  - reqwest's pool usually reuses the very connection that was proven, which narrows the window
    further, but nothing guarantees that.
  - Accepted and documented. Closing it fully needs a channel-bound credential, which this design does
    not add.

---

## Item 2: #77, tests for rejection branches no test executes

**Rule (PM):** every test has a negative control (the rejection fires on the bad input) and a positive
control (the same path accepts the good input). After the PR's coverage job runs, each target line must
be hit in its lcov artifact.
- **Download:** `gh run download <run> -n lcov`.
- **Check:** a small script that prints the hit count of each `file:line` listed here, a rewrite of the
  old `lcov_sec.py`. The baseline is run 37097848025 (artifact `lcov`, kept until 2027-01-01), where
  every line below is 0. The PR body carries the before/after table, with both run ids.
- **Line numbers:** #74 lands first and shifts `crates/synapsed/src/lib.rs`. Those targets are named by
  branch here and get their line numbers re-measured at #77's merge-base.

| # | target (at 274cfed) | test file | negative / positive |
|---|---|---|---|
| C1 | `certificate.rs:581` `issuer_key_id` ≠ parent key → `BadSignature` | `tests/agent_certificates.rs` | cert validly signed by the parent but declaring another `issuer_key_id` / same cert with the right id |
| C2 | `certificate.rs:586` `NotYetValid` | same | `now` before `not_before` / `now` inside the window |
| C3 | `certificate.rs:589` `Expired` | same | `now` after `not_after` / inside |
| C4 | `certificate.rs:281,290,307` cert PEM: wrong label, wrong domain tag, trailing bytes | same | each mutation of a valid PEM / the valid PEM parses |
| C5 | `certificate.rs:259,364` invalid issuer public key → `verify_signature` false (cert, revocation) | same | a non-point 32-byte key / the real key verifies |
| C6 | `certificate.rs:381,390,401` revocation PEM: label, domain tag, length | same | as C4 |
| S1 | `sender_auth.rs:314` revocation with a bad signature refused | `tests/sender_authentication.rs` | forged signature → `false` and not revoked / genuine one → `true` and revoked |
| S2 | `sender_auth.rs:338–353` `MAX_REVOCATIONS` (4096) eviction | same | 9 issuers × 512 → total stays ≤ 4096 and the biggest issuer's oldest is evicted / below the cap nothing is evicted |
| S3 | `sender_auth.rs:388` `ChainTooLarge` by **link count** | same | 65 links with `max_chain_bytes` raised so the byte check (`:384`) passes / 64 links not refused for size. The existing 65-link unit test (`sender_auth.rs:720`) trips the byte check first, which is why `:388` is 0 |
| D1 | `synapsed` `NotYourLease` → 403 | `crates/synapsed/tests/api.rs` | role B acks role A's leased message / A acks it |
| D2 | `synapsed` `locked()` poisoned → 503 `store_unavailable` | unit test in `crates/synapsed/src/lib.rs` | `locked` on a poisoned mutex / on a healthy one. Unreachable over HTTP without a test hook; a unit test adds none |
| D3 | `synapsed` `message_id` with `/` or a control char → 400 `malformed` | `api.rs` | `a/b`, `a\u{7}` / `ab` queued |
| D4 | `synapsed` heartbeat summary too long or with a control char → 400 | `api.rs` | 501 chars, `\n` / 500 chars accepted |
| D5 | `synapsed` `mailbox_full` → 413 | `api.rs` | fill one mailbox past `max_bytes` (64 MiB) with bodies under axum's 2 MB limit / the send before it is queued |
| L1 | `sealing.rs:273` sealed marker present with an unknown value → `UnsupportedVersion` | `tests/sealing.rs` | the marker value changed / the genuine marker opens. The body's version byte (`:285`) gets the same pair if the baseline shows it at 0 too |
| L2 | `sealing.rs:288` undecryptable encapsulated key | — | **Unreachable; not tested.** hpke 0.14.1 `X25519` `from_bytes` fails only on length (`dhkex/x25519.rs:58–66`), and the slice is always `ENC_LEN` = 32. Recorded in the PR so the zero stays explained |
| K1 | `keystore.rs:213` second writer of the account name → `AccountExists` | `tests/keystore.rs` | name file already present, key absent → refused / fresh home → created |
| K2 | `keystore.rs:207` key file created between the check and the write | `tests/keystore.rs` | Only a real race reaches it: threads racing `init_account` on fresh homes, asserting exactly one winner per home. Best-effort: the PR reports whether `:207` was hit, and does not claim it if it was not |
| M1 | `mail.rs:493–495` role-lock timeout | `crates/synapse-cli/tests/mail.rs` | the test holds `<home>/sessions/<role>.lock` (`File::lock`, std at MSRV 1.89) → `claim` fails after 10 s naming the role / after release, the claim succeeds. Costs 10 s of test time. Adding a knob to shorten it would be new surface, so none is added |

**Not covered by this item:** `cfg(windows)` code and non-default features. The coverage job does not
measure them (header of the `coverage` job in `ci_cd.yml`). The keystore's "I/O-failure cleanup" in
#77's list has no stable line target yet. The plan locates it, or the PR says it is not done.

---

## Order and deliverables

1. This spec → PM approval.
2. A plan for #74 → TDD from test 1.6.1 → fresh Opus review → PR → [READY].
3. After #74 merges: a plan for #77 → tests → PR with the lcov before/after table → [READY].

One subagent at a time; Sonnet for #77's mechanical tests.
