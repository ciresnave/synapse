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

/// One message, as the mailbox carries it. `body` is opaque, and `Debug` shows only its length:
/// bodies are signed message content and can be large (final review M1).
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Envelope {
    pub message_id: String,
    /// The recipient role, `role@account`.
    pub to: String,
    pub from: String,
    pub body: Vec<u8>,
}

impl fmt::Debug for Envelope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Envelope")
            .field("message_id", &self.message_id)
            .field("to", &self.to)
            .field("from", &self.from)
            .field("body_len", &self.body.len())
            .finish()
    }
}

/// The longest `message_id` or `from` the mailbox accepts, in bytes.
pub const MAX_ADDRESS_LEN: usize = 256;

/// Structural checks only (final review I3): the mailbox keys storage on these strings, so it
/// refuses ones that cannot be sane keys. Whether a sender may write to a role, and whether the
/// role exists, is the caller's (M5's) decision.
fn well_formed(env: &Envelope) -> bool {
    let printable =
        |s: &str| !s.is_empty() && s.len() <= MAX_ADDRESS_LEN && !s.chars().any(char::is_control);
    let role_id = env.to.split_once('@').is_some_and(|(role, account)| {
        crate::keystore::valid_name(role) && crate::keystore::valid_name(account)
    });
    role_id && printable(&env.message_id) && printable(&env.from)
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
    /// The envelope's `to`, `message_id` or `from` is not well formed or is too long.
    Malformed,
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
            MailError::Malformed => write!(f, "the envelope's addressing is malformed"),
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
    /// How many messages the role has queued, and their total body bytes.
    fn stats(&self, role: &str) -> Result<(usize, u64), StoreError>;
    /// Visit the role's queue in ascending `seq`, stopping when `visit` returns `false`. Lets a
    /// store stop early instead of loading a whole queue (final review I2).
    fn scan(&self, role: &str, visit: &mut dyn FnMut(&Stored) -> bool) -> Result<(), StoreError>;
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
    fn stats(&self, role: &str) -> Result<(usize, u64), StoreError> {
        Ok(self.queues.get(role).map_or((0, 0), |q| {
            (
                q.len(),
                q.values().map(|s| s.envelope.body.len() as u64).sum(),
            )
        }))
    }

    fn scan(&self, role: &str, visit: &mut dyn FnMut(&Stored) -> bool) -> Result<(), StoreError> {
        let Some(queue) = self.queues.get(role) else {
            return Ok(());
        };
        let mut ordered: Vec<&Stored> = queue.values().collect();
        ordered.sort_by_key(|s| s.seq);
        for stored in ordered {
            if !visit(stored) {
                break;
            }
        }
        Ok(())
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
        let roles =
            Roles::restore(state.unwrap_or_default(), now).map_err(MailError::InvalidState)?;
        Ok(Mailbox {
            store,
            config,
            roles,
            poisoned: false,
        })
    }

    /// `Ok` only for the role's current epoch; the daemon checks every session with this (M5).
    pub fn check(&self, role: &str, epoch: u64) -> Result<(), MailError> {
        self.live()?;
        self.roles.check(role, epoch).map_err(MailError::Superseded)
    }

    /// Every claimed role and its current epoch (the daemon's `list`, M5).
    #[must_use]
    pub fn roles_snapshot(&self) -> RolesState {
        self.roles.snapshot()
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
    ///
    /// **Caller's contract (M5):** the sender is authenticated and allowed to write to `env.to`.
    /// The mailbox checks only that the addressing is well formed ([`MailError::Malformed`]) and
    /// bounds each role's queue; it cannot know which roles exist, so the number of roles is the
    /// caller's to bound. A resend of an acked message is a [`Enqueued::Duplicate`] for as long as
    /// acked history is retained, and is delivered again after [`Mailbox::sweep`] removes it.
    pub fn enqueue(&mut self, env: Envelope, now: DateTime<Utc>) -> Result<Enqueued, MailError> {
        self.live()?;
        if !well_formed(&env) {
            return Err(MailError::Malformed);
        }
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
            let (count, bytes) = txn.stats(role).map_err(MailError::Store)?;
            if count + 1 > config.max_messages
                || bytes.saturating_add(env.body.len() as u64) > config.max_bytes
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
        let written = self
            .store
            .write(&mut |txn| txn.set_roles(&snapshot).map_err(MailError::Store));
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
        self.roles
            .check(role, epoch)
            .map_err(MailError::Superseded)?;
        let lease = lease.unwrap_or(self.config.default_lease);
        if lease < self.config.min_lease || lease > self.config.max_lease {
            return Err(MailError::LeaseOutOfRange);
        }
        let until = now + lease;
        let mut out = Vec::new();
        if max == 0 {
            return Ok(out);
        }
        self.store.write(&mut |txn| {
            out.clear();
            // Collect up to `max` free messages in order, stopping the scan early, then lease them.
            let mut picked: Vec<Stored> = Vec::new();
            txn.scan(role, &mut |stored| {
                let free = match &stored.lease {
                    None => true,
                    Some(held) => held.until <= now || held.epoch != epoch,
                };
                if free {
                    picked.push(stored.clone());
                }
                picked.len() < max
            })
            .map_err(MailError::Store)?;
            for mut stored in picked {
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
        self.roles
            .check(role, epoch)
            .map_err(MailError::Superseded)?;
        let mut outcome = Acked::Removed;
        self.store.write(&mut |txn| {
            if txn
                .acked_at(role, message_id)
                .map_err(MailError::Store)?
                .is_some()
            {
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
            txn.record_ack(role, message_id, now)
                .map_err(MailError::Store)?;
            outcome = Acked::Removed;
            Ok(())
        })?;
        Ok(outcome)
    }

    /// Delete acked history older than the retention period. Queued mail is never touched.
    ///
    /// Acked history is outside the per-role queue bound and grows until swept, so the daemon
    /// (M5) must call this periodically (final review I4).
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

/// A durable [`MailStore`] on redb (M4 PR 2; D1, CireSnave: *"Redb is fine for now."*).
///
/// Every [`MailStore::write`] is exactly one redb write transaction, committed only when the
/// closure returns `Ok` and aborted otherwise; a process that dies inside it leaves the previous
/// committed state. Bodies live apart from lease metadata, so leasing a message never rewrites
/// its body. The file is created owner-only and checked on every open (M2's permission check).
#[cfg(feature = "mailbox-redb")]
pub struct RedbStore {
    db: redb::Database,
    body_writes: std::sync::atomic::AtomicU64,
}

#[cfg(feature = "mailbox-redb")]
use redb::ReadableTable;

#[cfg(feature = "mailbox-redb")]
mod redb_tables {
    use redb::TableDefinition;
    /// `(role, seq)` -> serde_json `Meta`.
    pub const META: TableDefinition<(&str, u64), &[u8]> = TableDefinition::new("meta");
    /// `(role, seq)` -> raw body bytes, written once.
    pub const BODIES: TableDefinition<(&str, u64), &[u8]> = TableDefinition::new("bodies");
    /// `(role, message_id)` -> `seq`.
    pub const BY_ID: TableDefinition<(&str, &str), u64> = TableDefinition::new("by_id");
    /// `(role, message_id)` -> acked-at, unix micros.
    pub const ACKED: TableDefinition<(&str, &str), i64> = TableDefinition::new("acked");
    /// `(acked-at micros, role, message_id)` -> nothing: the sweep's time index.
    pub const ACKED_BY_TIME: TableDefinition<(i64, &str, &str), ()> =
        TableDefinition::new("acked_by_time");
    /// `role` -> `(queued count, queued body bytes)`.
    pub const STATS: TableDefinition<&str, (u64, u64)> = TableDefinition::new("stats");
    /// `"seq"` -> the last sequence number issued.
    pub const COUNTERS: TableDefinition<&str, u64> = TableDefinition::new("counters");
    /// `"state"` -> serde_json `RolesState`.
    pub const ROLES: TableDefinition<&str, &[u8]> = TableDefinition::new("roles");
}

#[cfg(feature = "mailbox-redb")]
#[derive(Serialize, Deserialize)]
struct Meta {
    message_id: String,
    from: String,
    enqueued_at: DateTime<Utc>,
    lease: Option<Lease>,
    attempts: u32,
    body_len: u64,
}

#[cfg(feature = "mailbox-redb")]
fn store_err(what: &str) -> impl Fn(&dyn fmt::Display) -> StoreError + '_ {
    move |e| StoreError(format!("{what}: {e}"))
}

#[cfg(feature = "mailbox-redb")]
fn micros(at: DateTime<Utc>) -> i64 {
    at.timestamp_micros()
}

#[cfg(feature = "mailbox-redb")]
impl RedbStore {
    /// Create or open the store at `path`. A new file is made owner-only (Unix `0600`; on Windows
    /// it inherits the user profile's ACL); every open refuses a file broader than its owner.
    pub fn open(path: &std::path::Path) -> Result<Self, StoreError> {
        let existed = path.exists();
        let db = redb::Database::create(path).map_err(|e| store_err("open")(&e))?;
        #[cfg(unix)]
        if !existed {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
                .map_err(|e| store_err("set permissions")(&e))?;
        }
        #[cfg(not(unix))]
        let _ = existed;
        crate::keystore::check_owner_only(path, "mail store")
            .map_err(|e| store_err("permissions")(&e))?;
        // Create every table once, so later read paths can assume they exist.
        let txn = db.begin_write().map_err(|e| store_err("begin")(&e))?;
        {
            use redb_tables::*;
            txn.open_table(META).map_err(|e| store_err("table")(&e))?;
            txn.open_table(BODIES).map_err(|e| store_err("table")(&e))?;
            txn.open_table(BY_ID).map_err(|e| store_err("table")(&e))?;
            txn.open_table(ACKED).map_err(|e| store_err("table")(&e))?;
            txn.open_table(ACKED_BY_TIME)
                .map_err(|e| store_err("table")(&e))?;
            txn.open_table(STATS).map_err(|e| store_err("table")(&e))?;
            txn.open_table(COUNTERS)
                .map_err(|e| store_err("table")(&e))?;
            txn.open_table(ROLES).map_err(|e| store_err("table")(&e))?;
        }
        txn.commit().map_err(|e| store_err("commit")(&e))?;
        Ok(RedbStore {
            db,
            body_writes: std::sync::atomic::AtomicU64::new(0),
        })
    }

    /// How many body rows this handle has written (a diagnostic: lease updates must not add to it).
    #[must_use]
    pub fn body_writes(&self) -> u64 {
        self.body_writes.load(std::sync::atomic::Ordering::Relaxed)
    }
}

#[cfg(feature = "mailbox-redb")]
impl MailStore for RedbStore {
    fn write(
        &self,
        f: &mut dyn FnMut(&mut dyn MailTxn) -> Result<(), MailError>,
    ) -> Result<(), MailError> {
        let txn = self
            .db
            .begin_write()
            .map_err(|e| MailError::Store(store_err("begin")(&e)))?;
        let outcome = {
            let mut view = RedbTxn {
                txn: &txn,
                body_writes: &self.body_writes,
            };
            f(&mut view)
        };
        match outcome {
            Ok(()) => txn
                .commit()
                .map_err(|e| MailError::Store(store_err("commit")(&e))),
            Err(e) => {
                txn.abort()
                    .map_err(|ae| MailError::Store(store_err("abort")(&ae)))?;
                Err(e)
            }
        }
    }
}

#[cfg(feature = "mailbox-redb")]
struct RedbTxn<'a> {
    txn: &'a redb::WriteTransaction,
    body_writes: &'a std::sync::atomic::AtomicU64,
}

#[cfg(feature = "mailbox-redb")]
impl RedbTxn<'_> {
    fn seq_of(&self, role: &str, id: &str) -> Result<Option<u64>, StoreError> {
        let t = self
            .txn
            .open_table(redb_tables::BY_ID)
            .map_err(|e| store_err("by_id")(&e))?;
        Ok(t.get((role, id))
            .map_err(|e| store_err("by_id")(&e))?
            .map(|g| g.value()))
    }

    fn meta(&self, role: &str, seq: u64) -> Result<Option<Meta>, StoreError> {
        let t = self
            .txn
            .open_table(redb_tables::META)
            .map_err(|e| store_err("meta")(&e))?;
        let Some(raw) = t.get((role, seq)).map_err(|e| store_err("meta")(&e))? else {
            return Ok(None);
        };
        serde_json::from_slice(raw.value())
            .map(Some)
            .map_err(|e| store_err("meta decode")(&e))
    }

    fn body(&self, role: &str, seq: u64) -> Result<Vec<u8>, StoreError> {
        let t = self
            .txn
            .open_table(redb_tables::BODIES)
            .map_err(|e| store_err("bodies")(&e))?;
        // A META row without its body is corruption: serving an empty body would deliver (and let
        // a consumer ack) a message whose signed content is gone (final review I4).
        t.get((role, seq))
            .map_err(|e| store_err("bodies")(&e))?
            .map(|g| g.value().to_vec())
            .ok_or_else(|| StoreError("a queued message's body is missing".into()))
    }

    /// The META row for an id the BY_ID index names; a missing one is corruption (review I4).
    fn indexed_meta(&self, role: &str, seq: u64) -> Result<Meta, StoreError> {
        self.meta(role, seq)?
            .ok_or_else(|| StoreError("a queued message's metadata is missing".into()))
    }

    fn assemble(&self, role: &str, seq: u64, meta: Meta) -> Result<Stored, StoreError> {
        Ok(Stored {
            envelope: Envelope {
                message_id: meta.message_id,
                to: role.to_string(),
                from: meta.from,
                body: self.body(role, seq)?,
            },
            seq,
            enqueued_at: meta.enqueued_at,
            lease: meta.lease,
            attempts: meta.attempts,
        })
    }

    fn adjust_stats(&self, role: &str, count: i64, bytes: i64) -> Result<(), StoreError> {
        let mut t = self
            .txn
            .open_table(redb_tables::STATS)
            .map_err(|e| store_err("stats")(&e))?;
        let (c, b) = t
            .get(role)
            .map_err(|e| store_err("stats")(&e))?
            .map(|g| g.value())
            .unwrap_or((0, 0));
        // Underflow means the accounting is wrong; fail the transaction instead of clamping, which
        // would let a queue silently exceed its bound (final review I3).
        let underflow = || StoreError("queue statistics underflow".into());
        let next = (
            c.checked_add_signed(count).ok_or_else(underflow)?,
            b.checked_add_signed(bytes).ok_or_else(underflow)?,
        );
        if next == (0, 0) {
            t.remove(role).map_err(|e| store_err("stats")(&e))?;
        } else {
            t.insert(role, next).map_err(|e| store_err("stats")(&e))?;
        }
        Ok(())
    }
}

#[cfg(feature = "mailbox-redb")]
impl MailTxn for RedbTxn<'_> {
    fn stats(&self, role: &str) -> Result<(usize, u64), StoreError> {
        let t = self
            .txn
            .open_table(redb_tables::STATS)
            .map_err(|e| store_err("stats")(&e))?;
        let (c, b) = t
            .get(role)
            .map_err(|e| store_err("stats")(&e))?
            .map(|g| g.value())
            .unwrap_or((0, 0));
        Ok((usize::try_from(c).unwrap_or(usize::MAX), b))
    }

    fn scan(&self, role: &str, visit: &mut dyn FnMut(&Stored) -> bool) -> Result<(), StoreError> {
        let rows: Vec<(u64, Vec<u8>)> = {
            let t = self
                .txn
                .open_table(redb_tables::META)
                .map_err(|e| store_err("meta")(&e))?;
            let range = t
                .range((role, 0u64)..=(role, u64::MAX))
                .map_err(|e| store_err("meta")(&e))?;
            let mut rows = Vec::new();
            for entry in range {
                let (k, v) = entry.map_err(|e| store_err("meta")(&e))?;
                rows.push((k.value().1, v.value().to_vec()));
            }
            rows
        };
        for (seq, raw) in rows {
            let meta: Meta =
                serde_json::from_slice(&raw).map_err(|e| store_err("meta decode")(&e))?;
            let stored = self.assemble(role, seq, meta)?;
            if !visit(&stored) {
                break;
            }
        }
        Ok(())
    }

    fn queued(&self, role: &str, id: &str) -> Result<Option<Stored>, StoreError> {
        let Some(seq) = self.seq_of(role, id)? else {
            return Ok(None);
        };
        let meta = self.indexed_meta(role, seq)?;
        self.assemble(role, seq, meta).map(Some)
    }

    fn put(&mut self, role: &str, stored: Stored) -> Result<(), StoreError> {
        let id = stored.envelope.message_id.clone();
        let existing = self.seq_of(role, &id)?;
        let seq = existing.unwrap_or(stored.seq);
        // A body is written once. An update (fetch re-leasing a message) keeps the stored body, so
        // it must keep the stored length too, or STATS and the byte bound drift (final review I2).
        let body_len = match existing {
            Some(seq) => self.indexed_meta(role, seq)?.body_len,
            None => stored.envelope.body.len() as u64,
        };
        let meta = Meta {
            message_id: id.clone(),
            from: stored.envelope.from.clone(),
            enqueued_at: stored.enqueued_at,
            lease: stored.lease.clone(),
            attempts: stored.attempts,
            body_len,
        };
        let raw = serde_json::to_vec(&meta).map_err(|e| store_err("meta encode")(&e))?;
        {
            let mut t = self
                .txn
                .open_table(redb_tables::META)
                .map_err(|e| store_err("meta")(&e))?;
            t.insert((role, seq), raw.as_slice())
                .map_err(|e| store_err("meta")(&e))?;
        }
        if existing.is_none() {
            {
                let mut t = self
                    .txn
                    .open_table(redb_tables::BODIES)
                    .map_err(|e| store_err("bodies")(&e))?;
                t.insert((role, seq), stored.envelope.body.as_slice())
                    .map_err(|e| store_err("bodies")(&e))?;
            }
            self.body_writes
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            {
                let mut t = self
                    .txn
                    .open_table(redb_tables::BY_ID)
                    .map_err(|e| store_err("by_id")(&e))?;
                t.insert((role, id.as_str()), seq)
                    .map_err(|e| store_err("by_id")(&e))?;
            }
            self.adjust_stats(role, 1, i64::try_from(body_len).unwrap_or(i64::MAX))?;
        }
        Ok(())
    }

    fn remove(&mut self, role: &str, id: &str) -> Result<(), StoreError> {
        let Some(seq) = self.seq_of(role, id)? else {
            return Ok(());
        };
        let body_len = self.indexed_meta(role, seq)?.body_len;
        {
            let mut t = self
                .txn
                .open_table(redb_tables::META)
                .map_err(|e| store_err("meta")(&e))?;
            t.remove((role, seq)).map_err(|e| store_err("meta")(&e))?;
        }
        {
            let mut t = self
                .txn
                .open_table(redb_tables::BODIES)
                .map_err(|e| store_err("bodies")(&e))?;
            t.remove((role, seq)).map_err(|e| store_err("bodies")(&e))?;
        }
        {
            let mut t = self
                .txn
                .open_table(redb_tables::BY_ID)
                .map_err(|e| store_err("by_id")(&e))?;
            t.remove((role, id)).map_err(|e| store_err("by_id")(&e))?;
        }
        self.adjust_stats(role, -1, -i64::try_from(body_len).unwrap_or(i64::MAX))
    }

    fn acked_at(&self, role: &str, id: &str) -> Result<Option<DateTime<Utc>>, StoreError> {
        let t = self
            .txn
            .open_table(redb_tables::ACKED)
            .map_err(|e| store_err("acked")(&e))?;
        Ok(t.get((role, id))
            .map_err(|e| store_err("acked")(&e))?
            .and_then(|g| DateTime::from_timestamp_micros(g.value())))
    }

    fn record_ack(&mut self, role: &str, id: &str, at: DateTime<Utc>) -> Result<(), StoreError> {
        {
            let mut t = self
                .txn
                .open_table(redb_tables::ACKED)
                .map_err(|e| store_err("acked")(&e))?;
            t.insert((role, id), micros(at))
                .map_err(|e| store_err("acked")(&e))?;
        }
        let mut t = self
            .txn
            .open_table(redb_tables::ACKED_BY_TIME)
            .map_err(|e| store_err("acked_by_time")(&e))?;
        t.insert((micros(at), role, id), ())
            .map_err(|e| store_err("acked_by_time")(&e))?;
        Ok(())
    }

    fn sweep_acked(&mut self, before: DateTime<Utc>) -> Result<usize, StoreError> {
        let doomed: Vec<(i64, String, String)> = {
            let t = self
                .txn
                .open_table(redb_tables::ACKED_BY_TIME)
                .map_err(|e| store_err("acked_by_time")(&e))?;
            let range = t
                .range(..(micros(before), "", ""))
                .map_err(|e| store_err("acked_by_time")(&e))?;
            let mut out = Vec::new();
            for entry in range {
                let (k, _) = entry.map_err(|e| store_err("acked_by_time")(&e))?;
                let (at, role, id) = k.value();
                out.push((at, role.to_string(), id.to_string()));
            }
            out
        };
        {
            let mut by_time = self
                .txn
                .open_table(redb_tables::ACKED_BY_TIME)
                .map_err(|e| store_err("acked_by_time")(&e))?;
            let mut acked = self
                .txn
                .open_table(redb_tables::ACKED)
                .map_err(|e| store_err("acked")(&e))?;
            for (at, role, id) in &doomed {
                by_time
                    .remove((*at, role.as_str(), id.as_str()))
                    .map_err(|e| store_err("acked_by_time")(&e))?;
                acked
                    .remove((role.as_str(), id.as_str()))
                    .map_err(|e| store_err("acked")(&e))?;
            }
        }
        Ok(doomed.len())
    }

    fn next_seq(&mut self) -> Result<u64, StoreError> {
        let mut t = self
            .txn
            .open_table(redb_tables::COUNTERS)
            .map_err(|e| store_err("counters")(&e))?;
        let next = t
            .get("seq")
            .map_err(|e| store_err("counters")(&e))?
            .map_or(0, |g| g.value())
            + 1;
        t.insert("seq", next)
            .map_err(|e| store_err("counters")(&e))?;
        Ok(next)
    }

    fn roles(&self) -> Result<RolesState, StoreError> {
        let t = self
            .txn
            .open_table(redb_tables::ROLES)
            .map_err(|e| store_err("roles")(&e))?;
        match t.get("state").map_err(|e| store_err("roles")(&e))? {
            Some(raw) => {
                serde_json::from_slice(raw.value()).map_err(|e| store_err("roles decode")(&e))
            }
            None => Ok(RolesState::default()),
        }
    }

    fn set_roles(&mut self, state: &RolesState) -> Result<(), StoreError> {
        let raw = serde_json::to_vec(state).map_err(|e| store_err("roles encode")(&e))?;
        let mut t = self
            .txn
            .open_table(redb_tables::ROLES)
            .map_err(|e| store_err("roles")(&e))?;
        t.insert("state", raw.as_slice())
            .map_err(|e| store_err("roles")(&e))?;
        Ok(())
    }
}
