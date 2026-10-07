# S-1 plan: mailbox depth per role in `/v1/list`

Spec: `docs/superpowers/specs/2026-10-07-s1-mailbox-depth.md` (PM-approved 2026-10-07 with the
`pending: null` amendment). Base: `origin/main` `447e648`. TDD: each step is red first.

## 1. Core: `Depth`, `MailTxn::scan_leases`, `Mailbox::depths` (`src/mailbox.rs`)

**Tests first.** Add to `tests/mailbox_suite/mod.rs`, and register the test in the `cases!` lists of both
`tests/mailbox.rs` and `tests/mailbox_redb.rs`. It is red at first because `depths` does not exist.

- **`depths_count_queued_and_leased_per_role`:**
  - Start with a claimed `lane` that has no mail: zeros, and `oldest = None`.
  - Enqueue m1 at t0 and m2 at t0+1s: queued 2, leased 0, oldest t0.
  - Fetch 1: queued 1, leased 1.
  - Ack m1: queued 1, leased 0, oldest t0+1s.
  - A role that never claimed but has mail still shows its mail only if it is in the role table. `list`
    shows role-table roles only, so `depths` covers exactly those.
- **`an_expired_or_superseded_lease_counts_as_queued`:**
  - Fetch with a 5 s lease. At `now` + 6 s the count is queued again.
  - Fetch again, then a takeover claim: the old epoch's lease counts as queued.

**Implementation.**
- `pub struct Depth { pub queued: usize, pub leased: usize, pub oldest_enqueued_at: Option<DateTime<Utc>> }`.
  It derives `Debug, Clone, Copy, PartialEq, Eq, Default`.
- `MailTxn::scan_leases(&self, role, visit: &mut dyn FnMut(DateTime<Utc>, Option<&Lease>))`. Its default
  method is built on `scan`.
- `Mailbox::depths(&self, now) -> Result<BTreeMap<String, Depth>, MailError>`:
  - call `live()`;
  - one `store.write` that changes nothing;
  - for each role in `self.roles.snapshot()`, `leased` means `lease.epoch == current && lease.until > now`.
    This is the same predicate that `fetch` negates.

## 2. Redb override and the body guard

**Test first** (`tests/mailbox_redb.rs`), named `depths_read_no_body`:
- put 2 messages;
- read `body_reads()`;
- call `depths`: the count is unchanged;
- control: `scan` raises the count by 2.

It is red because `body_reads` does not exist.

**Implementation:**
- `RedbStore.body_reads: AtomicU64`, passed into `RedbTxn` and incremented in `RedbTxn::body`;
- `pub fn body_reads()`;
- `RedbTxn::scan_leases` reads the META range and decodes `Meta`, without calling `body`.

## 3. Poisoned (`tests/mailbox.rs`)

Extend `a_failed_claim_write_poisons…` (it uses a `FailingStore`):
- before the failure, `depths` returns the counts (the control);
- after it, `depths` returns `Err(Poisoned)`.

It is red until `depths` calls `live()`.

## 4. Daemon (`crates/synapsed`)

**Unit test** in `lib.rs` `mod tests`. The test is `list_entries_carry_pending_or_null`:
- With `Some(depths)`, the entry has `pending.queued` and the other fields.
- With `None`, every entry has `pending: null`, while `online`, `epoch` and `summary` are unchanged.

**Integration test** in `tests/api.rs`. The test is `list_shows_mailbox_depth_per_role`:
- alpha and beta claim;
- alpha sends 2 messages to beta;
- beta stays silent: `pending` = {2, 0, some time};
- beta fetches 1: {1, 1};
- beta acks it: {1, 0};
- alpha has no mail: {0, 0, null}.

**Implementation:**
- Pull the JSON mapping out of `list` into `fn list_entries(roles, presence, depths: Option<&BTreeMap<..>>, now) -> Vec<Value>`.
- `list` takes the mailbox lock once, reads `roles_snapshot()` and `depths(now).ok()` under it, then
  builds the response.

## 5. Docs and finish

- Add a `pending` row to the M5 spec's endpoint table: `docs/superpowers/specs/2026-10-02-m5-synapsed-design.md`, line 44.
- Run: `cargo test --workspace` for the mailbox and synapsed targets, clippy `-D warnings` and fmt.
- One fresh review (Sonnet: the diff touches no auth or token code).
- Version: the PM allocates (expected rc.12). Bump all 8 places, push, verify with ls-remote, then
  `[READY]`.
- Tell agentlife through the PM: S-1 is shipped, the field shape, and the `null` semantics.
