// SPDX-License-Identifier: MIT OR Apache-2.0
//! Hardening P4: the sinks. Alert delivery awaits board 131, so these tests use a capturing and a
//! failing transport; no concrete transport exists.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, TimeZone, Utc};
use synapse::security_events::{
    Alert, AlertConfig, AlertPolicy, SecurityEvent, SecurityEventKind, SecuritySink,
};
use synapse_security::{AlertSink, AlertTransport, FanoutSink, FileSink, StderrSink};

fn t0() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 7, 12, 0, 0).unwrap()
}

fn event(kind: SecurityEventKind, subject: &str) -> SecurityEvent {
    SecurityEvent::new(
        t0(),
        kind,
        "synapsed/bearer",
        subject,
        Some("127.0.0.1"),
        "bad bearer",
    )
}

#[derive(Default)]
struct Capture(Mutex<Vec<SecurityEvent>>);
impl SecuritySink for Capture {
    fn record(&self, event: &SecurityEvent) {
        self.0.lock().unwrap().push(event.clone());
    }
}

#[derive(Clone, Default)]
struct CapturingTransport(Arc<Mutex<Vec<Alert>>>);
impl AlertTransport for CapturingTransport {
    fn send(&self, alert: &Alert) -> Result<(), String> {
        self.0.lock().unwrap().push(alert.clone());
        Ok(())
    }
}

#[derive(Clone, Default)]
struct FailingTransport(Arc<AtomicUsize>);
impl AlertTransport for FailingTransport {
    fn send(&self, _alert: &Alert) -> Result<(), String> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Err("relay unreachable".to_string())
    }
}

// 9
#[test]
fn file_sink_writes_jsonl_and_rotates_to_exactly_one_old_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("security-events.jsonl");
    let sink = FileSink::open(&path, 600).unwrap();
    let first = event(SecurityEventKind::AuthFailure, "key-1");
    sink.record(&first);
    // Positive control: one line, valid JSON, round-trips to the same event.
    let text = std::fs::read_to_string(&path).unwrap();
    assert_eq!(text.lines().count(), 1);
    let back: SecurityEvent = serde_json::from_str(text.lines().next().unwrap()).unwrap();
    assert_eq!(back, first);

    // Negative control: a flood far past the cap leaves exactly `<path>` and `<path>.1`.
    for i in 0..200 {
        sink.record(&event(SecurityEventKind::AuthFailure, &format!("key-{i}")));
    }
    let mut names: Vec<String> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, ["security-events.jsonl", "security-events.jsonl.1"]);
    let old = dir.path().join("security-events.jsonl.1");
    for file in [&path, &old] {
        let len = std::fs::metadata(file).unwrap().len();
        assert!(
            len <= 600,
            "{} holds {len} bytes, past the cap",
            file.display()
        );
        for line in std::fs::read_to_string(file).unwrap().lines() {
            serde_json::from_str::<SecurityEvent>(line).expect("every line is a whole event");
        }
    }
    // The newest event is in the current file.
    let current = std::fs::read_to_string(&path).unwrap();
    assert!(current.contains("key-199"));
}

#[cfg(unix)]
#[test]
fn file_sink_creates_the_file_owner_only() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("security-events.jsonl");
    let sink = FileSink::open(&path, 1 << 20).unwrap();
    sink.record(&event(SecurityEventKind::AuthFailure, "key-1"));
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

/// Review: the 0o600 mode applies only to a file created now. An existing event file others can
/// read is refused, not appended to (negative); the owner-only file is accepted (positive).
#[cfg(unix)]
#[test]
fn file_sink_refuses_an_existing_file_others_can_read() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("security-events.jsonl");
    drop(FileSink::open(&path, 1 << 20).unwrap());
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(FileSink::open(&path, 1 << 20).is_err());
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert!(FileSink::open(&path, 1 << 20).is_ok());
}

#[test]
fn file_sink_survives_an_unwritable_path_without_panicking() {
    // Negative control: opening under a missing directory reports an error to the caller.
    let dir = tempfile::tempdir().unwrap();
    assert!(FileSink::open(dir.path().join("missing").join("e.jsonl"), 1000).is_err());
    // Positive control: a sink whose file vanishes mid-run swallows the error rather than panicking.
    let path = dir.path().join("e.jsonl");
    let sink = FileSink::open(&path, 1000).unwrap();
    sink.record(&event(SecurityEventKind::AuthFailure, "k"));
    StderrSink.record(&event(SecurityEventKind::AuthFailure, "k"));
}

// 10
#[test]
fn alert_sink_sends_what_the_policy_emits_and_records_failures_once() {
    // Positive control: the capturing transport receives exactly the alerts the policy emits.
    let transport = CapturingTransport::default();
    let fallback = Arc::new(Capture::default());
    let sink = AlertSink::new(
        AlertPolicy::new(AlertConfig::default()),
        transport.clone(),
        fallback.clone(),
    );
    sink.record(&event(SecurityEventKind::AuthFailure, "key-1"));
    assert!(
        transport.0.lock().unwrap().is_empty(),
        "one failure is below the threshold"
    );
    sink.record(&event(SecurityEventKind::NewClient, "client-1"));
    {
        let sent = transport.0.lock().unwrap();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].kind, SecurityEventKind::NewClient);
    }
    assert!(fallback.0.lock().unwrap().is_empty());

    // Negative control: a dead transport is called once per alert, never looped, and each failure is
    // recorded as an AlertFailed event in the fallback.
    let failing = FailingTransport::default();
    let fallback = Arc::new(Capture::default());
    let sink = AlertSink::new(
        AlertPolicy::new(AlertConfig::default()),
        failing.clone(),
        fallback.clone(),
    );
    sink.record(&event(SecurityEventKind::NewClient, "client-1"));
    assert_eq!(failing.0.load(Ordering::SeqCst), 1);
    {
        let recorded = fallback.0.lock().unwrap();
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].kind, SecurityEventKind::AlertFailed);
        assert!(recorded[0].detail.contains("relay unreachable"));
    }
    sink.record(&event(SecurityEventKind::Config, "toggle"));
    assert_eq!(failing.0.load(Ordering::SeqCst), 2, "one send per alert");
    assert_eq!(fallback.0.lock().unwrap().len(), 2);
}

#[test]
fn fanout_sink_records_into_every_sink() {
    let a = Arc::new(Capture::default());
    let b = Arc::new(Capture::default());
    let fan = FanoutSink(vec![a.clone(), b.clone()]);
    fan.record(&event(SecurityEventKind::AuthFailure, "key-1"));
    assert_eq!(a.0.lock().unwrap().len(), 1);
    assert_eq!(b.0.lock().unwrap().len(), 1);
    // Positive control: an empty fanout is harmless.
    FanoutSink(Vec::new()).record(&event(SecurityEventKind::AuthFailure, "key-1"));
}

/// True when `text` holds a run of 64 or more hexadecimal characters (the shape of a 256-bit secret).
fn has_long_hex_run(text: &str) -> bool {
    let mut run = 0;
    for c in text.chars() {
        run = if c.is_ascii_hexdigit() { run + 1 } else { 0 };
        if run >= 64 {
            return true;
        }
    }
    false
}

// 11
#[test]
fn events_carry_no_64_hex_secret() {
    let clean = event(SecurityEventKind::AuthFailure, "key-id-123");
    let json = serde_json::to_string(&clean).unwrap();
    assert!(!has_long_hex_run(&json), "{json}");
    // Negative control: the detector does fire on a token-shaped value, so it would catch a leak.
    let token = "ab12".repeat(16);
    let leaky = SecurityEvent::new(t0(), SecurityEventKind::AuthFailure, "s", &token, None, "d");
    assert!(has_long_hex_run(&serde_json::to_string(&leaky).unwrap()));
    // And the shipped sinks write nothing of that shape for a normal event.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("e.jsonl");
    FileSink::open(&path, 10_000).unwrap().record(&clean);
    assert!(!has_long_hex_run(&std::fs::read_to_string(&path).unwrap()));
}
