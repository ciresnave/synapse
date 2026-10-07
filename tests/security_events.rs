// SPDX-License-Identifier: MIT OR Apache-2.0
//! Hardening P4: the core of the security-event mechanism (no I/O). Alert delivery awaits board 131.
//!
//! Every test pairs a negative control (the protection fires) with a positive control (a legitimate
//! case is unaffected). Time is injected; nothing sleeps.

use chrono::{DateTime, Duration, TimeZone, Utc};
use synapse::security_events::{
    AlertConfig, AlertPolicy, AlertReason, FailureLimiter, LimiterConfig, SecurityEvent,
    SecurityEventKind, Verdict, sanitize,
};

fn t0() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 7, 12, 0, 0).unwrap()
}

fn secs(n: i64) -> Duration {
    Duration::seconds(n)
}

fn limiter(max_keys: usize) -> FailureLimiter {
    FailureLimiter::new(LimiterConfig {
        free_failures: 5,
        window: Duration::minutes(5),
        base_delay: secs(1),
        max_delay: secs(60),
        lockout_after: 20,
        lockout: Duration::minutes(15),
        max_keys,
    })
}

fn event(
    kind: SecurityEventKind,
    surface: &str,
    subject: &str,
    at: DateTime<Utc>,
) -> SecurityEvent {
    SecurityEvent::new(at, kind, surface, subject, None, "detail")
}

// 1
#[test]
fn sanitize_escapes_controls_and_truncates_on_a_char_boundary() {
    // Negative control: control characters are escaped, never passed through.
    let cleaned = sanitize("a\u{1b}[2Jb\r\n\u{7}");
    assert_eq!(cleaned, "a\\u{1b}[2Jb\\r\\n\\u{7}");
    assert!(!cleaned.chars().any(char::is_control));

    // Negative control: length is bounded, multibyte input included, and the cut is on a char boundary.
    let long = "x".repeat(1000);
    assert_eq!(sanitize(&long).chars().count(), 256);
    let multibyte = "\u{e9}".repeat(1000);
    let cut = sanitize(&multibyte);
    assert_eq!(cut.chars().count(), 256);
    assert!(cut.chars().all(|c| c == '\u{e9}'));
    let emoji = "\u{1f600}".repeat(1000);
    assert_eq!(sanitize(&emoji).chars().count(), 256);

    // Positive control: plain ASCII, including the boundary length, is unchanged.
    assert_eq!(sanitize("synapsed/bearer key-1"), "synapsed/bearer key-1");
    let exact = "y".repeat(256);
    assert_eq!(sanitize(&exact), exact);

    // The constructor applies it to every text field.
    let e = SecurityEvent::new(
        t0(),
        SecurityEventKind::AuthFailure,
        "s\u{1b}",
        "u\n",
        Some("p\r"),
        "d\u{7}",
    );
    assert_eq!(e.surface, "s\\u{1b}");
    assert_eq!(e.subject, "u\\n");
    assert_eq!(e.source.as_deref(), Some("p\\r"));
    assert_eq!(e.detail, "d\\u{7}");
}

// 2
#[test]
fn limiter_free_budget_then_doubling_capped_delay() {
    let l = limiter(100);
    let now = t0();
    // Positive control: the free budget never delays.
    for n in 1..=5 {
        assert_eq!(
            l.record_failure("A", now),
            (Verdict::Allow, false),
            "failure {n}"
        );
    }
    assert_eq!(l.check("A", now), Verdict::Allow);
    // Negative control: past the budget the delay starts at base and doubles up to the cap.
    let expected = [1, 2, 4, 8, 16, 32, 60, 60];
    for (i, want) in expected.iter().enumerate() {
        let (verdict, locked) = l.record_failure("A", now);
        assert_eq!(verdict, Verdict::Delay(secs(*want)), "failure {}", 6 + i);
        assert!(!locked);
    }
    // The delay is honoured: still delayed inside it, allowed after it.
    assert_eq!(l.check("A", now + secs(10)), Verdict::Delay(secs(50)));
    assert_eq!(l.check("A", now + secs(60)), Verdict::Allow);
    // Positive control: another key is unaffected throughout.
    assert_eq!(l.check("B", now), Verdict::Allow);
    assert_eq!(l.record_failure("B", now), (Verdict::Allow, false));
}

// 3
#[test]
fn limiter_locks_out_once_then_recovers() {
    let l = limiter(100);
    let now = t0();
    let mut newly = 0;
    for n in 1..=19 {
        let (verdict, locked) = l.record_failure("A", now);
        assert!(
            !matches!(verdict, Verdict::Refuse { .. }),
            "failure {n} must not lock"
        );
        newly += usize::from(locked);
    }
    let (verdict, locked) = l.record_failure("A", now);
    assert_eq!(
        verdict,
        Verdict::Refuse {
            until: now + Duration::minutes(15)
        }
    );
    assert!(locked);
    newly += 1;
    // Further failures while locked stay refused and are not reported as a new lockout.
    let (verdict, locked) = l.record_failure("A", now + secs(30));
    assert!(matches!(verdict, Verdict::Refuse { .. }));
    assert!(!locked);
    assert!(matches!(
        l.check("A", now + secs(60)),
        Verdict::Refuse { .. }
    ));
    assert_eq!(newly, 1, "newly_locked is true exactly once");
    // Positive control: after the lockout passes the key is allowed again, with a fresh budget.
    let after = now + Duration::minutes(15);
    assert_eq!(l.check("A", after), Verdict::Allow);
    assert_eq!(l.record_failure("A", after), (Verdict::Allow, false));
}

// 4
#[test]
fn limiter_failures_outside_the_window_stop_counting() {
    let l = limiter(100);
    let now = t0();
    for key in ["old", "fresh"] {
        for _ in 0..5 {
            l.record_failure(key, now);
        }
    }
    // Negative control: inside the window the 6th failure is delayed.
    assert_eq!(
        l.record_failure("fresh", now + secs(60)).0,
        Verdict::Delay(secs(1))
    );
    // Positive control: once the first five are older than the window, the next one is free.
    assert_eq!(
        l.record_failure("old", now + Duration::minutes(6)),
        (Verdict::Allow, false)
    );
}

// 5
#[test]
fn limiter_success_resets_the_key() {
    let l = limiter(100);
    let now = t0();
    for _ in 0..6 {
        l.record_failure("good", now);
        l.record_failure("bad", now);
    }
    assert!(matches!(l.check("good", now), Verdict::Delay(_)));
    l.record_success("good");
    assert_eq!(l.check("good", now), Verdict::Allow);
    assert_eq!(l.record_failure("good", now), (Verdict::Allow, false));
    // Negative control: a key that never succeeded stays penalised.
    assert!(matches!(l.check("bad", now), Verdict::Delay(_)));
}

// 6
#[test]
fn limiter_is_bounded_and_evicts_the_oldest_failure() {
    let l = FailureLimiter::new(LimiterConfig {
        free_failures: 1,
        window: Duration::minutes(5),
        base_delay: secs(1),
        max_delay: secs(60),
        lockout_after: 20,
        lockout: Duration::minutes(15),
        max_keys: 3,
    });
    let now = t0();
    l.record_failure("k1", now);
    l.record_failure("k2", now + secs(1));
    l.record_failure("k2", now + secs(1));
    l.record_failure("k3", now + secs(2));
    assert_eq!(l.tracked_keys(), 3);
    // Positive control: a known key failing again does not evict anything.
    assert_eq!(
        l.record_failure("k1", now + secs(3)).0,
        Verdict::Delay(secs(1))
    );
    assert_eq!(l.tracked_keys(), 3);
    // Negative control: a 4th key evicts the key whose last failure is oldest (k2) and nothing else.
    l.record_failure("k4", now + secs(4));
    assert_eq!(l.tracked_keys(), 3);
    assert_eq!(
        l.record_failure("k2", now + secs(5)).0,
        Verdict::Allow,
        "k2 was evicted, so it starts fresh"
    );
    assert_eq!(l.tracked_keys(), 3);
    for i in 0..50 {
        l.record_failure(&format!("flood-{i}"), now + secs(6 + i));
        assert!(l.tracked_keys() <= 3);
    }
}

// 6b (review): an attacker must not be able to clear its own lockout by spraying fresh keys until
// the locked entry is evicted. A locked key is never evicted while an unlocked one can be.
#[test]
fn a_flood_of_new_keys_cannot_evict_a_lockout() {
    let l = FailureLimiter::new(LimiterConfig {
        free_failures: 1,
        window: Duration::minutes(5),
        base_delay: secs(1),
        max_delay: secs(60),
        lockout_after: 3,
        lockout: Duration::minutes(15),
        max_keys: 3,
    });
    let now = t0();
    for i in 0..3 {
        l.record_failure("attacker", now + secs(i));
    }
    assert!(matches!(
        l.check("attacker", now + secs(3)),
        Verdict::Refuse { .. }
    ));
    for i in 0..100 {
        l.record_failure(&format!("spray-{i}"), now + secs(10 + i));
        assert!(l.tracked_keys() <= 3);
    }
    // Negative control: still locked out after the spray.
    assert!(
        matches!(l.check("attacker", now + secs(200)), Verdict::Refuse { .. }),
        "spraying fresh keys evicted the lockout"
    );
    // Positive control: the lockout still ends on time.
    assert_eq!(
        l.check("attacker", now + Duration::minutes(16)),
        Verdict::Allow
    );
}

// 7
#[test]
fn alert_policy_immediate_kinds_alert_and_threshold_kinds_wait() {
    let immediate = AlertPolicy::new(AlertConfig::default());
    let alert = immediate
        .observe(&event(
            SecurityEventKind::NewClient,
            "public/mcp",
            "client-1",
            t0(),
        ))
        .expect("NewClient alerts at once");
    assert_eq!(alert.reason, AlertReason::Immediate);
    assert_eq!(alert.kind, SecurityEventKind::NewClient);
    assert_eq!(alert.count, 1);

    // Negative control: nine failures for one (surface, subject) do not alert; the tenth does.
    let policy = AlertPolicy::new(AlertConfig::default());
    for i in 0..9 {
        let e = event(
            SecurityEventKind::AuthFailure,
            "synapsed/bearer",
            "key-1",
            t0() + secs(i),
        );
        assert!(policy.observe(&e).is_none(), "event {}", i + 1);
    }
    let tenth = event(
        SecurityEventKind::AuthFailure,
        "synapsed/bearer",
        "key-1",
        t0() + secs(9),
    );
    let alert = policy.observe(&tenth).expect("the 10th alerts");
    assert_eq!(alert.reason, AlertReason::Threshold);
    assert_eq!(alert.count, 10);

    // Positive control: nine events spread across subjects never alert.
    let spread = AlertPolicy::new(AlertConfig::default());
    for i in 0..9 {
        let e = event(
            SecurityEventKind::AuthFailure,
            "synapsed/bearer",
            &format!("key-{i}"),
            t0(),
        );
        assert!(spread.observe(&e).is_none());
    }
    // Positive control: ten events spaced wider than the window never alert.
    let slow = AlertPolicy::new(AlertConfig::default());
    for i in 0..10 {
        let e = event(
            SecurityEventKind::AuthFailure,
            "synapsed/bearer",
            "key-1",
            t0() + Duration::minutes(i * 2),
        );
        assert!(slow.observe(&e).is_none());
    }
}

// 8
#[test]
fn alert_policy_coalesces_a_flood_into_one_alert_and_one_digest() {
    let policy = AlertPolicy::new(AlertConfig::default());
    let mut alerts = Vec::new();
    for i in 0..100 {
        let e = event(
            SecurityEventKind::AuthFailure,
            "synapsed/bearer",
            "key-1",
            t0() + Duration::milliseconds(i),
        );
        alerts.extend(policy.observe(&e));
    }
    // Negative control: a flood of 100 produces exactly one alert, not 91.
    assert_eq!(alerts.len(), 1);
    assert_eq!(alerts[0].reason, AlertReason::Threshold);
    // Nothing is due before the coalescing period has passed.
    assert!(policy.flush(t0() + Duration::minutes(14)).is_empty());
    // Then one digest, counting the events that were held (the 11th through the 100th).
    let digests = policy.flush(t0() + Duration::minutes(15) + secs(1));
    assert_eq!(digests.len(), 1);
    assert_eq!(digests[0].reason, AlertReason::Digest);
    assert_eq!(digests[0].kind, SecurityEventKind::AuthFailure);
    assert_eq!(digests[0].surface, "synapsed/bearer");
    assert_eq!(digests[0].count, 90);
    // The digest is not repeated.
    assert!(policy.flush(t0() + Duration::minutes(40)).is_empty());

    // Positive control: a different (kind, surface) is not held by this pair's coalescing.
    let other = policy.observe(&event(
        SecurityEventKind::NewClient,
        "public/mcp",
        "c",
        t0() + Duration::minutes(16),
    ));
    assert!(other.is_some());
    // Positive control: after the period an alert flows again.
    let later = AlertPolicy::new(AlertConfig::default());
    assert!(
        later
            .observe(&event(SecurityEventKind::Config, "s", "u", t0()))
            .is_some()
    );
    assert!(
        later
            .observe(&event(SecurityEventKind::Config, "s", "u", t0() + secs(1)))
            .is_none()
    );
    assert!(
        later
            .observe(&event(
                SecurityEventKind::Config,
                "s",
                "u",
                t0() + Duration::minutes(16)
            ))
            .is_some()
    );
}

// --- P6: the event gate (moved from synapsed's private copy into the core, time injected) ---

use synapse::security_events::EventGate;

#[test]
fn gate_holds_back_a_repeat_within_the_interval_and_reports_the_count_later() {
    let gate = EventGate::new(secs(1));
    let k = SecurityEventKind::UnverifiedSender;
    assert_eq!(
        gate.pass(k, "transport/knock", "d", t0()).as_deref(),
        Some("d")
    );
    // Negative control: two repeats inside the interval are held back.
    assert_eq!(gate.pass(k, "transport/knock", "d", t0()), None);
    assert_eq!(
        gate.pass(
            k,
            "transport/knock",
            "d",
            t0() + Duration::milliseconds(999)
        ),
        None
    );
    // After the interval the next line carries the count of what was held back, once.
    assert_eq!(
        gate.pass(k, "transport/knock", "d", t0() + secs(1))
            .as_deref(),
        Some("d; 2 similar events held back")
    );
    assert_eq!(
        gate.pass(k, "transport/knock", "d", t0() + secs(2))
            .as_deref(),
        Some("d")
    );
}

#[test]
fn gate_keys_on_kind_and_surface_never_on_subject() {
    let gate = EventGate::new(secs(1));
    let k = SecurityEventKind::UnverifiedSender;
    assert!(gate.pass(k, "transport/knock", "a", t0()).is_some());
    // Positive controls: another surface, and another kind on the same surface, pass.
    assert!(gate.pass(k, "transport/sealing", "a", t0()).is_some());
    assert!(
        gate.pass(SecurityEventKind::RateLimited, "transport/knock", "a", t0())
            .is_some()
    );
}

#[test]
fn a_zero_interval_gate_passes_everything() {
    let gate = EventGate::new(Duration::zero());
    let k = SecurityEventKind::ReplayRefused;
    for _ in 0..3 {
        assert_eq!(
            gate.pass(k, "transport/replay", "d", t0()).as_deref(),
            Some("d")
        );
    }
}
