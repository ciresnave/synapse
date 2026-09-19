// SPDX-License-Identifier: MIT OR Apache-2.0
//! Replay suppression and bounded inbound state (P2 slice e).
//!
//! Every entry point takes `now`, so this module has no clock, no I/O and no locks: the caller owns
//! all three. See `docs/superpowers/specs/2026-09-17-replay-suppression-design.md`.

use crate::sender_auth::{ContradictedReason, SenderVerdict, UnverifiableReason};
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
        if self.past < Duration::zero() || self.ahead < Duration::zero() {
            return Err("the replay window must not be negative");
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

    /// Order matters: the retention sweep runs before the lookup, so a message whose id was
    /// recorded more than `retention` ago is treated as new (never as a drop) even though the
    /// same id was seen before. See
    /// `docs/superpowers/specs/2026-09-17-replay-suppression-design.md` §5 for why this differs
    /// from the brief.
    pub fn check(
        &mut self,
        key_id: &str,
        message_id: &str,
        signed: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Decision {
        self.sweep_retention(now);

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
        self.evict_for_capacity(now);
        Decision::Deliver(freshness)
    }

    /// Remove entries whose `now - recorded_at > retention`.
    fn sweep_retention(&mut self, now: DateTime<Utc>) {
        let retention = self.config.retention;
        self.seen
            .retain(|_, entry| now - entry.recorded_at <= retention);
    }

    /// Evict the oldest-recorded entries until the record is back within capacity, raising the
    /// horizon past anything evicted.
    ///
    /// The raise is clamped to `now`: an entry signed far in the future must not push the horizon
    /// past the wall clock, or every later message would be classified `Unchecked` for the rest of
    /// the process's life (`Fresh` would become unreachable). Residual: a message first delivered
    /// `Ahead` and later evicted here could, once the wall clock passes its signed time, be
    /// delivered `Fresh` on a replay. That is outside the spec's guarantee, which is stated over
    /// messages first delivered `Fresh`.
    fn evict_for_capacity(&mut self, now: DateTime<Utc>) {
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
            // The record can no longer speak for anything signed at or before the evicted entry,
            // but never past `now`.
            let raised = entry.signed.min(now);
            if raised > self.horizon {
                self.horizon = raised;
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
    Admit {
        key_id: String,
    },
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
            UnverifiableReason::UnsupportedVersion => "unsupported_protocol_version",
            UnverifiableReason::Unsigned => "unsigned",
            UnverifiableReason::UnknownSender => "unknown_sender",
            UnverifiableReason::UnknownIssuer => "unknown_issuer",
            UnverifiableReason::InvalidChain => "invalid_chain",
            UnverifiableReason::ChainTooLarge => "chain_too_large",
            UnverifiableReason::NoSendPermission => "no_send_permission",
        },
        SenderVerdict::Contradicted { reason } => match reason {
            ContradictedReason::NonCanonicalTimestamp => "non_canonical_timestamp",
            ContradictedReason::KeyMismatch => "key_mismatch",
            ContradictedReason::BadSignature => "bad_signature",
            ContradictedReason::IdentityMismatch => "identity_mismatch",
        },
    }
}

/// Truncate an attacker-controlled string to at most 256 characters, on a char boundary.
fn truncate_knock_string(s: &str) -> String {
    s.chars().take(256).collect()
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
        knocks.sort_by_key(|k| std::cmp::Reverse(k.last_seen));
        knocks
    }

    fn record_knock(
        &mut self,
        verdict: &SenderVerdict,
        claimed_global_id: &str,
        proof_key_id: &str,
        now: DateTime<Utc>,
    ) {
        // Both strings come off the wire before any verification, so they are attacker-controlled
        // and unbounded in length; truncate (char-boundary-safe) before they are stored anywhere,
        // including as map keys, so the capacity bound is over bounded keys.
        let claimed_global_id = truncate_knock_string(claimed_global_id);
        let key_id = if proof_key_id.is_empty() {
            "unsigned".to_string()
        } else {
            truncate_knock_string(proof_key_id)
        };
        let key = (claimed_global_id.clone(), key_id.clone());
        let entry = self.knocks.entry(key).or_insert_with(|| Knock {
            claimed_global_id,
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
        assert_eq!(
            g.check("k1", "m1", t(10), t(10)),
            Decision::Deliver(Freshness::Fresh)
        );
        assert_eq!(g.check("k1", "m1", t(10), t(11)), Decision::Drop);
        // Control: a different id from the same key still gets through.
        assert_eq!(
            g.check("k1", "m2", t(11), t(11)),
            Decision::Deliver(Freshness::Fresh)
        );
        // Control: the same id from a different key is a different message.
        assert_eq!(
            g.check("k2", "m1", t(11), t(11)),
            Decision::Deliver(Freshness::Fresh)
        );
    }

    #[test]
    fn classification_at_the_window_boundaries() {
        let mut g = guard();
        // Exactly `past` old is still fresh; one second beyond is stale.
        assert_eq!(
            g.check("k", "a", t(10), t(310)),
            Decision::Deliver(Freshness::Fresh)
        );
        assert_eq!(
            g.check("k", "b", t(10), t(311)),
            Decision::Deliver(Freshness::Stale)
        );
        // Exactly `ahead` in the future is fresh; one second beyond is ahead.
        assert_eq!(
            g.check("k", "c", t(370), t(310)),
            Decision::Deliver(Freshness::Fresh)
        );
        assert_eq!(
            g.check("k", "d", t(371), t(310)),
            Decision::Deliver(Freshness::Ahead)
        );
    }

    #[test]
    fn anything_signed_at_or_before_the_horizon_is_unchecked() {
        let mut g = ReplayGuard::new(ReplayConfig::default(), t(100));
        assert_eq!(
            g.check("k", "a", t(100), t(110)),
            Decision::Deliver(Freshness::Unchecked)
        );
        assert_eq!(
            g.check("k", "b", t(99), t(110)),
            Decision::Deliver(Freshness::Unchecked)
        );
        // Control: after the horizon, inside the window, it is fresh.
        assert_eq!(
            g.check("k", "c", t(101), t(110)),
            Decision::Deliver(Freshness::Fresh)
        );
    }

    #[test]
    fn a_marked_message_is_recorded_too() {
        let mut g = guard();
        assert_eq!(
            g.check("k", "m", t(10), t(400)),
            Decision::Deliver(Freshness::Stale)
        );
        assert_eq!(g.check("k", "m", t(10), t(401)), Decision::Drop);
    }

    #[test]
    fn a_replay_flood_cannot_age_its_own_id_out_of_the_record() {
        let mut g = guard();
        assert_eq!(
            g.check("k", "m", t(10), t(10)),
            Decision::Deliver(Freshness::Fresh)
        );
        // Each drop refreshes recorded_at, so the id never expires while copies keep arriving.
        for step in 1..20 {
            assert_eq!(g.check("k", "m", t(10), t(10 + step * 300)), Decision::Drop);
        }
    }

    #[test]
    fn after_retention_the_id_is_forgotten_but_never_looks_fresh() {
        let mut g = guard();
        assert_eq!(
            g.check("k", "m", t(10), t(10)),
            Decision::Deliver(Freshness::Fresh)
        );
        // 6 min later the entry is swept; the same message is delivered again, as stale.
        assert_eq!(
            g.check("k", "m", t(10), t(400)),
            Decision::Deliver(Freshness::Stale)
        );
    }

    #[test]
    fn capacity_eviction_raises_the_horizon() {
        let config = ReplayConfig {
            capacity: 2,
            ..ReplayConfig::default()
        };
        let mut g = ReplayGuard::new(config, t(0));
        assert_eq!(
            g.check("k", "a", t(10), t(10)),
            Decision::Deliver(Freshness::Fresh)
        );
        assert_eq!(
            g.check("k", "b", t(11), t(11)),
            Decision::Deliver(Freshness::Fresh)
        );
        // Inserting a third entry evicts "a" (oldest recorded_at) and moves the horizon to t(10).
        assert_eq!(
            g.check("k", "c", t(12), t(12)),
            Decision::Deliver(Freshness::Fresh)
        );
        assert_eq!(g.len(), 2);
        assert_eq!(g.horizon(), t(10));
        // A message signed at or before the horizon can no longer be called fresh.
        assert_eq!(
            g.check("k", "d", t(10), t(13)),
            Decision::Deliver(Freshness::Unchecked)
        );
        // Control: one signed after the horizon still is.
        assert_eq!(
            g.check("k", "e", t(13), t(13)),
            Decision::Deliver(Freshness::Fresh)
        );
    }

    #[test]
    fn capacity_eviction_never_raises_the_horizon_past_now() {
        let config = ReplayConfig {
            capacity: 2,
            ..ReplayConfig::default()
        };
        let mut g = ReplayGuard::new(config, t(0));
        // A message signed far in the future is classified Ahead but still recorded.
        assert_eq!(
            g.check("k", "a", t(10_000_000), t(10)),
            Decision::Deliver(Freshness::Ahead)
        );
        assert_eq!(
            g.check("k", "b", t(11), t(11)),
            Decision::Deliver(Freshness::Fresh)
        );
        // A third entry evicts "a" (oldest recorded_at); the raise must clamp to `now`, not
        // jump to the year-3000 `signed` time on the evicted entry.
        assert_eq!(
            g.check("k", "c", t(12), t(12)),
            Decision::Deliver(Freshness::Fresh)
        );
        assert!(g.horizon() <= t(12));
        // A subsequent in-window message must still be delivered Fresh: freshness has not been
        // permanently disabled.
        assert_eq!(
            g.check("k", "d", t(13), t(13)),
            Decision::Deliver(Freshness::Fresh)
        );
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
    fn a_negative_past_or_ahead_is_refused() {
        let negative_past = ReplayConfig {
            past: Duration::seconds(-1),
            ahead: Duration::seconds(60),
            retention: Duration::seconds(3600),
            capacity: 10,
        };
        assert_eq!(
            negative_past.validate(),
            Err("the replay window must not be negative")
        );
        let negative_ahead = ReplayConfig {
            past: Duration::seconds(60),
            ahead: Duration::seconds(-1),
            retention: Duration::seconds(3600),
            capacity: 10,
        };
        assert_eq!(
            negative_ahead.validate(),
            Err("the replay window must not be negative")
        );
        // Control: zero is not negative and is accepted (subject to the retention check).
        let zero = ReplayConfig {
            past: Duration::zero(),
            ahead: Duration::zero(),
            retention: Duration::zero(),
            capacity: 10,
        };
        assert!(zero.validate().is_ok());
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

    use crate::sender_auth::{ContradictedReason, SenderVerdict, UnverifiableReason};

    fn verified() -> SenderVerdict {
        SenderVerdict::Verified {
            key_id: "k1".to_string(),
        }
    }

    fn unsigned() -> SenderVerdict {
        SenderVerdict::Unverifiable {
            reason: UnverifiableReason::Unsigned,
        }
    }

    fn bad_signature() -> SenderVerdict {
        SenderVerdict::Contradicted {
            reason: ContradictedReason::BadSignature,
        }
    }

    fn state(accept_unverified: bool) -> InboundState {
        InboundState::new(
            ReplayConfig::default(),
            GateConfig {
                accept_unverified,
                knock_capacity: 4,
            },
            t(0),
        )
    }

    #[test]
    fn a_verified_sender_is_admitted_with_its_key_id() {
        let mut s = state(false);
        assert_eq!(
            s.admit(&verified(), "alice@x", "k1", t(10)),
            Admission::Admit {
                key_id: "k1".to_string()
            }
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
        assert_eq!(
            s.admit(&unsigned(), "bob@x", "", t(10)),
            Admission::AdmitUnverified
        );
        assert_eq!(s.counters().dropped_unverifiable, 0);
    }

    #[test]
    fn contradicted_is_rejected_under_both_settings() {
        for accept in [false, true] {
            let mut s = state(accept);
            assert_eq!(
                s.admit(&bad_signature(), "mallory@x", "k9", t(10)),
                Admission::Reject
            );
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
    fn a_knock_with_an_oversized_claimed_id_is_truncated_and_bounded() {
        let mut s = state(false);
        let huge_id: String = std::iter::repeat('a').take(10_000).collect();
        let huge_key: String = std::iter::repeat('b').take(10_000).collect();
        s.admit(&unsigned(), &huge_id, &huge_key, t(10));
        let knocks = s.knocks();
        assert_eq!(knocks.len(), 1);
        assert_eq!(knocks[0].claimed_global_id.chars().count(), 256);
        assert_eq!(knocks[0].key_id.chars().count(), 256);
        // Control: a second, differently-huge pair that truncates to the SAME 256 chars is one knock.
        let mut still_huge_id = huge_id.clone();
        still_huge_id.push_str("more-tail-that-gets-cut-off");
        s.admit(&unsigned(), &still_huge_id, &huge_key, t(11));
        assert_eq!(
            s.knocks().len(),
            1,
            "the capacity bound must be over the truncated key"
        );
    }

    #[test]
    fn the_counters_follow_the_freshness_of_what_is_delivered() {
        let mut s = state(false);
        assert_eq!(
            s.check("k", "m1", t(10), t(10)),
            Decision::Deliver(Freshness::Fresh)
        );
        assert_eq!(s.check("k", "m1", t(10), t(11)), Decision::Drop);
        assert_eq!(
            s.check("k", "m2", t(10), t(400)),
            Decision::Deliver(Freshness::Stale)
        );
        let counters = s.counters();
        assert_eq!(counters.dropped_replay, 1);
        assert_eq!(counters.delivered_stale, 1);
        assert_eq!(counters.delivered_ahead, 0);
    }
}
