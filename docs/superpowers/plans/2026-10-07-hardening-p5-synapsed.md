# Hardening P5: `synapsed` countermeasures and events, audit rows 1–5 (plan)

**Spec:** `docs/superpowers/specs/2026-10-07-brute-force-hardening-design.md` §3, rows 1–5. P4 (#84) built
the parts: `synapse::security_events` (`SecurityEvent`, `FailureLimiter`) and `synapse-security`
(`FileSink`).

**Scope:** `crates/synapsed` only. Additive on the wire: no new status codes. Failures are answered
**later**, never differently; a health reply over budget is answered later, never refused.

**Alert DELIVERY is still only a seam** (`AlertTransport`, no implementation, board 131). P5 writes events
to the file sink only; no `AlertSink` is wired.

## Rules every row follows

1. **Delay the failure, never the attempt.** A request is evaluated at full speed. Only a *failed* answer
   (or, for row 3, an over-budget one) is held back before it is sent. So a legitimate caller is never
   slowed by someone else's failures, even under the global keys rows 1–3 use.
2. **No lockout anywhere in synapsed.** Every limiter has `lockout_after = u32::MAX`. A global key that
   could lock would let any local user lock the owner out.
3. **Never call `record_success`.** On a global key a success would reset an attacker's count.
4. **Record before rejecting.** The event is written on the failure path, before the response is sent.
5. **No secret in an event.** Subjects are `bearer`, `claim`, `health`, a role's global id, or a claimed
   global id (attacker-supplied, so it passes through `sanitize` like every field). Never a token, nonce,
   signature or challenge.

### What this does and does not buy (stated in the PR)

Delaying failures bounds how fast an attacker *learns* failures, but does not bound how many guesses
run in parallel: the guesses are already evaluated. Against a 2^256 bearer and an Ed25519 claim, the
entropy is the real countermeasure. The new value of P5 is the **record**: every attempt now leaves
an event in an owner-only file, where before it left nothing.

## Files

| file | change |
|---|---|
| `crates/synapsed/Cargo.toml` | add `synapse-security = { path = "../synapse-security", version = "<workspace version>" }`. The version bump becomes **six** places. |
| `crates/synapsed/src/lib.rs` | `SecurityConfig`, `Daemon::with_security`, the sink, the limiters, the delay layer, rows 1–5 |
| `crates/synapsed/src/main.rs` | doc line only: events go to `<home>/security-events.jsonl` |
| `crates/synapsed/tests/api.rs` | the tests below |

## API

```rust
/// Tuning for the daemon's limiters and its event file. `Default` is what `synapsed` runs with.
#[derive(Debug, Clone)]
pub struct SecurityConfig {
    pub bearer: LimiterConfig,   // row 1, global key "bearer"
    pub claim: LimiterConfig,    // row 2, keys: claimed global id, and "claim" as backstop
    pub health: LimiterConfig,   // row 3, global key "health", counts every challenged request
    pub misuse: LimiterConfig,   // rows 4–5, key: the session's global id
    /// At most one event line per (kind, surface) per interval; later ones are counted and the
    /// count rides on the next line written. Zero writes every event.
    pub event_interval: std::time::Duration,
}

impl Daemon {
    /// Replaces the default tuning. Call after `open`, before `serve`.
    #[must_use]
    pub fn with_security(self, config: SecurityConfig) -> Daemon;
}

pub const SECURITY_EVENTS_FILE: &str = "security-events.jsonl";
pub enum DaemonError { ..., /// The security event file could not be opened safely.
                       Security(String) }
```

- **`DaemonConfig` is unchanged.** A new pub field would break every struct literal.
- `Daemon` holds `Shared` by value until `serve`, which wraps it in an `Arc`; `with_security` then
  rebuilds the limiters without `Arc::get_mut`.
- **`Daemon::open` opens `FileSink::open(<home>/security-events.jsonl, 4 MiB)`.** If that fails (for
  example, the file exists and others can reach it), `open` returns `DaemonError::Security` and the
  daemon does not start, as for the keystore.

**Defaults** (all `window` 5 min, `lockout_after` `u32::MAX`, `lockout` 0):

| limiter | free_failures | base_delay | max_delay | max_keys |
|---|---|---|---|---|
| bearer | 10 | 250 ms | 10 s | 16 |
| claim | 5 | 500 ms | 30 s | 10 000 |
| health | 300 | 10 ms | 2 s | 4 |
| misuse | 20 | 250 ms | 10 s | 10 000 |

`event_interval` defaults to 1 s. **Why the gate:** the sink rotates at 4 MiB, so an unthrottled flood
on one surface would rotate the evidence of every other surface out of the file. The gate keys on
`(kind, surface)`, not on the subject, because the claim subject is attacker-chosen.

## Mechanism

- **The delay layer.** `ApiError` gains `delay: Option<std::time::Duration>`. `into_response` puts
  a `FailureDelay(d)` extension on the response, and a helper does the same for an `Ok` response (row 3).
  A middleware `delay_failures`, layered **inside** `host_guard` (a 421 is not counted), runs the
  handler, then sleeps for `FailureDelay` if present before returning. This is the only `sleep`.
- **`fn fail(&self, limiter, key, now) -> Option<Duration>`** calls `record_failure` and maps
  `Delay(d)` to `Some(d)`. `Allow` is `None`, and so is `Refuse`, which is unreachable with
  `lockout_after = u32::MAX`.
- **Events** go through the gate into the `FileSink` (as `Arc<dyn SecuritySink>`). Every event has
  `source: None` (loopback: the address says nothing), and `at = Utc::now()`.

## Rows

| row | where | counted | key(s) | event |
|---|---|---|---|---|
| 1 | `authed` returns 401 `unauthorized`: missing, malformed or unknown token. A 409 `superseded` is NOT counted | every 401 | `"bearer"` | `AuthFailure`, surface `synapsed/bearer`, subject `bearer`, on **every** 401 |
| 2 | `claim`: every 400 `malformed` (bad chain, nonce or signature encoding, or invalid JSON), and every 403 `claim_refused` (wrong audience, failed verification). A 415 is NOT counted (no body was even read) | every such failure | the claimed global id (`chain[0].subject_global_id`, when the chain parsed and is non-empty) **and** `"claim"`; the delay is the larger of the two | `AuthFailure`, surface `synapsed/claim`, subject = claimed id or `claim`, detail = the error kind |
| 3 | `health` with a well-formed `?challenge=` | every challenged request (it is a budget, not a failure count) | `"health"` | `RateLimited`, surface `synapsed/health`, subject `health`, only when a delay is owed. Still 200 with the proof, never 429: the CLI treats a non-ok reply as a failed proof and warns of a squat |
| 4 | `send` answers 404 `unknown_recipient` | every one | the session's global id | `RateLimited`, surface `synapsed/recipient`, subject = the sender's global id, only when a delay is owed |
| 5 | `ack` answers 404 `not_found` or 403 `not_your_lease` | every one | the session's global id | `RateLimited`, surface `synapsed/ack`, subject = global id, only when a delay is owed |

Rows 1 and 2 log every failure (each is a failed authentication). Rows 3–5 log only once over budget,
because an occasional 404 is ordinary.

## Tests (`crates/synapsed/tests/api.rs`), red first

A helper `start_with(SecurityConfig)` uses a small config: every limiter `free_failures` 1,
`base_delay` 200 ms, `max_delay` 2 s; `event_interval` 0. So failure 2 owes 200 ms and failure 3 owes
400 ms. 200 ms is the margin for slow CI runners. A helper reads `<home>/security-events.jsonl` as lines
of JSON.

| test | negative control | positive control |
|---|---|---|
| `bad_bearers_are_delayed_and_recorded` | Three bad tokens: all 401. The 3rd takes ≥ 200 ms. The file has ≥ 3 `auth_failure` lines on `synapsed/bearer`. No line contains any of the three token hex strings | a valid token then answers in < 200 ms |
| `refused_claims_are_delayed_and_recorded` | Three claims with a corrupted signature: 403, the 3rd ≥ 200 ms, `auth_failure` lines on `synapsed/claim` whose subject is the role's global id; one malformed `nonce_hex` gives 400 and is recorded too. No line contains the nonce hex or the signature | a valid claim then succeeds in < 200 ms |
| `challenged_health_over_budget_is_slowed_not_refused` | 3rd challenged request: 200 with a valid proof, ≥ 200 ms, and one `rate_limited` line on `synapsed/health` | an unchallenged `/v1/health` stays < 200 ms |
| `repeated_unknown_recipients_are_delayed_per_role` | Role A sends to an unknown role three times: 404 each time, the 3rd ≥ 200 ms, a `rate_limited` line with A's global id | role B's first 404 is < 200 ms (a separate key) |
| `repeated_bad_acks_are_delayed_per_role` | three acks of unknown ids: 404, the 3rd ≥ 200 ms, a `rate_limited` line on `synapsed/ack` | another role's ack is unaffected |
| `the_event_gate_coalesces_a_flood` | with `event_interval` 60 s and `free_failures` 100 (so nothing is delayed), five bad bearers write exactly one `synapsed/bearer` line | with interval 0 (the other tests) every failure is written |
| `a_too_open_security_event_file_refuses_to_start` (`#[cfg(unix)]`) | a 0644 `security-events.jsonl` in the home: `Daemon::open` is `Err(DaemonError::Security(_))` | a 0600 file opens |

The existing tests keep using `Daemon::open` without `with_security`, so they run with the defaults
and none of them should slow down. The whole `api.rs` binary's run time is compared before and after.

## Out of scope

- `AlertSink` wiring (board 131), and the client-side `ProofFailure` event (P8).
- Per-source keys: everything on loopback shares one address.
- Bounding the number of in-flight delayed failures. Each is a sleeping tokio task.
