# Replay Suppression Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Drop replayed messages, mark the ones whose timestamp cannot be trusted, deny unverified
senders by default, and bound every inbound collection.

**Architecture:** A new `src/replay.rs` holds all new state and every decision over it, with no I/O,
no locks and no clock of its own — each entry point takes `now: DateTime<Utc>`, so tests drive time
instead of sleeping. `TransportManager` owns one `InboundState` (gate plus guard) and consults it in
`receive_messages`. A generic `Bounded<V>` from the same module bounds the three pre-existing
collections.

**Tech Stack:** Rust, `chrono` (already a dependency, re-exported as
`crate::synapse::blockchain::serialization::DateTimeWrapper` for wire types), `tokio` locks as
already used in `src/transport/manager.rs`. No new dependencies.

**Spec:** `docs/superpowers/specs/2026-09-17-replay-suppression-design.md`

## Global Constraints

- **Branch:** `feat/replay-suppression`, stacked on `feat/sealing` (#41). Worktree
  `scratchpad/synapse-replay`. Held for the PM's daily merge pass.
- **`CARGO_TARGET_DIR=C:/Projects/synapse/target`** for every command except the final full run,
  which uses a **fresh** directory.
- **Defaults, exactly:** window `past` 5 min, `ahead` 1 min; `retention` 6 min; replay `capacity`
  100_000; `knock_capacity` 256; `accept_unverified` false; tracking ttl 1 h; `outbound` capacity
  10_000; `synapse-mcp` `kept` and `sent_with_ack` capacity 1_000.
- **`retention >= past + ahead`** is enforced; a config violating it is refused at startup.
- **The replay record is keyed by `(key_id, message_id)`** — the verified signing key, never the
  claimed `global_id`.
- **An unverified (`NotChecked`) message's id is never recorded.** Recording it would let anyone
  suppress a genuine message.
- **`Contradicted` is always dropped**, under both settings.
- **No new dependency, and no version bump** (P2 bumps once, when the set is complete).
- **New `.rs` files carry the SPDX header** `// SPDX-License-Identifier: MIT OR Apache-2.0`.
- **Tests bind loopback only.** No test sleeps to advance logical time.
- Checks: `cargo test --no-fail-fast` keeps the failing set `{test_transport_error_handling}`;
  `cargo fmt -- --check` exits 0; `cargo clippy -- -D warnings` exits 0; clippy also exits 0 on each
  new or changed test target.

## File Structure

| file | responsibility |
|---|---|
| `src/replay.rs` (new) | `Freshness`, `Decision`, `ReplayConfig`, `ReplayGuard`, `GateConfig`, `Admission`, `Knock`, `InboundCounters`, `InboundState`, `Bounded<V>`; all unit tests for them |
| `src/lib.rs` | add `pub mod replay;` |
| `src/transport/manager.rs` | own `InboundState`; gate and guard in `receive_messages`; `freshness` on `ReceivedMessage`; builder knobs; bound `outbound`; `Expired` |
| `src/transport/abstraction.rs` | `DeliveryConfirmation::Expired` |
| `src/mcp_server.rs` | optional config keys with validation; bound `kept` and `sent_with_ack`; `freshness`, `dropped`, `knocking` in the tool output |
| `tests/replay_suppression.rs` (new) | integration tests 7–12 of spec §10 over loopback UDP |
| `tests/mcp_surface.rs` | test 13 of spec §10 |

---

### Task 1: `src/replay.rs` — the replay guard and `Bounded`

**Files:**
- Create: `src/replay.rs`
- Modify: `src/lib.rs` (add `pub mod replay;` in the alphabetical list beside `pub mod sealing;`)

**Interfaces:**
- Consumes: nothing outside `chrono`.
- Produces:
  - `pub enum Freshness { Fresh, Stale, Ahead, Unchecked, NotChecked }`, with
    `Freshness::name(&self) -> &'static str` giving `"fresh"`, `"stale"`, `"ahead"`, `"unchecked"`,
    `"not_checked"`, and `impl std::fmt::Display`.
  - `pub enum Decision { Deliver(Freshness), Drop }`
  - `pub struct ReplayConfig { pub past: Duration, pub ahead: Duration, pub retention: Duration, pub capacity: usize }`
    with `Default` and `ReplayConfig::validate(&self) -> Result<(), &'static str>`.
  - `pub struct ReplayGuard` with
    `ReplayGuard::new(config: ReplayConfig, started: DateTime<Utc>) -> Self`,
    `check(&mut self, key_id: &str, message_id: &str, signed: DateTime<Utc>, now: DateTime<Utc>) -> Decision`,
    `len(&self) -> usize`, `horizon(&self) -> DateTime<Utc>`.
  - `pub struct Bounded<V>` with `new(ttl: Duration, capacity: usize)`, `insert(&mut self, key: String, value: V, now: DateTime<Utc>) -> Option<V>`, `get(&self, key: &str) -> Option<&V>`, `get_mut(&mut self, key: &str) -> Option<&mut V>`, `remove(&mut self, key: &str) -> Option<V>`, `contains_key(&self, key: &str) -> bool`, `keys(&self) -> impl Iterator<Item = &String>`, `sweep(&mut self, now: DateTime<Utc>) -> usize`, `age_of(&self, key: &str, now: DateTime<Utc>) -> Option<Duration>`, `ttl(&self) -> Duration`, `len(&self) -> usize`, `is_empty(&self) -> bool`.

`Duration` here is `chrono::Duration` (used elsewhere in this crate), not `std::time::Duration`.

- [ ] **Step 1: Write the failing unit tests** at the bottom of `src/replay.rs`, in
  `#[cfg(test)] mod tests`. These are spec §10 tests 1–6.

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn t(secs: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_700_000_000 + secs, 0).expect("valid timestamp")
    }

    fn guard() -> ReplayGuard {
        ReplayGuard::new(ReplayConfig::default(), t(0))
    }

    #[test]
    fn the_same_key_and_id_is_dropped_the_second_time() {
        let mut g = guard();
        assert_eq!(g.check("k1", "m1", t(10), t(10)), Decision::Deliver(Freshness::Fresh));
        assert_eq!(g.check("k1", "m1", t(10), t(11)), Decision::Drop);
        // Control: a different id from the same key still gets through.
        assert_eq!(g.check("k1", "m2", t(11), t(11)), Decision::Deliver(Freshness::Fresh));
        // Control: the same id from a different key is a different message.
        assert_eq!(g.check("k2", "m1", t(11), t(11)), Decision::Deliver(Freshness::Fresh));
    }

    #[test]
    fn classification_at_the_window_boundaries() {
        let mut g = guard();
        // Exactly `past` old is still fresh; one second beyond is stale.
        assert_eq!(g.check("k", "a", t(10), t(310)), Decision::Deliver(Freshness::Fresh));
        assert_eq!(g.check("k", "b", t(10), t(311)), Decision::Deliver(Freshness::Stale));
        // Exactly `ahead` in the future is fresh; one second beyond is ahead.
        assert_eq!(g.check("k", "c", t(370), t(310)), Decision::Deliver(Freshness::Fresh));
        assert_eq!(g.check("k", "d", t(371), t(310)), Decision::Deliver(Freshness::Ahead));
    }

    #[test]
    fn anything_signed_at_or_before_the_horizon_is_unchecked() {
        let mut g = ReplayGuard::new(ReplayConfig::default(), t(100));
        assert_eq!(g.check("k", "a", t(100), t(110)), Decision::Deliver(Freshness::Unchecked));
        assert_eq!(g.check("k", "b", t(99), t(110)), Decision::Deliver(Freshness::Unchecked));
        // Control: after the horizon, inside the window, it is fresh.
        assert_eq!(g.check("k", "c", t(101), t(110)), Decision::Deliver(Freshness::Fresh));
    }

    #[test]
    fn a_marked_message_is_recorded_too() {
        let mut g = guard();
        assert_eq!(g.check("k", "m", t(10), t(400)), Decision::Deliver(Freshness::Stale));
        assert_eq!(g.check("k", "m", t(10), t(401)), Decision::Drop);
    }

    #[test]
    fn a_replay_flood_cannot_age_its_own_id_out_of_the_record() {
        let mut g = guard();
        assert_eq!(g.check("k", "m", t(10), t(10)), Decision::Deliver(Freshness::Fresh));
        // Each drop refreshes recorded_at, so the id never expires while copies keep arriving.
        for step in 1..20 {
            assert_eq!(g.check("k", "m", t(10), t(10 + step * 300)), Decision::Drop);
        }
    }

    #[test]
    fn after_retention_the_id_is_forgotten_but_never_looks_fresh() {
        let mut g = guard();
        assert_eq!(g.check("k", "m", t(10), t(10)), Decision::Deliver(Freshness::Fresh));
        // 6 min later the entry is swept; the same message is delivered again, as stale.
        assert_eq!(g.check("k", "m", t(10), t(400)), Decision::Deliver(Freshness::Stale));
    }

    #[test]
    fn capacity_eviction_raises_the_horizon() {
        let config = ReplayConfig { capacity: 2, ..ReplayConfig::default() };
        let mut g = ReplayGuard::new(config, t(0));
        assert_eq!(g.check("k", "a", t(10), t(10)), Decision::Deliver(Freshness::Fresh));
        assert_eq!(g.check("k", "b", t(11), t(11)), Decision::Deliver(Freshness::Fresh));
        // Inserting a third entry evicts "a" (oldest recorded_at) and moves the horizon to t(10).
        assert_eq!(g.check("k", "c", t(12), t(12)), Decision::Deliver(Freshness::Fresh));
        assert_eq!(g.len(), 2);
        assert_eq!(g.horizon(), t(10));
        // A message signed at or before the horizon can no longer be called fresh.
        assert_eq!(g.check("k", "d", t(10), t(13)), Decision::Deliver(Freshness::Unchecked));
        // Control: one signed after the horizon still is.
        assert_eq!(g.check("k", "e", t(13), t(13)), Decision::Deliver(Freshness::Fresh));
    }

    #[test]
    fn retention_shorter_than_the_window_is_refused() {
        let bad = ReplayConfig {
            past: Duration::seconds(300),
            ahead: Duration::seconds(60),
            retention: Duration::seconds(359),
            capacity: 10,
        };
        assert!(bad.validate().is_err());
        assert!(ReplayConfig::default().validate().is_ok());
    }

    #[test]
    fn bounded_expires_by_age_and_by_capacity() {
        let mut b: Bounded<u32> = Bounded::new(Duration::seconds(60), 2);
        b.insert("a".to_string(), 1, t(0));
        b.insert("b".to_string(), 2, t(1));
        assert_eq!(b.len(), 2);
        // Capacity: inserting a third drops the oldest insertion.
        b.insert("c".to_string(), 3, t(2));
        assert_eq!(b.len(), 2);
        assert!(!b.contains_key("a"));
        assert!(b.contains_key("c"));
        // Age: everything older than the ttl goes on sweep.
        assert_eq!(b.sweep(t(100)), 2);
        assert!(b.is_empty());
        // Control: a fresh entry survives a sweep.
        b.insert("d".to_string(), 4, t(100));
        assert_eq!(b.sweep(t(101)), 0);
        assert_eq!(b.get("d"), Some(&4));
        // age_of is how the caller tells an expired entry from a live one.
        assert_eq!(b.age_of("d", t(130)), Some(Duration::seconds(30)));
        assert_eq!(b.age_of("nope", t(130)), None);
    }
}
```

- [ ] **Step 2: Run the tests and check that they fail**

Run: `cargo test --lib replay:: 2>&1 | tail -20`
Expected: compilation fails — `src/replay.rs` does not exist yet, then once created, the types do
not exist.

- [ ] **Step 3: Implement `src/replay.rs`** (the part this task covers: everything except the gate,
  which is Task 2).

```rust
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Replay suppression and bounded inbound state (P2 slice e).
//!
//! Every entry point takes `now`, so this module has no clock, no I/O and no locks: the caller owns
//! all three. See `docs/superpowers/specs/2026-09-17-replay-suppression-design.md`.

use chrono::{DateTime, Duration, Utc};
use std::collections::HashMap;

/// Whether a message's signed timestamp could be checked, and what it said.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Freshness {
    /// Inside the window and after the horizon: known not to be a replay.
    Fresh,
    /// Signed more than `past` ago.
    Stale,
    /// Signed more than `ahead` in the future.
    Ahead,
    /// Signed at or before the horizon, so this node cannot say: it may be genuine.
    Unchecked,
    /// The sender was not verified, so nothing was checked (only with `accept_unverified`).
    NotChecked,
}

impl Freshness {
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Freshness::Fresh => "fresh",
            Freshness::Stale => "stale",
            Freshness::Ahead => "ahead",
            Freshness::Unchecked => "unchecked",
            Freshness::NotChecked => "not_checked",
        }
    }
}

impl std::fmt::Display for Freshness {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Deliver(Freshness),
    Drop,
}

#[derive(Debug, Clone)]
pub struct ReplayConfig {
    pub past: Duration,
    pub ahead: Duration,
    pub retention: Duration,
    pub capacity: usize,
}

impl Default for ReplayConfig {
    fn default() -> Self {
        Self {
            past: Duration::minutes(5),
            ahead: Duration::minutes(1),
            retention: Duration::minutes(6),
            capacity: 100_000,
        }
    }
}

impl ReplayConfig {
    /// `retention` must cover the whole window, or an id could be forgotten while a copy of it
    /// would still be called `Fresh` (spec §5).
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.capacity == 0 {
            return Err("the replay record's capacity must be at least 1");
        }
        if self.retention < self.past + self.ahead {
            return Err("the replay retention must be at least the past plus ahead window");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
struct Seen {
    recorded_at: DateTime<Utc>,
    signed: DateTime<Utc>,
}

/// The seen-message record and the horizon before which this node makes no statement.
#[derive(Debug)]
pub struct ReplayGuard {
    config: ReplayConfig,
    seen: HashMap<(String, String), Seen>,
    horizon: DateTime<Utc>,
}

impl ReplayGuard {
    #[must_use]
    pub fn new(config: ReplayConfig, started: DateTime<Utc>) -> Self {
        Self {
            config,
            seen: HashMap::new(),
            horizon: started,
        }
    }

    #[must_use]
    pub fn horizon(&self) -> DateTime<Utc> {
        self.horizon
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.seen.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.seen.is_empty()
    }

    pub fn check(
        &mut self,
        key_id: &str,
        message_id: &str,
        signed: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Decision {
        let key = (key_id.to_string(), message_id.to_string());
        if let Some(entry) = self.seen.get_mut(&key) {
            // Refresh, so a flood cannot age its own id out of the record while it is arriving.
            entry.recorded_at = now;
            return Decision::Drop;
        }
        let freshness = if signed <= self.horizon {
            Freshness::Unchecked
        } else if now - signed > self.config.past {
            Freshness::Stale
        } else if signed - now > self.config.ahead {
            Freshness::Ahead
        } else {
            Freshness::Fresh
        };
        self.seen.insert(
            key,
            Seen {
                recorded_at: now,
                signed,
            },
        );
        self.evict(now);
        Decision::Deliver(freshness)
    }

    fn evict(&mut self, now: DateTime<Utc>) {
        let retention = self.config.retention;
        self.seen
            .retain(|_, entry| now - entry.recorded_at <= retention);
        while self.seen.len() > self.config.capacity {
            let Some((key, entry)) = self
                .seen
                .iter()
                .min_by_key(|(_, entry)| entry.recorded_at)
                .map(|(key, entry)| (key.clone(), *entry))
            else {
                break;
            };
            self.seen.remove(&key);
            // The record can no longer speak for anything signed at or before the evicted entry.
            if entry.signed > self.horizon {
                self.horizon = entry.signed;
            }
        }
    }
}

/// An insertion-ordered map with an age limit and a hard cap; the oldest insertion is evicted first.
#[derive(Debug)]
pub struct Bounded<V> {
    ttl: Duration,
    capacity: usize,
    entries: HashMap<String, (DateTime<Utc>, V)>,
    order: std::collections::VecDeque<String>,
}

impl<V> Bounded<V> {
    #[must_use]
    pub fn new(ttl: Duration, capacity: usize) -> Self {
        Self {
            ttl,
            capacity,
            entries: HashMap::new(),
            order: std::collections::VecDeque::new(),
        }
    }

    pub fn insert(&mut self, key: String, value: V, now: DateTime<Utc>) -> Option<V> {
        self.sweep(now);
        let previous = self.entries.insert(key.clone(), (now, value));
        if previous.is_none() {
            self.order.push_back(key);
        }
        while self.entries.len() > self.capacity {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            self.entries.remove(&oldest);
        }
        previous.map(|(_, value)| value)
    }

    #[must_use]
    pub fn get(&self, key: &str) -> Option<&V> {
        self.entries.get(key).map(|(_, value)| value)
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut V> {
        self.entries.get_mut(key).map(|(_, value)| value)
    }

    pub fn remove(&mut self, key: &str) -> Option<V> {
        self.order.retain(|k| k != key);
        self.entries.remove(key).map(|(_, value)| value)
    }

    #[must_use]
    pub fn contains_key(&self, key: &str) -> bool {
        self.entries.contains_key(key)
    }

    /// How long ago this key was inserted, if it is still held.
    #[must_use]
    pub fn age_of(&self, key: &str, now: DateTime<Utc>) -> Option<Duration> {
        self.entries.get(key).map(|(inserted, _)| now - *inserted)
    }

    #[must_use]
    pub fn ttl(&self) -> Duration {
        self.ttl
    }

    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.entries.keys()
    }

    /// Remove everything older than the ttl. Returns how many entries went.
    pub fn sweep(&mut self, now: DateTime<Utc>) -> usize {
        let ttl = self.ttl;
        let before = self.entries.len();
        self.entries
            .retain(|_, (inserted, _)| now - *inserted <= ttl);
        if self.entries.len() != before {
            let live: std::collections::HashSet<String> = self.entries.keys().cloned().collect();
            self.order.retain(|key| live.contains(key));
        }
        before - self.entries.len()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
```

Add to `src/lib.rs`, next to `pub mod sealing;`:

```rust
pub mod replay;
```

- [ ] **Step 4: Run the tests and check that they pass**

Run: `cargo test --lib replay:: 2>&1 | grep -E "^test |test result"`
Expected: 9 tests, all ok.

Then `cargo fmt` and `cargo clippy -- -D warnings` (expected: exit 0).

- [ ] **Step 5: Commit**

```bash
git add src/replay.rs src/lib.rs
git commit -m "feat(replay): the replay guard and a bounded map (P2e)"
```

---

### Task 2: The delivery gate, counters and the knock record

**Files:**
- Modify: `src/replay.rs` (add the gate; extend the test module)

**Interfaces:**
- Consumes: `Freshness`, `Decision`, `ReplayConfig`, `ReplayGuard` from Task 1;
  `crate::sender_auth::{SenderVerdict, UnverifiableReason, ContradictedReason}`.
- Produces:
  - `pub struct GateConfig { pub accept_unverified: bool, pub knock_capacity: usize }` with `Default`.
  - `pub enum Admission { Admit { key_id: String }, AdmitUnverified, Reject }`
  - `pub struct Knock { pub claimed_global_id: String, pub key_id: String, pub reason: &'static str, pub first_seen: DateTime<Utc>, pub last_seen: DateTime<Utc>, pub count: u64 }`
  - `pub struct InboundCounters { pub admitted: u64, pub dropped_contradicted: u64, pub dropped_unverifiable: u64, pub dropped_replay: u64, pub delivered_stale: u64, pub delivered_ahead: u64, pub delivered_unchecked: u64, pub delivered_not_checked: u64 }`, `Clone`, `Copy`, `Default`, `Debug`.
  - `pub struct InboundState` with
    `InboundState::new(replay: ReplayConfig, gate: GateConfig, started: DateTime<Utc>) -> Self`,
    `admit(&mut self, verdict: &SenderVerdict, claimed_global_id: &str, proof_key_id: &str, now: DateTime<Utc>) -> Admission`,
    `check(&mut self, key_id: &str, message_id: &str, signed: DateTime<Utc>, now: DateTime<Utc>) -> Decision`,
    `counters(&self) -> InboundCounters`,
    `knocks(&self) -> Vec<Knock>` (newest `last_seen` first).
  - `pub fn verdict_reason(verdict: &SenderVerdict) -> &'static str` giving `"unsigned"`,
    `"unknown_sender"`, `"non_canonical_timestamp"`, `"key_mismatch"`, `"bad_signature"` or
    `"verified"`.

`InboundState::check` is the counting wrapper around `ReplayGuard::check`: it increments
`dropped_replay` or the matching `delivered_*` counter, and `admit` increments `admitted` or a
`dropped_*` counter and records the knock.

- [ ] **Step 1: Write the failing tests** (append to the same `mod tests`)

```rust
    use crate::sender_auth::{ContradictedReason, SenderVerdict, UnverifiableReason};

    fn verified() -> SenderVerdict {
        SenderVerdict::Verified { key_id: "k1".to_string() }
    }

    fn unsigned() -> SenderVerdict {
        SenderVerdict::Unverifiable { reason: UnverifiableReason::Unsigned }
    }

    fn bad_signature() -> SenderVerdict {
        SenderVerdict::Contradicted { reason: ContradictedReason::BadSignature }
    }

    fn state(accept_unverified: bool) -> InboundState {
        InboundState::new(
            ReplayConfig::default(),
            GateConfig { accept_unverified, knock_capacity: 4 },
            t(0),
        )
    }

    #[test]
    fn a_verified_sender_is_admitted_with_its_key_id() {
        let mut s = state(false);
        assert_eq!(
            s.admit(&verified(), "alice@x", "k1", t(10)),
            Admission::Admit { key_id: "k1".to_string() }
        );
        assert_eq!(s.counters().admitted, 1);
        assert!(s.knocks().is_empty());
    }

    #[test]
    fn unverified_is_rejected_by_default_and_recorded_as_a_knock() {
        let mut s = state(false);
        assert_eq!(s.admit(&unsigned(), "bob@x", "", t(10)), Admission::Reject);
        assert_eq!(s.admit(&unsigned(), "bob@x", "", t(20)), Admission::Reject);
        let counters = s.counters();
        assert_eq!(counters.dropped_unverifiable, 2);
        assert_eq!(counters.admitted, 0);
        let knocks = s.knocks();
        assert_eq!(knocks.len(), 1, "the same pair is one knock, counted twice");
        assert_eq!(knocks[0].claimed_global_id, "bob@x");
        assert_eq!(knocks[0].key_id, "unsigned");
        assert_eq!(knocks[0].reason, "unsigned");
        assert_eq!(knocks[0].first_seen, t(10));
        assert_eq!(knocks[0].last_seen, t(20));
        assert_eq!(knocks[0].count, 2);
    }

    #[test]
    fn unverified_is_admitted_unchecked_when_the_setting_is_on() {
        let mut s = state(true);
        assert_eq!(s.admit(&unsigned(), "bob@x", "", t(10)), Admission::AdmitUnverified);
        assert_eq!(s.counters().dropped_unverifiable, 0);
    }

    #[test]
    fn contradicted_is_rejected_under_both_settings() {
        for accept in [false, true] {
            let mut s = state(accept);
            assert_eq!(s.admit(&bad_signature(), "mallory@x", "k9", t(10)), Admission::Reject);
            assert_eq!(s.counters().dropped_contradicted, 1);
            assert_eq!(s.knocks()[0].reason, "bad_signature");
        }
    }

    #[test]
    fn the_knock_record_is_bounded_and_drops_the_least_recently_seen() {
        let mut s = state(false);
        for (i, id) in ["a", "b", "c", "d", "e"].iter().enumerate() {
            s.admit(&unsigned(), id, "", t(10 + i as i64));
        }
        let knocks = s.knocks();
        assert_eq!(knocks.len(), 4, "knock_capacity is 4");
        assert!(!knocks.iter().any(|k| k.claimed_global_id == "a"));
        assert_eq!(knocks[0].claimed_global_id, "e", "newest last_seen first");
    }

    #[test]
    fn the_counters_follow_the_freshness_of_what_is_delivered() {
        let mut s = state(false);
        assert_eq!(s.check("k", "m1", t(10), t(10)), Decision::Deliver(Freshness::Fresh));
        assert_eq!(s.check("k", "m1", t(10), t(11)), Decision::Drop);
        assert_eq!(s.check("k", "m2", t(10), t(400)), Decision::Deliver(Freshness::Stale));
        let counters = s.counters();
        assert_eq!(counters.dropped_replay, 1);
        assert_eq!(counters.delivered_stale, 1);
        assert_eq!(counters.delivered_ahead, 0);
    }
```

- [ ] **Step 2: Run them and check that they fail**

Run: `cargo test --lib replay:: 2>&1 | tail -20`
Expected: compilation fails — `InboundState`, `GateConfig`, `Admission` do not exist.

- [ ] **Step 3: Implement the gate** in `src/replay.rs`

```rust
use crate::sender_auth::{ContradictedReason, SenderVerdict, UnverifiableReason};

#[derive(Debug, Clone)]
pub struct GateConfig {
    /// Deliver `Unverifiable` messages instead of dropping them. Default: false.
    pub accept_unverified: bool,
    /// How many distinct (claimed id, key id) pairs the knock record holds. Default: 256.
    pub knock_capacity: usize,
}

impl Default for GateConfig {
    fn default() -> Self {
        Self {
            accept_unverified: false,
            knock_capacity: 256,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Admission {
    /// Verified: its id goes in the replay record under this key id.
    Admit { key_id: String },
    /// Unverified but accepted by configuration: delivered `NotChecked`, never recorded.
    AdmitUnverified,
    Reject,
}

/// Someone who tried to reach this node and was kept out. Grants nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Knock {
    pub claimed_global_id: String,
    pub key_id: String,
    pub reason: &'static str,
    pub first_seen: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
    pub count: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
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

#[must_use]
pub fn verdict_reason(verdict: &SenderVerdict) -> &'static str {
    match verdict {
        SenderVerdict::Verified { .. } => "verified",
        SenderVerdict::Unverifiable { reason } => match reason {
            UnverifiableReason::Unsigned => "unsigned",
            UnverifiableReason::UnknownSender => "unknown_sender",
        },
        SenderVerdict::Contradicted { reason } => match reason {
            ContradictedReason::NonCanonicalTimestamp => "non_canonical_timestamp",
            ContradictedReason::KeyMismatch => "key_mismatch",
            ContradictedReason::BadSignature => "bad_signature",
        },
    }
}

/// The gate and the guard together: what a receiver needs to decide about an inbound message.
#[derive(Debug)]
pub struct InboundState {
    gate: GateConfig,
    guard: ReplayGuard,
    knocks: HashMap<(String, String), Knock>,
    counters: InboundCounters,
}

impl InboundState {
    #[must_use]
    pub fn new(replay: ReplayConfig, gate: GateConfig, started: DateTime<Utc>) -> Self {
        Self {
            gate,
            guard: ReplayGuard::new(replay, started),
            knocks: HashMap::new(),
            counters: InboundCounters::default(),
        }
    }

    pub fn admit(
        &mut self,
        verdict: &SenderVerdict,
        claimed_global_id: &str,
        proof_key_id: &str,
        now: DateTime<Utc>,
    ) -> Admission {
        match verdict {
            SenderVerdict::Verified { key_id } => {
                self.counters.admitted += 1;
                Admission::Admit {
                    key_id: key_id.clone(),
                }
            }
            SenderVerdict::Contradicted { .. } => {
                self.counters.dropped_contradicted += 1;
                self.record_knock(verdict, claimed_global_id, proof_key_id, now);
                Admission::Reject
            }
            SenderVerdict::Unverifiable { .. } => {
                if self.gate.accept_unverified {
                    self.counters.delivered_not_checked += 1;
                    return Admission::AdmitUnverified;
                }
                self.counters.dropped_unverifiable += 1;
                self.record_knock(verdict, claimed_global_id, proof_key_id, now);
                Admission::Reject
            }
        }
    }

    pub fn check(
        &mut self,
        key_id: &str,
        message_id: &str,
        signed: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Decision {
        let decision = self.guard.check(key_id, message_id, signed, now);
        match decision {
            Decision::Drop => self.counters.dropped_replay += 1,
            Decision::Deliver(Freshness::Stale) => self.counters.delivered_stale += 1,
            Decision::Deliver(Freshness::Ahead) => self.counters.delivered_ahead += 1,
            Decision::Deliver(Freshness::Unchecked) => self.counters.delivered_unchecked += 1,
            Decision::Deliver(Freshness::Fresh | Freshness::NotChecked) => {}
        }
        decision
    }

    #[must_use]
    pub fn counters(&self) -> InboundCounters {
        self.counters
    }

    /// The knock record, newest `last_seen` first.
    #[must_use]
    pub fn knocks(&self) -> Vec<Knock> {
        let mut knocks: Vec<Knock> = self.knocks.values().cloned().collect();
        knocks.sort_by(|a, b| b.last_seen.cmp(&a.last_seen));
        knocks
    }

    fn record_knock(
        &mut self,
        verdict: &SenderVerdict,
        claimed_global_id: &str,
        proof_key_id: &str,
        now: DateTime<Utc>,
    ) {
        let key_id = if proof_key_id.is_empty() {
            "unsigned".to_string()
        } else {
            proof_key_id.to_string()
        };
        let key = (claimed_global_id.to_string(), key_id.clone());
        let entry = self.knocks.entry(key).or_insert_with(|| Knock {
            claimed_global_id: claimed_global_id.to_string(),
            key_id,
            reason: verdict_reason(verdict),
            first_seen: now,
            last_seen: now,
            count: 0,
        });
        entry.last_seen = now;
        entry.reason = verdict_reason(verdict);
        entry.count += 1;
        while self.knocks.len() > self.gate.knock_capacity {
            let Some(oldest) = self
                .knocks
                .iter()
                .min_by_key(|(_, knock)| knock.last_seen)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            self.knocks.remove(&oldest);
        }
    }
}
```

- [ ] **Step 4: Run the tests and check that they pass**

Run: `cargo test --lib replay:: 2>&1 | grep -E "^test |test result"`
Expected: 15 tests, all ok. Then `cargo fmt` and `cargo clippy -- -D warnings`.

- [ ] **Step 5: Commit**

```bash
git add src/replay.rs
git commit -m "feat(replay): deny unverified senders by default, with counters and a knock record (P2e)"
```

---

### Task 3: Wire the gate and guard into `TransportManager`

**Files:**
- Modify: `src/transport/manager.rs` (`ReceivedMessage`, the manager's fields, `receive_messages`,
  the builder, new accessors)
- Modify: `tests/receiver_acknowledgement.rs` (the hand-built `ReceivedMessage` gains `freshness`)
- Test: `tests/replay_suppression.rs` (new; spec §10 tests 7–11)

**Interfaces:**
- Consumes: `crate::replay::{Admission, Decision, Freshness, GateConfig, InboundCounters, InboundState, Knock, ReplayConfig}`.
- Produces:
  - `ReceivedMessage.freshness: Freshness` (public field, after `payload`).
  - `TransportManagerBuilder::replay_config(ReplayConfig)`, `::gate_config(GateConfig)`.
  - `TransportManager::inbound_counters(&self) -> InboundCounters` (async),
    `TransportManager::knocks(&self) -> Vec<Knock>` (async).

The manager holds `inbound: TokioRwLock<InboundState>`, built with `Utc::now()` as the horizon at
construction time. `TransportManager::new` uses `ReplayConfig::default()` and `GateConfig::default()`,
so every existing construction site keeps compiling.

In `receive_messages`, for each incoming message, in this order:

1. `let verdict = store.verify(&incoming.message);`
2. `let admission = inbound.admit(&verdict, &incoming.message.from_global_id, &incoming.message.sender_proof.key_id, now);`
3. On `Admission::Reject` → `continue` (nothing is delivered, no ack is applied).
4. On `Admission::Admit { key_id }` → `inbound.check(&key_id, &incoming.message.message_id.0.to_string(), incoming.message.timestamp.0, now)`; on `Decision::Drop` → `continue`; otherwise the freshness comes from the decision.
5. On `Admission::AdmitUnverified` → freshness is `Freshness::NotChecked`, and **nothing is recorded**.
6. Acks (`delivery_ack::is_ack`) are applied only after passing steps 3–5, so a replayed ack is
   dropped before `apply_ack`.
7. Otherwise open the payload (slice d) and push a `ReceivedMessage` with its freshness.

`now` is read **once** per `receive_messages` call, as `Utc::now()`, and passed to every step.

- [ ] **Step 1: Write the failing integration tests** in `tests/replay_suppression.rs`. Copy the
  `free_udp_port`, `udp_node`, `send_raw` and `receive_one` helpers from `tests/sealing.rs:354-408`
  — a test binary cannot import another's helpers, and this crate's tests already duplicate them.
  A verbatim replay is just `send_raw` called twice with the same `SecureMessage`.

```rust
// SPDX-License-Identifier: MIT OR Apache-2.0
//! P2 slice e: replays are dropped, unverified senders are denied, and the counters say so.
//! Test numbers refer to docs/superpowers/specs/2026-09-17-replay-suppression-design.md §10.

use synapse::CryptoManager;
use synapse::replay::{Freshness, GateConfig};
use synapse::sender_auth::TrustStore;
use synapse::types::{SecureMessage, SecurityLevel};

const ALICE: &str = "alice@synapse.test";
const BOB: &str = "bob@synapse.test";

fn signer() -> CryptoManager {
    let mut crypto = CryptoManager::new();
    crypto.generate_keypair().unwrap();
    crypto
}

fn store_for(alice: &CryptoManager) -> TrustStore {
    let mut store = TrustStore::new();
    store.pin(ALICE, alice.public_key_bytes().unwrap());
    store
}

/// A signed, unsealed message from Alice to Bob at `Authenticated`, so these tests exercise the
/// gate and the guard without involving slice d's sealing.
fn signed(alice: &CryptoManager, text: &[u8]) -> SecureMessage {
    let mut m = SecureMessage::new(BOB, ALICE, text.to_vec(), SecurityLevel::Authenticated);
    alice.sign_secure_message(&mut m).expect("sign");
    m
}

/// Poll for up to 2 s and return everything that arrived.
async fn drain(
    manager: &synapse::transport::TransportManager,
) -> Vec<synapse::transport::ReceivedMessage> {
    let mut out = Vec::new();
    for _ in 0..20 {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        out.extend(manager.receive_messages().await.expect("receive"));
    }
    out
}

// §10 test 7
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_resent_datagram_is_dropped_the_second_time() {
    let alice = signer();
    let (bob, port) = udp_node(store_for(&alice), None).await;
    let message = signed(&alice, b"once only");

    send_raw(port, &message);
    let first = receive_one(&bob).await;
    assert!(first.sender.is_verified());
    assert_eq!(first.freshness, Freshness::Fresh);

    // The identical datagram again: a verbatim replay.
    send_raw(port, &message);
    assert!(
        drain(&bob).await.is_empty(),
        "the replay must not be delivered"
    );
    assert_eq!(bob.inbound_counters().await.dropped_replay, 1);

    // Control: a genuinely new message from the same sender still arrives.
    send_raw(port, &signed(&alice, b"second message"));
    let second = receive_one(&bob).await;
    assert_eq!(second.freshness, Freshness::Fresh);
    assert_eq!(bob.inbound_counters().await.dropped_replay, 1);
}

// §10 test 8
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unverified_sender_is_dropped_by_default_and_shows_as_a_knock() {
    let alice = signer();
    // Bob pins nobody, so Alice is Unverifiable(UnknownSender).
    let (bob, port) = udp_node(TrustStore::new(), None).await;
    send_raw(port, &signed(&alice, b"who am i"));

    assert!(
        drain(&bob).await.is_empty(),
        "an unpinned sender is denied by default"
    );
    let counters = bob.inbound_counters().await;
    assert_eq!(counters.dropped_unverifiable, 1);
    assert_eq!(counters.admitted, 0);
    let knocks = bob.knocks().await;
    assert_eq!(knocks.len(), 1);
    assert_eq!(knocks[0].claimed_global_id, ALICE);
    assert_eq!(
        knocks[0].key_id,
        synapse::sender_auth::key_id(&alice.public_key_bytes().unwrap())
    );
    assert_eq!(knocks[0].reason, "unknown_sender");
    assert_eq!(knocks[0].count, 1);
}

// §10 test 8, control: the opt-in setting delivers the same message, marked.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn accept_unverified_delivers_it_marked_not_checked() {
    let alice = signer();
    let (bob, port) = udp_node_with(
        TrustStore::new(),
        None,
        GateConfig {
            accept_unverified: true,
            ..GateConfig::default()
        },
    )
    .await;
    send_raw(port, &signed(&alice, b"who am i"));
    let received = receive_one(&bob).await;
    assert_eq!(received.freshness, Freshness::NotChecked);
    assert!(!received.sender.is_verified());
}

// §10 test 9
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_contradicted_message_is_dropped_under_both_settings() {
    for accept_unverified in [false, true] {
        let alice = signer();
        let impostor = signer();
        // Alice's id is pinned to Alice's key, but the message is signed by the impostor's key.
        let mut message =
            SecureMessage::new(BOB, ALICE, b"not me".to_vec(), SecurityLevel::Authenticated);
        impostor.sign_secure_message(&mut message).expect("sign");

        let (bob, port) = udp_node_with(
            store_for(&alice),
            None,
            GateConfig {
                accept_unverified,
                ..GateConfig::default()
            },
        )
        .await;
        send_raw(port, &message);
        assert!(
            drain(&bob).await.is_empty(),
            "a contradicted sender is always denied"
        );
        assert_eq!(bob.inbound_counters().await.dropped_contradicted, 1);
        assert_eq!(bob.knocks().await[0].reason, "key_mismatch");
    }
}

// §10 test 10: the suppression test. An unsigned copy must not reserve a genuine id.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unsigned_copy_cannot_suppress_a_genuine_message() {
    let alice = signer();
    let (bob, port) = udp_node_with(
        store_for(&alice),
        None,
        GateConfig {
            accept_unverified: true,
            ..GateConfig::default()
        },
    )
    .await;

    let genuine = signed(&alice, b"the real thing");
    // The same message_id, unsigned, sent first.
    let mut forged = SecureMessage::new(
        BOB,
        ALICE,
        b"the forgery".to_vec(),
        SecurityLevel::Authenticated,
    );
    forged.message_id = genuine.message_id.clone();

    send_raw(port, &forged);
    let first = receive_one(&bob).await;
    assert_eq!(first.freshness, Freshness::NotChecked);

    send_raw(port, &genuine);
    let second = receive_one(&bob).await;
    assert_eq!(
        second.freshness,
        Freshness::Fresh,
        "the genuine message must not be suppressed"
    );
    assert!(second.sender.is_verified());
    assert_eq!(bob.inbound_counters().await.dropped_replay, 0);
}

// §10 test 11
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_replayed_ack_is_dropped_before_it_is_applied() {
    use synapse::transport::abstraction::DeliveryConfirmation;

    let alice = signer();
    let bob_signer = signer();
    let mut alice_store = TrustStore::new();
    alice_store.pin(BOB, bob_signer.public_key_bytes().unwrap());
    let (alice_node, alice_port) = udp_node(alice_store, None).await;
    let (bob_node, bob_port) = udp_node(store_for(&alice), None).await;

    // Alice sends, asking for an ack at her own port. request_ack changes signed metadata, so the
    // message is signed afterwards.
    let mut request =
        SecureMessage::new(BOB, ALICE, b"please ack".to_vec(), SecurityLevel::Authenticated);
    request.request_ack(format!("127.0.0.1:{alice_port}"));
    alice.sign_secure_message(&mut request).expect("sign");
    let message_id = request.message_id.0.to_string();

    // A raw listener would see the ack, but Alice's own node must apply it, so send through her
    // manager exactly as tests/receiver_acknowledgement.rs does.
    send_to_manager(&alice_node, &request, bob_port).await;

    let received = receive_one(&bob_node).await;
    bob_node
        .acknowledge(&received, &bob_signer)
        .await
        .expect("ack");

    // Alice applies the ack.
    let _ = drain(&alice_node).await;
    assert_eq!(
        alice_node.delivery_status(&message_id).await,
        Some(DeliveryConfirmation::Acknowledged)
    );

    // Capture the ack Bob sent and replay it verbatim.
    let ack = captured_ack(&received, &bob_signer);
    send_raw(alice_port, &ack);
    let _ = drain(&alice_node).await;
    assert_eq!(alice_node.inbound_counters().await.dropped_replay, 1);
    assert_eq!(
        alice_node.delivery_status(&message_id).await,
        Some(DeliveryConfirmation::Acknowledged),
        "the status is unchanged, and the replayed ack never reached apply_ack"
    );
}
```

**Three names above must be read out of the code before this test file is written, and the code
followed where it differs.** They are not guesses to be pasted:

1. **How a message is sent to a chosen address.** `send_to_manager` is a placeholder for whatever
   `tests/receiver_acknowledgement.rs` already does — read that file and copy the call it uses,
   inlining it here.
2. **How the ack's bytes are obtained.** `captured_ack` is a placeholder. Read
   `src/delivery_ack.rs` and `TransportManager::acknowledge`'s return type: if the receipt exposes
   the ack message, clone it. If it does not, bind a `tokio::net::UdpSocket` at the `reply_to`
   address **instead of** Alice's manager port, as `tests/sealing.rs:444` does, read the datagram
   and `serde_json::from_slice` it — then send the same bytes to Alice's real port twice, so the
   first is applied and the second is the replay.
3. **Whether `request_ack` must precede signing.** Slice b's spec says `synapse.reply_to` is covered
   by the signature, so `request_ack` comes first and signing second; confirm against
   `src/delivery_ack.rs` before relying on it.

`udp_node_with(store, sealing_key, gate)` is `udp_node` plus `.gate_config(gate)`; write `udp_node`
as a thin wrapper over it so the file has one builder.

- [ ] **Step 2: Run them and check that they fail**

Run: `cargo test --test replay_suppression 2>&1 | tail -20`
Expected: compilation fails — `synapse::replay` has no `freshness` on `ReceivedMessage`, and
`inbound_counters` does not exist.

- [ ] **Step 3: Implement the wiring**

In `src/transport/manager.rs`:

```rust
// in ReceivedMessage, after `payload`:
    /// Whether the signed timestamp could be checked, and what it said (P2 slice e).
    pub freshness: crate::replay::Freshness,

// in the TransportManager struct, beside `sealing_key`:
    /// The delivery gate and the replay record (P2 slice e).
    inbound: TokioRwLock<crate::replay::InboundState>,

// in TransportManager::new:
            inbound: TokioRwLock::new(crate::replay::InboundState::new(
                crate::replay::ReplayConfig::default(),
                crate::replay::GateConfig::default(),
                chrono::Utc::now(),
            )),
```

`receive_messages`'s loop body becomes:

```rust
        let store = self.trust_store.read().await;
        let sealing_key = self.sealing_key.read().await;
        let mut inbound = self.inbound.write().await;
        let now = chrono::Utc::now();
        let mut delivered = Vec::with_capacity(all_messages.len());
        for incoming in all_messages {
            let message = &incoming.message;
            let verdict = store.verify(message);
            let admission = inbound.admit(
                &verdict,
                &message.from_global_id,
                &message.sender_proof.key_id,
                now,
            );
            let freshness = match admission {
                crate::replay::Admission::Reject => continue,
                crate::replay::Admission::AdmitUnverified => crate::replay::Freshness::NotChecked,
                crate::replay::Admission::Admit { key_id } => {
                    match inbound.check(
                        &key_id,
                        &message.message_id.0.to_string(),
                        message.timestamp.0,
                        now,
                    ) {
                        crate::replay::Decision::Drop => continue,
                        crate::replay::Decision::Deliver(freshness) => freshness,
                    }
                }
            };
            // Acks are control traffic: applied here, never handed to the application.
            if delivery_ack::is_ack(message) {
                self.apply_ack_locked(message, &verdict).await;
                continue;
            }
            let payload = crate::sealing::open(message, sealing_key.as_ref());
            delivered.push(ReceivedMessage {
                incoming,
                sender: verdict,
                payload,
                freshness,
            });
        }
        Ok(delivered)
```

`apply_ack` takes `&self` and locks `outbound`; it does not touch `inbound`, so calling it while
`inbound` is held is safe. Keep the existing name — the rename above is only illustrative; call
`self.apply_ack(message, &verdict).await` and confirm with the compiler that no lock is taken twice.
If a deadlock appears, drop the `inbound` guard before the ack call by collecting admitted acks into
a local `Vec` and applying them after the loop.

Builder additions:

```rust
    /// Replay window and record size (P2 slice e).
    pub fn replay_config(mut self, config: crate::replay::ReplayConfig) -> Self {
        self.replay = config;
        self
    }

    /// Which sender verdicts are admitted (P2 slice e).
    pub fn gate_config(mut self, config: crate::replay::GateConfig) -> Self {
        self.gate = config;
        self
    }
```

with `replay: crate::replay::ReplayConfig` and `gate: crate::replay::GateConfig` fields on
`TransportManagerBuilder`, defaulted in `new()`, and in `build()`:

```rust
        manager.inbound = TokioRwLock::new(crate::replay::InboundState::new(
            self.replay,
            self.gate,
            chrono::Utc::now(),
        ));
```

Accessors:

```rust
    /// Counts of what was admitted, dropped and marked (P2 slice e).
    pub async fn inbound_counters(&self) -> crate::replay::InboundCounters {
        self.inbound.read().await.counters()
    }

    /// Who tried to reach this node and was kept out. Grants nothing.
    pub async fn knocks(&self) -> Vec<crate::replay::Knock> {
        self.inbound.read().await.knocks()
    }
```

In `tests/receiver_acknowledgement.rs`, the hand-built `ReceivedMessage` gains
`freshness: synapse::replay::Freshness::Fresh,`.

- [ ] **Step 4: Run the tests and check that they pass**

Run:
```
cargo test --test replay_suppression --test receiver_acknowledgement --test sealing --test sender_authentication --test mcp_surface 2>&1 | grep -E "^test |test result"
```
Expected: all pass. `mcp_surface` still passes because `synapse-mcp` pins every peer, so its senders
are verified. If it fails, the cause is a test peer that was never pinned — fix the test, not the
gate.

Then `cargo fmt`, `cargo clippy -- -D warnings`, and
`cargo clippy --test replay_suppression -- -D warnings`.

- [ ] **Step 5: Commit**

```bash
git add src/transport/manager.rs tests/replay_suppression.rs tests/receiver_acknowledgement.rs
git commit -m "feat(transport)!: drop replays and deny unverified senders on receive (P2e)"
```

---

### Task 4: Bound the ack tracking, and add `Expired`

**Files:**
- Modify: `src/transport/abstraction.rs` (`DeliveryConfirmation::Expired`)
- Modify: `src/transport/manager.rs` (`outbound` becomes `Bounded<Outbound>`)
- Test: `tests/replay_suppression.rs` (spec §10 test 12)

**Interfaces:**
- Consumes: `crate::replay::Bounded` from Task 1.
- Produces: `DeliveryConfirmation::Expired`; `TransportManagerBuilder::tracking_limits(ttl: chrono::Duration, capacity: usize)`; `TransportManager::tracked_count(&self) -> usize` (async), used by the test to show the bound.

`outbound: TokioRwLock<crate::replay::Bounded<Outbound>>`. `track_if_ack_requested` inserts with
`Utc::now()` and keeps its `or_insert` semantics: resending must never downgrade an `Acknowledged`
entry, so check `contains_key` first and only insert when absent. `delivery_status` sweeps first,
then reads; an entry older than the ttl that is still `Sent` or `Delivered` reports `Expired`.

- [ ] **Step 1: Write the failing test**

```rust
#[tokio::test]
async fn an_unacknowledged_tracked_message_expires() {
    // Alice's manager is built with .tracking_limits(chrono::Duration::seconds(0), 10).
    // She sends one message with request_ack to a port nobody listens on.
    //   - delivery_status(id) is Some(DeliveryConfirmation::Expired).
    //   - Control: with the default 1 h ttl, the same send reports Sent.
}

#[tokio::test]
async fn ack_tracking_is_capped() {
    // Alice's manager is built with .tracking_limits(chrono::Duration::hours(1), 2).
    // She sends four messages with request_ack.
    //   - tracked_count() == 2.
    //   - delivery_status of the first id is None; of the last, Some(Sent).
}
```

- [ ] **Step 2: Run them and check that they fail**

Run: `cargo test --test replay_suppression 2>&1 | tail -20`
Expected: `Expired`, `tracking_limits` and `tracked_count` do not exist.

- [ ] **Step 3: Implement**

```rust
// src/transport/abstraction.rs, in DeliveryConfirmation:
    /// No acknowledgement arrived within the tracking window (P2 slice e). It says nothing about
    /// whether the message was delivered.
    Expired,
```

**How `Expired` is produced.** `Bounded::sweep` deletes entries older than the ttl, so an expired
entry would simply vanish and `delivery_status` would return `None` — which reads as "never tracked".
To report expiry instead, `delivery_status` asks the age **before** sweeping, using `age_of` from
Task 1:

```rust
    pub async fn delivery_status(&self, message_id: &str) -> Option<DeliveryConfirmation> {
        let now = chrono::Utc::now();
        let mut outbound = self.outbound.write().await;
        let ttl = outbound.ttl();
        let expired = outbound
            .age_of(message_id, now)
            .is_some_and(|age| age > ttl);
        let status = outbound.get(message_id).map(|entry| entry.status);
        outbound.sweep(now);
        match status {
            // An ack that arrived is final; only an unacknowledged entry expires.
            Some(DeliveryConfirmation::Sent | DeliveryConfirmation::Delivered) if expired => {
                Some(DeliveryConfirmation::Expired)
            }
            other => other,
        }
    }
```

So an expired-but-not-yet-evicted entry reports `Expired`; once the sweep removes it, the next call
returns `None`. With a ttl of 0 seconds, the first call after the send already reports `Expired`,
which is what the test below relies on.

Builder:

```rust
    /// How long a sent message's ack is tracked, and how many at once (P2 slice e).
    pub fn tracking_limits(mut self, ttl: chrono::Duration, capacity: usize) -> Self {
        self.tracking_ttl = ttl;
        self.tracking_capacity = capacity;
        self
    }
```

Defaults: `chrono::Duration::hours(1)` and `10_000`, in both `TransportManager::new` and
`TransportManagerBuilder::new`.

- [ ] **Step 4: Run the tests and check that they pass**

Run: `cargo test --test replay_suppression --test receiver_acknowledgement 2>&1 | grep -E "^test |test result"`
Expected: all pass. Then `cargo fmt` and both clippy invocations.

Note: `DeliveryConfirmation` gains a variant, so every `match` over it must be updated. Run
`cargo build --all-targets` and fix each one; `src/mcp_server.rs` serialises it with serde, so check
what JSON name `Expired` gets and use it in Task 5.

- [ ] **Step 5: Commit**

```bash
git add src/transport/abstraction.rs src/transport/manager.rs src/replay.rs tests/replay_suppression.rs
git commit -m "feat(transport)!: bound ack tracking by age and count, with an Expired status (P2e)"
```

---

### Task 5: `synapse-mcp` — bounds, freshness, drop counts and knocks

**Files:**
- Modify: `src/mcp_server.rs`
- Test: `tests/mcp_surface.rs`

**Interfaces:**
- Consumes: everything from Tasks 1–4.
- Produces: the config keys in spec §7; `poll`'s `freshness` and `dropped`; `list`'s `knocking`;
  `ack`'s new refusal text.

Config additions to `McpConfig`, all `#[serde(default)]` with the spec's defaults:

```rust
    /// Deliver messages from senders this server cannot verify, marked `not_checked`.
    #[serde(default)]
    pub accept_unverified: bool,
    #[serde(default = "default_replay_past")]
    pub replay_past_seconds: i64,
    #[serde(default = "default_replay_ahead")]
    pub replay_ahead_seconds: i64,
    #[serde(default = "default_replay_retention")]
    pub replay_retention_seconds: i64,
    #[serde(default = "default_replay_capacity")]
    pub replay_capacity: usize,
    #[serde(default = "default_tracking_ttl")]
    pub tracking_ttl_seconds: i64,
```

`start()` builds `ReplayConfig` from them, calls `validate()`, and on error returns
`config_error("the replay retention must be at least the past plus ahead window")` — the fixed text
from `validate`, with no paths. It passes `.replay_config(..)`, `.gate_config(GateConfig { accept_unverified: config.accept_unverified, ..Default::default() })` and
`.tracking_limits(Duration::seconds(config.tracking_ttl_seconds), 10_000)` to the builder.

`kept` becomes `Mutex<crate::replay::Bounded<ReceivedMessage>>` with
`Bounded::new(Duration::seconds(config.tracking_ttl_seconds), 1_000)`, and `sent_with_ack` becomes
`Mutex<crate::replay::Bounded<()>>` with the same limits — its keys are the ids, and `poll` iterates
`keys()`.

`message_view` gains `"freshness": received.freshness.name()`.

`poll`'s reply gains `"dropped"`:

```rust
        let counters = inner.manager.inbound_counters().await;
        json!({
            "messages": messages,
            "deliveries": deliveries,
            "dropped": {
                "contradicted": counters.dropped_contradicted,
                "unverifiable": counters.dropped_unverifiable,
                "replay": counters.dropped_replay,
            },
        })
```

`list`'s reply gains `"knocking"`, built from `inner.manager.knocks().await`, each entry
`{"claimed_global_id", "key_id", "reason", "first_seen", "last_seen", "count"}` with the timestamps
as RFC 3339 strings.

`ack`'s not-found refusal becomes, when the id is absent:
`format!("message_id {} is no longer held: only ids that poll returned recently can be acknowledged", args.message_id)`.

Description changes:
- `poll` keeps its slice c text **verbatim** and appends, as one sentence: `" A message's freshness says whether its signed timestamp could be checked; only fresh means the message is known not to be a replay."`
- `list` appends: `" knocking lists senders that were refused, with the key they presented; they are not peers and are granted nothing."`

- [ ] **Step 1: Write the failing tests** in `tests/mcp_surface.rs`, extending the existing
  `server_with` helper with the new config keys.

```rust
#[tokio::test]
async fn poll_reports_freshness_and_drop_counts() {
    // Alice sends to Bob. Bob polls.
    //   - the message view has "freshness" == "fresh"
    //   - reply["dropped"] has contradicted, unverifiable and replay, all 0
}

#[tokio::test]
async fn list_reports_who_was_refused() {
    // A raw unsigned SecureMessage is sent to Bob's port, then Bob polls (which drops it) and lists.
    //   - poll returned no messages and dropped.unverifiable == 1
    //   - list()["knocking"][0] has reason "unsigned" and key_id "unsigned"
    //   - Control: list()["peers"] still shows the configured peer, so the two are distinct.
}

#[tokio::test]
async fn ack_refuses_a_message_that_is_no_longer_held() {
    // Bob's server is configured with tracking_ttl_seconds = 0, so kept evicts at once.
    //   - poll returns the message
    //   - ack on its id is refused, and the text contains "no longer held"
}

#[tokio::test]
async fn a_retention_shorter_than_the_window_is_refused_at_startup() {
    // replay_retention_seconds = 10 with the default 300 + 60 window.
    //   - start() returns Err
    //   - the message contains "retention" and names no path
    //   - Control: the same config with the default retention starts.
}
```

- [ ] **Step 2: Run them and check that they fail**

Run: `cargo test --test mcp_surface 2>&1 | tail -20`
Expected: the new config fields and JSON keys do not exist.

- [ ] **Step 3: Implement** the changes described above in `src/mcp_server.rs`.

- [ ] **Step 4: Run the tests and check that they pass, then 3 more times**

Run: `cargo test --test mcp_surface --test mcp_stdio_process 2>&1 | grep -E "^test |test result"`
Expected: all pass, four runs in a row. Then `cargo fmt`, `cargo clippy -- -D warnings`, and
`cargo clippy --test mcp_surface --test mcp_stdio_process --test replay_suppression -- -D warnings`.

- [ ] **Step 5: Commit**

```bash
git add src/mcp_server.rs tests/mcp_surface.rs
git commit -m "feat(mcp): freshness, drop counts, knocks and bounded state (P2e)"
```

---

### Task 6: Mutation, full verification, docs, PR

- [ ] **Step 1: Mutation check.** Predict first, in writing: skipping the record insertion makes
  **only** the replay tests fail — unit tests
  `the_same_key_and_id_is_dropped_the_second_time`, `a_marked_message_is_recorded_too`,
  `a_replay_flood_cannot_age_its_own_id_out_of_the_record`, and integration tests
  `a_resent_datagram_is_dropped_the_second_time` and
  `a_replayed_ack_is_dropped_before_it_is_applied`.

  In `ReplayGuard::check`, comment out the `self.seen.insert(..)` call, leaving
  `self.evict(now); Decision::Deliver(freshness)`. Run:
  ```
  cargo test --no-fail-fast --lib replay:: --test replay_suppression --test mcp_surface 2>&1 | grep -E "FAILED|test result"
  ```
  Record the failing set, compare it with the prediction, restore the line, and confirm
  `git status --short` is empty.

- [ ] **Step 2: Full verification** in a **fresh** target directory.

```bash
export CARGO_TARGET_DIR=.../scratchpad/target-replay-fresh   # must not exist yet
date -u +%FT%TZ            # record the window start
cargo test --no-fail-fast  # expect only {test_transport_error_handling}
date -u +%FT%TZ            # record the window end
```
Then count Windows Firewall event 2097 in that window with `Get-WinEvent`, and state the positive
control (the query finds a 2097 from an earlier window). Report the named `ok` count and the
doc-test count. Run `cargo fmt -- --check` and `cargo clippy -- -D warnings` (both exit 0).

- [ ] **Step 3: Docs.** In `CAPABILITY_INVENTORY.md`, beside the slice a and slice d notes, add a
  dated note: replay suppression and bounded inbound state are built on branch
  `feat/replay-suppression` (held), `main` has neither, and unverified senders are denied by default
  there. State plainly that the router's email path is still unauthenticated (spec §8).

- [ ] **Step 4: Push and open the PR.**

```bash
git push origin feat/replay-suppression
gh pr create --draft --base feat/sealing --head feat/replay-suppression \
  --title "feat!: replay suppression and bounded inbound state (P2 slice e)" --body-file <(...)
```
The body carries: what changed; the breaking changes (`ReceivedMessage.freshness`,
`DeliveryConfirmation::Expired`, unverified senders denied by default); the verification numbers from
Step 2 with their ref; the mutation result; and the stack position (into `feat/sealing` → `#39` →
`#38` → main).

- [ ] **Step 5: Report.** Post a `[READY]`-style comment on the PR for the PM's daily pass (the PM
  session is down between passes), and tell CireSnave: what landed, the measured numbers, and that
  slice f (account keys and agent certificates) is next and needs its own design session.
