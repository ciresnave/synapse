// SPDX-License-Identifier: MIT OR Apache-2.0
//! The fetch-scan slice's measurement: `fetch(max=1)` past a leased half of a full role. A
//! measurement, not a check: run with
//! `cargo test --release --features mailbox-redb --test mailbox_fetch_cost -- --ignored --nocapture`.
//! Prints p50/p99 over 100 calls for roles of 1k, 5k and 10k messages with half of them leased
//! (1 KiB bodies). Numbers go in the PR that changed the scan, with the ref and machine; a number
//! without its ref says nothing about the present.
#![cfg(feature = "mailbox-redb")]

mod mailbox_suite;

use std::time::Instant;

use chrono::Duration;
use mailbox_suite as suite;
use synapse::mailbox::{MailConfig, Mailbox, RedbStore};

#[test]
#[ignore = "measurement; a minute or two"]
fn fetch_one_past_a_leased_half() {
    for total in [1_000usize, 5_000, 10_000] {
        let dir = tempfile::tempdir().unwrap();
        let store = RedbStore::open(&dir.path().join("mailbox.redb")).unwrap();
        let config = MailConfig::default();
        let mut mb = Mailbox::open(store, config, suite::t0() - Duration::seconds(1)).unwrap();
        let ids = suite::Ids::new();
        let body = vec![b'x'; 1024];
        let now = suite::t0();
        let addr = "r0@acct".to_string();
        let epoch = ids.claim(&mut mb, "r0", now);
        for i in 0..total {
            mb.enqueue(suite::env(&format!("r0-{i}"), &addr, &body), now)
                .unwrap();
        }
        mb.fetch(&addr, epoch, total / 2, None, now).unwrap();
        let mut samples = Vec::new();
        for _ in 0..100 {
            let t = Instant::now();
            mb.fetch(&addr, epoch, 1, None, now).unwrap();
            samples.push(t.elapsed());
        }
        samples.sort();
        println!(
            "{total} messages, {} leased at start: fetch(max=1) p50={:?} p99={:?}",
            total / 2,
            samples[49],
            samples[98]
        );
    }
}
