// SPDX-License-Identifier: MIT OR Apache-2.0
//! The mailbox (M4): mail for a role waits for that role, across session restarts, daemon
//! restarts and crashes.
//!
//! Delivery is at-least-once. [`Mailbox::fetch`] **leases** messages; [`Mailbox::ack`] removes
//! one; a lease that expires makes its message deliverable again; consumers dedupe by
//! `message_id`. A role's newest claim (M3) voids every lease an older epoch held. Retention
//! ([`Mailbox::sweep`]) deletes **acked** history only, never undelivered mail.
//!
//! The mailbox never parses what it carries: an [`Envelope`]'s body is opaque bytes (the daemon
//! stores a signed message there). Every operation is one atomic write transaction on a
//! [`MailStore`], and a claim's new role state is written in the same call that grants it.
//! No clock: `now` is passed in. Design: `docs/superpowers/specs/2026-10-02-m4-mailbox-design.md`.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Mutex;

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::certificate::RevocationLookup;
use crate::roles::{ClaimError, ClaimRequest, Grant, InvalidState, Roles, RolesState, Superseded};

/// Bounds and timings (PM-approved defaults, spec §4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailConfig {
    pub default_lease: Duration,
    pub min_lease: Duration,
    pub max_lease: Duration,
    /// How long acked history (and with it, dedupe of resends) is kept.
    pub retention: Duration,
    /// Per role, counting queued (unacked) messages.
    pub max_messages: usize,
    /// Per role, counting queued envelope body bytes.
    pub max_bytes: u64,
}

impl Default for MailConfig {
    fn default() -> Self {
        MailConfig {
            default_lease: Duration::seconds(60),
            min_lease: Duration::seconds(5),
            max_lease: Duration::minutes(15),
            retention: Duration::days(7),
            max_messages: 10_000,
            max_bytes: 64 * 1024 * 1024,
        }
    }
}

/// One message, as the mailbox carries it. `body` is opaque.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Envelope {
    pub message_id: String,
    /// The recipient role, `role@account`.
    pub to: String,
    pub from: String,
    pub body: Vec<u8>,
}

/// Who holds a message, and until when.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lease {
    pub epoch: u64,
    pub until: DateTime<Utc>,
}

/// A queued message as a store keeps it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stored {
    pub envelope: Envelope,
    /// Enqueue order across the whole store; fetch returns ascending `seq`.
    pub seq: u64,
    pub enqueued_at: DateTime<Utc>,
    pub lease: Option<Lease>,
    /// How many times it has been leased.
    pub attempts: u32,
}

/// A leased message, as `fetch` hands it out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Delivery {
    pub envelope: Envelope,
    pub enqueued_at: DateTime<Utc>,
    pub lease_until: DateTime<Utc>,
    pub attempts: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Enqueued {
    Queued,
    /// `(to, message_id)` is already queued, or was acked within retention. Nothing changed.
    Duplicate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Acked {
    Removed,
    /// It was acked before; acking is idempotent.
    AlreadyAcked,
}

/// A store failure, described without message contents.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoreError(pub String);

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "mail store: {}", self.0)
    }
}

impl std::error::Error for StoreError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MailError {
    /// The caller's epoch is not the role's current one.
    Superseded(Superseded),
    /// The recipient's queue is at its message or byte bound; the sender is told, nothing dropped.
    MailboxFull,
    /// No queued message with that id.
    NotFound,
    /// The message is not leased to the caller's epoch.
    NotYourLease,
    /// A requested lease is outside `[min_lease, max_lease]`.
    LeaseOutOfRange,
    Claim(ClaimError),
    InvalidState(InvalidState),
    Store(StoreError),
    /// A store write failed after a claim was granted in memory. Reopen the mailbox.
    Poisoned,
}

impl fmt::Display for MailError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MailError::Superseded(s) => write!(f, "{s}"),
            MailError::MailboxFull => write!(f, "the recipient's mailbox is full"),
            MailError::NotFound => write!(f, "no such queued message"),
            MailError::NotYourLease => write!(f, "the message is not leased to this epoch"),
            MailError::LeaseOutOfRange => write!(f, "the requested lease is out of range"),
            MailError::Claim(e) => write!(f, "{e}"),
            MailError::InvalidState(e) => write!(f, "{e}"),
            MailError::Store(e) => write!(f, "{e}"),
            MailError::Poisoned => write!(f, "the mailbox must be reopened after a store failure"),
        }
    }
}

impl std::error::Error for MailError {}

/// One write transaction's view of the store.
pub trait MailTxn {
    /// The role's queue, ascending `seq`.
    fn queue(&self, role: &str) -> Result<Vec<Stored>, StoreError>;
    fn queued(&self, role: &str, id: &str) -> Result<Option<Stored>, StoreError>;
    /// Insert, or replace by `envelope.message_id`.
    fn put(&mut self, role: &str, stored: Stored) -> Result<(), StoreError>;
    fn remove(&mut self, role: &str, id: &str) -> Result<(), StoreError>;
    fn acked_at(&self, role: &str, id: &str) -> Result<Option<DateTime<Utc>>, StoreError>;
    fn record_ack(&mut self, role: &str, id: &str, at: DateTime<Utc>) -> Result<(), StoreError>;
    /// Delete acked history recorded before `before`; returns how many.
    fn sweep_acked(&mut self, before: DateTime<Utc>) -> Result<usize, StoreError>;
    fn next_seq(&mut self) -> Result<u64, StoreError>;
    fn roles(&self) -> Result<RolesState, StoreError>;
    fn set_roles(&mut self, state: &RolesState) -> Result<(), StoreError>;
}

/// A transactional store.
pub trait MailStore {
    /// One atomic write transaction: every change `f` makes commits together, or none does.
    fn write(
        &self,
        f: &mut dyn FnMut(&mut dyn MailTxn) -> Result<(), MailError>,
    ) -> Result<(), MailError>;
}

/// An in-memory store. `write` applies the closure to a copy and keeps it only on `Ok`.
#[derive(Default)]
pub struct MemoryStore {
    state: Mutex<MemState>,
}

#[derive(Clone, Default)]
struct MemState {
    queues: BTreeMap<String, BTreeMap<String, Stored>>,
    acked: BTreeMap<(String, String), DateTime<Utc>>,
    seq: u64,
    roles: RolesState,
}

impl MemoryStore {
    #[must_use]
    pub fn new() -> Self {
        MemoryStore::default()
    }
}

impl MailStore for MemoryStore {
    fn write(
        &self,
        f: &mut dyn FnMut(&mut dyn MailTxn) -> Result<(), MailError>,
    ) -> Result<(), MailError> {
        let mut guard = self
            .state
            .lock()
            .map_err(|_| MailError::Store(StoreError("memory store lock poisoned".into())))?;
        let mut copy = guard.clone();
        f(&mut copy)?;
        *guard = copy;
        Ok(())
    }
}

impl MailTxn for MemState {
    fn queue(&self, role: &str) -> Result<Vec<Stored>, StoreError> {
        let mut queue: Vec<Stored> = self
            .queues
            .get(role)
            .map(|q| q.values().cloned().collect())
            .unwrap_or_default();
        queue.sort_by_key(|s| s.seq);
        Ok(queue)
    }

    fn queued(&self, role: &str, id: &str) -> Result<Option<Stored>, StoreError> {
        Ok(self.queues.get(role).and_then(|q| q.get(id)).cloned())
    }

    fn put(&mut self, role: &str, stored: Stored) -> Result<(), StoreError> {
        self.queues
            .entry(role.to_string())
            .or_default()
            .insert(stored.envelope.message_id.clone(), stored);
        Ok(())
    }

    fn remove(&mut self, role: &str, id: &str) -> Result<(), StoreError> {
        if let Some(queue) = self.queues.get_mut(role) {
            queue.remove(id);
        }
        Ok(())
    }

    fn acked_at(&self, role: &str, id: &str) -> Result<Option<DateTime<Utc>>, StoreError> {
        Ok(self.acked.get(&(role.to_string(), id.to_string())).copied())
    }

    fn record_ack(&mut self, role: &str, id: &str, at: DateTime<Utc>) -> Result<(), StoreError> {
        self.acked.insert((role.to_string(), id.to_string()), at);
        Ok(())
    }

    fn sweep_acked(&mut self, before: DateTime<Utc>) -> Result<usize, StoreError> {
        let len = self.acked.len();
        self.acked.retain(|_, at| *at >= before);
        Ok(len - self.acked.len())
    }

    fn next_seq(&mut self) -> Result<u64, StoreError> {
        self.seq += 1;
        Ok(self.seq)
    }

    fn roles(&self) -> Result<RolesState, StoreError> {
        Ok(self.roles.clone())
    }

    fn set_roles(&mut self, state: &RolesState) -> Result<(), StoreError> {
        self.roles = state.clone();
        Ok(())
    }
}

/// The mailbox: role epochs (M3) in memory, everything else in the store.
pub struct Mailbox<S: MailStore> {
    store: S,
    config: MailConfig,
    roles: Roles,
    poisoned: bool,
}

impl<S: MailStore> Mailbox<S> {
    /// Open over `store`, restoring the role epochs it holds. Claims signed at or before `now`
    /// are refused after this (M3: a captured claim cannot be replayed across a restart).
    pub fn open(store: S, config: MailConfig, now: DateTime<Utc>) -> Result<Self, MailError> {
        let mut state = None;
        store.write(&mut |txn| {
            state = Some(txn.roles().map_err(MailError::Store)?);
            Ok(())
        })?;
        let roles = Roles::restore(state.unwrap_or_default(), now).map_err(MailError::InvalidState)?;
        Ok(Mailbox {
            store,
            config,
            roles,
            poisoned: false,
        })
    }

    /// The underlying store (tests and the daemon's diagnostics).
    pub fn store(&self) -> &S {
        &self.store
    }

    fn live(&self) -> Result<(), MailError> {
        if self.poisoned {
            Err(MailError::Poisoned)
        } else {
            Ok(())
        }
    }

    /// Queue `env` for its recipient. Mail may wait for a role that has never claimed.
    pub fn enqueue(&mut self, env: Envelope, now: DateTime<Utc>) -> Result<Enqueued, MailError> {
        self.live()?;
        let config = self.config.clone();
        let mut outcome = Enqueued::Queued;
        self.store.write(&mut |txn| {
            let role = env.to.as_str();
            let id = env.message_id.as_str();
            if txn.queued(role, id).map_err(MailError::Store)?.is_some()
                || txn.acked_at(role, id).map_err(MailError::Store)?.is_some()
            {
                outcome = Enqueued::Duplicate;
                return Ok(());
            }
            let queue = txn.queue(role).map_err(MailError::Store)?;
            let bytes: u64 = queue.iter().map(|s| s.envelope.body.len() as u64).sum();
            if queue.len() + 1 > config.max_messages
                || bytes + env.body.len() as u64 > config.max_bytes
            {
                return Err(MailError::MailboxFull);
            }
            let seq = txn.next_seq().map_err(MailError::Store)?;
            txn.put(
                role,
                Stored {
                    envelope: env.clone(),
                    seq,
                    enqueued_at: now,
                    lease: None,
                    attempts: 0,
                },
            )
            .map_err(MailError::Store)?;
            outcome = Enqueued::Queued;
            Ok(())
        })?;
        Ok(outcome)
    }

    /// Take a role (M3) and persist the new epoch in the same call. If the store write fails the
    /// mailbox is poisoned: the epoch was granted in memory but not recorded.
    pub fn claim(
        &mut self,
        req: &ClaimRequest,
        account_keys: &dyn Fn(&str) -> Option<[u8; 32]>,
        revoked: &dyn RevocationLookup,
        now: DateTime<Utc>,
    ) -> Result<Grant, MailError> {
        self.live()?;
        let grant = self
            .roles
            .claim(req, account_keys, revoked, now)
            .map_err(MailError::Claim)?;
        let snapshot = self.roles.snapshot();
        let written = self.store.write(&mut |txn| {
            txn.set_roles(&snapshot).map_err(MailError::Store)
        });
        if let Err(e) = written {
            self.poisoned = true;
            return Err(e);
        }
        Ok(grant)
    }

    /// Lease up to `max` deliverable messages, oldest first. A message is deliverable when it has
    /// no lease, its lease expired, or its lease belongs to an older epoch (a takeover voids it).
    pub fn fetch(
        &mut self,
        role: &str,
        epoch: u64,
        max: usize,
        lease: Option<Duration>,
        now: DateTime<Utc>,
    ) -> Result<Vec<Delivery>, MailError> {
        self.live()?;
        self.roles.check(role, epoch).map_err(MailError::Superseded)?;
        let lease = lease.unwrap_or(self.config.default_lease);
        if lease < self.config.min_lease || lease > self.config.max_lease {
            return Err(MailError::LeaseOutOfRange);
        }
        let until = now + lease;
        let mut out = Vec::new();
        self.store.write(&mut |txn| {
            out.clear();
            for mut stored in txn.queue(role).map_err(MailError::Store)? {
                if out.len() == max {
                    break;
                }
                let free = match &stored.lease {
                    None => true,
                    Some(held) => held.until <= now || held.epoch != epoch,
                };
                if !free {
                    continue;
                }
                stored.lease = Some(Lease { epoch, until });
                stored.attempts = stored.attempts.saturating_add(1);
                out.push(Delivery {
                    envelope: stored.envelope.clone(),
                    enqueued_at: stored.enqueued_at,
                    lease_until: until,
                    attempts: stored.attempts,
                });
                txn.put(role, stored).map_err(MailError::Store)?;
            }
            Ok(())
        })?;
        Ok(out)
    }

    /// Acknowledge a message this epoch holds the lease on (expired or not: the holder processed
    /// it). Idempotent.
    pub fn ack(
        &mut self,
        role: &str,
        epoch: u64,
        message_id: &str,
        now: DateTime<Utc>,
    ) -> Result<Acked, MailError> {
        self.live()?;
        self.roles.check(role, epoch).map_err(MailError::Superseded)?;
        let mut outcome = Acked::Removed;
        self.store.write(&mut |txn| {
            if txn.acked_at(role, message_id).map_err(MailError::Store)?.is_some() {
                outcome = Acked::AlreadyAcked;
                return Ok(());
            }
            let stored = txn
                .queued(role, message_id)
                .map_err(MailError::Store)?
                .ok_or(MailError::NotFound)?;
            match stored.lease {
                Some(held) if held.epoch == epoch => {}
                _ => return Err(MailError::NotYourLease),
            }
            txn.remove(role, message_id).map_err(MailError::Store)?;
            txn.record_ack(role, message_id, now).map_err(MailError::Store)?;
            outcome = Acked::Removed;
            Ok(())
        })?;
        Ok(outcome)
    }

    /// Delete acked history older than the retention period. Queued mail is never touched.
    pub fn sweep(&mut self, now: DateTime<Utc>) -> Result<usize, MailError> {
        self.live()?;
        let before = now - self.config.retention;
        let mut swept = 0;
        self.store.write(&mut |txn| {
            swept = txn.sweep_acked(before).map_err(MailError::Store)?;
            Ok(())
        })?;
        Ok(swept)
    }
}
