# Replay Suppression and Bounded Inbound State (P2 slice e)

**Status:** approved in outline by CireSnave, 2026-09-17; this document is the spec he reviews.
**Branch:** `feat/replay-suppression`, stacked on `feat/sealing` (#41).
**Depends on:** slice a (sender authentication, on `main` as `ee1bf8c`), slice b (receiver
acknowledgement, #38), slice c (`synapse-mcp`, #39), slice d (sealing, #41).

## 1. The problem

Slice a signs `message_id` and `timestamp`, and `TransportManager::receive_messages`
(`src/transport/manager.rs:599`) verifies the signature. Nothing reads either field afterwards, so a
captured message re-verifies forever: an attacker who records one datagram can resend it verbatim, as
often as they like, and every copy is delivered with a `Verified` verdict. Slice d's sealing does not
help — a replayed sealed message opens exactly as the original did.

Four collections also grow without bound, which the PM made an acceptance criterion of this slice
(recorded in slice a's spec §"slice table", slice b's spec §7 and slice c's spec §8):

| collection | where | today |
|---|---|---|
| `outbound` (ack tracking) | `src/transport/manager.rs:181` | one entry per message sent with `request_ack`, forever |
| `kept` (messages `poll` returned) | `src/mcp_server.rs:84` | one entry per polled message, forever |
| `sent_with_ack` (ids this server sent) | `src/mcp_server.rs:86` | one entry per sent message, forever |
| the seen-message record | — | added by this slice; must be bounded from the start |

## 2. Decisions taken (CireSnave, 2026-09-17)

1. **Sure replays are dropped; uncertain messages are delivered, marked.** A repeat of a
   (verified sender, `message_id`) pair already in the record is a sure replay. A message that is
   merely outside the accepted time window may still be genuine, so it is delivered with a mark.
2. **The window is 5 minutes into the past and 1 minute into the future,** configurable.
3. **Restarts:** a message signed before this process started cannot be checked against the (lost)
   record, so it is delivered marked, not dropped.
4. **Marked messages are recorded too,** subject to the record's cap, so a second copy of a marked
   message is dropped as a sure replay.
5. **The other three collections get an age limit and a hard cap,** oldest evicted first.
6. **Unverified senders are denied by default, in the transport,** with an opt-in setting. Deliveries
   that are dropped are counted by reason.
7. **Automatic peer discovery is a later slice, after key rotation (slice f).** This slice adds only
   the bounded record of who is knocking, which grants nothing. See §9.

## 3. Architecture

A new `src/replay.rs` holds all the new state and the decisions over it. It has no I/O, no locks and
no clock of its own: every entry point takes `now: DateTime<Utc>`, so tests drive time directly
rather than sleeping.

```
receive_messages
  └─ for each incoming message
       ├─ TrustStore::verify            (slice a)  → SenderVerdict
       ├─ InboundGate::admit            (this slice, §4) → Admit | Reject(reason)
       ├─ sealing::open                 (slice d)  → Payload
       └─ ReplayGuard::check            (this slice, §5) → Deliver(Freshness) | Drop
```

`TransportManager` owns one `InboundState` behind its existing `TokioRwLock` pattern:

```rust
pub struct InboundState {
    gate: InboundGate,      // §4: verdict policy, knock record, counters
    guard: ReplayGuard,     // §5: seen-message record, horizon
}
```

Ack messages pass through the same gate and guard before `apply_ack` runs, so a duplicate ack is
dropped before it can be applied. Acks are still never handed to the application.

## 4. The delivery gate

```rust
pub struct GateConfig {
    /// Deliver `Unverifiable` messages instead of dropping them. Default: false.
    pub accept_unverified: bool,
    /// Maximum distinct (claimed id, key id) pairs remembered in the knock record. Default: 256.
    pub knock_capacity: usize,
}
```

| verdict | `accept_unverified = false` (default) | `accept_unverified = true` |
|---|---|---|
| `Verified { key_id }` | admitted | admitted |
| `Contradicted { reason }` | **always dropped**, counted, recorded as a knock | always dropped, counted, recorded as a knock |
| `Unverifiable { reason }` | dropped, counted, recorded as a knock | admitted, `Freshness::NotChecked`, **never recorded** in the replay record |

`Contradicted` is dropped under both settings: a bad signature, a key that is not the pinned one, or a
non-canonical timestamp has no innocent reading.

An admitted `NotChecked` message is never entered in the replay record. Entering it would let anyone
suppress a genuine message by sending an unsigned copy of its `message_id` first.

**The knock record** answers "who is trying to reach me, and with which key?" so a mis-pinned peer is
visible instead of looking like a network fault. It is keyed by
`(from_global_id, key_id)`, where `key_id` is the proof's key id, or the literal `unsigned` when
`alg` is `none`:

```rust
pub struct Knock {
    pub claimed_global_id: String,
    pub key_id: String,          // "unsigned" when the message carried no signature
    pub reason: &'static str,    // the verdict reason that kept it out
    pub first_seen: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
    pub count: u64,
}
```

It holds at most `knock_capacity` pairs; when full, the entry with the oldest `last_seen` is evicted.
It grants nothing, and nothing reads it back into the trust store. It is in-memory only.

**Counters**, returned by value:

```rust
pub struct InboundCounters {
    pub admitted: u64,
    pub dropped_contradicted: u64,
    pub dropped_unverifiable: u64,
    pub dropped_replay: u64,
    pub delivered_stale: u64,
    pub delivered_ahead: u64,
    pub delivered_unchecked: u64,
    pub delivered_not_checked: u64,
}
```

`TransportManager::inbound_counters()` and `TransportManager::knocks()` expose them.

## 5. The replay guard

```rust
pub struct ReplayConfig {
    pub past: Duration,       // default 5 min: older than this is Stale
    pub ahead: Duration,      // default 1 min: further ahead than this is Ahead
    pub retention: Duration,  // default 6 min (past + ahead): how long an id is remembered
    pub capacity: usize,      // default 100_000 recorded ids
}
```

`ReplayGuard::check(&mut self, key_id: &str, message_id: &str, signed: DateTime<Utc>, now:
DateTime<Utc>) -> Decision`, where `Decision` is `Drop` or `Deliver(Freshness)`:

```rust
pub enum Freshness {
    Fresh,        // inside the window and after the horizon
    Stale,        // signed more than `past` ago
    Ahead,        // signed more than `ahead` in the future
    Unchecked,    // signed at or before the horizon: cannot be checked, may be genuine
    NotChecked,   // sender not verified (only with accept_unverified)
}
```

The record is keyed by `(key_id, message_id)` — the **verified signing key**, not the claimed
`global_id`, so two names sharing a key cannot be used to slip a copy past the check.

Order of decisions:

1. If `(key_id, message_id)` is in the record → `Drop`, count `dropped_replay`, log at debug. The
   entry's `recorded_at` is refreshed to `now`, so a replay flood cannot age its own id out of the
   record while it is still arriving.
2. Otherwise classify: `signed <= horizon` → `Unchecked`; `now - signed > past` → `Stale`;
   `signed - now > ahead` → `Ahead`; else `Fresh`.
3. Record `(key_id, message_id)` with `recorded_at = now` and `signed`, then return
   `Deliver(freshness)`.

**Eviction** runs on each `check`, in this order:

- entries with `now - recorded_at > retention` are removed; then
- while the record is over `capacity`, the entry with the oldest `recorded_at` is removed **and the
  horizon is raised** to that entry's `signed` time if it is later than the current horizon.

**The horizon** is the moment before which this node cannot make a statement. It starts at process
start (`InboundState::new(now)`), and rises on capacity eviction. Anything signed at or before it is
delivered as `Unchecked`. This is what makes the guarantee below true without persistent state:

> **Guarantee.** For any two messages with the same `(key_id, message_id)` where the first was
> delivered as `Fresh`, the second is dropped, provided it arrives within `retention` of the first
> and the record has not evicted for capacity in between. If it has, the second is delivered as
> `Unchecked` — never as `Fresh`.

Because `retention >= past + ahead`, an id stays recorded for at least as long as the window that
would have called a copy of it `Fresh`.

## 6. Bounding the other three collections

A shared helper in `src/replay.rs`:

```rust
pub struct Bounded<V> { /* insertion-ordered map with ttl + capacity */ }
impl<V> Bounded<V> {
    pub fn new(ttl: Duration, capacity: usize) -> Self;
    pub fn insert(&mut self, key: String, value: V, now: DateTime<Utc>) -> Option<V>;
    pub fn get(&self, key: &str) -> Option<&V>;
    pub fn get_mut(&mut self, key: &str) -> Option<&mut V>;
    pub fn remove(&mut self, key: &str) -> Option<V>;
    pub fn sweep(&mut self, now: DateTime<Utc>) -> usize;  // returns entries removed
    pub fn len(&self) -> usize;
}
```

Expiry by age runs on `insert` and on `sweep`; capacity evicts the oldest insertion first.

| collection | ttl | capacity | behaviour at the limit |
|---|---|---|---|
| `outbound` | 1 h | 10 000 | an entry still at `Sent`/`Delivered` when it expires is reported as `DeliveryConfirmation::Expired` for as long as it remains; once evicted, `delivery_status` returns `None` |
| `kept` (`synapse-mcp`) | 1 h | 1 000 | `ack` on an evicted id is refused: "message_id … is no longer held" |
| `sent_with_ack` (`synapse-mcp`) | 1 h | 1 000 | the id simply stops appearing in `poll`'s `deliveries` |

`DeliveryConfirmation` gains one variant:

```rust
pub enum DeliveryConfirmation { Sent, Delivered, Acknowledged, Expired }
```

`Expired` means "no acknowledgement arrived within the tracking window", never "not delivered". An
ack that arrives after expiry but before eviction still moves the entry to `Acknowledged`; after
eviction it is ignored, exactly as an untracked ack is today.

## 7. Configuration

`TransportManagerBuilder` gains `.replay_config(ReplayConfig)`, `.gate_config(GateConfig)` and
`.tracking_limits(ttl, capacity)`. All three have the defaults above, so existing construction sites
compile unchanged.

`McpConfig` gains optional keys, all with the defaults above:

```toml
accept_unverified = false          # deliver unverified senders' messages, marked
replay_past_seconds = 300
replay_ahead_seconds = 60
replay_retention_seconds = 360
replay_capacity = 100000
tracking_ttl_seconds = 3600
```

A config that sets `replay_retention_seconds` below `replay_past_seconds + replay_ahead_seconds` is
rejected at startup with a fixed message, because it would break the guarantee in §5.

## 8. Surfaces

`ReceivedMessage` gains `pub freshness: Freshness`.

`synapse-mcp`:

- **`poll`** — each message view gains `"freshness"` (the lowercase variant name). The reply gains
  `"dropped"`, an object of the counters in §4, so an agent can see that traffic is being refused.
- **`list`** — gains `"knocking"`: the knock record, newest `last_seen` first, each entry with
  `claimed_global_id`, `key_id`, `reason`, `first_seen`, `last_seen` and `count`. The tool
  description says these are **not** peers and are granted nothing.
- **`ack`** — the refusal for an evicted message names the new reason (§6).

The `poll` description keeps its slice c text verbatim, with one sentence appended: *"A message's
freshness says whether its signed timestamp could be checked; only `fresh` means the message is
known not to be a replay."*

**The router's email path is not gated, and cannot be in this slice.** Measured on the sealing branch before it was squash-merged as #41 (`63d4945`, whose `src/router.rs` is identical):
`SynapseRouter` (`src/router.rs:22`) holds a `CryptoManager`, an `IdentityRegistry` and an
`EmailTransport` — it has no `TransportManager` and no `TrustStore`, so it has nothing to verify a
sender against. `SynapseRouter::receive_messages` already carries a doc comment saying its senders
are unauthenticated. Giving the router a trust store is a change of its construction API and belongs
with whatever slice moves the router onto `TransportManager`; this slice leaves that comment in place
and adds the same warning to `process_email_message`. **Consequence to state plainly: everything in
this slice protects the `TransportManager` path, which is what `synapse-mcp` and the fabric use. The
router's email path is unauthenticated before this slice and remains so after it.**

## 9. Not in this slice

- **Automatic peer discovery and trust on first use.** Decided 2026-09-17: its own slice, **after**
  the widened slice f below. An unknown-but-continuous peer will be able to **send messages only**;
  tools, commands and anything acted on automatically will need an explicit grant. The knock record
  in §4 is the raw material it will build on, and it is deliberately in-memory and authority-free
  until then.
- **Account keys and agent certificates.** Decided 2026-09-17: **slice f is widened** from "key
  rotation and revocation" to *account keys, agent certificates, rotation and revocation*, because
  rotation is a certificate re-signing and building the narrow half first would be rebuilt. An
  account holder keeps one long-lived account key and signs a statement binding an agent's signing
  and sealing keys, a label, a validity window, **coarse named permissions** and a serial. Receivers
  pin one key per account holder, not per agent. The permissions are what a consumer authorizes an
  action against — OverMind asked for exactly this on 2026-09-17 — while what each named permission
  may do stays the receiver's local policy. Revocation is a statement signed by the account key,
  with short validity windows rather than a fetched revocation list.
- **Provider-backed introduction.** Decided 2026-09-17, and **its own project**, neither part of
  Synapse nor of any one website: two account holders sign in to one site, each with their own OIDC
  provider, and the site exchanges their account public keys. The site is the single registered
  client at each provider, holds no secrets, and is used once, at introduction — never in the message
  path. Both the site and each party's local software show the other's key fingerprint, so a
  substituted key is visible. It must be hostable by any website; the first public host will be
  ThinkersJournal.com. Synapse's side is a client for it, designed after slice f.
- **Persistent state of any kind.** The horizon covers restarts instead.
- **Rate limiting.** A flood of distinct verified messages is not a replay; bounding that is a
  transport concern, and the record's capacity plus the horizon keeps the memory cost fixed.
- **Authenticating the router's email path** (§8): it needs a trust store the router does not
  have, and a change to how the router is built.
- **Key rotation and revocation** (slice f).

## 10. Testing

Unit tests in `src/replay.rs` drive `now` directly, with no sleeps:

1. A second `check` of the same `(key_id, message_id)` returns `Drop`. **Control:** a different
   `message_id` from the same key returns `Deliver(Fresh)`.
2. `Stale`, `Ahead` and `Unchecked` classification at the boundaries (exactly `past`, one
   microsecond beyond, and so on).
3. A marked message's id is recorded: a `Stale` delivery followed by an identical message returns
   `Drop`.
4. After `retention`, the id is gone: the same message is delivered again — as `Stale`, never as
   `Fresh`.
5. Capacity eviction raises the horizon, and a message signed before it then arrives `Unchecked`
   rather than `Fresh`. **Control:** a message signed after the horizon is still `Fresh`.
6. `Bounded` expires by age and by capacity, oldest first.

Integration tests in `tests/replay_suppression.rs`, over loopback UDP, with real signed messages:

7. **The replay itself.** Capture the datagram a real send produces, deliver it once, then resend the
   identical bytes with a raw socket: the second copy never appears. **Control:** the first copy does
   appear, and a second, genuinely new message from the same sender also appears.
8. **Unverified senders are dropped by default**, and the counter and knock record show it.
   **Control:** with `accept_unverified` on, the same message is delivered with `NotChecked`.
9. **A contradicted message is dropped under both settings.**
10. **Suppression is impossible:** an unsigned message carrying a real message's `message_id`,
    delivered first, does not prevent the genuine signed message from arriving.
11. **A duplicate ack is dropped** before `apply_ack` runs: the delivery status is `Acknowledged`
    once and the counters show one dropped replay. **Control:** the first ack does move the status.
12. **Expiry:** with a 0-second tracking ttl, an unacknowledged tracked message reports `Expired`.
13. **`synapse-mcp`:** `poll` reports `freshness` and the `dropped` counters; `list` reports
    `knocking`; `ack` on an evicted id is refused with the new text; a config with a too-short
    retention is refused at startup.

**Mutation check.** Predict, then run: skipping the record insertion (step 3 of §5) makes **only**
the replay tests fail (1, 3, 7 and 11), and no others.

**Acceptance.**

- `cargo test --no-fail-fast` in a **fresh** target directory keeps the failing set
  `{test_transport_error_handling}`, with the Firewall event 2097 count for the run's window
  reported.
- `cargo fmt -- --check` and `cargo clippy -- -D warnings` exit 0, including on the new test target.
- The four collections in §1 each have a test that shows a bound.
- No version bump: P2 bumps once, when the set is complete.
