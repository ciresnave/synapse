// SPDX-License-Identifier: MIT OR Apache-2.0
//! The mailbox suite (M4) run against `MemoryStore`, one named test per case.

mod mailbox_suite;

use mailbox_suite as suite;
use synapse::mailbox::{MailError, MailStore, MemoryStore, Stored};

macro_rules! cases {
    ($($name:ident),* $(,)?) => {
        $( #[test] fn $name() { suite::$name(MemoryStore::new); } )*
    };
}

cases!(
    enqueue_then_fetch_leases_and_hides,
    an_expired_lease_redelivers_in_order,
    ack_removes_for_good_and_a_resend_is_a_duplicate,
    a_duplicate_while_queued_is_not_queued_twice,
    a_superseded_epoch_cannot_fetch_or_ack,
    a_takeover_frees_the_old_leases_at_once,
    only_the_lease_holder_acks,
    ack_is_idempotent,
    the_bound_refuses_by_count_and_by_bytes_and_keeps_the_queue,
    sweep_deletes_old_acked_history_never_queued_mail,
    a_lease_outside_the_range_is_refused,
    mail_waits_for_a_role_that_has_not_claimed_yet,
    claims_are_written_through_the_store,
);

/// MemoryStore's transaction contract: a closure that changes state and then fails leaves the
/// prior state (review focus 1).
#[test]
fn a_failed_write_leaves_the_prior_state() {
    let store = MemoryStore::new();
    let stored = Stored {
        envelope: suite::env("m1", "lane@acct", b"x"),
        seq: 1,
        enqueued_at: suite::t0(),
        lease: None,
        attempts: 0,
    };
    let failed = store.write(&mut |txn| {
        txn.put("lane@acct", stored.clone()).map_err(MailError::Store)?;
        Err(MailError::MailboxFull)
    });
    assert_eq!(failed, Err(MailError::MailboxFull));
    let mut queue_len = None;
    store
        .write(&mut |txn| {
            queue_len = Some(txn.queue("lane@acct").map_err(MailError::Store)?.len());
            Ok(())
        })
        .unwrap();
    assert_eq!(queue_len, Some(0), "the failed transaction's put must not survive");
}
