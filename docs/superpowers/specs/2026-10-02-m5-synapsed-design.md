# M5: `synapsed` daemon and `synapse` CLI (design)

**Status:** DRAFT for PM approval. Nothing is built.
**Plan:** `docs/superpowers/plans/2026-09-29-claude-peers-replacement.md`, milestone M5. The PM tasked it
on 2026-10-02, with a stop after M5's PR is READY: no M6, no publish, no version bump in the PR.
**Measured at:** `origin/main@71e147b` (5.4.0).
**Builds on:** M2 keystore, M3 roles, M4 mailbox + `RedbStore`. Rulings from DECIDED-ARCHIVE item
82: redb; the name `synapse`; ack on delivery, with an opt-in processed ack.

## 1. Goal

One daemon per user per machine owns the durable mailbox and serves it on loopback. Any process in
any language, Claude or not, can then:
- claim a role;
- send, fetch and ack mail;
- heartbeat;
- list who is around;

all with zero config. The `synapse` CLI is the plain adapter (c), and M6/M7 build their adapters on
the same API.

## 2. Shape

```
crates/synapsed/      binary `synapsed` + lib: axum server on 127.0.0.1:<port> over Mailbox<RedbStore>
crates/synapse-cli/   binary `synapse`: existing `id` commands + new mail commands; a small client
                      that auto-starts `synapsed` if nothing answers
```

- **The core is unchanged.** M0's guard keeps it vendor-free, and the daemon is model-agnostic too.
- **Store:** `<SYNAPSE_HOME>/mailbox.redb` (M2's home). It is owner-only, checked on open. redb's
  file lock makes the daemon single-instance: a second daemon fails to open the store and exits.

## 3. API (loopback JSON over HTTP/1.1)

| method | path | auth | body → reply |
|---|---|---|---|
| POST | `/v1/claim` | none (the request is self-proving) | `ClaimRequest` (M3) → `{global_id, epoch, session}` |
| POST | `/v1/send` | session | `{to, message_id?, body_b64}` → `{message_id, outcome: "queued" \| "duplicate"}` |
| POST | `/v1/fetch` | session | `{max?, lease_secs?}` → `{messages: [{message_id, from, body_b64, enqueued_at, lease_until, attempts}]}` |
| POST | `/v1/ack` | session | `{message_id}` → `{outcome: "removed" \| "already_acked"}` |
| POST | `/v1/heartbeat` | session | `{summary?}` → `{}` |
| GET | `/v1/list` | session | → `{roles: [{global_id, epoch, online, last_seen, summary}]}` |
| GET | `/v1/health` | none | → `{ok: true, version}` |

**Behaviour:**
- **Sender identity:** `send`'s `from` is the session's verified role, never anything the client
  says. That fixes claude-peers' unchecked `from_id`.
- **Errors** are JSON `{error: <kind>, message}`, with HTTP 409 for `superseded` (carrying the
  `current` epoch), 413 for `mailbox_full`, 400 for `malformed`, and 401 for a bad or absent session.
- **Bodies** are opaque bytes (base64 in JSON) and land in `Envelope.body` unchanged.
- **Sweep:** the daemon runs `sweep` hourly (an M4 obligation).
- **`now`:** the daemon's clock, taken once per request. The core stays clock-free.

## 4. Questions for the PM

- **Q1. Address.** Recommended: **`127.0.0.1:7920`**, overridable by `SYNAPSE_ADDR`. That avoids
  claude-peers on 7899 and FAM on 7900/7910 during side-by-side running. It binds loopback only and
  refuses a non-loopback `SYNAPSE_ADDR`.
- **Q2. Per-request auth.**
  - **Recommended: a session token issued by `/v1/claim`.** The token is 32 random bytes, tied to
    `(role, epoch)` and held only in daemon memory. Every call sends `Authorization: Bearer <hex>`.
    A token for a superseded epoch gets `superseded`, and a daemon restart invalidates every token,
    so clients re-claim (as M3's restart rule already requires).
  - **The alternative:** sign every request with the role key. That is stronger against a local
    process that can read another process's memory but not its key file. It costs a signature per
    call and is harder for non-Rust clients.
  - On a single-user loopback, holding the key file and holding the token are equivalent, because
    either one can claim.
- **Q3. Presence.** Recommended: **in memory, not durable.**
  - A role is `online` if it heartbeated (or made any authenticated call) within 90 s; clients beat
    every 30 s.
  - The summary is the last one sent.
  - `list` shows every role in the persisted `RolesState` (offline ones too, unlike claude-peers),
    plus presence when known.
  - After a daemon restart everyone shows offline until their next call.
- **Q4. Auto-start (zero-config).** Recommended:
  - If `/v1/health` doesn't answer, the CLI spawns `synapsed` detached: `CREATE_NO_WINDOW |
    DETACHED_PROCESS` on Windows, `setsid` on Unix, with stdio null.
  - It then polls health for up to 5 s.
  - Two CLIs racing both spawn, and the loser's daemon exits on the store lock. That is the same
    singleton pattern as claude-peers' broker, in Rust.
  - `synapsed` is found next to the `synapse` binary, else on PATH.
- **Q5. PR split.** The plan sized M5 at 2–3 PRs. Recommended: **two PRs, delivered one at a time.**
  - **M5a:** the daemon: the API above, auth, presence, the sweep timer, and integration tests that
    drive it over real loopback HTTP.
  - **M5b:** the CLI: `synapse claim|send|inbox|ack|list` and auto-start.

  The [TASK]'s stop condition is "after M5's PR is READY". Please confirm whether that means after
  M5b (the full milestone), or stop after M5a for review.

## 5. Acceptance

**M5a:**
- Every endpoint is tested over real HTTP against a temp home: claim, send, fetch, ack, heartbeat,
  list and health.
- `from` is always the session's role, and a forged `from` field in the body is ignored or refused.
- A superseded session gets 409 with `current`.
- A missing or bad token gets 401.
- Every API response is checked for leaked tokens and key material.
- Loopback only: a non-loopback `SYNAPSE_ADDR` is refused.
- A second daemon over the same home exits on the store lock.

**M5b:**
- A CLI run against a stopped daemon auto-starts it, and the command succeeds.
- Two concurrent first commands leave exactly one daemon.
- A round trip between two roles via the CLI works.

**Both:** the M0 guard stays green, and full suite, clippy and fmt are clean.

## 6. Size

Estimated at 2 PRs and about 2 lane-days, in line with the plan's 1.5–2. The long poles are
Windows detach for auto-start (M5b) and the HTTP integration tests (M5a).
