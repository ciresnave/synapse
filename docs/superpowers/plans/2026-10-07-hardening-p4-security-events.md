# Hardening P4: security events, the failure limiter, sinks and the alert policy (plan)

**Spec:** `docs/superpowers/specs/2026-10-07-brute-force-hardening-design.md` §2. The PM approved it in
principle on 2026-10-07 and named P4 next.

**Scope:** additive only. P4 adds the building blocks; P5–P8 wire them into live surfaces. Nothing in P4
changes any existing behaviour.

**Alert delivery is NOT in P4.** It waits on CireSnave's board 131 (the relay vendor and its secret).
P4 defines the delivery seam (`AlertTransport`) and ships no concrete email transport.

## Files

| file | what |
|---|---|
| `src/security_events.rs` (new; `pub mod security_events;` in `lib.rs`) | Core types and logic, no I/O: `SecurityEventKind`, `SecurityEvent`, `SecuritySink`, `FailureLimiter`, `AlertPolicy` |
| `crates/synapse-security/` (new crate, `synapse-security`) | The sinks, with I/O: `FileSink`, `StderrSink`, `AlertSink<T: AlertTransport>`, `FanoutSink` |
| `tests/security_events.rs` (new) | Core behaviour tests |
| `crates/synapse-security/tests/sinks.rs` (new) | Sink tests |

**Manifests.** The new crate uses `version.workspace = true`, the workspace `edition`/`rust-version`/licence
fields as the other members do, and `synapse = { path = "../..", version = "<same as the others>" }`. The
version bump is therefore **five** places from now on. No new third-party dependency: `serde`,
`serde_json` and `chrono` are already in the workspace.

## Core API (`src/security_events.rs`)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecurityEventKind { AuthFailure, ProofFailure, UnverifiedSender, ReplayRefused,
                             RateLimited, Lockout, PermissionsTooOpen, NewClient, Config, AlertFailed }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SecurityEvent {
    pub at: DateTime<Utc>,
    pub kind: SecurityEventKind,
    pub surface: String,         // e.g. "synapsed/bearer"; sanitized
    pub subject: String,         // key id / role / username — NEVER a secret; sanitized
    pub source: Option<String>,  // sanitized
    pub detail: String,          // sanitized
}
impl SecurityEvent {
    /// Builds an event; every text field passes through `sanitize`.
    pub fn new(at, kind, surface: &str, subject: &str, source: Option<&str>, detail: &str) -> Self;
}
/// Control chars escaped (as `printable` does in synapsectl), then truncated to 256 chars on a char boundary.
pub fn sanitize(s: &str) -> String;

pub trait SecuritySink: Send + Sync { fn record(&self, event: &SecurityEvent); }
```

**`FailureLimiter`.** Counts failures only, keyed by caller-chosen strings, with time injected.

```rust
pub struct LimiterConfig {
    pub free_failures: u32,      // failures allowed in `window` before any delay   (default 5)
    pub window: Duration,        // chrono::Duration                              (default 5 min)
    pub base_delay: Duration,    // first delay once over free_failures           (default 1 s)
    pub max_delay: Duration,     // delay doubles per extra failure, capped here  (default 60 s)
    pub lockout_after: u32,      // failures in window that lock the key          (default 20)
    pub lockout: Duration,       // how long a lockout lasts                       (default 15 min)
    pub max_keys: usize,         // bound on tracked keys; evict least-recently-failed (default 10_000)
}
pub enum Verdict { Allow, Delay(Duration), Refuse { until: DateTime<Utc> } }
impl FailureLimiter {
    pub fn new(config: LimiterConfig) -> Self;
    /// Before an attempt: Allow, or Delay/Refuse if the key is over its budget or locked.
    pub fn check(&self, key: &str, now: DateTime<Utc>) -> Verdict;
    /// After a FAILED attempt: record it and return the verdict for the next attempt.
    /// The FIRST transition into lockout is reported once: returns (verdict, newly_locked: bool).
    pub fn record_failure(&self, key: &str, now: DateTime<Utc>) -> (Verdict, bool);
    /// After a SUCCESS, forget the key, so legitimate callers never accumulate.
    pub fn record_success(&self, key: &str);
}
```

- **Interior mutability** is a `std::sync::Mutex`, never held across `.await`.
- **Failures** older than `window` fall out of the count.
- **The `max_keys` bound** stops a flood of distinct keys from exhausting memory. When it is full,
  inserting evicts the key whose last failure is oldest.

**`AlertPolicy`.** Pure logic, with time injected.

```rust
pub struct AlertConfig { pub threshold: u32 /*10*/, pub threshold_window: Duration /*5 min*/,
                         pub coalesce: Duration /*15 min*/ }
pub struct Alert { pub kind: SecurityEventKind, pub surface: String, pub first: SecurityEvent,
                   pub count: u32, pub reason: AlertReason }
pub enum AlertReason { Immediate, Threshold, Digest }
impl AlertPolicy {
    pub fn new(config: AlertConfig) -> Self;
    /// Feed every event; returns an alert to send now, if any.
    pub fn observe(&self, event: &SecurityEvent) -> Option<Alert>;
    /// Called periodically; returns digests for (kind, surface) pairs whose held events are due.
    pub fn flush(&self, now: DateTime<Utc>) -> Vec<Alert>;
}
```

- **Immediate kinds:** `NewClient`, `Config`, `PermissionsTooOpen`, `ProofFailure`, `Lockout` and
  `AlertFailed`.
- **Every other kind alerts on a threshold:** once one `(kind, surface, subject)` reaches `threshold`
  events within `threshold_window`.
- **Coalescing:** after an alert for `(kind, surface)`, any further alert for that pair within `coalesce`
  is held and counted. `flush` emits one `Digest` with the held count once `coalesce` has passed.

## Sinks (`crates/synapse-security`)

- **`FileSink::open(path, max_bytes)`**
  - The file is created owner-only, using the same approach as `synapsed`'s `write_announce`: mode
    `0o600` on Unix; on Windows it inherits the home's ACL.
  - It is opened for append, and `record` writes one JSON line and flushes.
  - When the file would exceed `max_bytes`, it renames the file to `<path>.1`, replacing any older `.1`,
    and starts a fresh file. A flood can therefore never fill the disk past about twice `max_bytes`.
  - I/O errors are swallowed after one `eprintln!`: a sink must never take down the caller.
- **`StderrSink`** writes one line per event: `security: <kind> <surface> <subject> <detail>`.
- **`trait AlertTransport: Send + Sync { fn send(&self, alert: &Alert) -> Result<(), String>; }`**
  - **No implementation ships** in P4 (board 131). Tests use a capturing transport.
- **`AlertSink<T>`**
  - It wraps an `AlertPolicy` and a transport. `record` calls `policy.observe` and sends any alert.
  - A failed send is recorded as an `AlertFailed` event into a fallback sink, normally the `FileSink`.
    It is never re-sent through the same transport, so a dead transport cannot loop.
- **`FanoutSink(Vec<Arc<dyn SecuritySink>>)`** records into each sink.

## Tests (TDD: write them first, see them fail, then implement)

Each test has a negative control (the protection fires) and a positive control (a legitimate case is
unaffected).

**Core (`tests/security_events.rs`)**

1. **`sanitize`:** control characters become `\u{1b}` and similar; a 1000-character string is cut to
   256 characters on a char boundary (multibyte input included). Plain ASCII passes unchanged.
2. **Limiter, free budget:** 5 failures from key A return `Allow`; the 6th returns `Delay(1s)`, then the
   delays double and cap at `max_delay`. Key B is unaffected throughout.
3. **Limiter, lockout:** the 20th failure in the window returns `Refuse`, and `newly_locked` is true
   exactly once. After `lockout` has passed, the key returns `Allow`.
4. **Limiter, window:** failures older than `window` stop counting.
5. **Limiter, success:** `record_success` resets the key.
6. **Limiter, `max_keys`:** with `max_keys = 3`, inserting a 4th key evicts the one whose last failure is
   oldest. The map never exceeds 3 entries.
7. **AlertPolicy, immediate:** `NewClient` alerts at once. `AuthFailure` does not alert until the 10th
   event in 5 min for one `(surface, subject)`; 9 spread across subjects do not alert.
8. **AlertPolicy, coalescing:** 100 `AuthFailure` events in a burst produce exactly 1 alert, then 1
   digest from `flush` after 15 min, with `count` covering the held events.

**Sinks (`crates/synapse-security/tests/sinks.rs`)**

9. **`FileSink`:** it writes valid JSONL that round-trips to `SecurityEvent`. Rotation keeps exactly
   `<path>` and `<path>.1`. On Unix the file mode is `0o600`, behind `cfg(unix)`, so it runs in CI only.
10. **`AlertSink`:** a capturing transport receives the alerts the policy emits. A transport that always
    fails produces `AlertFailed` events in the fallback sink, and its send is called once per alert,
    never in a loop.
11. **No secrets:** an event built with subject `"key-id-123"` and detail `"bad bearer"` serializes
    without any 64-hex-character run. This is a guard against someone later passing a token in.

## Done when

- All the new tests pass.
- Every existing guard passes: `cargo test --test core_is_model_agnostic`, `security_claims_match_the_tree`
  and the other tree-scan guards. The new crate must contain no vendor or model names.
- `cargo fmt --check` is clean, and `cargo clippy --workspace --all-targets -D warnings` is clean.
- One commit for the tests and the implementation. No version bump: the PM allocates it at the gate.
