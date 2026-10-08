// SPDX-License-Identifier: MIT OR Apache-2.0
//! S-1 follow-up (b): how long `Mailbox::depths` holds the mailbox, with roles at `max_messages`.
//! A measurement, not a check: run with
//! `cargo test --release --features mailbox-redb --test mailbox_depth_cost -- --ignored --nocapture`.
//!
//! Numbers taken at `e1468f7` on one Windows 11 laptop (this laptop, not a benchmark of any other
//! machine): 1 KiB bodies, each role holding 10k messages with half of them leased, 100 samples each.
//! - `depths` hold, p50/p99 by number of full roles: 1 role 19.8/29.6 ms; 2 roles 36.4/47.4 ms;
//!   3 roles 50.3/60.9 ms; 4 roles 64.7/78.4 ms.
//! - enqueue: p50 2.8 ms, p99 3.7 ms.
//! - `fetch(max=1)` past the leased half: p50 46 ms, p99 63 ms.
//!
//! Re-take them with the command above before relying on them at another ref or on another machine.
#![cfg(feature = "mailbox-redb")]

mod mailbox_suite;

use std::time::{Duration as StdDuration, Instant};

use chrono::Duration;
use mailbox_suite as suite;
use synapse::mailbox::{MailConfig, Mailbox, RedbStore};

fn stats(label: &str, mut samples: Vec<StdDuration>) {
    samples.sort();
    let at = |q: f64| samples[((samples.len() - 1) as f64 * q).round() as usize];
    println!(
        "{label}: n={} p50={:?} p99={:?} max={:?}",
        samples.len(),
        at(0.50),
        at(0.99),
        samples[samples.len() - 1]
    );
}

#[test]
#[ignore = "measurement; minutes long"]
fn depth_scan_hold_time_at_max_messages() {
    let dir = tempfile::tempdir().unwrap();
    let store = RedbStore::open(&dir.path().join("mailbox.redb")).unwrap();
    let config = MailConfig::default();
    let per_role = config.max_messages;
    let mut mb = Mailbox::open(store, config, suite::t0() - Duration::seconds(1)).unwrap();
    let ids = suite::Ids::new();
    let body = vec![b'x'; 1024];
    let now = suite::t0();
    let mut epochs = Vec::new();

    for (n, role) in ["r0", "r1", "r2", "r3"].iter().enumerate() {
        let addr = format!("{role}@acct");
        epochs.push((addr.clone(), ids.claim(&mut mb, role, now)));
        let fill = Instant::now();
        let mut enq = Vec::with_capacity(per_role);
        for i in 0..per_role {
            let t = Instant::now();
            mb.enqueue(suite::env(&format!("{role}-{i}"), &addr, &body), now)
                .unwrap();
            enq.push(t.elapsed());
        }
        println!(
            "--- {} full role(s) ({per_role} each; fill took {:?})",
            n + 1,
            fill.elapsed()
        );
        stats("  enqueue while filling", enq);
        // Lease half of this role's mail so the scan sees both kinds.
        let (addr, epoch) = epochs.last().unwrap();
        mb.fetch(addr, *epoch, per_role / 2, None, now).unwrap();

        let mut scans = Vec::new();
        for _ in 0..100 {
            let t = Instant::now();
            let depths = mb.depths(now).unwrap();
            scans.push(t.elapsed());
            assert_eq!(depths.len(), n + 1);
            assert_eq!(
                depths[addr.as_str()].queued + depths[addr.as_str()].leased,
                per_role
            );
        }
        stats("  depths()   [the hold]", scans);

        // The operations it would stall, for scale: a fetch of 1 and an enqueue to a role with room.
        let mut fetches = Vec::new();
        for _ in 0..100 {
            let t = Instant::now();
            mb.fetch(addr, *epoch, 1, None, now).unwrap();
            fetches.push(t.elapsed());
        }
        stats("  fetch(max=1), past the leased half", fetches);
        let mut small = Vec::new();
        for i in 0..100 {
            let t = Instant::now();
            mb.enqueue(suite::env(&format!("s{n}-{i}"), "spare@acct", &body), now)
                .unwrap();
            small.push(t.elapsed());
        }
        stats("  enqueue (room)", small);
    }
}
