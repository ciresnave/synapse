# Glossary

Terms this repository uses with a specific meaning, each defined once. Where a term is defined by code, the source is named so
the definition can be checked; this page is not a design document. Alphabetical.

| Term | Meaning | Source |
|------|---------|--------|
| **Account key** | An Ed25519 key pinned in a `TrustStore` by its `key_id`; a certificate chain whose root names one is trusted for the account. | `src/sender_auth.rs` |
| **Ack** | The consumer's statement that a leased message was handled: `Mailbox::ack` removes it. Until then it can be delivered again. | `src/mailbox.rs` |
| **Dead set** | A role's messages that were leased `max_deliveries` times and never acked. Bounded by the queue bounds and kept for retention, with their bodies: bounded and visible, never silently dropped. Counted as `pending.dead` in `list`. | `src/mailbox.rs` |
| **At-least-once** | The mailbox's delivery guarantee: a message may arrive more than once (a lease that expires makes it deliverable again), so consumers dedupe by `message_id`. | `src/mailbox.rs` |
| **Channel adapter** | `synapse-claude-channel`: the process that puts mailbox messages into a Claude Code session. Synapse moves bytes between processes; it does not put them into an agent, that is each agent's client or an adapter. | `crates/synapse-claude-channel`, `ROADMAP.md` |
| **Claim** | A role taking ownership of its mailbox. A role's newest claim voids every lease an older epoch held. | `src/roles.rs`, `src/mailbox.rs` |
| **Envelope** | One message as the mailbox carries it: `message_id`, `to` (`role@account`), `from`, and an opaque `body` (the daemon stores a signed message there). The mailbox never parses it. | `src/mailbox.rs` |
| **Guard** | A CI job that fails on the day a recorded fact stops being true (for example a dependency licence or advisory check). A red guard is news, not a regression. | `.github/workflows/ci_cd.yml` |
| **Home** | The daemon's data directory (mailbox file, keys, start diagnostics). Refused when its permissions are too open (`permissions_too_open`). | `crates/synapsed` |
| **Instance id** | The identifier a daemon proves to the CLI before the CLI sends any credential. | `crates/synapsed` |
| **Lease** | A time-limited claim on a fetched message. Defaults: 60 s, minimum 5 s, maximum 15 minutes. | `MailConfig` in `src/mailbox.rs` |
| **`MailStore`** | The storage trait under the mailbox; every mailbox operation is one atomic write transaction on it. Implemented in memory and on redb. | `src/mailbox.rs`, `tests/mailbox_suite/mod.rs` |
| **Mailbox** | Per-role durable mail that waits for the role across session restarts, daemon restarts and crashes. Retention deletes acked history only (default 7 days), never undelivered mail. | `src/mailbox.rs` |
| **MCP server** | `synapse-mcp-server`: the generic MCP adapter over `synapsed`, served over stdio. | `crates/synapse-mcp-server` |
| **redb** | The embedded database under the daemon's mailbox (`mailbox-redb`). Its on-disk file carries Synapse's own `FORMAT` stamp (currently 2). | `src/mailbox.rs` |
| **Role** | A named mailbox owner, addressed `role@account`. | `src/roles.rs` |
| **Sealing** | Encrypting a message body to its recipient's X25519 key (HPKE, RFC 9180). Only `encrypted_content` is sealed; a signed `synapse.sealed = "v1"` marker says so. Sealing keys are never derived from a node's Ed25519 key. | `src/sealing.rs` |
| **Soak** | A long-running test of the daemon with a steady message flow (one message per 10 minutes), run in the restarttest session. Procedure: `docs/RESTARTTEST_SOAK_PROCEDURE.md`. | `docs/` |
| **`synapsectl`** | The command-line client (binary `synapse`): `identity` (account and role keys), `claim`, `send`, `inbox`, `ack`, `list` against the daemon. | `crates/synapsectl` |
| **`synapsed`** | The local daemon: owns the mailbox, listens on loopback, speaks the `/v1` HTTP API. | `crates/synapsed` |
| **Transport** | One way to move a message between processes: `TransportType` is TCP, UDP, WebSocket, HTTP, Email, Auto-Discovery, QUIC, NAT-Traversal or Custom. | `src/transport/abstraction.rs` |
| **`TrustSource`** | The seam a vouching crate would implement to say whether a key is trusted; a failing source is an error, never a pass. | `src/synapse/services/trust_source.rs` |
| **`TrustStore`** | Ed25519 public keys (and X25519 sealing keys) pinned out of band, by global id. Nothing in a received message can add or change an entry. | `src/sender_auth.rs` |
| **`whoami`** | The adapter call that reports who this session is and, since rc.52, `push: {capability_key, method}` (`null` on the generic adapter). | `crates/synapse-claude-channel` |
| **Release candidate (`rc`)** | The version series `6.0.0-rc.N`. Every pushed change bumps N; the number is allocated at the PM's gate. Nothing is published before CireSnave's go. | `Cargo.toml`, `docs/CHANGELOG.md` |
