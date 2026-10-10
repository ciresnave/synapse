# Plan: channel adapter acks on the model's word, bounded re-push, readiness gate (fix C)

Spec: `docs/superpowers/specs/2026-10-10-channel-ack-and-gate.md` (merged, #115). PM: rc.32 provisional,
confirmed at the gate; PM allocates the number. Crate: `crates/synapse-claude-channel`.

## Task 1: real-client wire log (decides the gate; blocks task 5 only) — DONE, n=1, see task 5

Build `tee_mcp` (release), put it between the real Claude Code and the adapter in the live restarttest
session (OverMind owns the relaunch; PM go required), and record, from the `C>S`/`S>C` lines and the
transcript, the order of: `notifications/initialized`, the first client request after it, and the first
channel event Claude Code shows. The tee has no timestamps; order is what is read. Deliver the order into
the PR body as measured (n stated, log path, which session). Outcome table:

| Observed order | Gate |
|---|---|
| first post-`initialized` request, then channel registration is evidenced by a shown event | keep the spec's gate |
| events pushed after the first request are still dropped | change/drop the gate; KEEP tasks 2-4 (the ack is the correctness fix) |
| the log cannot show registration | say so; do not guess a delay; ask the PM |

Tasks 2-4 and 6 do not depend on this log and proceed first.

## Task 2: red tests for ack-on-model (spec tests 1-3), `tests/channel.rs`

Reuse `Home`, `quick()`, `send`, `Catcher`, and the failing-`Notifier` helper already in the file.
1. After `push_once` with a succeeding notifier, `/v1/list` for the role shows `leased == 1`, `queued == 0`
   (today 0/0, so red).
2. Calling the existing `ack` tool with the pushed `meta.message_id` removes it; a later `push_once` pushes
   nothing.
3. Unacked, it is pushed again after the lease ends (`lease_secs` at its 5 s minimum), same `message_id`.
Run red, read the failing test names, commit nothing yet.

## Task 3: green: stop acking on write

`push_once`: delete the `confirm` call and the `done` ack count; return the number pushed. Doc comment of
`push_once` and the module header say "push, leave leased; the model's `ack` removes it". `confirm` stays
(used by tests/other adapters); do not delete it from `synapse-mcp-server`.
Mutation check: restore the `confirm` after `notify`; predict tests 1 and 2 fail; assert the edit applied
once; `touch` after restore.

## Task 4: bounded re-push (spec tests 4-5)

> **Superseded in rc.33** (spec "rc.33 amendment"): the process-local cap, the batch widening
> (`ledger.capped()`) and "stays pending past the cap" were removed. The adapter now acks a message
> itself after a successful write once the daemon's `attempts` >= `max_pushes` AND this process has
> pushed it twice. What follows is the rc.32 design, kept as history.

- `ChannelConfig.max_pushes: u32` (default 3). Adding a public field is a breaking change to a struct built
  with literals in tests/main: note it, update every literal (`grep -rn "ChannelConfig {"` over `git ls-files`).
- A `PushLedger` (`Mutex<HashMap<String, u32>>`, in memory) passed to `push_once` and owned by the loop.
  Count increments only after a successful `notify` (test 5: a failing notifier does not count).
- At `count >= max_pushes` the message is skipped, stays leased/pending in the daemon.
- **Starvation:** a capped message is still returned by `pull` and takes a batch slot, so `max_pushes`
  capped messages could hide newer mail. Fix: request `batch + ledger.capped()` (saturating; the daemon
  bounds `max`, check `/v1/fetch` max in `synapsed` before relying on it). Test 4 includes "another message
  still is pushed" with `batch = 1` and one capped message, which fails without this.
- Ledger growth: entries carry `last_seen`; one no pull has returned for max(2*lease, 2*poll) is forgotten
  (a leased message is not returned until its lease ends, so "not in this pull" alone must not forget it).
  Unit-tested with synthetic `Instant`s. Known limit: the daemon clamps `max` at 100 (`MAX_FETCH`), so with
  90+ never-acked messages newer mail waits for rotation, and past ~190 entries are forgotten and re-pushed.
Mutation check: remove the cap comparison; predict test 4 fails.

## Task 5: readiness gate (spec test 6) — DEFERRED (PM ruling 2026-10-10 16:43Z)

Task 1 measured `initialized`, `tools/list`, then a shown event (n=1), which does not test the racing case.
Not built in this PR; the c7 re-test with this adapter decides it. What follows is the design if it is built.

Shape if kept: an `AtomicBool ready` on `ChannelServer`, set by the first `list_tools`/`call_tool` after
`on_initialized`; the loop does not `pull` while it is false. Test 6 drives a fake client that sends
`initialized` and waits: nothing leased (`queued` unchanged) until its first request, then leased.
Mutation check: force `ready` true; predict test 6 fails. If task 1 drops the gate, delete this task and
the spec's section 4 gets a one-paragraph note in the PR (docs only).

## Task 6: INSTRUCTIONS and docs (spec test 7)

- `INSTRUCTIONS` gains the trusted ack sentence; a constant-pinning test asserts it (like the server name).
  The sentence is not added to the per-message frame.
- `docs/SOAK_*` procedure: check 3 -> "pushed, then acked by the model"; check 7 and stop condition 2 ->
  "shown more than `max_pushes` times". Find them with `git grep -n "stop condition"` first.
- `docs/CHANGELOG.md` entry; say alert delivery is still only a seam (board 131).

## Task 7: version, gates, review, PR

- Version: PM-allocated (provisionally 6.0.0-rc.32) in the FOURTEEN places (1+3+2+2+1+3+2 per
  `git grep -c 6.0.0-rc.31`); Python byte replace, assert count; changelog.
- `python .github/spdx_gate.py`, `cargo fmt --check`, `cargo clippy --workspace --all-targets --no-deps
  --features auth`, `cargo test -p synapse-claude-channel`; then the workspace tests in the background.
- One fresh final review (model: sonnet, checklist first), then PR; read checks AND `reviewThreads` by
  GraphQL AND annotation text; then `[READY]`. Verify the push with `git ls-remote`.

## Not in this PR

Live push remains uncovered by any CI test (needs CireSnave's dev-channel approval; cutover not approved).
Batching acks; `synapse stats` CLI (separate task); the unfiled #107 minors.
