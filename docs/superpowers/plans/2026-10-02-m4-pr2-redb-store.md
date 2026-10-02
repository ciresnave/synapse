# M4 PR 2: RedbStore, persistence, and crash tests (implementation plan)

> **For agentic workers:** REQUIRED SUB-SKILL: use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task by task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** a durable `MailStore` on redb 4.3.0. It passes the same 14-case suite as `MemoryStore`.
The role epochs, the queue and acked history survive a reopen. A real process death inside a
transaction leaves the prior state, which is shown by a commit-before-abort control. The store file
is owner-only.

**Spec:** `docs/superpowers/specs/2026-10-02-m4-mailbox-design.md` (§5, PR 2 acceptance). The PM
approved Q1–Q5 on 2026-10-02. **D1, CireSnave verbatim:** *"Redb is fine for now."*
**Constraints carried from PR 1's review (ledger):**
- an `(at, role, id)` index for `sweep_acked`;
- lease metadata apart from bodies;
- a persisted `seq` counter written in the same transaction.

## Global constraints

- **Toolchain:** stable, via PowerShell, with `cargo --version` in logs.
- **Version:** no bump; the PM allocates it.
- **Files:** SPDX headers on every new file.
- **Model-agnostic:** no vendor names in core.
- **Feature `mailbox-redb = ["dep:redb"]`**, included in `native`. `RedbStore` is
  `#[cfg(feature = "mailbox-redb")]`.
- **The store file is `<path>` as given by the caller** (M5 chooses `<SYNAPSE_HOME>/mailbox.redb`).
  - **Created owner-only:** `0600` on Unix; Windows inherits the user profile's ACL.
  - **Checked on open** with M2's `keystore::check_owner_only`, which becomes `pub(crate)`.
- **Every `MailStore::write`** is exactly one redb `WriteTransaction`. It commits only if the closure
  returns `Ok`, and otherwise drops, which aborts.

## Table layout (redb `TableDefinition`s; values are serde_json bytes unless noted)

| table | key | value |
|---|---|---|
| `META` | `(role: &str, seq: u64)` | `Meta { message_id, from, enqueued_at, lease, attempts, body_len }` |
| `BODIES` | `(role, seq)` | raw body `&[u8]`, written once, never rewritten |
| `BY_ID` | `(role, message_id)` | `seq: u64` |
| `ACKED` | `(role, message_id)` | `at` (i64 unix micros) |
| `ACKED_BY_TIME` | `(at i64 micros, role, message_id)` | `()` (an empty `&[u8]`) |
| `STATS` | `role` | `(count u64, bytes u64)` |
| `COUNTERS` | `"seq"` | `u64` |
| `ROLES` | `"state"` | serde_json `RolesState` |

**How each `MailTxn` op maps:**
- `put`: an insert or an update via `BY_ID`. A new insert writes `BODIES` and updates `STATS`; an
  update rewrites `META` only.
- `remove`: deletes `META`, `BODIES` and `BY_ID`, and adjusts `STATS`.
- `scan`: a `META` range over `(role, 0..=MAX)`, reading the body of each visited row and stopping
  when the visitor returns false.
- `record_ack`: writes both `ACKED` and `ACKED_BY_TIME`.
- `sweep_acked`: a range over `ACKED_BY_TIME` up to `before`, deleting both entries.

## Review focus

1. **No partial commit.** A closure `Err` (or a panic) must leave the file unchanged, so no `commit()`
   happens on any error path.
2. **`STATS` must agree with the queue** after any mix of put, update, remove and failed
   transactions. A test cross-checks `stats()` against a `scan()` count and sum.
3. **`BODIES` must never be rewritten by a lease update.** The test counts writes via a debug
   counter, or checks that the body bytes are unchanged and `META` changed.
4. **The crash test must be able to tell commit from no-commit.** The control (commit, then abort)
   must show the new state, so the test isn't vacuous.
5. **Epochs persist:** a reopen continues the epochs, and an epoch-0 record on disk is refused by
   M3's `restore` validation.

---

### Task 1: RedbStore and the suite against it

**Files:**
- Modify `src/mailbox.rs` (add `RedbStore`).
- Modify `Cargo.toml` (redb dependency and feature).
- Modify `src/keystore.rs` (`check_owner_only` becomes `pub(crate)`).
- Test: `tests/mailbox_redb.rs`.

**Interfaces:**
`pub struct RedbStore;`
`impl RedbStore { pub fn open(path: &Path) -> Result<Self, StoreError> }` (creates or opens, then
checks permissions) and `impl MailStore for RedbStore`.

- [ ] **Step 1: Write the failing tests.**
  - `tests/mailbox_redb.rs` (`#![cfg(feature = "mailbox-redb")]`) runs all 14 suite cases with
    `|| RedbStore::open(&fresh_temp_path()).unwrap()`, using the `cases!` macro.
  - Plus:
    - `a_reopen_keeps_queue_acks_and_epochs`
    - `stats_agree_with_a_scan_after_mixed_operations`
    - `a_lease_update_does_not_rewrite_the_body`
    - `a_failed_write_leaves_the_file_unchanged`
    - `the_store_file_is_owner_only` (on Unix: mode 0600 and refusal of 0644; on Windows: an
      icacls Everyone:R grant makes `open` fail)
- [ ] **Step 2: Run.** Expected: a compile FAIL (no `RedbStore`).
- [ ] **Step 3: Implement** the table layout above.
- [ ] **Step 4: Run.** Expected: PASS.
- [ ] **Step 5: Commit** `feat(mailbox): RedbStore -- the durable MailStore on redb 4.3 (M4 PR2)`.

### Task 2: Crash tests (real process death)

**Files:** `tests/mailbox_crash.rs` (`#![cfg(feature = "mailbox-redb")]`).

**Mechanism:**
- The test re-runs its own binary (`std::env::current_exe()`) with
  `--exact child_entry --nocapture --test-threads=1`.
- It passes `SYNAPSE_CRASH_DB` (the path) and `SYNAPSE_CRASH_OP` (the operation) in the environment.
- `child_entry` returns immediately when `SYNAPSE_CRASH_OP` is unset, so it's a no-op in normal runs.
  When it is set:
  1. open a `Mailbox` over a `CrashingStore(RedbStore)` whose `write` calls the inner `write` with a
     wrapper closure that runs `f` and then calls `std::process::abort()`, i.e. inside the transaction,
     before commit;
  2. run the named operation.
- The control op `"commit-then-abort"` performs a normal enqueue, which commits, and then aborts.

- [ ] **Step 1: Write the tests:**
  - `a_death_mid_enqueue_leaves_no_message`
  - `a_death_mid_fetch_leaves_no_lease`
  - `a_death_mid_ack_leaves_the_message_queued`
  - `a_death_mid_claim_leaves_the_old_epoch`
  - `the_control_commit_then_abort_keeps_the_write`

  The parent sets up the prior state with a normal `Mailbox`, closes it, spawns the child, asserts
  the child died abnormally (`!status.success()`), reopens, and asserts.
- [ ] **Step 2: Run.** Before `CrashingStore` exists the tests can't compile, so RED here is the
  control. Temporarily make the crash wrapper commit before aborting, and see the four death tests
  FAIL (they find the new state) while the control passes. That proves they can tell the two apart.
  Ledger it.
- [ ] **Step 3: Restore** abort-before-commit and run. Expected: all 5 PASS.
- [ ] **Step 4:** The full workspace suite, clippy, fmt, and the M0 guard.
- [ ] **Step 5: Commit** `test(mailbox): process-death crash tests for redb (M4 PR2)`.

### After the tasks

- One Sonnet final reviewer with this Review Focus.
- Then push, open the PR (base main, rebased after #69 if it has merged), and send `[READY]`.
