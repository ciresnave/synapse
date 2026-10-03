# M5b: the `synapse` mail commands and `synapsed` auto-start (implementation plan)

**Spec:** `docs/superpowers/specs/2026-10-02-m5-synapsed-design.md` (PM approved Q1–Q5 as
recommended, 2026-10-02). M5a (`synapsed`, #71) is merged; this is the second and last M5 PR.
- **Stop rule:** deliver the PR and a [READY], then stop. No M6, no publish, no version bump.
- **Measured at:** `origin/main@7a04da4` (5.5.0).

## Global constraints

- Toolchain stable, SPDX headers on new files, no vendor names, no new non-Rust deps (board 104):
  the HTTP client is `reqwest` with default features off (`blocking`, `json`), so no TLS stack.
- The core and `synapsed` are unchanged.

## Design decisions (within the approved spec)

1. **Commands:** `synapse claim | send | inbox | ack | list`, each with `--role <role>` (or
   `SYNAPSE_ROLE`). `send --to` takes `beta` or `beta@acct`; a bare role gets this account. The
   message is the positional argument, else stdin. `inbox` and `list` print one JSON object per
   line, so any language can consume them; `inbox` adds `body` (the text) when the body is UTF-8.
2. **Discovery:** read `<home>/synapsed.json` {addr, instance_id, pid}, then `GET /v1/health` at
   `addr`. A missing file, unreadable file, or failed health means: spawn `synapsed` and poll for
   up to 5 s until a fresh announce (a different instance id from the stale one) answers health.
3. **Spawn:** `synapsed` next to the `synapse` executable, else from PATH. stdio null. Windows:
   `DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW`. Unix: `process_group(0)`
   (std; no libc dependency), so a terminal's Ctrl-C does not reach the daemon. The environment is
   inherited, so `SYNAPSE_HOME` and `SYNAPSE_ADDR` reach it. Racing CLIs both spawn; the loser's
   daemon exits on redb's store lock before it announces, and both CLIs find the winner.
4. **Sessions are cached per role,** in `<home>/sessions/<role>.json` {instance_id, token,
   global_id, epoch}, written atomically and owner-only (as the announce file is). Without a cache
   every CLI command would re-claim, bump the epoch, and supersede the role's other holder. A cache
   for another daemon instance is ignored. The spec's Q2 already notes that on a single-user
   loopback the token and the key file are equivalent, since either can claim.
5. **Claim** (`synapse claim`, or implicitly when there is no usable cached session, or on a 401):
   `sign_claim_for(identity, instance_id, random nonce, max(now, started_at + 1 s))`, with
   `started_at` from `/v1/health`. `synapse claim` always claims fresh (a deliberate takeover).
6. **409 `superseded` is never answered by re-claiming:** the CLI exits with an error naming the
   current epoch and pointing at `synapse claim`. Stealing a role back must be explicit.
7. **Sends are idempotent across the 401 retry:** the CLI picks the client message id (UUID v4)
   when `--id` is absent, so a retried send is a `duplicate`, never a second message.

## Tasks

### Task 1: failing tests (`crates/synapse-cli/tests/mail.rs`)

Each test uses a temp `SYNAPSE_HOME` with `synapse id init --account acct`, `SYNAPSE_ADDR=127.0.0.1:0`
(the CLI learns the real port from the announce file), and a guard that kills the daemon by its
announced pid on drop. The helper builds `synapsed` into the same target directory first, since
`cargo test -p synapse-cli` alone does not build another package's binary.

- `a_command_against_a_stopped_daemon_starts_it` (spec acceptance 1)
- `two_concurrent_first_commands_leave_one_daemon` (spec acceptance 2): two first commands at
  once; both succeed; every daemon pid either CLI reports spawning is dead except the announced one.
- `a_round_trip_between_two_roles` (spec acceptance 3): claim, send, inbox shows `from` =
  `alpha@acct` and the body, ack gives `removed`, the inbox is then empty.
- `a_cached_session_is_reused`: repeated commands leave the role's epoch at 1.
- `a_superseded_session_is_reported_not_stolen`: a stale cached session gets a non-zero exit naming
  `superseded`, and the epoch is unchanged.
- `a_restarted_daemon_is_restarted_and_reclaimed`: kill the daemon; the next command starts a new
  one and succeeds.

### Task 2: implement (`crates/synapse-cli/src/{main.rs, mail.rs}`) until Task 1 passes

### Task 3: full suite, clippy `-D warnings`, fmt, M0 guard; one fresh review; PR; [READY]
