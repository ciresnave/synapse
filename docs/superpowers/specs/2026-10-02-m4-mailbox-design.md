# M4: durable mailbox (design)

**Status:** DRAFT for PM approval. Nothing is built.
**Plan:** `docs/superpowers/plans/2026-09-29-claude-peers-replacement.md`, milestone M4. The PM tasked it
on 2026-10-02.
**Measured at:** `origin/main@2e27cde` (5.2.0).
**Store engine (D1, CireSnave verbatim):** *"Redb is fine for now."* The current release is redb 4.3.0
(`cargo search`, 2026-10-02).

## 1. Goal

Mail for a role waits for that role: across session restarts and daemon restarts, and it survives a
crash at any point. This replaces claude-peers' broker table, whose `/poll-messages` marked mail
delivered as it handed it out. A crash between poll and push lost the message silently, and mail to a
peer that restarted was stranded under its old id.

**The semantics are fixed by the plan (§3):**
- delivery is at-least-once;
- `fetch` **leases** messages, `ack` removes them, and an expired lease redelivers;
- consumers dedupe by `message_id`;
- retention deletes **acked** history only, never undelivered mail.

**M3's carry-over:** the role table's state is persisted in the **same store, in the same
transaction** as the epoch it grants. The store writes before it replies, so a crash can never hand
out an epoch it then forgets.

## 2. Shape

- **`src/mailbox.rs` (core, model-agnostic)** holds the semantics behind a `MailStore` trait. There is
  no clock (`now` is passed in) and no network.
- **`MemoryStore`** is an in-memory implementation, used by the semantics tests.
- **`RedbStore`** is the persistent implementation: one redb file, every operation one write
  transaction. It is behind a cargo feature `mailbox-redb`, which is included in `native`, so
  minimal and WASM builds don't pull in redb.
- **The stored unit is an opaque `Envelope`:** `message_id`, `to` (`role@account`), `from`, `body:
  Vec<u8>`, and `enqueued_at`. The daemon (M5) puts a serialized, already signed `SecureMessage` in
  `body`. The mailbox never parses it, so the mailbox doesn't bind to one message format.

## 3. Operations and rules

| op | who | effect |
|---|---|---|
| `enqueue(env, now)` | the daemon, on behalf of a verified sender | appends to `to`'s queue unless `(to, message_id)` is queued or in acked history (**dedupe**, `Duplicate` returned). Refuses with `MailboxFull` past the bound (Q4), and **never drops silently** |
| `fetch(role, epoch, max, now)` | the role's current holder | requires `roles.check(role, epoch)`. Returns up to `max` messages that are unleased or whose lease has expired, oldest first, and leases each to `(epoch, now + LEASE)` |
| `ack(role, epoch, message_id, now)` | the current holder | requires a current epoch, and the message must be leased to this epoch. It moves the message from the queue to acked history. Acking twice gives `Ok` (idempotent); acking an unknown id gives `NotFound` |
| `claim(req, …, now)` | a session | M3's `Roles::claim`, plus persisting `RolesState` **in the same transaction**. **A takeover voids the old epoch's leases immediately** (Q2): those messages are fetchable by the new holder at once |
| `sweep(now)` | the daemon, periodically | deletes acked history older than the retention period (Q3). Undelivered mail is never deleted |

**A superseded epoch** gets `Superseded { current }` from `fetch` and from `ack` (PM ruling M3 Q2).

**Crash safety:** each op is one redb write transaction, committed before returning. A crash before
commit leaves the prior state, and a crash after commit leaves the new state. There is no
in-between, and redb's own guarantee covers the file.

## 4. Questions for the PM

- **Q1. Lease length.** Recommended: **60 s by default, set by the caller per `fetch` within
  [5 s, 15 min].** That's long enough for a model turn, and short enough that a crashed session's
  mail comes back quickly.
- **Q2. What happens to an old holder's leases on takeover?** Recommended: **void them immediately**,
  so a restarted lane gets its in-flight mail at once instead of after the lease runs out. It can't
  cause a double-process, because the old session's `ack` is refused once it's superseded.
- **Q3. Retention of acked history.** Recommended: **7 days**, configurable. That's long enough to
  dedupe a late duplicate and debug "did X arrive?". claude-peers used 90 days, but its table never
  distinguished undelivered from delivered properly.
- **Q4. Bound per role.** Recommended: **10 000 messages or 64 MiB per role, whichever comes first.**
  Past it, `enqueue` refuses with `MailboxFull` and the sender is told. Mail is never dropped silently.
- **Q5. Split into 2 PRs, as the plan sized it.** Recommended:
  - **PR 1:** the semantics, `MemoryStore`, and the tests with injected clocks;
  - **PR 2:** `RedbStore`, persisted roles, and the crash tests.

  Each PR is deployable on its own: PR 1 is a usable library, and PR 2 makes it durable.

## 5. Acceptance

**PR 1:**
- `enqueue` then `fetch` returns the message, and a second `fetch` before the lease expires returns
  nothing.
- After the lease expires, `fetch` returns the message again. That's redelivery.
- `ack` removes the message for good. A re-sent duplicate (the same `message_id`) after the ack gives
  `Duplicate` and is not redelivered.
- A superseded epoch's `fetch`/`ack` gives `Superseded`.
- A takeover makes the old epoch's leased messages fetchable immediately.
- `sweep` deletes acked history older than the retention period, and never queued mail. This is
  tested with a queue holding unacked mail that's older than the retention period.
- At the bound, `MailboxFull` is returned, and nothing already queued is lost.

**PR 2:**
- The same semantic tests run against `RedbStore`, through one shared test suite generic over
  `MailStore`.
- **Kill between steps (real process death):**
  - a child process opens the store, starts the next operation, and calls `std::process::abort()`
    before committing;
  - after it, the parent reopens the store and finds the prior state intact;
  - this is run for enqueue, fetch-lease, ack and claim.
  - **Mutation check:** with commit-before-abort, the parent sees the new state, which proves the
    test can tell the two apart.
- After a reopen, epochs continue: `claim` then reopen then `claim` gives epoch + 1, and the epoch-0
  validation from M3 applies on load.
- The store file is owner-only and checked on open, reusing M2's permission check.

**Both PRs:** no vendor names in core (the M0 guard).

## 6. Size

Estimated at 2 PRs and 1.5–2 lane-days, as in the plan. The crash tests are the long pole.
