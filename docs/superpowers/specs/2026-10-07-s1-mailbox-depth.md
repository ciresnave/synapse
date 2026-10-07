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
  - It works on a poisoned mailbox the same way `list`'s other reads do. **(Check in the plan:** `live()`
    refuses when poisoned. `list` should then answer 503 `store_unavailable`, as other routes do.)
- **Daemon (`synapsed` `list`):**
  - Call `depths(now)` once per request, under the same mailbox lock as `roles_snapshot`, so the epochs
    and counts agree.
  - Add `pending` to each entry.

## Answers to agentlife's questions

1. **Cheap and consistent?**
   - Consistent: yes. Snapshot and counts come from one lock hold and one store transaction.
   - Cheap: one metadata scan per role, with no bodies. The worst case is 10 000 rows per role.
2. **Every session, or only agentlife's?**
   - Every session, recommended.
   - `list` already shows every role to any session of the same user, on loopback only.
   - Counts reveal that mail exists, never its content or sender.
   - Restricting by role name would be the first per-role authorization rule in the daemon. Nothing
     needs it.
3. **M7 / M9 timing:** neither is tasked to this lane yet. That is the PM's to answer.

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
  - Assert through `RedbStore::body_writes`'s sibling counter, or by a test that deletes a body row and
    still gets counts.
  - The plan picks one.
  - Control: `scan` on the same store errors or counts the body read.
- **Daemon (`tests/api.rs`):**
  - `list` shows `pending` for an offline role with mail;
  - it moves from queued to leased after the recipient fetches;
  - all zeros after the ack;
  - a role with no mail shows zeros and `null`.

## Version

Additive public API in core and an additive field in a JSON response. The PM allocates the rc number at
gate time.
