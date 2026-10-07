// SPDX-License-Identifier: MIT OR Apache-2.0
//! Brute-force countermeasures and security events for the email servers (hardening P7, audit rows
//! 13, 14 and 18).
//!
//! Both servers speak TCP, so a source IP is the peer of a completed handshake and cannot be spoofed
//! off-path. That is why these limiters may refuse by source and lock a username, where the UDP
//! knock budget (`transport::security`) may only budget the record. The limiters run whether or not a
//! sink is attached: the sink decides only whether events are written.

use std::net::SocketAddr;
use std::sync::{Arc, OnceLock};

use chrono::{DateTime, Duration, Utc};

use crate::security_events::{
    EventGate, FailureLimiter, LimiterConfig, SecurityEvent, SecurityEventKind, SecuritySink,
    Verdict,
};

/// The longest username used as a limiter key or carried into an event. A username can be as long
/// as one line, and every tracked key would otherwise hold that much.
const MAX_USERNAME_CHARS: usize = 256;

/// Tuning for the login guard on SMTP `AUTH` and IMAP `LOGIN`.
#[derive(Debug, Clone)]
pub struct LoginLimits {
    /// Keyed by username. Locking it locks that account for everyone, which the spec accepts (row 14).
    pub per_user: LimiterConfig,
    /// Keyed by source IP: the backstop against one source spraying usernames.
    pub per_source: LimiterConfig,
    /// At most one event line per (kind, surface) per interval; zero writes every event.
    pub event_interval: Duration,
}

impl Default for LoginLimits {
    fn default() -> Self {
        let limiter = |free_failures, lockout_after| LimiterConfig {
            free_failures,
            window: Duration::minutes(15),
            base_delay: Duration::seconds(1),
            max_delay: Duration::seconds(30),
            lockout_after,
            lockout: Duration::minutes(15),
            max_keys: 10_000,
        };
        Self {
            per_user: limiter(3, 10),
            per_source: limiter(10, 50),
            event_interval: Duration::seconds(1),
        }
    }
}

/// Tuning for the SMTP server's inbound limits, keyed by source IP. Over a limit, the source is
/// refused for the rest of the minute. Zero refuses every connection or message.
#[derive(Debug, Clone)]
pub struct InboundLimits {
    /// Connections accepted per source per minute.
    pub connections_per_minute: u32,
    /// `MAIL` commands accepted per source per minute.
    pub messages_per_minute: u32,
    /// At most one event line per (kind, surface) per interval; zero writes every event.
    pub event_interval: Duration,
}

impl Default for InboundLimits {
    fn default() -> Self {
        Self {
            connections_per_minute: 120,
            messages_per_minute: 240,
            event_interval: Duration::seconds(1),
        }
    }
}

/// A per-minute count on `FailureLimiter`: it never delays, and the attempt one past the limit locks
/// the key for a minute. The attempt that locks is the one refused first, so it carries the event.
fn per_minute(limit: u32) -> FailureLimiter {
    FailureLimiter::new(LimiterConfig {
        free_failures: u32::MAX,
        window: Duration::minutes(1),
        base_delay: Duration::zero(),
        max_delay: Duration::zero(),
        lockout_after: limit.saturating_add(1),
        lockout: Duration::minutes(1),
        max_keys: 10_000,
    })
}

/// The IP of a peer, or `unknown` when the socket could not say.
pub(crate) fn source_ip(peer: std::io::Result<SocketAddr>) -> String {
    peer.map_or_else(|_| "unknown".to_string(), |addr| addr.ip().to_string())
}

struct Login {
    per_user: FailureLimiter,
    per_source: FailureLimiter,
    gate: EventGate,
}

struct Inbound {
    connections: FailureLimiter,
    messages: FailureLimiter,
    gate: EventGate,
}

/// The limiters and the event sink one server (or an SMTP and IMAP pair) shares. Clones share
/// everything, the sink slot included, so a sink set after a server is cloned still reaches it.
#[derive(Clone)]
pub(crate) struct EmailSecurity {
    sink: Arc<OnceLock<Arc<dyn SecuritySink>>>,
    login: Arc<Login>,
    inbound: Arc<Inbound>,
}

impl Default for EmailSecurity {
    fn default() -> Self {
        Self {
            sink: Arc::new(OnceLock::new()),
            login: Arc::new(Login::new(LoginLimits::default())),
            inbound: Arc::new(Inbound::new(InboundLimits::default())),
        }
    }
}

impl Login {
    fn new(limits: LoginLimits) -> Self {
        Self {
            per_user: FailureLimiter::new(limits.per_user),
            per_source: FailureLimiter::new(limits.per_source),
            gate: EventGate::new(limits.event_interval),
        }
    }
}

impl Inbound {
    fn new(limits: InboundLimits) -> Self {
        Self {
            connections: per_minute(limits.connections_per_minute),
            messages: per_minute(limits.messages_per_minute),
            gate: EventGate::new(limits.event_interval),
        }
    }
}

fn bounded(username: &str) -> String {
    username.chars().take(MAX_USERNAME_CHARS).collect()
}

fn delay_of(verdict: Verdict) -> std::time::Duration {
    match verdict {
        Verdict::Delay(d) => d.to_std().unwrap_or_default(),
        _ => std::time::Duration::ZERO,
    }
}

impl EmailSecurity {
    /// The same sink slot with new login limits.
    pub(crate) fn with_login(self, limits: LoginLimits) -> Self {
        Self {
            login: Arc::new(Login::new(limits)),
            ..self
        }
    }

    /// The same sink slot with new inbound limits.
    pub(crate) fn with_inbound(self, limits: InboundLimits) -> Self {
        Self {
            inbound: Arc::new(Inbound::new(limits)),
            ..self
        }
    }

    /// Where events go. The first call wins; later ones are ignored.
    pub(crate) fn set_sink(&self, sink: Arc<dyn SecuritySink>) {
        let _ = self.sink.set(sink);
    }

    #[allow(clippy::too_many_arguments)]
    fn emit(
        &self,
        gate: &EventGate,
        kind: SecurityEventKind,
        surface: &str,
        subject: &str,
        source: &str,
        detail: &str,
        now: DateTime<Utc>,
    ) {
        let Some(sink) = self.sink.get() else { return };
        if let Some(detail) = gate.pass(kind, surface, detail, now) {
            sink.record(&SecurityEvent::new(
                now,
                kind,
                surface,
                subject,
                Some(source),
                &detail,
            ));
        }
    }

    /// Before checking a password: false if the username or the source is locked out, in which case
    /// the caller answers exactly as for a wrong password without checking it (bcrypt is costly).
    pub(crate) fn login_allowed(
        &self,
        surface: &str,
        username: &str,
        source: &str,
        now: DateTime<Utc>,
    ) -> bool {
        let username = bounded(username);
        let locked = |verdict| matches!(verdict, Verdict::Refuse { .. });
        if locked(self.login.per_user.check(&username, now))
            || locked(self.login.per_source.check(source, now))
        {
            let kind = SecurityEventKind::AuthFailure;
            let gate = &self.login.gate;
            self.emit(gate, kind, surface, &username, source, "locked", now);
            return false;
        }
        true
    }

    /// After a wrong password: records the failure and its events, and returns how long to hold the
    /// answer. Successes are never recorded, so a real login cannot reset an attacker's count.
    pub(crate) fn login_failed(
        &self,
        surface: &str,
        username: &str,
        source: &str,
        now: DateTime<Utc>,
    ) -> std::time::Duration {
        let username = bounded(username);
        let (user, user_locked) = self.login.per_user.record_failure(&username, now);
        let (src, source_locked) = self.login.per_source.record_failure(source, now);
        let gate = &self.login.gate;
        let failure = SecurityEventKind::AuthFailure;
        self.emit(
            gate,
            failure,
            surface,
            &username,
            source,
            "bad_credentials",
            now,
        );
        let lockout = SecurityEventKind::Lockout;
        if user_locked {
            self.emit(gate, lockout, surface, &username, source, "per_user", now);
        }
        if source_locked {
            self.emit(gate, lockout, surface, source, source, "per_source", now);
        }
        delay_of(user).max(delay_of(src))
    }

    /// At accept: false if the source is over its connections per minute.
    pub(crate) fn connection_allowed(&self, source: &str, now: DateTime<Utc>) -> bool {
        let connections = &self.inbound.connections;
        self.inbound_allowed(connections, source, "connections_per_minute", now)
    }

    /// At `MAIL`: false if the source is over its messages per minute.
    pub(crate) fn message_allowed(&self, source: &str, now: DateTime<Utc>) -> bool {
        let messages = &self.inbound.messages;
        self.inbound_allowed(messages, source, "messages_per_minute", now)
    }

    fn inbound_allowed(
        &self,
        limiter: &FailureLimiter,
        source: &str,
        limit: &str,
        now: DateTime<Utc>,
    ) -> bool {
        if matches!(limiter.check(source, now), Verdict::Refuse { .. }) {
            return false;
        }
        let (_, tripped) = limiter.record_failure(source, now);
        if tripped {
            let kind = SecurityEventKind::RateLimited;
            let gate = &self.inbound.gate;
            let surface = "email/smtp-inbound";
            self.emit(gate, kind, surface, source, source, limit, now);
            return false;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn t0() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 7, 12, 0, 0).unwrap()
    }

    fn quick(lockout_after: u32) -> LoginLimits {
        let limiter = LimiterConfig {
            free_failures: 1,
            window: Duration::minutes(15),
            base_delay: Duration::milliseconds(1),
            max_delay: Duration::milliseconds(5),
            lockout_after,
            lockout: Duration::minutes(15),
            max_keys: 100,
        };
        LoginLimits {
            per_user: limiter.clone(),
            per_source: LimiterConfig {
                lockout_after: 1_000,
                ..limiter
            },
            event_interval: Duration::zero(),
        }
    }

    #[test]
    fn a_login_lockout_ends_on_time() {
        let s = EmailSecurity::default().with_login(quick(2));
        let now = t0();
        assert!(s.login_allowed("t", "alice", "10.0.0.1", now));
        s.login_failed("t", "alice", "10.0.0.1", now);
        s.login_failed("t", "alice", "10.0.0.1", now);
        // Negative control: locked.
        assert!(!s.login_allowed("t", "alice", "10.0.0.1", now + Duration::minutes(14)));
        // Positive control: the lockout ends.
        assert!(s.login_allowed("t", "alice", "10.0.0.1", now + Duration::minutes(15)));
    }

    #[test]
    fn an_inbound_limit_resets_after_its_minute() {
        let s = EmailSecurity::default().with_inbound(InboundLimits {
            connections_per_minute: 2,
            messages_per_minute: 1,
            event_interval: Duration::zero(),
        });
        let now = t0();
        assert!(s.connection_allowed("10.0.0.1", now));
        assert!(s.connection_allowed("10.0.0.1", now));
        // Negative control: the third in the minute is refused, and so is the next.
        assert!(!s.connection_allowed("10.0.0.1", now));
        assert!(!s.connection_allowed("10.0.0.1", now + Duration::seconds(30)));
        // Positive controls: another source is unaffected, and the limit resets after the minute.
        assert!(s.connection_allowed("10.0.0.2", now));
        assert!(s.connection_allowed("10.0.0.1", now + Duration::seconds(61)));
        // Messages count separately from connections.
        assert!(s.message_allowed("10.0.0.1", now));
        assert!(!s.message_allowed("10.0.0.1", now));
    }
}
