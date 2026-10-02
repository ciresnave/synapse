# M4 PR 1: mailbox semantics and MemoryStore (implementation plan)

> **For agentic workers:** REQUIRED SUB-SKILL: use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task by task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `src/mailbox.rs`, at-least-once delivery for roles. `enqueue` dedupes and refuses past a
bound, `fetch` leases, `ack` removes, an expired lease redelivers, a takeover voids the old leases,
and `sweep` deletes acked history only. It runs over a `MailStore` whose every call is one atomic
write transaction. PR 1 ships `MemoryStore`; PR 2 adds `RedbStore`.

**Architecture:**
- Semantics live in `Mailbox<S: MailStore>`. `Roles` (M3) lives in the mailbox's memory; its
  `RolesState` is written through the store in the same call that grants an epoch.
- A failed store write after a grant **poisons** the mailbox. Every later call returns
  `Poisoned` until it is reopened, so an epoch the store didn't record can never be acted on.

**Spec:** `docs/superpowers/specs/2026-10-02-m4-mailbox-design.md`. The PM approved Q1–Q5 as
recommended on 2026-10-02.

## Global constraints

- **Toolchain:** stable, via PowerShell, with `cargo --version` in logs. No version bump (the PM
  allocates). SPDX headers on every new file. No vendor names in core.
- **Approved defaults (`MailConfig::default()`):**
  - `default_lease` 60 s, `min_lease` 5 s, `max_lease` 15 min;
  - `retention` 7 days;
  - `max_messages` 10 000 per role, `max_bytes` 64 MiB per role (envelope body lengths).
- **Never drop undelivered mail:** the sweep touches the acked-history table only.
- **Dedupe key:** `(to, message_id)`, against both the queue and acked history.

## Review focus

1. **A store error mid-operation must leave the prior state.** `MemoryStore::write` applies the
   closure to a clone and swaps it in only on `Ok`.
2. **A takeover voids old leases:** a message leased to epoch 1 is fetchable by epoch 2 at once.
3. **Ack authority:** only the epoch holding the lease may ack. A superseded epoch gets `Superseded`
   before the store is touched.
4. **The bound counts bytes as well as messages.** An enqueue that would cross either one is refused,
   and nothing already queued changes.
5. **Ordering:** `fetch` returns the oldest first (by enqueue sequence), and redelivery keeps the
   original order.

---

### Task 1: Types, the store trait, MemoryStore, and the full semantics

**Files:**
- Create `src/mailbox.rs`.
- Modify `src/lib.rs` (add `pub mod mailbox;`).
- Tests: `tests/mailbox_suite/mod.rs` (a generic suite), `tests/mailbox.rs` (instantiates it for
  `MemoryStore`), and `tests/common_roles/mod.rs` (the keystore and claim fixtures).

**Interfaces (produced; PR 2 consumes `MailStore` and `MailTxn`):**
```rust
pub struct MailConfig { pub default_lease: Duration, pub min_lease: Duration, pub max_lease: Duration,
                        pub retention: Duration, pub max_messages: usize, pub max_bytes: u64 }
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Envelope { pub message_id: String, pub to: String, pub from: String, pub body: Vec<u8> }
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lease { pub epoch: u64, pub until: DateTime<Utc> }
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stored { pub envelope: Envelope, pub seq: u64, pub enqueued_at: DateTime<Utc>,
                    pub lease: Option<Lease>, pub attempts: u32 }
pub struct Delivery { pub envelope: Envelope, pub enqueued_at: DateTime<Utc>, pub lease_until: DateTime<Utc>, pub attempts: u32 }
pub enum Enqueued { Queued, Duplicate }
pub enum Acked { Removed, AlreadyAcked }
pub struct StoreError(pub String);      // Display: "mail store: {0}" (never message bodies)
pub enum MailError { Superseded(Superseded), MailboxFull, NotFound, NotYourLease, LeaseOutOfRange,
                     Claim(ClaimError), InvalidState(InvalidState), Store(StoreError), Poisoned }
pub trait MailTxn {
    fn queue(&self, role: &str) -> Result<Vec<Stored>, StoreError>;           // ascending seq
    fn queued(&self, role: &str, id: &str) -> Result<Option<Stored>, StoreError>;
    fn put(&mut self, role: &str, stored: Stored) -> Result<(), StoreError>;  // insert or replace by id
    fn remove(&mut self, role: &str, id: &str) -> Result<(), StoreError>;
    fn acked_at(&self, role: &str, id: &str) -> Result<Option<DateTime<Utc>>, StoreError>;
    fn record_ack(&mut self, role: &str, id: &str, at: DateTime<Utc>) -> Result<(), StoreError>;
    fn sweep_acked(&mut self, before: DateTime<Utc>) -> Result<usize, StoreError>;
    fn next_seq(&mut self) -> Result<u64, StoreError>;
    fn roles(&self) -> Result<RolesState, StoreError>;
    fn set_roles(&mut self, state: &RolesState) -> Result<(), StoreError>;
}
pub trait MailStore {
    /// One atomic write transaction: every change `f` makes commits together, or none does.
    fn write(&self, f: &mut dyn FnMut(&mut dyn MailTxn) -> Result<(), MailError>) -> Result<(), MailError>;
}
pub struct MemoryStore { .. }  // Mutex<MemState>; write = clone, apply, swap on Ok
pub struct Mailbox<S: MailStore> { .. }
impl<S: MailStore> Mailbox<S> {
    pub fn open(store: S, config: MailConfig, now: DateTime<Utc>) -> Result<Self, MailError>;  // Roles::restore(store roles)
    pub fn enqueue(&mut self, env: Envelope, now: DateTime<Utc>) -> Result<Enqueued, MailError>;
    pub fn claim(&mut self, req: &ClaimRequest, account_keys: &dyn Fn(&str) -> Option<[u8; 32]>,
                 revoked: &dyn RevocationLookup, now: DateTime<Utc>) -> Result<Grant, MailError>;
    pub fn fetch(&mut self, role: &str, epoch: u64, max: usize, lease: Option<Duration>, now: DateTime<Utc>) -> Result<Vec<Delivery>, MailError>;
    pub fn ack(&mut self, role: &str, epoch: u64, message_id: &str, now: DateTime<Utc>) -> Result<Acked, MailError>;
    pub fn sweep(&mut self, now: DateTime<Utc>) -> Result<usize, MailError>;
}
```

**Rules (each one pinned by a test):**
- **`fetch` leases** a message when it has no lease, its lease expired (`until <= now`), or its lease
  belongs to a different epoch. Under `fetch`'s `check`, any other epoch is necessarily older, so
  this is the takeover voiding.
- **`ack`** requires the lease to belong to this epoch, expired or not. This is at-least-once: the
  holder processed it.

- [ ] **Step 1: Write the suite (failing).**
  - The suite exposes `pub fn run_all<S: MailStore>(make: impl Fn() -> S)` calling each case.
  - `tests/mailbox.rs` has one `#[test]` per case, each calling `suite::case_x(MemoryStore::new)`,
    so failures report by name.
  - **Cases:**
    1. `enqueue_then_fetch_leases_and_hides`
    2. `an_expired_lease_redelivers_in_order`
    3. `ack_removes_for_good_and_a_resend_is_a_duplicate`
    4. `a_duplicate_while_queued_is_not_queued_twice`
    5. `a_superseded_epoch_cannot_fetch_or_ack`
    6. `a_takeover_frees_the_old_leases_at_once`
    7. `only_the_lease_holder_acks` (unleased gives `NotYourLease`; unknown id gives `NotFound`)
    8. `ack_is_idempotent`
    9. `the_bound_refuses_by_count_and_by_bytes_and_keeps_the_queue`
    10. `sweep_deletes_old_acked_history_never_queued_mail`
    11. `a_lease_outside_the_range_is_refused`
    12. `mail_waits_for_a_role_that_has_not_claimed_yet`
    13. `a_failed_write_leaves_the_prior_state` (MemoryStore only: a closure that `put`s, then
        returns `Err`, leaves the queue unchanged)
    14. `claims_are_written_through_the_store` (after a claim, `store.write` reading `roles()`
        shows the epoch)
  - **Fixtures:**
    - claims use M3's `sign_claim` with a temp `Keystore`;
    - roles are `lane@acct` and `peer@acct`;
    - the clock is `t0 = 2026-10-02T12:00:00Z`, with the mailbox opened at `t0 - 1s`.
- [ ] **Step 2: Run** `cargo test -p synapse --test mailbox`. Expected: a compile FAIL (no module).
- [ ] **Step 3: Implement.**
  - `Mailbox` keeps `roles: Roles` and `poisoned: bool`.
  - **`claim`:**
    1. `self.roles.claim(..)`;
    2. `store.write(set_roles(&snapshot))`;
    3. on a store error, set `poisoned = true` and return `Store`.
  - **`fetch`/`ack`:** `self.roles.check(role, epoch)` comes first, and only then the store.
  - **`open`:** reads the roles with one `write` call (the trait has only `write`; a read is a write
    that changes nothing), then `Roles::restore(state, now)`. `InvalidState` gives
    `MailError::InvalidState`.
- [ ] **Step 4: Run.** Expected: all 14 PASS.
- [ ] **Step 5: Commit** `feat(mailbox): at-least-once role mailbox over a transactional store (M4 PR1)`.

### After the task
- The full suite, clippy (`--keep-going`), fmt, and the M0 guard.
- Compare by name with `2e27cde`.
- One Sonnet final reviewer with this Review Focus.
- Then push, take #68 out of draft, update the summary, and send `[READY]`.
