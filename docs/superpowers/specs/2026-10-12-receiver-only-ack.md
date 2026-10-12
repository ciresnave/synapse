# Receiver-only ack as the generic default, and a fetch that passes over named messages (PR 4)

**Status:** implemented in the same PR; written for review. PM rulings 2026-10-12: *"AckPolicy::ReceiverOnly is
the generic default; BoundedAttempts only for the Claude renderer (rc.33 stays for Claude). PR4 ... may include the
daemon-side peek/parked state you found; flag the daemon change in its [READY]."*
**Parent:** `2026-10-12-agent-facing-mcp-adapter.md` sections 3 (row 4a), 4.2 and 5.4.

## The problem

FAM's trap 4a: an MCP notification has no acknowledgement, so a write that succeeded means "left the pipe", not
"read". The Claude Code renderer acks after bounded attempts because 0 of 5 measured trials ever called `ack`
(rc.33). That is a measured trade for one client. For a client nobody has measured, removing the message on the
adapter's word is FAM's measured loss: a client that registers no handler consumes its own backlog silently.

Not removing is not enough on its own. The push loop leases a message to read it, so a message it has stopped
pushing would still be re-leased every tick: hidden from the receiver's own `fetch` for `lease_secs` and
occupying a batch slot that newer mail needs.

## Design

1. **`AckPolicy::ReceiverOnly`**, and the `Renderer::ack_policy` default. After `max_pushes` successful writes of
   a message by this process the adapter stops pushing it; it never acks.
2. **The loop asks the daemon to pass over what it has parked.** `/v1/fetch` takes an optional
   `exclude: [message_id]` (at most 100 ids, each at most 256 bytes; otherwise 400 `malformed`). An excluded message
   is not leased, its `attempts` does not move and it does not count toward `max`. It is purely a selection filter:
   **no store format change, no migration.** The core gains `Mailbox::fetch_excluding`; `fetch` is unchanged and
   calls it with nothing excluded.
3. **The parked list is the adapter's, in memory**, at most 100, oldest dropped first. A new adapter process starts
   empty and pushes each pending message up to the bound again. That is bounded and visible (same `message_id`),
   and it is the price of keeping no new state in the daemon.
4. **`exclude` is sent only when non-empty**, so an adapter on `BoundedAttempts` (the Claude renderer) never sends it
   and works against a daemon that predates it. An adapter on `ReceiverOnly` against an old daemon gets 400 on the
   first look after parking (the old body type denies unknown fields). It then clears its parked list, so the next
   look carries no `exclude` and push keeps working (messages are pushed up to the bound again). The same clearing
   happens on any other refused look (daemon down); the cost is bounded repeats, never a loss.

## What is NOT changed

- `BoundedAttempts` and rc.33 behaviour for Claude Code: the soak currently running measures exactly that.
- The `ack` tool, leases, epochs and acked-history retention.
- Out-of-band advertisement of the push method in `whoami` (note section 3, row 2): not in this PR.

## Tests (red first)

- Suite (memory and redb stores): `fetch_excluding_skips_the_named_messages_and_leaves_them_unleased`,
  `fetch_excluding_counts_delivered_messages_not_skipped_ones`.
- Daemon: `fetch_can_exclude_messages_it_would_otherwise_lease` (including the three 400 cases).
- Adapter: the trait default is `ReceiverOnly`; a renderer on the default is pushed exactly `max_pushes` times, then
  never again, and the message is still pending and fetchable by the receiver.

## Known limits

- A parked message is pushed again by each new adapter process (up to the bound each).
- `max_pushes` below 2 acts as 2, as for `BoundedAttempts`: the first write can precede the client's registration.
- redb loads the body of each excluded row before the filter sees it (at most 100 per look), so the `scan_free`
  property "a skipped row's body is never loaded" does not hold for excluded ids.
- More than 100 parked messages: the oldest are pushed again.
- A client that does read the push and never acks leaves the message pending until something acks it. That is the
  intended contract of `ReceiverOnly`; use `BoundedAttempts` only with a measurement.
- Alert delivery is still only a seam (`AlertTransport` has no implementation, board 131).
