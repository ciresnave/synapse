# M5a: the `synapsed` daemon (implementation plan)

> **For agentic workers:** REQUIRED SUB-SKILL: use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task by task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `crates/synapsed`, a loopback JSON daemon over `Mailbox<RedbStore>`. It provides claim,
send, fetch, ack, heartbeat, list and health, with the PM's four security conditions. It is tested
over real HTTP.

**Spec:** `docs/superpowers/specs/2026-10-02-m5-synapsed-design.md`. The PM approved Q1–Q5 as
recommended on 2026-10-02.
- **Stop rule:** after M5a, send [READY] and stop. M5b starts from the new main.
- **No version bump.**
- **The PM's conditions:**
  1. claims prove the role key before any token is issued;
  2. a Host-header check, no CORS, and Bearer on every route except health (claim is the
     token-issuing exception and is self-proving);
  3. constant-time token handling, and tokens never logged;
  4. the tests listed under Task 2.

## Global constraints

- **Toolchain:** stable, via PowerShell, with `cargo --version` in logs. SPDX headers on every new
  file.
- **Model-agnostic:** no vendor names in core. The core changes are only `Mailbox::check` and
  `Mailbox::roles_snapshot`, both read-only pass-throughs.
- **Address:** default `127.0.0.1:7920`, overridden by `SYNAPSE_ADDR`. A non-loopback IP is refused
  at startup.
- **Store:** `<SYNAPSE_HOME>/mailbox.redb` via `RedbStore::open`, plus the account key from M2's
  `Keystore::open(home)`. Claims are verified against that account key.
- **Tokens:**
  - 32 bytes from `ring::rand::SystemRandom`, sent to the client as 64 hex characters;
  - stored ONLY as their SHA-256, in `HashMap<[u8; 32], Session>`, looked up by hash. The secret is
    never compared byte-wise and never stored.
  - Tokens are never logged; the daemon logs only roles and epochs.
- **Every request:**
  - The `Host` header must equal `127.0.0.1:<port>` or `localhost:<port>`, otherwise 421.
  - POST bodies must be `Content-Type: application/json`, otherwise 415.
  - No `Access-Control-*` headers on any response.
  - `/v1/health` and `/v1/claim` take no Bearer; every other route requires
    `Authorization: Bearer <64 hex>`, otherwise 401.
- **Session check:** an authenticated call first runs `mailbox.check(role, epoch)`. A superseded
  session gets 409 `{error:"superseded", current}`.
- **Error map:**
  - `MailboxFull` gives 413;
  - `Malformed` and `LeaseOutOfRange` give 400;
  - `NotFound` gives 404;
  - `NotYourLease` gives 403;
  - `Claim(_)` gives 403 `claim_refused`;
  - `Store` and `Poisoned` give 503.
- **Bodies:** `deny_unknown_fields`, so a `from` sent by the client is a 400. `from` always comes from
  the session.
- **Sweep:** hourly, on a tokio interval.

## Review focus

1. **Authentication bypass:** any route reachable without a Bearer, apart from health and claim.
   Any way to reach the mailbox under another role's identity.
2. **Token secrecy:** a token in any log line, error body, `Debug` output, or response other than the
   claim reply.
3. **The Host check:** missing Host, Host with another port, `localhost` vs `127.0.0.1`, IPv6.
4. **A superseded token after takeover** is refused before any store access.
5. **Mutex scope:** no `await` while holding the mailbox lock, and no deadlock between the mailbox
   lock and the session or presence locks.

---

### Task 1: The crate, state, auth and endpoints

**Files:**
- Create `crates/synapsed/{Cargo.toml, src/lib.rs, src/main.rs}`.
- Modify `src/mailbox.rs`: add `pub fn check(&self, role, epoch) -> Result<(), Superseded>` and
  `pub fn roles_snapshot(&self) -> RolesState`.

**Interfaces (lib):**
```rust
pub struct DaemonConfig { pub home: PathBuf, pub addr: SocketAddr, pub sweep_every: std::time::Duration }
pub enum DaemonError { NotLoopback, Keystore(KeystoreError), Store(StoreError), Mail(MailError), Io(std::io::ErrorKind) }
pub struct Daemon { /* Arc<State> */ }
impl Daemon {
    pub fn open(cfg: &DaemonConfig) -> Result<Daemon, DaemonError>;          // keystore + store + mailbox; refuses non-loopback
    pub async fn serve(self, listener: tokio::net::TcpListener, shutdown: impl Future<Output = ()> + Send + 'static) -> std::io::Result<()>;
}
pub mod wire { ClaimBody{chain_pem, nonce_hex, signed_at, signature_b64}, SendBody{to, message_id: Option<String>, body_b64}, FetchBody{max: Option<usize>, lease_secs: Option<u64>}, AckBody{message_id}, HeartbeatBody{summary: Option<String>} }
```

- [ ] **Step 1: Write failing tests** for the API's basic behaviour (Task 2 lists every test).
- [ ] **Step 2: Run.** Expected: a compile FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: PASS.
- [ ] **Step 5: Commit.**

### Task 2: Security and contract tests over real HTTP (the PM's condition 4, plus review focus)

**File:** `crates/synapsed/tests/api.rs`. The daemon is bound on `127.0.0.1:0`, with a temp
`SYNAPSE_HOME` holding a keystore whose account has the roles `alpha` and `beta`.

**Tests:**
- `health_answers_without_a_token`
- `a_round_trip_between_two_roles`: claim both, alpha sends to beta, beta fetches (with `from ==
  alpha@acct`) and acks.
- `a_forged_from_is_refused`: a `from` field in the send body gives 400. A send without it has the
  session role as its from.
- `a_bad_or_missing_token_is_401`, on each authenticated route.
- `an_old_epoch_token_is_409_after_takeover`: claim twice; the first token on send gives 409 with
  `current: 2`.
- `a_claim_without_key_proof_issues_no_token`: a tampered signature gives 403 `claim_refused` and no
  `session` field.
- `a_wrong_host_header_is_421`: raw TCP with `Host: evil.example:PORT`, and with the right host but
  the wrong port.
- `no_response_carries_cors_headers`: including an `OPTIONS` preflight.
- `a_post_without_json_content_type_is_415`
- `a_non_loopback_bind_is_refused`: `Daemon::open` with `0.0.0.0:0` gives `NotLoopback`.
- `a_second_daemon_on_the_same_home_fails`: the store lock.
- `no_token_or_key_material_leaks`: every response body except claim's must not contain the token,
  and none may contain `PRIVATE KEY`.

### After the tasks
- The full suite, clippy, fmt, and the M0 guard.
- One fresh reviewer: **Opus, because the PM asked for an Opus review of the auth diff**.
- Then push, take #71 out of draft, and send [READY]. STOP.
