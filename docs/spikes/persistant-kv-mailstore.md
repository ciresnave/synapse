# Spike: a `MailStore` over `persistant-kv` 0.4.28

Measured 2026-10-14 at synapse `origin/main` 7c8bbd3 (rc.65), `persistant-kv` / `persistant-blob` 0.4.28 from crates.io.
Not adopted: Synapse stays on redb. The adapter is kept as `persistant-kv-mailstore.rs.txt` (not compiled, no
dependency added); it needs `futures`, `persistant-kv`, `persistant-blob`, `serde_json` and `chrono`.

## What was built

`KvMailStore<K: KvBackend>`: the sync `MailStore` over the async `KvBackend` through `futures::executor::block_on`.
Keys: `m/q/{role}/{seq:020}` (queue entry, JSON `Stored`), `m/i/{role}/{id}` (id -> seq), `m/a/{role}/{id}` (acked
at), `m/seq`, `m/roles`. The `(role, seq)` scan is `ordered_scan` on the `m/q/{role}/` prefix. A write takes a process
mutex and keeps an undo journal that it replays if the closure returns `Err`.

## Result

`synapse::mailbox::conformance::run_all` (17 cases) passes on `MemoryKv` and on `KvOnBlob<MemoryBlob>`. Only
`get`, `put`, `delete` and `ordered_scan` are used.

## What PersistAnt would need

| Gap | Why |
|---|---|
| `kv.batch_atomic` (several keys commit together or not at all) | `MailStore::write` puts the queue entry, index, sequence number and roles in one transaction. The mutex + undo journal is neither crash-safe nor multi-process safe. `KvOnBlob` cannot give it; a relational backend can. |
| Ordered scan that returns values | The scan reads one key per message (N+1). |
| Key scheme | A role containing `/` aliases a prefix. Synapse's role charset would have to be checked or a separator it excludes used. |

PersistAnt's lane replied (2026-10-14 21:28Z) that it is building `KvBackend::batch` with a `Require` precondition and
`ordered_scan_values`, certified on Postgres first. That is their plan, not a measurement.

## What it found in our own suite

Control: with the undo journal removed, `run_all` still **passed**. No case tested that a failed write leaves nothing
behind, so passing the suite was necessary, not sufficient, for the atomic multi-key transaction. The suite now has
`a_failed_write_changes_nothing`.
