# Brute-force hardening: countermeasures and alerting for every guessable surface (design)

**Status:** DRAFT for PM approval. Nothing is built.
**Why:** CireSnave, verbatim: *"Any potentially brute force findable secret should have countermeasures
and likely alerting to a person in charge about any potential brute force discovery attempts. Do all of
our projects have countermeasures and alerting in all situations like those?"*
**Answer for Synapse today:** no. The audit (PM [FINDING], 2026-10-04) found 18 surfaces:
- 0 have both a countermeasure and alerting;
- 2 have a countermeasure only;
- 16 have neither.

**Measured at:** `origin/main@264ceb6` (5.7.2). Every `file:line` below is at that ref.
**Also read:** `ciresnave/auth-framework@fa66c43`, for what the public endpoint (the companion spec) needs from it.

## 1. Principles

1. **Deleting beats hardening.**
   - A surface no shipped code uses is removed, not defended. Rows 14–17 are library code with no
     caller outside their own module, examples and tests.
   - Removing them is a breaking change to the published crate's public API, so they go together under
     one major version (§5).
2. **Every live surface gets a countermeasure, an event and an alert.**
   - **Countermeasure:** something that makes repeated attempts fail or slow down. Large key entropy is
     not enough on its own, because it says nothing about attempts.
   - **Event:** a record of the attempt, so detection never depends on a log line someone happens to
     read.
   - **Alert:** a route from the event to a person, via `SECURITY_ALERT_EMAIL` where that is
     configured.
3. **Record the attempt before rejecting it.** The event is written on the rejection path itself:
   - a 401, 403 or 429;
   - a failed signature;
   - a failed proof.

   Lightbulb's audit was blind to 401s because it logged only after authentication succeeded.
4. **Limiters are keyed on what the attacker controls least,** and the key is stated per row.

## 2. Shared mechanism: `SecurityEvent`, sinks, alert policy

### 2.1 In the core: types only, no I/O

The core stays vendor-free and network-free. M0's guard enforces this.

```rust
pub enum SecurityEventKind {
    AuthFailure,            // bad bearer / bad claim / bad password / bad API key
    ProofFailure,           // a listener failed the synapsed health proof (port squat)
    UnverifiedSender,       // a knock: signature/chain did not verify
    ReplayRefused,
    RateLimited,            // a limiter tripped
    Lockout,                // a key/user crossed the lockout threshold
    PermissionsTooOpen,     // a secret file others can reach was refused
    NewClient,              // public endpoint: a client identity's first successful use
    Config,                 // off switch toggled, key created/revoked
}
pub struct SecurityEvent {
    pub at: DateTime<Utc>,
    pub kind: SecurityEventKind,
    pub surface: &'static str,   // e.g. "synapsed/bearer"
    pub subject: String,         // what was attempted, never the secret itself (key id, role, source)
    pub source: Option<String>,  // peer address / Access identity where known
    pub detail: String,          // bounded, printable
}
pub trait SecuritySink: Send + Sync { fn record(&self, event: &SecurityEvent); }
```

- **No secret ever enters an event.** Events carry key *ids*, never keys, and hashes never pass through
  this path either. A test asserts this, as in M5a's no-leak test.
- **A limiter** is a small token-bucket keyed by a caller-chosen key. It counts **failures only**, so a
  legitimate caller never pays for it.

  ```
  FailureLimiter::check(key) -> Allow | Delay(d) | Refuse
  ```

### 2.2 Sinks: in the binaries, not the core

| sink | where | what |
|---|---|---|
| `FileSink` | default everywhere | Owner-only, append-only `<home>/security-events.jsonl`, one JSON line per event. It is size-capped and rotates once (keeping the previous file), so a flood cannot fill the disk. |
| `StderrSink` | default for CLI commands | One line per event, through `printable`. |
| `AlertSink` | only when `SECURITY_ALERT_EMAIL` is set | Sends email through an SMTP relay configured beside it: `SECURITY_ALERT_SMTP_URL`, a deployment secret. It uses the existing email transport's sending path, so there is no new dependency. |

**The alert policy decides what reaches a person.**
- **Immediate:** `NewClient`, `Config`, `PermissionsTooOpen`, `ProofFailure`, and the first `Lockout`
  for a key.
- **Threshold:** `AuthFailure`, `UnverifiedSender`, `ReplayRefused` and `RateLimited` alert when one
  `(surface, key)` passes N in a window. The defaults are N = 10 in 5 min.
- **Coalescing:** at most one email per `(kind, surface)` per 15 min; anything further is held for a
  digest. A flood therefore produces one email, not a flood of email.
- **No silent failure:** if the alert itself fails to send, that failure is recorded as an event in the
  file sink.

**Local-only daemon (the per-user `synapsed`).** Its attacker would be another local user, or a process
on loopback.
- By default it uses `FileSink` only.
- It gets `AlertSink` only when its owner sets `SECURITY_ALERT_EMAIL`.
- The public endpoint always has `AlertSink`, because its deployment refuses to start without it.

## 3. Per-surface plan: all 18 rows

**"Today"** is the audit row. **"Planned"** is the countermeasure, its limiter key, the event and the
alert.

| # | surface | today | planned | PR |
|---|---|---|---|---|
| 1 | `synapsed` Bearer session (`crates/synapsed/src/lib.rs` `authed`) | 2^256; no attempt response | Delete nothing. A `FailureLimiter` on the 401 path, keyed by a **global** key: everything on loopback shares one address, so a per-IP key means nothing here. A refusal answers after a growing delay. `AuthFailure` event; threshold alert. | P5 |
| 2 | `synapsed` `/v1/claim` | replay and freshness refusal | Keep that refusal, and add a limiter on `claim_refused` and `malformed`, keyed by the claimed `global_id`, with a global key as backstop. `AuthFailure` event; threshold alert. | P5 |
| 3 | `synapsed` health proof (an HMAC oracle) | none | Limit `?challenge=` requests globally. Proving is cheap, so the limit is generous, but it is a limit. The real signal is the **client** side: a failed proof already warns on stderr (#78), and it also becomes a `ProofFailure` event and an immediate alert. | P5, P8 |
| 4 | `synapsed` `unknown_recipient` 404 | an oracle for role names | Limit by the **session's role**. A burst of 404s from one role is `RateLimited`, then a threshold alert. | P5 |
| 5 | `synapsed` ack `NotFound`/`NotYourLease` | only the caller's own queue | As row 4, same key. | P5 |
| 6 | announce file and session cache | owner-only check | Unchanged as a countermeasure. A refusal becomes a `PermissionsTooOpen` event and an immediate alert, because a file others can reach is an attack or a misconfiguration. | P8 |
| 7 | keystore account and role keys | owner-only, unencrypted | As row 6. Encryption at rest is out of scope and listed under §6. | P8 |
| 8 | certificates and revocation | signatures; enrolment is local only | A failed revocation signature (`sender_auth.rs:314`) and a failed chain become events. | P6 |
| 9 | transport listeners: unverified senders | rejected, knock record | Limit the knock path, keyed by **(claimed global id, source address)** with a per-source backstop. Knocks become events, so they survive in the file sink. Threshold alert, plus an immediate alert for a new knock source after quiet. | P6 |
| 10 | replay suppression | bounded refusal | A `ReplayRefused` event, keyed by sender; threshold alert. | P6 |
| 11 | sealing | open failures | `NotSealed`, `UnsupportedVersion` and `Undecryptable` become `UnverifiedSender`-class events. | P6 |
| 12 | `synapse-mcp-server` | stdio and a loopback UDP transport | Inherits P6 through its `TransportManager`. Events go to its own file sink. | P6 |
| 13 | email Direct-mode SMTP (`email_unified.rs:277`) | open inbound by design, no secret | Keep it open by design. Limit by **source IP** on connections and messages per minute; a trip is `RateLimited`. | P7 |
| 14 | `SynapseEmailServer` default users (`email_server/auth.rs:104-150`) | admin/admin and emrp/emrp123 | **Delete the default users** (P1). Then a `FailureLimiter` on IMAP LOGIN and SMTP AUTH, keyed by **username** with **source IP** as backstop, and a lockout after M failures. `AuthFailure` and `Lockout` events (P7). | P1, P7 |
| 15 | `synapse::auth` `login_with_password` | unsalted SHA-256, an existence oracle | **Delete** (P2). | P2 |
| 16 | `synapse::auth` JWT default secret | forgeable | **Delete** (P2): this is the same module. | P2 |
| 17 | `auth_enterprise.rs`, `auth_v4_example.rs` | enforced nowhere | **Delete** (P3). | P3 |
| 18 | IMAP/SMTP failed-login logging | not logged | Covered by row 14's events (P7). | P7 |

## 4. What "nothing uses it" rests on, with controls

Each query was run with `git grep` at `264ceb6`. Matches inside the module's own directory or file are
excluded.

**P1: email default users.**
- `SynapseAuthHandler::new()` is called by:
  - `SynapseEmailServer::new`/`new_with_scope` (`email_server/mod.rs:47,105`);
  - `create_test_auth_handler` (`auth.rs:328`, public; it also adds `testuser/testpass`).
- `create_test_auth_handler` is used by `examples/email_server_demo.rs:42`, `create_test_email_server`
  (`mod.rs:224`), and the `smtp_server.rs` tests (`:716`, `:754`).
- No shipped binary or router path starts `SynapseEmailServer`. `router_merged` only ever stores `None`.
- **Plan:**
  - `new()` starts with **no users**;
  - `create_test_auth_handler` becomes `#[cfg(test)]`, with the example given its own setup;
  - the IMAP server stays, because the email transport's tests use it as a peer (`email_unified.rs:57`).
- **Red first:** a fresh `SynapseAuthHandler` refuses `admin/admin` and `emrp/emrp123`. That test fails
  on main.
- **Positive control:** a user added with `add_user_with_password` still authenticates.

**P2: `synapse::auth`.**
- Outside `src/synapse/auth/`, only `utils` is used:
  - `blockchain/verification.rs:399`;
  - `tests/key_manager_properties.rs:12`;
  - `tests/security_test.rs:9`;
  - the orphan `src/auth_integration.rs:599`, which `lib.rs` does not declare.
- Control: the same query does find those four.
- **Plan:** delete `SynapseAuth` (`mod.rs`), `api`, `example`, `middleware` and `trust_bridge`, and
  keep `utils`. That removes `login_with_password`, `get_jwt_secret` (with its fallback at
  `mod.rs:568-572`) and the hardcoded `secret_key` (`mod.rs:293`).
- **Red-first check:** the removed public items no longer compile. A deny-test, like the existing
  `dead_code_is_gone.rs`, asserts that the strings `default_jwt_secret_change_in_production` and
  `synapse-secret-key` appear in no tracked source.
  - Positive control: the same scan finds a known string.

**P3: `auth_enterprise.rs` and `auth_v4_example.rs`.**
- Declared `pub mod` at `lib.rs:264-265`.
- No callers in `src` or `crates`. Their only users are three examples:
  - `examples/enterprise_ai_auth_platform.rs`;
  - `examples/federated_auth_demo.rs`;
  - `examples/auth_framework_v4_demo.rs`.
- **Plan:** delete the modules and those examples. The orphan `src/auth_integration.rs` goes in the same
  PR.

## 5. PRs, order and versions

One PR per concern. Each is red-first where behaviour changes.

| PR | content | breaking? |
|---|---|---|
| P1 | email: no default users; `create_test_auth_handler` becomes test-only | **yes** (public behaviour and API) |
| P2 | delete `synapse::auth` except `utils`, plus the deny-test for the two secret strings | **yes** (public API) |
| P3 | delete `auth_enterprise`, `auth_v4_example`, their 3 examples, and the orphan `auth_integration.rs` | **yes** (public API) |
| P4 | `SecurityEvent`, `FailureLimiter` and `SecuritySink` in the core; `FileSink`, `StderrSink` and `AlertSink` in a small shared crate the binaries use | additive |
| P5 | `synapsed`: rows 1–5 | additive (new responses: delays and 429) |
| P6 | transports and `synapse-mcp-server`: rows 8–12 | additive |
| P7 | email limiters and lockout: rows 13, 14, 18 | additive |
| P8 | CLI events: rows 3 (client side), 6 and 7 | additive |

- **Versions.** P1–P3 are breaking. By CireSnave's rule that means a major bump, **6.0.0**, so it is best
  to land them back to back and take one major for the three. P4–P8 are minors after that. The PM
  allocates every number at gate time.
- **Nothing here publishes.** Publishing is still held on RUSTSEC-2026-0258 (board 102).

**Tests per PR**, following the PM's rule for #77: a negative control (the attempt is refused, delayed or
locked out, **and** an event is written) and a positive control (a legitimate caller is unaffected).
- Alerting is tested against a capturing sink.
- `AlertSink` gets one test against a local SMTP peer: the existing `SynapseSmtpServer` in Direct mode,
  on loopback.

## 6. Out of scope (recorded, not planned)

- **Encrypting the keystore at rest** (row 7). The threat is the same OS user, against whom no file
  protection helps.
- **Persisting limiter state across restarts** for the local `synapsed`: an attacker who can restart it
  is already the owner. The public endpoint's limiter is covered in its own spec.
- **Unused crates.io surface** beyond rows 14–17: the old `synapse::api`, `blockchain` and `services`
  trees. They are not brute-force surfaces; whether to delete them is the FAM/Synapse merge's call.
