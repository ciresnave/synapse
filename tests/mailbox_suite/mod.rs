// SPDX-License-Identifier: MIT OR Apache-2.0
//! The mailbox semantics, generic over the store, so every `MailStore` (MemoryStore in M4 PR 1,
//! RedbStore in PR 2) is held to the same cases. Design:
//! `docs/superpowers/specs/2026-10-02-m4-mailbox-design.md`.
#![allow(dead_code)]

use chrono::{DateTime, Duration, TimeZone, Utc};
use synapse::certificate::RevocationLookup;
use synapse::keystore::Keystore;
use synapse::mailbox::{Acked, Enqueued, Envelope, MailConfig, MailError, MailStore, Mailbox};
use synapse::roles::{ClaimRequest, Superseded, sign_claim};

pub fn t0() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 2, 12, 0, 0).unwrap()
}

struct NoRevocations;
impl RevocationLookup for NoRevocations {
    fn is_revoked(&self, _issuer_key_id: &str, _serial: &[u8; 16]) -> bool {
        false
    }
}

/// A keystore with account `acct`, and helpers to claim its roles.
pub struct Ids {
    _dir: Option<tempfile::TempDir>,
    path: std::path::PathBuf,
    store: Keystore,
    nonce: std::cell::Cell<u8>,
}

impl Ids {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        Keystore::init_account(dir.path(), "acct").expect("init");
        let store = Keystore::open(dir.path()).expect("open");
        Ids {
            path: dir.path().to_path_buf(),
            _dir: Some(dir),
            store,
            nonce: std::cell::Cell::new(0),
        }
    }

    /// The same account, reopened from `path` (a crash test's child process). `nonce_base` keeps
    /// the child's claim nonces apart from the parent's.
    pub fn open_existing(path: &std::path::Path, nonce_base: u8) -> Self {
        Ids {
            _dir: None,
            path: path.to_path_buf(),
            store: Keystore::open(path).expect("reopen keystore"),
            nonce: std::cell::Cell::new(nonce_base),
        }
    }

    /// Where the keystore lives, for handing to a child process.
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    pub fn request(&self, role: &str, now: DateTime<Utc>) -> ClaimRequest {
        let n = self.nonce.get().wrapping_add(1);
        self.nonce.set(n);
        let identity = self.store.role(role, t0()).expect("role");
        sign_claim(&identity, [n; 16], now).expect("sign")
    }

    /// Claim `role` and return the mailbox's result unchanged (for failure-path tests).
    pub fn try_claim<S: MailStore>(
        &self,
        mailbox: &mut Mailbox<S>,
        role: &str,
        now: DateTime<Utc>,
    ) -> Result<synapse::roles::Grant, synapse::mailbox::MailError> {
        let req = self.request(role, now);
        let key_id = self.store.account().key_id;
        let key = self.store.account_public_key();
        mailbox.claim(
            &req,
            &move |kid: &str| (kid == key_id).then_some(key),
            &NoRevocations,
            now,
        )
    }

    /// Claim `role` (e.g. "lane") on `mailbox`; returns the granted epoch.
    pub fn claim<S: MailStore>(
        &self,
        mailbox: &mut Mailbox<S>,
        role: &str,
        now: DateTime<Utc>,
    ) -> u64 {
        let req = self.request(role, now);
        let key_id = self.store.account().key_id;
        let key = self.store.account_public_key();
        mailbox
            .claim(
                &req,
                &move |kid: &str| (kid == key_id).then_some(key),
                &NoRevocations,
                now,
            )
            .expect("claim")
            .epoch
    }
}

pub fn open<S: MailStore>(store: S) -> Mailbox<S> {
    Mailbox::open(store, MailConfig::default(), t0() - Duration::seconds(1)).expect("open")
}

pub fn env(id: &str, to: &str, body: &[u8]) -> Envelope {
    Envelope {
        message_id: id.to_string(),
        to: to.to_string(),
        from: "peer@acct".to_string(),
        body: body.to_vec(),
    }
}

fn ids_of(deliveries: &[synapse::mailbox::Delivery]) -> Vec<String> {
    deliveries
        .iter()
        .map(|d| d.envelope.message_id.clone())
        .collect()
}

const LANE: &str = "lane@acct";

pub fn enqueue_then_fetch_leases_and_hides<S: MailStore>(make: impl Fn() -> S) {
    let ids = Ids::new();
    let mut mb = open(make());
    let epoch = ids.claim(&mut mb, "lane", t0());
    assert_eq!(
        mb.enqueue(env("m1", LANE, b"hi"), t0()).unwrap(),
        Enqueued::Queued
    );
    let got = mb.fetch(LANE, epoch, 10, None, t0()).unwrap();
    assert_eq!(ids_of(&got), ["m1"]);
    assert_eq!(got[0].envelope.body, b"hi");
    assert_eq!(got[0].attempts, 1);
    assert_eq!(
        got[0].lease_until,
        t0() + Duration::seconds(60),
        "default lease is 60s"
    );
    let again = mb
        .fetch(LANE, epoch, 10, None, t0() + Duration::seconds(59))
        .unwrap();
    assert!(
        again.is_empty(),
        "a leased message is hidden until its lease expires"
    );
}

pub fn an_expired_lease_redelivers_in_order<S: MailStore>(make: impl Fn() -> S) {
    let ids = Ids::new();
    let mut mb = open(make());
    let epoch = ids.claim(&mut mb, "lane", t0());
    for id in ["m1", "m2", "m3"] {
        mb.enqueue(env(id, LANE, b"x"), t0()).unwrap();
    }
    assert_eq!(
        ids_of(&mb.fetch(LANE, epoch, 2, None, t0()).unwrap()),
        ["m1", "m2"]
    );
    let later = t0() + Duration::seconds(61);
    let redelivered = mb.fetch(LANE, epoch, 10, None, later).unwrap();
    assert_eq!(
        ids_of(&redelivered),
        ["m1", "m2", "m3"],
        "oldest first, redelivery keeps order"
    );
    assert_eq!(redelivered[0].attempts, 2);
    assert_eq!(redelivered[2].attempts, 1);
}

pub fn ack_removes_for_good_and_a_resend_is_a_duplicate<S: MailStore>(make: impl Fn() -> S) {
    let ids = Ids::new();
    let mut mb = open(make());
    let epoch = ids.claim(&mut mb, "lane", t0());
    mb.enqueue(env("m1", LANE, b"x"), t0()).unwrap();
    mb.fetch(LANE, epoch, 10, None, t0()).unwrap();
    assert_eq!(mb.ack(LANE, epoch, "m1", t0()).unwrap(), Acked::Removed);
    let much_later = t0() + Duration::hours(1);
    assert!(
        mb.fetch(LANE, epoch, 10, None, much_later)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        mb.enqueue(env("m1", LANE, b"x"), much_later).unwrap(),
        Enqueued::Duplicate,
        "a resend of an acked message is recognised"
    );
    assert!(
        mb.fetch(LANE, epoch, 10, None, much_later)
            .unwrap()
            .is_empty()
    );
}

pub fn a_duplicate_while_queued_is_not_queued_twice<S: MailStore>(make: impl Fn() -> S) {
    let ids = Ids::new();
    let mut mb = open(make());
    let epoch = ids.claim(&mut mb, "lane", t0());
    assert_eq!(
        mb.enqueue(env("m1", LANE, b"x"), t0()).unwrap(),
        Enqueued::Queued
    );
    assert_eq!(
        mb.enqueue(env("m1", LANE, b"x"), t0()).unwrap(),
        Enqueued::Duplicate
    );
    assert_eq!(
        ids_of(&mb.fetch(LANE, epoch, 10, None, t0()).unwrap()),
        ["m1"]
    );
}

pub fn a_superseded_epoch_cannot_fetch_or_ack<S: MailStore>(make: impl Fn() -> S) {
    let ids = Ids::new();
    let mut mb = open(make());
    let old = ids.claim(&mut mb, "lane", t0());
    mb.enqueue(env("m1", LANE, b"x"), t0()).unwrap();
    mb.fetch(LANE, old, 10, None, t0()).unwrap();
    let new = ids.claim(&mut mb, "lane", t0() + Duration::seconds(1));
    assert_eq!(new, old + 1);
    let current = Superseded { current: new };
    assert_eq!(
        mb.fetch(LANE, old, 10, None, t0() + Duration::seconds(2))
            .unwrap_err(),
        MailError::Superseded(current)
    );
    assert_eq!(
        mb.ack(LANE, old, "m1", t0() + Duration::seconds(2))
            .unwrap_err(),
        MailError::Superseded(current)
    );
}

pub fn a_takeover_frees_the_old_leases_at_once<S: MailStore>(make: impl Fn() -> S) {
    let ids = Ids::new();
    let mut mb = open(make());
    let old = ids.claim(&mut mb, "lane", t0());
    mb.enqueue(env("m1", LANE, b"x"), t0()).unwrap();
    mb.fetch(LANE, old, 10, None, t0()).unwrap();
    let soon = t0() + Duration::seconds(2); // well inside the old 60s lease
    let new = ids.claim(&mut mb, "lane", soon);
    let got = mb.fetch(LANE, new, 10, None, soon).unwrap();
    assert_eq!(
        ids_of(&got),
        ["m1"],
        "the new holder gets in-flight mail without waiting"
    );
    assert_eq!(mb.ack(LANE, new, "m1", soon).unwrap(), Acked::Removed);
}

pub fn only_the_lease_holder_acks<S: MailStore>(make: impl Fn() -> S) {
    let ids = Ids::new();
    let mut mb = open(make());
    let epoch = ids.claim(&mut mb, "lane", t0());
    mb.enqueue(env("m1", LANE, b"x"), t0()).unwrap();
    assert_eq!(
        mb.ack(LANE, epoch, "m1", t0()).unwrap_err(),
        MailError::NotYourLease,
        "never fetched"
    );
    assert_eq!(
        mb.ack(LANE, epoch, "nope", t0()).unwrap_err(),
        MailError::NotFound
    );
    mb.fetch(LANE, epoch, 10, None, t0()).unwrap();
    let after_expiry = t0() + Duration::seconds(90);
    assert_eq!(
        mb.ack(LANE, epoch, "m1", after_expiry).unwrap(),
        Acked::Removed,
        "the holder may ack after its lease ran out: it processed the message"
    );
}

pub fn ack_is_idempotent<S: MailStore>(make: impl Fn() -> S) {
    let ids = Ids::new();
    let mut mb = open(make());
    let epoch = ids.claim(&mut mb, "lane", t0());
    mb.enqueue(env("m1", LANE, b"x"), t0()).unwrap();
    mb.fetch(LANE, epoch, 10, None, t0()).unwrap();
    assert_eq!(mb.ack(LANE, epoch, "m1", t0()).unwrap(), Acked::Removed);
    assert_eq!(
        mb.ack(LANE, epoch, "m1", t0()).unwrap(),
        Acked::AlreadyAcked
    );
}

pub fn the_bound_refuses_by_count_and_by_bytes_and_keeps_the_queue<S: MailStore>(
    make: impl Fn() -> S,
) {
    let ids = Ids::new();
    let config = MailConfig {
        max_messages: 2,
        max_bytes: 10,
        ..MailConfig::default()
    };
    let mut mb = Mailbox::open(make(), config, t0() - Duration::seconds(1)).expect("open");
    let epoch = ids.claim(&mut mb, "lane", t0());
    mb.enqueue(env("m1", LANE, b"abcd"), t0()).unwrap();
    assert_eq!(
        mb.enqueue(env("big", LANE, b"1234567"), t0()).unwrap_err(),
        MailError::MailboxFull,
        "4 + 7 bytes crosses the 10-byte bound"
    );
    mb.enqueue(env("m2", LANE, b"ef"), t0()).unwrap();
    assert_eq!(
        mb.enqueue(env("m3", LANE, b"g"), t0()).unwrap_err(),
        MailError::MailboxFull,
        "a third message crosses the 2-message bound"
    );
    assert_eq!(
        ids_of(&mb.fetch(LANE, epoch, 10, None, t0()).unwrap()),
        ["m1", "m2"]
    );
    // Acked mail no longer counts against the bound.
    mb.ack(LANE, epoch, "m1", t0()).unwrap();
    assert_eq!(
        mb.enqueue(env("m3", LANE, b"g"), t0()).unwrap(),
        Enqueued::Queued
    );
}

pub fn sweep_deletes_old_acked_history_never_queued_mail<S: MailStore>(make: impl Fn() -> S) {
    let ids = Ids::new();
    let mut mb = open(make());
    let epoch = ids.claim(&mut mb, "lane", t0());
    mb.enqueue(env("acked", LANE, b"x"), t0()).unwrap();
    mb.enqueue(env("waiting", LANE, b"y"), t0()).unwrap();
    mb.fetch(LANE, epoch, 1, None, t0()).unwrap();
    mb.ack(LANE, epoch, "acked", t0()).unwrap();

    let eight_days = t0() + Duration::days(8);
    assert_eq!(
        mb.sweep(eight_days).unwrap(),
        1,
        "the 8-day-old acked record goes"
    );
    // Its dedupe record went with it, so a resend now queues again (by design, after retention).
    assert_eq!(
        mb.enqueue(env("acked", LANE, b"x"), eight_days).unwrap(),
        Enqueued::Queued
    );
    // The never-fetched message, older than retention, is still there.
    let got = mb.fetch(LANE, epoch, 10, None, eight_days).unwrap();
    assert_eq!(ids_of(&got), ["waiting", "acked"]);
}

pub fn a_lease_outside_the_range_is_refused<S: MailStore>(make: impl Fn() -> S) {
    let ids = Ids::new();
    let mut mb = open(make());
    let epoch = ids.claim(&mut mb, "lane", t0());
    for lease in [Duration::seconds(4), Duration::minutes(16)] {
        assert_eq!(
            mb.fetch(LANE, epoch, 1, Some(lease), t0()).unwrap_err(),
            MailError::LeaseOutOfRange
        );
    }
    mb.enqueue(env("m1", LANE, b"x"), t0()).unwrap();
    let got = mb
        .fetch(LANE, epoch, 1, Some(Duration::minutes(15)), t0())
        .unwrap();
    assert_eq!(got[0].lease_until, t0() + Duration::minutes(15));
}

pub fn mail_waits_for_a_role_that_has_not_claimed_yet<S: MailStore>(make: impl Fn() -> S) {
    let ids = Ids::new();
    let mut mb = open(make());
    mb.enqueue(env("early", LANE, b"x"), t0()).unwrap();
    let later = t0() + Duration::minutes(10);
    let epoch = ids.claim(&mut mb, "lane", later);
    assert_eq!(
        ids_of(&mb.fetch(LANE, epoch, 10, None, later).unwrap()),
        ["early"]
    );
}

pub fn claims_are_written_through_the_store<S: MailStore>(make: impl Fn() -> S) {
    let ids = Ids::new();
    let mut mb = open(make());
    let epoch = ids.claim(&mut mb, "lane", t0());
    let mut seen = None;
    mb.store()
        .write(&mut |txn| {
            seen = txn
                .roles()
                .map_err(MailError::Store)?
                .roles
                .get(LANE)
                .map(|r| r.epoch);
            Ok(())
        })
        .unwrap();
    assert_eq!(seen, Some(epoch));
}
