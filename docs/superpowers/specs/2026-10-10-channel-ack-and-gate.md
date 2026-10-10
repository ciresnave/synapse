# Channel adapter: stop acking on write, ack on the model's word (fix C)

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
Delivery becomes at-least-once: a message the session never showed, or never acked, comes back when
its lease ends (30 s today) or at once if a relaunch claims a new epoch.

**2. The server says so.** `INSTRUCTIONS` gain: after reading each `<channel source="synapse">` event,
call `ack` with its `meta.message_id`; a message you do not ack arrives again. This is trusted server
text, not message text. A message body can still never instruct the model (the untrusted frame is
unchanged), and the ack instruction is not repeated inside the frame.

**3. Re-push bound.** The adapter remembers, in memory, how many times it has pushed each
`message_id` and stops pushing one after `max_pushes` (default 3). The message stays leased/queued in
the daemon (visible in `synapse list` as pending), so a session that never acks cannot make the
adapter push the same text forever, and nothing is silently dropped. An adapter restart forgets the
count; that costs at most `max_pushes` more pushes.

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

## What this does not fix

- A model that never calls `ack` leaves mail pending forever (bounded push count, 3). That is visible, not
  lost. Whether models ack reliably is measured in the live test, not assumed.
- Shown twice: possible when a lease ends before the model acks. The `message_id` lets the model tell.
  Soak stop condition 2 (shown twice with no `ack failed`) must be re-worded: a redelivery after an
  un-acked lease is now expected, so the check becomes "shown more than `max_pushes` times".
- `ack` is now a model action: it costs a tool call per message. Batching is out of scope.

## Tests (red first)

1. `push_once` with a succeeding notifier leaves `pending.leased == 1`, `queued == 0` (today: 0/0).
2. After the `ack` tool is called with the pushed `meta.message_id`, the message is gone and is not
   pushed again.
3. A message pushed and not acked is pushed again after its lease ends, with the same `message_id`.
4. A message pushed `max_pushes` times is not pushed again and stays pending; another message still is.
5. A failing notifier does not count toward `max_pushes` (the write never happened).
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
