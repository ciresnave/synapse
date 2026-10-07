# Hardening P6: transport events and the knock budget, audit rows 8–12 (plan)

**Spec:** `docs/superpowers/specs/2026-10-07-brute-force-hardening-design.md` §3, rows 8–12. P4 (#84) built
`synapse::security_events`; P5 (#85) wired `synapsed`.

**Scope:** the core `TransportManager` receive path and `TrustStore` revocations, plus
`synapse-mcp-server`. Additive: nothing changes what is delivered, dropped or answered. No new public
field on an existing struct (that would break struct literals); new builder methods and new functions
only.

**Alert DELIVERY is still only a seam** (`AlertTransport`, no implementation, board 131). P6 writes
events to a sink only.

## The one departure from the spec (row 9): PM ruled A (2026-10-07)

The spec says "limit the knock path". On `synapsed` (P5), "limit" meant *delay the failure answer*. An
inbound transport has no answer to delay: a rejected sender gets nothing back, so there is no oracle
to slow down. The two remaining ways to "limit" are:

- **(A, recommended) Budget the record, not the message.** Every message is still verified. The limiter,
  keyed **(claimed global id, source address)** with a **per-source** backstop, decides only whether a
  knock is written as its own `UnverifiedSender` event (under budget), or counted and summarised in one
  `RateLimited` event when the key trips. This keeps a flood from rotating the event file.
- **(B) Drop before verifying** once a key is over budget, saving the Ed25519/chain work. Rejected:
  the source address on UDP is spoofable, so a spoofer who sends bad messages under a real peer's
  claimed id and address would silence that peer. A lockout keyed on spoofable input is a DoS tool.

Under (A) the honest statement for the PR is the same as P5's: the cryptography is the countermeasure;
P6's new value is the **record**.

"An immediate alert for a new knock source after quiet": `AlertPolicy`'s immediate kinds are fixed, and
no alert is delivered yet. P6 marks such an event with `detail` beginning `new_source` (no knock from that
source in the window), so the policy can promote it when board 131 lands. It does not change
`AlertPolicy`.

## Files

| file | change |
|---|---|
| `src/security_events.rs` | `EventGate`: the per-(kind, surface) gate P5 wrote inside synapsed, with time injected (the core reads no clock) |
| `src/sender_auth.rs` | `TrustStore::try_add_revocation(Revocation) -> Result<bool, &'static str>` (`Err` names the refusal, `Ok(false)` a duplicate); `add_revocation` now delegates to it, same behaviour, and each signature is verified once |
| `src/transport/manager.rs` | `TransportManagerBuilder::security_sink`, `::knock_limits`; events on rows 8–11 |
| `crates/synapse-mcp-server/{Cargo.toml,src/lib.rs,src/main.rs}` | `SynapseMcpServer::start_with_sink`; `main` opens a `FileSink` at `<config stem>-security-events.jsonl` beside the config file (per stem, so two servers sharing a directory keep separate files), and refuses to start if it cannot |
| `tests/transport_security_events.rs` | new; the tests below |
| `crates/synapse-mcp-server/tests/mcp_surface.rs` | one test for row 12 |

`synapse-mcp-server` gains a `synapse-security` dependency, so the version bump becomes **seven** places
(6.0.0-rc.6, the PM's allocation).

## API

```rust
// synapse::security_events
pub struct EventGate { /* per (kind, surface): last written, held back */ }
impl EventGate {
    pub fn new(interval: chrono::Duration) -> Self;           // zero passes everything
    /// Some(detail, with "; N similar events held back" appended when N > 0), or None to hold back.
    pub fn pass(&self, kind: SecurityEventKind, surface: &str, detail: &str,
                now: DateTime<Utc>) -> Option<String>;
}

// synapse::transport::TransportManagerBuilder
pub fn security_sink(self, sink: Arc<dyn SecuritySink>) -> Self;   // default: none, nothing emitted
pub fn knock_limits(self, limits: KnockLimits) -> Self;

pub struct KnockLimits { pub per_sender: LimiterConfig, pub per_source: LimiterConfig,
                         pub event_interval: chrono::Duration }
```

`synapsed` keeps its own gate; moving it onto `EventGate` is a follow-up, not P6.

**Defaults** (`lockout_after` forced to `u32::MAX`, window 5 min):

| limiter | key | free_failures | max_keys |
|---|---|---|---|
| per_sender | `<claimed id>\|<source>` | 10 | 10 000 |
| per_source | `<source>` | 50 | 10 000 |

`event_interval` 1 s. Delays from the limiter are never slept on; only "over budget or not" is read.

## Rows

All events: `source = Some(incoming.source)` where a message is involved, `subject` never a key,
signature or payload; every text field passes `sanitize`.

| row | where | event | surface | subject | detail |
|---|---|---|---|---|---|
| 8 | `add_revocations`, for each revocation `try_add_revocation` refuses with (`unsupported_version`, `unknown_issuer`, `bad_signature`); a duplicate is not a failure | `UnverifiedSender` | `transport/revocation` | issuer key id | the refusal |
| 8 | a knock whose reason is `invalid_chain`/`unknown_issuer`/`chain_too_large` | as row 9 | | | |
| 9 | `inbound.admit` returns `Reject` | under budget: `UnverifiedSender`; on the first trip per window: `RateLimited` | `transport/knock` | claimed global id | `verdict_reason`, key id; `new_source` prefix as above |
| 10 | `inbound.check` returns `Drop` | `ReplayRefused` | `transport/replay` | the verified key id | `replayed` |
| 11 | `sealing::open` returns `CouldNotOpen(e)` | `UnverifiedSender` | `transport/sealing` | `from_global_id` | `e.name()` |
| 12 | `synapse-mcp-server` | inherits rows 8–11 through its manager | | | |

## Tests, red first (`tests/transport_security_events.rs`, capturing sink)

| test | negative control | positive control |
|---|---|---|
| knock_is_an_event | an unsigned message → one `UnverifiedSender` on `transport/knock`, source set | a pinned sender's message → no event, delivered |
| knock_flood_trips_one_rate_limited | `free_failures` 2, 5 knocks → 2 `UnverifiedSender` + 1 `RateLimited` | a different claimed id from the same source still gets its own event until the per-source budget |
| knock_over_budget_is_still_verified | (A's guarantee) after the trip, a valid message from the pinned sender at the same source is delivered | — |
| replay_is_an_event | the same signed message twice → one `ReplayRefused` | the first copy produced none |
| unopenable_is_an_event | a sealed message to a node with no sealing key → `UnverifiedSender` on `transport/sealing`, `not_sealed` | a correctly sealed one → none |
| bad_revocation_is_an_event | a revocation signed by an unpinned key → `unknown_issuer`; a tampered one → `bad_signature` | a valid one → stored, no event; the same again → no event |
| no_secret_in_any_event | every event above: no signing key, signature or payload bytes in any field | the scan finds a known field (the claimed id) |
| gate_holds_back_and_reports | `EventGate` unit tests in `security_events.rs` | interval zero passes all |
| no_sink_no_change | the existing replay_suppression tests pass unchanged with no sink | — |

`synapse-mcp-server`: `start_with_sink` + an unsigned UDP knock → the capturing sink holds one
`UnverifiedSender`; the binary opens `<stem>-security-events.jsonl` empty at startup and writes the knock
to it (process test).
