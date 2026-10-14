# Synapse roadmap

Written 2026-10-14 against `origin/main` f506140 (6.0.0-rc.47). Current state, not history: delete what is done, move what
is worth keeping to its home. Priorities across projects live in the portfolio roadmap; this file is Synapse's own path.

**The public flip is CireSnave's decision**, not this file's. Everything below is "ready so he can say go", and nothing here
publishes anything.

## What Synapse is

A provider-agnostic agent-to-agent transport: authenticated messages between processes (TCP/UDP/QUIC/email), a local daemon
(`synapsed`) with an at-least-once mailbox (redb), a CLI (`synapsectl`), an MCP server, and a Claude Code channel adapter.
It moves bytes between processes; it does not put them into an agent (that is each agent's client or an adapter).

## Release path to the public flip (ordered)

| # | Item | State at f506140 | Blocks flip? |
|---|------|------------------|--------------|
| 1 | CI green and protected: required checks, `--locked`, Cargo.lock tracked, actions on newest majors | done (rc.43-rc.46) | no |
| 2 | Docker image push to `thinkersjournal/synapse` | works (sha-f506140 on Hub, per PM 2026-10-14) | no |
| 3 | Coverage upload | done (org-level `CODECOV_TOKEN`, 2026-10-12) | no |
| 4 | **`cargo publish --dry-run` in dependency order**: `synapse` (root) -> `synapse-security` -> `synapsed-client` -> `synapse-mcp-server` -> `synapse-claude-channel`; `synapsed` and `synapsectl` after `synapsed-client` | order read from the `crates/*/Cargo.toml` path deps at f506140; **dry-run itself not yet run** | **yes** |
| 5 | **Cold first run from the README** on a clean machine/profile: every command in README works as written | not run | **yes** |
| 6 | `docker-compose.yml` matches the daemon (it still describes a Postgres + 8080 + config-mount service) | stale | yes (it ships in the repo) |
| 7 | `whoami` / `push` advertisement: what the MCP server and the daemon say a client can do matches what they do | open | no, but a public surface |
| 8 | Soak ends with `sent.log` matching the transcript | running (rc.33 daemon, 1 msg/10 min since 2026-10-12T18:31Z) | no (evidence) |
| 9 | Alert delivery: `AlertTransport` has no implementation (board 131); alerts are logged only | seam only | no, said plainly in the docs |
| 10 | Dependency currency: `auth-framework` is `0.6.0-rc42` here, crates.io has `0.6.0-rc55` | 13 rcs behind | CireSnave's "newest at all times" rule |
| 11 | crates.io names: `synapse` is at 1.1.0 there (ours is 6.0.0-rc.47, higher); the six other names are `0.0.0` placeholders. Ownership/permission to publish them is **unverified** | unknown | **yes** |
| 12 | `docs/` pass: stale pages (`DEPLOYMENT_GUIDE`, `CONFIGURATION_GUIDE`, `API_REFERENCE`) against the daemon-era surface | unmeasured | no |

Each item is its own PR with a version bump (PM allocates the number). Items 4-6 go first: they are mechanical and each one
can fail the first public user.

## What Synapse gives and takes

| Project | Synapse takes | Synapse gives | State |
|---------|---------------|---------------|-------|
| **auth-framework** | Account-holder auth, OAuth, JWT, credential stores, behind the optional `auth` feature (`auth-framework = 0.6.0-rc42`) | Real usage as a consumer; findings (e.g. the lettre `tokio1`/native-tls compile_error, the aws-lc-rs C dependency the owner accepted) | live; currency gap in item 10 |
| **OverMind** | Orchestration: choosing sessions, spawning, restart tooling; the 17-paper AI-to-AI literature summaries that `DESIGN_AI_TO_AI_COMMUNICATION.md` cites | Messaging between modules and lanes (OverMind's layer spec says messaging is Synapse's, not OverMind's); a Synapse plug-in point (`on_message_in`); the soak host (OverMind owns the restarttest session) | design agreed in OverMind's `ARCHITECTURE-LAYERS-SPEC.md`; OverMind's `dispatch.py` was built against FAM, not Synapse (its DESIGN-PROPOSAL, possibly dated) |
| **PersistAnt** | Nothing yet. Its `DESIGN-KINDS.md` proposes shared blob/kv/lock/rel kinds and lists Synapse's mailbox (redb), keystore, locks and the legacy Postgres layer as inputs and as a later migration (rows 2 and 6) | The measured requirements: one atomic multi-key transaction with ordered composite-key scan, 0600 files, no-clobber publish by hard link, OS file lock | proposal on PersistAnt's side; nothing adopted. Synapse stays on redb until it exists and passes the mailbox contract tests |
| **KISS** | Nothing | Nothing | **no relationship today**: `git grep -i synapse origin/main` in KISS finds 0 files, and KISS standardises ML kernel interfaces. Do not invent one |
| **Trust vouching** (to be spun out) | Optional trust-hook trait implementation, published on crates.io | `TrustSource` (`src/synapse/services/trust_source.rs`), the seam a vouching crate would implement | spin-out not scheduled (CireSnave 2026-10-08) |
| **Introduction app** (not started) | Account-owner introductions, so Synapse never authenticates account owners itself | Agent certificates (slice f) are the cryptographic side | waits for a lane to state requirements |

## After the flip (unordered, not tasked)

- Channel adapter push default and wake-on-mail across lanes (the claude-peers cutover is a separate question to CireSnave).
- `synapse stats` CLI reader for `/v1/stats` (PM allocates the number if asked).
- Deferred minors listed in HANDOFF (M5a/M5b/M6b), and issues #1, #20-22, #48, #49, #55, #56.
- The 6.0.0 final cut (rc series -> 6.0.0) once items 4-6 and 11 are done.
