# Synapse Changelog

All notable changes to the Synapse project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased] - 6.0.0 release-candidate series

This file was not maintained between 1.1.0 and the 6.0.0 release candidates. The breaking changes of
the earlier candidates are recorded in their pull requests (`ciresnave/synapse`), not here.

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
- `ParticipantProfile::trust_ratings` is now an opaque `serde_json::Value` (default `null`). Stored profiles
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
  `blockchain_blocks`, `blockchain_transactions`, two views and `participants.trust_score`.
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
