# Channel adapter: stop acking on write, ack on the model's word (fix C; amended in rc.33)

> **rc.33: the "model's word" half did not hold (0 of 5 unprimed trials acked). The adapter now acks
> after bounded delivery attempts; read "rc.33 amendment" below before the rc.32 sections.**

From the synapse lane, 2026-10-10. PM ruling 2026-10-10 14:28Z: *"fix C stands as ruled: option C =
readiness gate + at-least-once with model-side ack; implement it."* Evidence: `origin/main` `8551289`
(6.0.0-rc.30), `docs/SOAK_PHASE1_RESULT_2026-10-09.md` check 7.

## The problem

`push_once` (crates/synapse-claude-channel/src/lib.rs) leases a message, writes the channel
notification, and on a successful **transport write** calls `confirm` (ack). A successful write says
the bytes left the adapter. It does not say Claude Code displayed the event. Check 7 measured the gap
once (n=1): `p1-0003` was queued while the session was down, the new session's adapter pushed and
acked it, `/v1/stats` counted it acked, and the transcript has no event for it. The next message was
shown. Cause not established; one candidate, unverified, is that Claude Code drops a channel
notification that arrives before it has registered the channel.

Whatever the cause, the adapter cannot know from the write that the session saw the message, so it
must not remove the message on the write.

## Design

**1. No ack on write.** `push_once` pushes and leaves the message leased. The message is removed only
when the model calls the existing `ack` tool with that `message_id` (the id in the event's `meta`).
A message the session never acked comes back when its lease ends (30 s today) or at once if a
relaunch claims a new epoch. **rc.33 correction: this is NOT at-least-once.** Claude Code gives no
confirmation that an event was shown, and the adapter removes a message itself after a bounded number
of attempts (see "rc.33 amendment"), so a message whose every push was dropped is lost.

**2. The server says so.** `INSTRUCTIONS` gain: after reading each `<channel source="synapse">` event,
call `ack` with its `meta.message_id`; a message you do not ack arrives again (rc.33 text: the same
message can arrive more than once with the same `meta.message_id`, act on it ONCE, and an unacked one is
repeated a few times and then dropped). This is trusted server text, not message text. A message body can still never instruct the model (the untrusted frame is
unchanged), and the ack instruction is not repeated inside the frame.

**3. Re-push bound (rc.32; REPLACED in rc.33, see the amendment).** rc.32 kept the push count in the
adapter's memory and stopped pushing a message after `max_pushes` (default 3), leaving it pending in
the daemon. Measured on rc.32 (n=5 unprimed trials, 2026-10-10): the model never acked, so every
message sat pending, and an unacked message was redelivered to every new session with the count reset.

**4. Readiness gate.** The loop does not lease before the session has shown it is up. The gate is the
**first MCP request the client makes after `initialized`** (expected: `tools/list`). Rationale: it is the
earliest signal on the wire that Claude Code has finished wiring the server. **Not known:** whether
that precedes channel registration. The plan's first task takes the wire log with the real client
(`tee_mcp`, repeatable probe from #108) and records the order of `initialized`, the first request, and
the first accepted channel event. If the first request does not come before registration, the gate
changes or is dropped (the ack in 1 is the correctness fix; the gate only avoids a wasted push). I stop
and ask rather than guess a delay.

> **2026-10-10 update (implementation PR, rc.32): the readiness gate (section 4) is DEFERRED, not shipped.**
> Sections 1-3 shipped. The wire log (Claude Code 2.1.296 via `tee_mcp`, n=1, rc.29 adapter) read
> `initialize`, result, `notifications/initialized`, `tools/list`, result, then a channel event that was
> shown. It does not test an event racing registration, so the gate stays unproven; the c7 re-test (queued
> while down, then relaunch) with the new adapter decides whether to build it.

## rc.33 amendment: the adapter acks after bounded attempts (PM 2026-10-10 18:04Z, 18:05Z)

**What this is: bounded delivery ATTEMPTS. There is no display confirmation, and it is NOT
at-least-once.** Evidence (rc.32 adapter, Claude Code 2.1.296, claude-sonnet-5-5, n=5 unprimed plain
messages, one wording): every message was shown and pushed 3 times, the model called `ack` 0 times
(each time it judged the message informational or untrusted), and an unacked message was redelivered
to the next fresh session with its push count reset. The c7 case (a message pushed before the client
registered the channel, lost: FAIL n=1 on rc.29) is the loss this design still has to cover.

**Rule.** After a SUCCESSFUL write, the adapter acks the message itself when both hold:
(daemon `attempts` >= `max_pushes`, default 3) AND (this process has itself made >= 2 successful pushes
of that `message_id`). `attempts` is the daemon's per-message lease counter, returned on every fetched
message and kept across sessions and adapter restarts, so no daemon change was needed. The second
clause means a fresh session never removes a message after a single push, so the c7 case is
re-pushed ~`lease_secs` later. A failed write does not count toward the two. The `ack` tool stays as an
optional early removal.

**Bound.** Pushes per message across sessions are at most `max(max_pushes, attempts_at_first_pull + 2)`,
not 3. **Loss window.** A message is lost only if every push in that bound was dropped (about
`lease_secs` apart, 30 s by default, so roughly 30-60 s for a message that was fresh) or if the daemon's
attempts was already spent by sessions that never wrote it (`attempts` counts leases, not writes: a
session that leased and died, or a failed write, still uses one; one successful write always precedes
any removal). A daemon that sent no `attempts` falls back to this process's push count.

**Repeats.** The same message arrives up to that many times with the same `meta.message_id`. The server
`INSTRUCTIONS` say so and tell the model to act on it ONCE and ignore a repeat of an id it has handled;
acting three times on one request is the new hazard, and the instruction is the only guard.

**Tests** (real `synapsed`, `crates/synapse-claude-channel/tests/channel.rs`): the adapter acks at the
third attempt and every push carries the same id; a message whose leases earlier sessions spent is
pushed twice before the ack; a failed write does not count toward the two. Mutations: remove the final
ack (pending forever: the three tests fail); ack at the first push (the second test fails); count a
failed write (the third fails). Unit tests pin `should_ack`.

## What this does not fix

- rc.32 text, SUPERSEDED: "a model that never calls `ack` leaves mail pending forever". Measured: models
  did not ack (0 of 5 unprimed trials, one model/client/wording). rc.33 removes the message after
  bounded attempts instead.
- Shown more than once: expected. The `message_id` lets the model tell. Soak stop condition 2 is
  re-worded in rc.33 to the bound `max(max_pushes, attempts_at_first_pull + 2)`.
- `ack` is now a model action: it costs a tool call per message. Batching is out of scope.

## Tests (red first)

1. `push_once` with a succeeding notifier leaves `pending.leased == 1`, `queued == 0` (today: 0/0).
2. After the `ack` tool is called with the pushed `meta.message_id`, the message is gone and is not
   pushed again.
3. A message pushed and not acked is pushed again after its lease ends, with the same `message_id`.
4. (rc.32, superseded in rc.33: the adapter now acks the message after bounded attempts, see the
   amendment.) A message pushed `max_pushes` times is not pushed again and stays pending.
5. A failing notifier does not count toward `max_pushes` (the write never happened). (rc.33: it does
   not count toward the two pushes the ack needs; it still consumes a daemon attempt.)
6. The gate: no lease before the first post-`initialized` request; leases after it.
7. `INSTRUCTIONS` contains the ack sentence (a constant pinned by a test, like the server name).

Mutation checks (each asserts the mutation applied exactly once, predicts which tests fail): restore
the `confirm` call after notify (1 and 2 fail); remove the push-count check (4 fails); remove the gate
(6 fails).

## Version and docs

Rides the implementation PR: one version bump, number allocated by the PM. The soak procedure's
checks 3 and 7 and the stop conditions are updated in the same PR (check 3's "pushed, then acked" is
now "pushed, then acked by the model"). Alert delivery is still only a seam (`AlertTransport` has no
implementation, board 131).

## Questions for the PM

1. `max_pushes = 3` and lease 30 s: acceptable, or longer lease (60 s) to cut duplicates?
2. Gate signal: OK to decide it from the real-client wire log in the plan's first task?
