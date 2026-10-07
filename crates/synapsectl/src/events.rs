// SPDX-License-Identifier: MIT OR Apache-2.0
//! Security events from the command line (brute-force hardening P8, audit rows 3, 6 and 7).
//!
//! A refusal the CLI already makes (a listener that fails the health proof, a file others can
//! reach) is recorded before the command goes on or exits. Each event goes to stderr, and to
//! `<home>/cli-security-events.jsonl` when the home itself is owner-only: in a home others can
//! write, that name could be a planted link to one of the owner's files. The file is the CLI's
//! own, not synapsed's `security-events.jsonl`: synapsed holds that one open, and a rotation by
//! rename fails on Windows while another process holds the file. It is opened only when an event
//! occurs, so a clean command creates nothing. Alert delivery is still only a seam (board 131).

use std::path::Path;
use std::sync::Arc;

use synapse::keystore::check_owner_only;
use synapse::security_events::{SecurityEvent, SecurityEventKind, SecuritySink};
use synapse_security::{FanoutSink, FileSink, StderrSink};

/// The CLI's security event file in the home.
pub const EVENTS_FILE: &str = "cli-security-events.jsonl";
/// The file rotates past this size, as synapsed's does.
const EVENTS_MAX_BYTES: u64 = 4 * 1024 * 1024;

/// Record one event, built now, into stderr and, if `home` is owner-only, the event file.
pub fn record(
    home: Option<&Path>,
    kind: SecurityEventKind,
    surface: &str,
    subject: &str,
    source: Option<&str>,
    detail: &str,
) {
    let event = SecurityEvent::new(chrono::Utc::now(), kind, surface, subject, source, detail);
    let mut sinks: Vec<Arc<dyn SecuritySink>> = vec![Arc::new(StderrSink)];
    if let Some(home) = home.filter(|h| check_owner_only(h, "keystore home").is_ok()) {
        match FileSink::open(home.join(EVENTS_FILE), EVENTS_MAX_BYTES) {
            Ok(file) => sinks.push(Arc::new(file)),
            // No path: the home is the keystore's location, which output never names.
            Err(e) => eprintln!("security: could not open the security event file: {e}"),
        }
    }
    FanoutSink(sinks).record(&event);
}
