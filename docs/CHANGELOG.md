# Synapse Changelog

All notable changes to the Synapse project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased] - 6.0.0 release-candidate series

This file was not maintained between 1.1.0 and the 6.0.0 release candidates. The breaking changes of
the earlier candidates are recorded in their pull requests (`ciresnave/synapse`), not here.

### Docs (rc.30): soak procedure corrected, phase-1 live-push result

The two-channel development-channels dialog reads `Channels: server:claude-peers, server:synapse` (comma and
space); the procedure and two plans wrote it without the space, and now match. Adds
`docs/SOAK_PHASE1_RESULT_2026-10-09.md`, the measured phase-1 result. No code changes.

### Added (rc.29): `GET /v1/stats`, cumulative per-role mail counters

`synapsed` answers `GET /v1/stats` (session token, like `/v1/list`) with `sent`, `acked`, `redelivered` and
`expired` per claimed role, and `scope: "process"` plus `since`. The counters live in memory: a daemon restart
resets them. No message content, ids or bodies are in the answer. Additive: `mailbox::Delivery` gains
`prior_lease_lapsed` (a public struct, so a literal constructing one needs the field). Nothing else changes on
the wire or in the store. The soak procedure now reads these counters.

### Docs (rc.28): LAN-mode design note, and the `restarttest` live-push and soak procedure

Docs only; no code, wire or store change. `docs/LAN_MODE.md` is a design note (nothing built) on running Synapse
across two machines: why `synapsed` is loopback-only today, a LAN-listening hub (option A) against linked
per-machine daemons (option B), a harness for non-Claude models, and an honest size. `docs/RESTARTTEST_SOAK_PROCEDURE.md`
is the prepared, **not run** procedure for the M7 live push in the disposable `restarttest` lane and the M9
side-by-side soak: commands, rollback, what is measured, and stop conditions. It records that `synapsed` keeps no
cumulative sent/acked/redelivered counters, so the soak measures from a canary and the transcript. Neither
document approves a live test or a cutover.

### Breaking (rc.27): mail store format 2, and `fetch` no longer decodes every held message

`RedbStore` keeps a fixed-width `leases` row per leased message, so `fetch` skips the messages it must
withhold without JSON-decoding their metadata. On a role of 10 000 messages with 5 000 leased, `fetch(max=1)`
fell from p50 9.8 ms to 5.2 ms (release build, one Windows laptop, 1 KiB bodies; `tests/mailbox_fetch_cost.rs`).
**This is not a fix of the cost's growth.** The slice's own bar was "within 2x of the small case", and it is not
met: 5.2 ms is 2.4x the 1k case (2.1 ms), because `fetch` still walks every `META` and `leases` key in the role.
`fetch(max=1)` p50, half the role leased: 1k 2.9 -> 2.1 ms; 5k 6.8 -> 3.5 ms; 10k 9.8 -> 5.2 ms. A `READY` table
would remove the rest; it is tracked as a follow-up, to be done only if the M9 soak shows a need.

**The database file changes and cannot be downgraded.** The first open by this version rebuilds `leases` from the
existing rows and stamps format 2, all in one write transaction (a crash leaves the old file untouched and the next
open starts again). A store stamped with a newer format is refused with an error. rc.26 and earlier do not read the
stamp: opening a migrated file with them would leave `leases` stale, so do not. The wire protocol and the HTTP API
are unchanged.

### Added (rc.25): `synapse-claude-channel`, the Claude Code channel adapter (M7)

New crate and binary `synapse-claude-channel --role <role> [--home <dir>]`. It serves M6b's tools and, once a
second, leases mail from `synapsed` and pushes each message to Claude Code as a `notifications/claude/channel`
event whose text is escaped and framed as untrusted, then acks it. A failed write leaves the lease to run out,
so the message is redelivered. It caps the MCP protocol at 2025-11-25 (`server/discover` is answered `-32022`,
so Claude Code falls back to `initialize`) and declares the `tools` and `experimental: claude/channel`
capabilities. `synapse-mcp-server` gains `SynapseMcpServer::pull` and `confirm`. Not a cutover from
claude-peers; that is a separate decision.

### Breaking (rc.24): `synapse-mcp` is now the generic adapter over `synapsed` (M6b)

`synapse-mcp-server` (binary `synapse-mcp`) drops its UDP backend and its TOML config and talks to `synapsed`
through `synapse-client`, as one role. **Removed:** the `--config` file and `SYNAPSE_MCP_CONFIG`, the static
peers, the `poll` tool (replaced by `fetch`), `McpConfig`'s old fields, `SynapseMcpServer::start_with_sink`,
and the UDP transport it carried. **Now:** `synapse-mcp --role <role> [--home <dir>]` (or `SYNAPSE_ROLE`;
the home defaults as for the `synapse` CLI); tools `send(to, body, message_id?)`, `fetch(max?, lease_secs?)`,
`ack(message_id)`, `list()`, `set_summary(summary)`, `whoami()`. The adapter claims its role once, implicitly,
when the home has no session for it, and heartbeats every 30 s. A superseded or unknown session is reported as
a tool error and is never answered by re-claiming; take the role back with `synapse claim`. Fetched bodies
and summaries are escaped for control characters (newlines and tabs kept). New in `synapse-client`:
`Daemon::instance_id` and `Daemon::session_info`. Measured by the PM on 2026-10-08, on one box and its 11
MCP/settings files only: none names `synapse-mcp` as a server.
Alert delivery is still only a seam (`AlertTransport` has no implementation; board 131).

### Added (rc.23): `synapse-client` crate (M6a)

The blocking client for `synapsed` (find the daemon, prove it, claim a role, keep one session) moves out of
`synapsectl` into its own library crate, `synapse-client`, so the CLI and the coming MCP adapters share one
implementation. Pure move: `synapsectl` keeps its behaviour and its `tests/cli.rs` and `tests/mail.rs` pass
unchanged. The security-events surface string is still `synapsectl/health-proof`. A new test pins the exports
(`Daemon`, `MailError`, `encode_body`, `inbox_line`, `printable`, `events`).
Note: migration `002_drop_trust_tables.sql` is not applied anywhere.

### Changed (rc.22): auth-framework 0.3.0 -> 0.5.0-rc26

The optional `auth-framework` dependency moves to the newest published release. 0.3.0 pulled reqwest 0.11,
hyper 0.14 and h2 0.3.27 (RUSTSEC-2026-0258, no fix on the 0.3 line) and rustls-pemfile 1.0.4
(RUSTSEC-2025-0134, unmaintained); 0.5.0-rc26 resolves h2 0.4 and reqwest 0.12/0.13 only. `cargo audit`
at this change reports no vulnerabilities. Nothing that compiles today calls auth-framework (its only user,
`src/auth_integration_enhanced.rs`, is behind the deliberately broken `enhanced-auth` feature), so no
Synapse API changes. The `auth` feature now builds against a pre-release crate.
Two manifest lines had to follow: `lettre` gains `tokio1-native-tls` (auth-framework 0.5 enables lettre's
`tokio1`, which fails to compile beside the default `native-tls` without it; same TLS backend as before), and
`ring` gains `std` (another crate's graph used to switch it on; `src/synapse/auth/utils.rs` needs ring's errors
to be `std::error::Error`).

### Removed (breaking, rc.21): the trust system is spun out

The blockchain, staking, consensus and trust-score system is removed from the core. It was in-memory,
unwired in places (`ConsensusEngine`, `VerificationEngine` had no caller), unreachable from every shipped
binary, and is to be rebuilt as its own project around vouchers and revocations. Source is preserved at the
git tag `pre-trust-removal` (`30a6e06`). Core keeps authenticated senders (`sender_auth::TrustStore`,
`certificate`) and the optional `services::TrustSource` hook.

- `synapse::blockchain` and everything in it: `SynapseBlockchain`, `BlockchainConfig`, `StakingManager`,
  `ConsensusEngine`, `VerificationEngine`, `Block`, `Transaction`, `TrustReport`, and the bincode impls.
- `synapse::synapse::blockchain::serialization::*`: `DateTimeWrapper` and `UuidWrapper` are at `synapse::wire`
  (also re-exported from `synapse::types`).
- `SynapseNode` and `SynapseConfig`.
- `services::TrustManager`, `api::TrustAPI`, `models::trust` (`TrustBalance`, `TrustRatings`,
  `EntityTrustRatings`, `NetworkTrustRating`, `TrustCalculator` and the rest).
- `ParticipantProfile::trust_ratings` is now an opaque `serde_json::Value` (default `{}`, as the column's). Stored profiles
  still load: the old JSON is kept as is.
- `ParticipantRegistry::new(database, cache)` and `ParticipantAPI::new(registry, discovery, telemetry)` no longer
  take a `TrustManager`. `ParticipantStatistics` loses `trust_reports_today` and `average_trust_score`.
- `PrivacyManager::new_with_trust` (use `with_trust_source`) and `PrivacyManager::can_contact`, a stub with no
  caller that approved every request.
- `storage::Database`: `upsert_trust_balance`, `get_trust_balance`, `get_balances_for_decay`,
  `count_reports_since`, `record_trust_report`, `count_trust_reports_today`, `get_average_trust_score`.
  `storage::Cache`: `cache_trust_score`, `get_cached_trust_score`, `cache_block_hash`, `get_block_hash`.
- `From<bincode::error::{EncodeError, DecodeError}> for SynapseError`, and the `bincode` dependency
  (RUSTSEC-2025-0141 no longer applies).
- Migration `002_drop_trust_tables.sql` drops `trust_balances`, `trust_ratings`, `trust_reports`,
  `blockchain_blocks`, `blockchain_transactions` and two views (`DROP ... IF EXISTS`; `trust_reports` was never
  created by a migration). `participant_relationships.trust_score` is left alone.
- `[blockchain]` in `config/example.toml`; the README and API-reference trust sections;
  `docs/BLOCKCHAIN_TRUST_SYSTEM.md`.

### Fixed (rc.21)

- `ParticipantRegistry::search_participants` re-applies the privacy and trust filters to cached results. It
  used to return cached ids without checking the current policy or trust source.

### Added (rc.20)

- `synapse::services::TrustSource`, an optional hook for a trust score, with `NoTrustSource` (the default) and
  `TrustAnswer::{Score, Unsupported}`. Contact search (`min_trust_score`) and the privacy policy
  (`trust_threshold`) read through it. `ParticipantRegistry::with_trust_source` and
  `PrivacyManager::with_trust_source` set it.

### Changed (rc.20, behaviour)

- `PrivacyManager::evaluate_contact_request` no longer allows contact when a positive `trust_threshold` has no
  score. With no source, or a failing one, it returns an error (`TrustUnsupported` for no source). It used to log a warning and allow.
- `ParticipantRegistry` contact search with `min_trust_score` returns an error when the source fails. It used to
  treat a failure as a score of 0.0, and one failing lookup now fails the whole search rather than dropping that
  profile. A `min_trust_score` of 0.0 or less consults no source, as in `PrivacyManager`.

### Removed (breaking)

The binary codec that RUSTSEC-2025-0141 flags as unmaintained has been removed from the core types
(step B1 of `docs/superpowers/plans/2026-10-07-remove-bincode.md`). The wire format was always JSON
and is unchanged.

- `SimpleMessage::to_bytes` and `SimpleMessage::from_bytes`
- `SecureMessage::to_bytes` and `SecureMessage::from_bytes`
- `StreamChunk::to_bytes` and `StreamChunk::from_bytes`
- `StreamMetadata::to_bytes` and `StreamMetadata::from_bytes`
- The `bincode::Encode`/`Decode` impls on `MessageType`, `SecurityLevel`, `SimpleMessage`,
  `SecureMessage`, `StreamPriority`, `StreamType`, `StreamChunk`, `StreamMetadata`,
  `sender_auth::ProofAlg` and `sender_auth::SenderProof`.

Use `serde_json::to_vec` and `serde_json::from_slice` instead, which is the wire format.

### Moved

- `DateTimeWrapper` and `UuidWrapper` now live in `synapse::wire`. `synapse::types::…` and
  `synapse::blockchain::serialization::…` still re-export them.

### Added

- **`MultiTransportRouter::set_security_sink` (6.0.0-rc.16).** `MultiTransportRouter` builds its
  own Direct-mode email transport, outside any `TransportManager`, and gave it no sink. It now
  passes the sink to each of its transports, and `SynapseRouter::set_security_sink` passes the
  router's sink on to its `MultiTransportRouter`. Like `TransportManager`'s, this sink records only
  in Direct mode, whose SMTP listener enforces the inbound limits. `MultiTransportRouter` never
  starts its email transport itself, so today that listener is bound but not served; the path is
  wired and tested by injection, with the test starting the transport.
- **`SynapseRouter::set_security_sink` (6.0.0-rc.15).** The router's email transport now records
  its own security events to the sink you give it. That happens only in Direct mode, where its SMTP
  listener enforces the inbound limits. Before, it enforced them but recorded nothing. The router
  also passes the sink to its `SynapseEmailServer` (login failures and lockouts), but nothing
  populates that server yet. `SynapseEmailServer::set_security_sink` sets the sink on a server
  already behind an `Arc`.

### Changed (behaviour)

- **Email logins: attempts in progress count against the lockout (6.0.0-rc.13).** SMTP `AUTH` and
  IMAP `LOGIN` share one budget per username (10 failures) and per source address (50 failures)
  across both servers. A login whose password is still being checked now counts as a failure
  until it finishes. So while recent failures plus logins in progress reach a limit, even a
  correct password is refused. The refusal is logged as an `auth_failure` security event with
  detail `in_flight`, unlike a real lockout, which has detail `locked`. It clears when the logins
  in progress finish, unless they fail: each wrong password becomes a recorded failure and can
  complete a real lockout (15 minutes). This closes a race in which parallel connections could each guess
  past the limit.

## [1.0.0] - 2023-07-01

### Added

- Full production-ready release with all core features implemented
- Comprehensive error handling system with standardized API errors
- Enhanced security features and input validation
- Trust system security audit and improvements
- Rate limiting for all public endpoints
- Transport abstraction layer with HTTP, WebSocket, WebRTC support
- Circuit breaker patterns for external service resilience
- Complete API documentation with examples
- Telemetry and monitoring integration
- Docker containerization and deployment scripts
- CI/CD pipeline for automated testing and deployment
- WASM support for browser environments

### Changed

- Renamed message_routing_system to synapse throughout the codebase
- Enhanced error handling patterns across all components
- Optimized critical performance paths
- Updated all examples to use the latest API
- Improved configuration management with environment variable support
- Enhanced logging with structured log format

### Fixed

- Multiple concurrency issues in the participant registry
- Trust propagation algorithm security issues
- Database connection pool management
- Memory leaks in long-running WebRTC connections
- WASM compatibility issues on Safari mobile
- Registry search with special characters
- Blockchain transaction verification

## [0.9.0] - 2023-06-15

### New Features

- Initial WebAssembly (WASM) support
- Browser compatibility layer
- Trust system blockchain integration
- Enhanced identity resolution system
- Email transport layer
- Participant discovery system
- Registry service with search capabilities
- Message routing with transport selection
- Circuit breaker pattern implementation

### Improvements

- Refactored core architecture for better modularity
- Enhanced error handling
- Improved logging
- Updated configuration system

### Bug Fixes

- Multiple threading issues
- Connection handling edge cases
- Discovery service timeout issues
- Message routing failures in high-load scenarios
