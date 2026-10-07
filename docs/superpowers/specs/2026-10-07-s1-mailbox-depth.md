# S-1: mailbox depth per role in `/v1/list`

From the synapse lane, 2026-10-07. Ask: agentlife `docs/SYNAPSE-REQUIREMENTS.md` S-1 (agentlife
`origin/main`). Evidence: synapse `origin/main` `447e648` (6.0.0-rc.10).

## What agentlife needs

For each role in `/v1/list`, it needs three things:
- how much mail is waiting that nobody holds (`queued`);
- how much is held but not yet acked (`leased`);
- when the oldest unacked message arrived (`oldest_enqueued_at`).

agentlife uses them in two ways. It wakes a lazy agent when `queued > 0 && !online`, and it refuses to
lazy-stop a running agent while `queued + leased > 0`. The data is read-only, and the new field is additive.

## Shape

Each entry in `roles` gains a `pending` object:

```json
{"global_id": "alpha@acct", "epoch": 3, "online": false, "last_seen": null, "summary": null,
 "pending": {"queued": 2, "leased": 1, "oldest_enqueued_at": "2026-10-07T16:00:00+00:00"}}
```

**Definitions** (at `now`, against the role's current epoch `E` from the same role snapshot that `list`
already reads):
- **`leased`:** the role's stored messages whose lease is held by epoch `E` and has not expired
  (`until > now`).
- **`queued`:** every other stored message for the role. That is: no lease; an expired lease; or a lease
  from an older epoch, which a takeover voids. This is exactly the set that `fetch` would hand out next,
  so `queued > 0` means "a fetch now would return mail".
- **`oldest_enqueued_at`:** the minimum `enqueued_at` over all of the role's stored (unacked) messages,
  queued or leased. It is `null` when there are none.
- **A role with no mail** shows `{"queued": 0, "leased": 0, "oldest_enqueued_at": null}`.
- **When the depths cannot be read** (the mailbox is poisoned, or the store fails), `list` still answers
  200, and every role's `pending` is `null`.
  - Consumers fail closed per field: agentlife treats `null` as unknown, and so never wakes or lazy-stops
    on it.
  - This deliberately differs from the mail routes, which answer 503 `store_unavailable`. `list` is also
    agentlife's source of *online* state, which lives in memory and is still correct. A 503 would hide
    every role's presence because one field failed. (PM amendment, 2026-10-07.)
- **Acked history is not counted.**

## Mechanism

- **Core (`synapse::mailbox`):**
  - Add `Mailbox::depths(now) -> Result<BTreeMap<String, Depth>, MailError>`.
  - `Depth { queued: usize, leased: usize, oldest_enqueued_at: Option<DateTime<Utc>> }` is public and
    plain data.
  - It covers every role in the role table.
- **A new `MailTxn::scan_leases`:**
  - It visits `(enqueued_at, Option<Lease>)` per message and **never loads a body**.
  - It has a **default method** built on `scan`, so external `MailTxn` implementors keep compiling.
  - `RedbTxn` overrides it to read only the `META` table.
  - Why: `scan` assembles every body, and a role may hold up to 64 MiB (`MailConfig::max_bytes`). `list`
    is polled every 10 s by agentlife, so loading bodies would turn a status read into a bulk read.
  - Cost: one JSON decode of metadata per stored message, bounded by `max_messages` (10 000) per role.
- **Store access:** the mailbox has only write transactions today. `depths` uses one that writes
  nothing.
  - It makes no state change and sends no event.
  - It refuses with `Poisoned` like every other mailbox call (`live()`). The daemon maps any `Err` to
    `pending: null`, as above.
  - That covers a poisoned *mailbox* and a failed store. A poisoned Rust `Mutex` around the mailbox is a
    different failure: `list` still answers 503 for it, as every route does.
  - Only the redb store skips bodies. `MemoryStore`, which is test-only, uses the default
    `scan_leases`, and its `write` clones its whole state.
- **Daemon (`synapsed` `list`):**
  - Call `depths(now)` once per request, under the same mailbox lock as `roles_snapshot`, so the epochs
    and counts agree.
  - Add `pending` to each entry, or `null` on every entry when `depths` failed. The JSON is built by a pure
  function (roles, presence, `Option<&depths>`, now), so the `null` case is unit-tested without
  poisoning a live daemon.

## Answers to agentlife's questions

1. **Cheap and consistent?**
   - Consistent: yes. Snapshot and counts come from one lock hold and one store transaction.
   - Cost: one metadata scan per role, with no bodies on redb. The worst case is 10 000 JSON-decoded rows
     per role, all under the mailbox mutex, which stalls `send`, `fetch` and `ack` for that long.
   - It runs in a redb write transaction that commits with no changes, because `MailStore` has no read
     transaction.
   - **Unmeasured** at 2026-10-07. If agentlife's 10 s poll shows up in latency, the fix is a read
     transaction on `MailStore`, which would be its own slice. (Review, S-1.)
2. **Every session, or only agentlife's?**
   - Every session, recommended.
   - `list` already shows every role to any session of the same user, on loopback only.
   - Counts reveal that mail exists, never its content or sender.
   - Restricting by role name would be the first per-role authorization rule in the daemon. Nothing
     needs it.
3. **M7 / M9 timing:** there is no date, honestly.
   - Per `2026-09-29-claude-peers-replacement.md`, M7 (the Claude Code channel adapter) needs M6 (the
     adapters), which comes after M5.
   - M9 (the cutover) needs everything. It changes how *every* lane messages, so it needs CireSnave's go.
   - The PM's sequencing for the synapse lane:
     - finish S-1;
     - then the remaining hardening rows;
     - B2 only if board 138 is answered;
     - then an M6/M7 plan, as a plan only (no code), with an estimate to the PM.
   - The PM puts the M9 go/no-go on the board when M7 is ready.

**S-2 (an agentlife role):** a long-lived non-model client claims and heartbeats like any other client.
A session lasts as long as the role's epoch does; there is no token TTL (M5a deferred minor). Its
messages' `from` is the verified role. No change is needed. **S-3 (SSE):** not in this slice.

## Not in this slice

- **`synapsectl list` output:** the JSON passes through, and a human table column can follow later.
- **MCP adapter:** M6 is not tasked.
- **Any wake or notify mechanism.**

## Tests (red first)

- **Core, on `MemoryStore` and `RedbStore` through the shared suite:**
  - a fresh role has zero depth;
  - after enqueue: queued.
  - After a fetch: the count moves from queued to leased.
  - After the lease expires (inject `now`): it is queued again.
  - After a takeover (new epoch): the old lease counts as queued.
  - After an ack: the message is gone from both counts. The oldest entry is the minimum across both
    states.
- **Guard: `RedbTxn::scan_leases` reads no body.**
  - A new `RedbStore::body_reads()` counter, the sibling of `body_writes()`, stays flat across
    `depths`.
  - Control: a `scan` over the same store raises it.
- **Poisoned:**
  - Core: `depths` on a poisoned mailbox is `Err(Poisoned)`. Control: the same mailbox before the failed
    write returns counts.
  - Daemon: the pure builder with `None` gives `pending: null` on every role, with `online` intact.
    Control: with `Some`, it gives the counts.
- **Daemon (`tests/api.rs`):**
  - `list` shows `pending` for an offline role with mail;
  - it moves from queued to leased after the recipient fetches;
  - all zeros after the ack;
  - a role with no mail shows zeros and `null`.

## Version

Additive public API in core and an additive field in a JSON response. The PM allocates the rc number at
gate time.
