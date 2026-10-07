# Hardening P8: CLI security events, audit rows 3 (client side), 6 and 7 (plan)

**Spec:** `docs/superpowers/specs/2026-10-07-brute-force-hardening-design.md` §3, rows 3, 6 and 7. P4 (#84)
built `synapse::security_events` and the `synapse-security` sinks.

**Scope:** the `synapse` command line (`crates/synapsectl`) only. The countermeasures already exist:
- row 3: the health proof (#78) refuses a squatter and warns on stderr;
- rows 6 and 7: `check_owner_only` refuses an announce file, session cache or keystore item that
  others can reach.

P8 adds the missing event: each refusal is recorded before the command exits.

**Alert DELIVERY is still only a seam** (`AlertTransport`, no implementation, board 131). P8 writes
events to sinks only. `ProofFailure` and `PermissionsTooOpen` are on the alert policy's immediate list,
so they alert once delivery exists.

## Design

| refusal | where | event |
|---|---|---|
| a listener fails the health proof (row 3) | `mail::Squat::check`, once per command, as the warning is | `ProofFailure`, surface `synapsectl/health-proof`, subject and source the announced address |
| any `KeystoreError::PermissionsTooOpen(item)` (rows 6, 7) | `main`, on the error path, before printing it | `PermissionsTooOpen`, surface `synapsectl/files`, subject the item (`"session file"`, `"account key"`, ...) |

Recording at the one error exit covers every owner-only check the CLI reaches, in the keystore and in
`mail.rs` alike, because each one propagates with `?` (none is swallowed).

**Sinks** (spec §2.2: `FileSink` by default everywhere, `StderrSink` by default for CLI commands):
- `StderrSink` always.
- `FileSink` at `<home>/cli-security-events.jsonl`, 4 MiB cap like synapsed's, **only if the home
  itself passes `check_owner_only`**. In a home others can write, an attacker could plant the file name
  as a link to one of the owner's files, and the CLI would append to it. Then the event goes to stderr
  only.
- **Its own file, not synapsed's `security-events.jsonl`:** synapsed holds that file open, and a
  `FileSink` rotates by renaming, which fails on Windows while another process holds the file. The
  mcp-server took its own file for the same reason (P6).
- **Opened lazily:** only when an event occurs, so a clean command creates no file.

**No limiter.** One command records at most two events (one proof failure, one refusal), so a CLI run
is not a flood source. Repeated runs are the caller's own.

**No secret in an event:** the subject is an item name or an address, never a token or a key.

## Tests (red first), in `crates/synapsectl/tests/mail.rs`

1. `a_too_open_session_or_announce_file_is_refused` (row 6), extended: the refusal also writes one
   `PermissionsTooOpen` line with that target's item to `cli-security-events.jsonl`, and stderr has a
   `security: ` line. Red on main: there is no file.
2. `a_squatter_on_a_live_pids_port_gets_no_credential` (row 3), extended: exactly one `ProofFailure`
   line whose subject is the squatted address, and which does not carry the instance id (the proof key).
3. New (row 7): a too-open account key makes `synapse id show` fail **and** writes one
   `PermissionsTooOpen` line with subject `account key`.
4. Positive control: a clean claim and send leaves no `cli-security-events.jsonl`.
5. New: with the home itself too open, the event still reaches stderr, and no event file is created.

## Version

New dependency edge: `synapsectl` → `synapse-security`. **The bump becomes eight places:** add
`synapse-security` in synapsectl to the seven. The PR carries the PM's allocated number.
