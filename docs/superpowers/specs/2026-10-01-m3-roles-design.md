# M3: roles in core: claim, epoch, Superseded (design)

**Status:** DRAFT for PM approval. Nothing is built.
**Plan:** `docs/superpowers/plans/2026-09-29-claude-peers-replacement.md`, milestone M3. The PM tasked it
on 2026-10-01.
**Measured at:** `origin/main@4721f1c` (5.1.0).

## 1. Goal

A role is a stable identity that a restarted session **takes over** with no rediscovery. That is
the PM's #1 pain with claude-peers. The plan fixed the semantics (§3):

- `claim(role)` returns a new **epoch**, a strictly increasing number.
- Only the newest epoch is served. An older session gets **`Superseded`** on its next call, and its
  leases are void (leases arrive in M4).
- Anyone who holds the role's key can claim. The proof is a signature by that key, and the key is
  bound to `role@account` by the role certificate that M2's keystore issues.

M3 is **pure logic in core**: no I/O, no clock (`now` is passed in), no network. M4 persists its
state, and M5's daemon calls it.

## 2. What exists (at 4721f1c)

- **Identity (M2):** `Keystore::role(role, now)` gives a `RoleIdentity` with `global_id`
  `role@account`, a `CryptoManager` holding the role's Ed25519 signing key, and a one-link chain
  signed by the account key.
- **Binding (f1):** `certificate::validate_chain(chain, account_keys, revoked, now)` returns the
  leaf's `subject_global_id`, `subject_signing_key` and permissions.
- **Signing:** `CryptoManager::sign_message(&str)`, which is Ed25519 via ring. Verification is
  `ring::signature::UnparsedPublicKey::new(&ED25519, key).verify(…)`.
- **Freshness and replay:** `replay::ReplayGuard::check(key_id, id, signed_at, now) -> Decision`.
  It is bounded and already used at the transport.

## 3. Design

### 3.1 Types (new module `src/roles.rs`)

```rust
pub struct ClaimRequest {           // what a session sends to take a role
    pub chain: Vec<AgentCertificate>, // the role certificate (leaf first)
    pub nonce: [u8; 16],              // fresh per claim
    pub signed_at: DateTime<Utc>,
    pub signature: [u8; 64],          // by the leaf's signing key over claim_signing_input(..)
}
pub fn claim_signing_input(global_id: &str, nonce: &[u8; 16], signed_at: DateTime<Utc>) -> String;
    // "synapse/role-claim/v1\n<global_id>\n<hex nonce>\n<RFC3339 signed_at>" -- domain-separated text,
    // because CryptoManager signs &str
pub fn sign_claim(identity: &RoleIdentity, now) -> ClaimRequest;  // the client half

pub struct Roles { /* per role: current epoch + claimed_at; plus a ReplayGuard for claim nonces */ }
impl Roles {
    pub fn claim(&mut self, req: &ClaimRequest, account_keys, revoked, now) -> Result<Grant, ClaimError>;
    pub fn check(&self, global_id: &str, epoch: u64) -> Result<(), Superseded>;
    pub fn current(&self, global_id: &str) -> Option<u64>;
    pub fn snapshot(&self) -> RolesState;              // serde, for M4 to persist
    pub fn restore(state: RolesState, now) -> Roles;   // epochs survive restarts; nonces need not
}
pub struct Grant { pub global_id: String, pub epoch: u64, pub superseded: Option<u64> }
pub enum ClaimError { Chain(ChainError), BadSignature, Stale /*freshness*/, Replayed, NotARole }
pub struct Superseded { pub current: u64 }
```

### 3.2 Rules

1. **Binding.** `validate_chain` must pass at `now`. The claimed `global_id` is the leaf's
   `subject_global_id`. A request carries no separately named role, so it cannot claim one role with
   another role's certificate.
2. **Possession.** The signature verifies under the leaf's `subject_signing_key`, over
   `claim_signing_input(global_id, nonce, signed_at)`.
3. **Freshness and replay** go through `ReplayGuard`, keyed by the leaf signing key's `key_id` and
   the nonce. A replayed or stale claim is refused, so a captured claim can't bump the epoch again.
   The transport *delivers* stale messages with a mark; claims are stricter. Only
   `Deliver(Fresh)` is accepted. `Deliver(Stale | Ahead | …)` gives `Stale`, and `Drop` gives
   `Replayed`.
4. **Epoch.** The new epoch is the old epoch + 1, or 1 for a first claim. **The newest claim always
   wins.** `Grant.superseded` names the epoch it displaced.
5. **`check`** passes only for the current epoch. Any other epoch gets `Superseded { current }`.
   A role that was never claimed is not `Ok` either; that is `Superseded { current: 0 }`, so no
   epoch can be presumed valid.
6. **Persistence.** `snapshot`/`restore` carry epochs, so a daemon restart never reissues an old
   epoch: epochs stay strictly increasing for the life of the store. Replay state is not persisted.
   After a restart, the freshness window alone bounds a replay.

### 3.3 Out of scope (later milestones)

- leases and mailbox (M4);
- presence and summaries (M5);
- the daemon API (M5);
- persisting state (M4, through `snapshot`/`restore`).

## 4. Questions for the PM

- **Q1. Should a live holder block a takeover?** Recommended: **no, the newest claim always
  wins.** A restarted lane must be able to take its role back from a hung or crashed predecessor,
  and the PM's #1 pain is exactly that case. The cost: two live sessions configured as one role
  will fight, each superseding the other. They show it plainly as alternating `Superseded`
  results, and M5 can surface that in `list`.
- **Q2. What does a superseded session still get to do?** Recommended: **nothing that depends on
  the role's mailbox or presence.** `check` refuses every role-scoped call: fetch, ack, heartbeat,
  set-summary. *Sending* signed messages is the transport's concern, not the role table's. The old
  session's key is still the role's key, so receivers still verify it. M3 doesn't change that.
- **Q3. Freshness window for claims.** Recommended: reuse the transport's `ReplayConfig` defaults.
  One policy is easier to reason about than two.
- **Q4. Do claims need a permission?** Recommended: **no new permission.** Holding the role's key
  plus a valid certificate is the authorization (plan §3: "anyone who can read the role's key file
  can claim the role").

## 5. Acceptance (the plan turns these into tests)

- A first claim gets epoch 1, and a second gets epoch 2 with `superseded = Some(1)`. `check(1)`
  then gives `Superseded { current: 2 }`, and `check(2)` gives `Ok`.
- **Refusals:**
  - a chain from an unknown account (`Chain(UnknownIssuer)`);
  - an expired certificate;
  - a signature by another key, or over another `global_id`;
  - a replayed request (the same nonce): `Replayed`, with no epoch change;
  - a stale `signed_at`: `Stale`.
- **Round-trip:** `snapshot` then `restore` keeps every epoch, and the next claim continues from it.
- **End to end with M2:** a `RoleIdentity` from a real temp keystore signs a claim, and `Roles`
  grants it.
- **No vendor names** (the M0 guard), no I/O, no clock.

## 6. Size

Estimated at 1 PR and 0.5–1 lane-day, as in the plan.
