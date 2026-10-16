# Spike 2: a `MailStore` over `persistant-kv` 0.4.32 `batch` + `ordered_scan_values`

Measured 2026-10-16 at synapse `origin/main` 102d24f (rc.90), `persistant-kv` / `persistant-blob` 0.4.32 from crates.io.
Not adopted: Synapse stays on redb. Follows [spike 1](persistant-kv-mailstore.md) (0.4.28), which named two gaps.
The adapter is `persistant-kv-mailstore-batch.rs.txt` (not compiled in-tree; built in a scratch crate with the
`mailbox-conformance` feature and `futures`, `persistant-kv`, `persistant-blob`, `serde_json`, `chrono`).

## What changed from spike 1

No process mutex, no undo journal. A write is optimistic: reads are recorded (`seen`), writes buffered (`pending`,
read-your-writes), and commit is one `batch([Require(key, value-read)..., Put/Delete...])`. `Ok(false)` re-runs the
closure (up to 1000 times). A scan is `ordered_scan_values` pages overlaid with the transaction's own writes, so the N+1
read is gone. A blind overwrite also records the key's prior value, so two writers of the same key conflict.

## Results

| Run | Result |
|---|---|
| `synapse::mailbox::conformance::run_all` (includes `a_failed_write_changes_nothing`) on `MemoryKv` | passes |
| the same on `KvOnBlob<MemoryBlob>` | fails at the first case: `kv backend does not support: batch` (as 0.4.32's docs say: `kv.batch_atomic` is not offered on a blob store) |
| 4 threads x 25 `next_seq` writes on one `MemoryKv`, 1 ms between read and commit | 0 duplicate sequence numbers of 100 |
| same, **mutant: commit without the `Require`s** | 74 duplicates of 100 |

The mutant is the control for the concurrency row: the conformance suite alone cannot see a missing `Require`
(single-threaded), and the mutant shows the thread test can. Without the 1 ms window the mutant was NOT caught (0
duplicates): `MemoryKv` is too fast for the race to occur, so a green concurrency run without a widened window
proves nothing.

## What this does and does not show

- Shown: the two 0.4.28 gaps (`batch`, `ordered_scan_values`) are filled on `MemoryKv`, and the mailbox transaction
  maps onto them with no lock outside the store.
- Not shown: any durable backend. `KvOnRelational` (PostgreSQL) is the only 0.4.32 backend that offers `batch`, and no
  Postgres was run here, so crash safety and cross-process safety are unmeasured. `MemoryKv` is single-process.
- Not covered by `Require`: a phantom (a key inserted under a scanned prefix after the scan). Sequence allocation
  (`m/seq`) is read by every enqueue, so enqueues serialise on it; a reader-only scan inside a write could miss an
  insert. Whether `MailStore` callers depend on that is not checked.
- Cost: every key read in a write becomes a `Require`, so a `scan` over a big queue inside a write sends the whole
  queue back as preconditions. Not measured; redb's single-writer transaction does not pay it.
