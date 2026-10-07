# Synapse Changelog

All notable changes to the Synapse project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased] - 6.0.0 release-candidate series

This file was not maintained between 1.1.0 and the 6.0.0 release candidates. The breaking changes of
the earlier candidates are recorded in their pull requests (`ciresnave/synapse`), not here.

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

### Changed (behaviour)

- **Email logins: attempts in progress count against the lockout (6.0.0-rc.13).** SMTP `AUTH` and
  IMAP `LOGIN` share one budget per username (10 failures) and per source address (50 failures)
  across both servers. A login whose password is still being checked now counts as a failure
  until it finishes. So while recent failures plus logins in progress reach a limit, even a
  correct password is refused. The refusal is logged as an `auth_failure` security event with
  detail `in_flight`, unlike a real lockout, which has detail `locked`. It clears as soon as the
  logins in progress finish. This closes a race in which parallel connections could each guess
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
