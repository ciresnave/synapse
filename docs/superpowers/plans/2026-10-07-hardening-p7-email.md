# Hardening P7: email limiters, lockout and events, audit rows 13, 14 and 18 (plan)

**Spec:** `docs/superpowers/specs/2026-10-07-brute-force-hardening-design.md` §3, rows 13, 14 and 18. P4 (#84)
built `synapse::security_events`; P6 (#86) added `EventGate` and `TransportManagerBuilder::security_sink`.

**Scope:** `src/email_server/` (the SMTP and IMAP servers) and the Direct-mode email transport, which
owns a `SynapseSmtpServer`. There is no new public field on an existing struct, because that would break
struct literals. The additions are new builder methods, a new module, new email config-map keys, and one
provided (defaulted) method on `Transport`.

**Alert DELIVERY is still only a seam** (`AlertTransport`, no implementation, board 131). P7 writes
events to a sink only.

## Why P7 refuses where P6 only budgeted the record

P6 could not refuse anything: a UDP source is spoofable, so refusing by source would let a spoofer
silence a real peer (PM ruling A). Both P7 surfaces are TCP. The source IP is the peer of a completed
handshake, so it is not spoofable off-path. Refusing by source IP is therefore safe here, and so is
locking a username.

**Stated in the PR:** a username lockout lets anyone who knows a username lock that account for
`lockout`. The spec chose that trade (row 14). The lockout is short (15 min), and the per-source
backstop refuses a single sprayer before it can lock many accounts.

## Rules

1. **Record before rejecting.** The event is written on the failure path, before the response is sent.
2. **Delay the failure, never the attempt.** A failed login is answered after the limiter's delay. A
   locked key is refused **before** `authenticate` runs. bcrypt costs ~100 ms of CPU per call, so
   skipping it is part of the countermeasure.
3. **A refusal reads exactly like a wrong password** (`535 Authentication failed`,
   `NO LOGIN failed`), so a lockout is not an oracle for which usernames exist.
4. **Never call `record_success`.** A legitimate login would otherwise reset an attacker's count on the
   same username.
5. **Limiter keys are truncated to 256 characters** (P6's review finding). A username can be as long
   as one line.
6. **No secret in an event.** The subject is the username (sanitized) or the source IP. A password, or
   an AUTH PLAIN blob, never appears.

## Files

| file | change |
|---|---|
| `src/email_server/security.rs` | new: `LoginLimits`, `InboundLimits`, and the crate-private `EmailSecurity` (limiters, `EventGate`, an optional sink set once) |
| `src/email_server/mod.rs` | `pub mod security`; `SynapseEmailServer::with_security_sink`, `::with_login_limits` |
| `src/email_server/smtp_server.rs` | the peer IP passed into `handle_connection`; the inbound limits at accept and `MAIL`; the login guard on `AUTH PLAIN`; `with_login_limits`, `with_inbound_limits`, `set_security_sink` |
| `src/email_server/imap_server.rs` | the peer IP; the login guard on `LOGIN`; `with_login_limits`, `set_security_sink`; `serve(listener)`, as the SMTP server has |
| `src/transport/abstraction.rs` | `Transport::attach_security_sink(&self, Arc<dyn SecuritySink>)`, a provided method that does nothing by default |
| `src/transport/manager.rs` | `start_transport` attaches the manager's sink, if it has one, before `transport.start()` |
| `src/transport/security.rs` | `TransportEvents::sink()` (crate-private) |
| `src/transport/email_unified.rs` | override `attach_security_sink`, forwarding it to the Direct-mode `SynapseSmtpServer`; config keys for the inbound limits |
| `tests/email_security_events.rs` | new; the tests below |

## API

```rust
// synapse::email_server::security
pub struct LoginLimits { pub per_user: LimiterConfig, pub per_source: LimiterConfig,
                         pub event_interval: chrono::Duration }
pub struct InboundLimits { pub connections_per_minute: u32, pub messages_per_minute: u32,
                           pub event_interval: chrono::Duration }

// SynapseSmtpServer (builders consume and return Self; the sink slot is shared by clones)
pub fn with_login_limits(self, limits: LoginLimits) -> Self;
pub fn with_inbound_limits(self, limits: InboundLimits) -> Self;
pub fn set_security_sink(&self, sink: Arc<dyn SecuritySink>);      // first call wins
// SynapseImapServer: with_login_limits, set_security_sink
// SynapseEmailServer: with_security_sink(self, sink), with_login_limits(self, limits) -> both servers

// Transport (provided method, so no implementor breaks)
fn attach_security_sink(&self, _sink: Arc<dyn SecuritySink>) {}
```

**The limiters are on by default, with or without a sink.** A sink decides only whether events are
written. The countermeasure does not depend on it.

**Defaults:**

| limiter | key | window | free_failures | base / max delay | lockout_after | lockout | max_keys |
|---|---|---|---|---|---|---|---|
| login per_user | username | 15 min | 3 | 1 s / 30 s | 10 | 15 min | 10 000 |
| login per_source | source IP | 15 min | 10 | 1 s / 30 s | 50 | 15 min | 10 000 |

- **Inbound**, keyed by source IP: 120 connections and 240 messages per minute. Over the limit, the
  source is refused until the minute is over.
- **Inbound is built on `FailureLimiter` used as a counter.** Each limit has `free_failures = u32::MAX`
  (so it never delays), `lockout_after = N`, `window = lockout = 1 min`, and `max_keys = 10 000`. One
  `record_failure` is made per connection or message.
- `event_interval` is 1 s everywhere.
- **Source keys:** an IPv4 source is keyed by its address. An IPv6 source is keyed by its /64, because anyone holding a /64 can rotate addresses. An IPv4-mapped IPv6 address is keyed as its IPv4. Events still carry the full address as `source`.
- **Email transport config keys:** `email_inbound_connections_per_minute` and
  `email_inbound_messages_per_minute`. Both are parsed in `EmailLimits::from_config`; zero or a
  non-number is a config error.

## Rows

| row | where | countermeasure | event | surface | subject | source |
|---|---|---|---|---|---|---|
| 13 | `serve`, at accept | over `connections_per_minute`: `421 4.7.0 Too many connections, try again later`, then close | `RateLimited`, on the connection that first trips the limit in each window | `email/smtp-inbound` | source IP | peer addr |
| 13 | `MAIL` | over `messages_per_minute`: `451 4.7.1 Too many messages, try again later` | `RateLimited`, likewise | `email/smtp-inbound` | source IP | peer addr |
| 14, 18 | SMTP `AUTH PLAIN` (inline) | the login guard (rules 2–4) | `AuthFailure` on every failure or refusal (detail `bad_credentials` or `locked`); `Lockout` on the failure that first locks a key (detail `per_user` or `per_source`) | `email/smtp-auth` | username | peer addr |
| 14, 18 | IMAP `LOGIN` | the login guard | as above | `email/imap-login` | username | peer addr |

- `AUTH PLAIN` with no inline data answers `334` today, but the continuation is never read, and
  `AUTH LOGIN` answers `504`. Neither reaches `authenticate`, so neither needs a guard.
  `git grep '\.authenticate('` finds exactly these two call sites, IMAP and SMTP. The same query
  finds the tests' calls in `tests/email_server_has_no_default_users.rs`.
- Direct mode sets `require_auth: false` and uses `AcceptAllAuthHandler`. It gets row 13 only: the
  login guard runs only on `AUTH`, which a Direct-mode sender never needs.

## Tests, red first (`tests/email_security_events.rs`, capturing sink, real loopback sockets)

The limits are tuned small, with 1 ms delays, so nothing sleeps for long.

| test | negative control | positive control |
|---|---|---|
| imap_bad_login_is_an_event | a wrong password → `NO`, one `AuthFailure` on `email/imap-login`, subject the username, source 127.0.0.1 | the right password → `OK`, no event |
| imap_lockout_refuses_the_right_password | `lockout_after` 3: three failures → one `Lockout`; then the right password → `NO`, `AuthFailure` `locked` | a second user, from the same source, still logs in |
| smtp_auth_lockout | as above, over `AUTH PLAIN` (`535`) | a second user gets `235` |
| per_source_backstop | a spray of 5 usernames from one source with per_source `lockout_after` 4 → `Lockout` `per_source`; then a real user from that source is refused | — (the backstop is meant to refuse that source) |
| inbound_connection_limit | `connections_per_minute` 2: the 3rd connection reads `421`, and there is one `RateLimited` on `email/smtp-inbound` | connections 1–2 read `220` |
| inbound_message_limit | `messages_per_minute` 1: the 2nd `MAIL` reads `451`, with one `RateLimited` | the 1st `MAIL` reads `250` |
| direct_mode_inherits_the_manager_sink | a manager built with `security_sink` and an email transport with `email_inbound_connections_per_minute=1`: the 2nd raw connection to its port reads `421`, and the manager's sink holds the `RateLimited` | the 1st connection reads `220` |
| no_secret_in_any_event | every event above: neither password nor AUTH PLAIN blob in any field | the scan finds the username |
| no_sink_still_limits | the lockout test with no sink: same responses | — |

`security.rs` also gets unit tests with time injected: a lockout ends on time, and an inbound limit
resets after its window.

## Known limits (from the final review, stated in the PR)

- **The delay slows one connection, not a parallel attacker.** The lockout is the real bound.
- **Check-then-record race.** `login_allowed` and `login_failed` are separate steps with bcrypt between them, so parallel connections can each land about one guess past the limit per lockout cycle. That is bounded by the runtime's worker threads.
- **Lock state is visible by timing.** A locked attempt answers at once (no bcrypt, no delay). It reveals nothing about passwords. An unknown username already skips bcrypt, which is the pre-existing enumeration oracle in `auth.rs`.
- **Two paths enforce the limits but have no sink yet:** `router_merged`'s `ensure_email_transport` and its `SynapseEmailServer`. They are follow-ups.
