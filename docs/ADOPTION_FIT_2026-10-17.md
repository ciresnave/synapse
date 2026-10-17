# Adoption fit: auth-framework and PersistAnt (board 196, EXPECTATIONS 6.14)

Written 2026-10-17 at `6b8ad2e` (rc.102) plus #202 (rc.103). External facts are from `origin/main` of
`auth-framework` (README, FEATURE_FLAGS, PROTOCOLS) and `PersistAnt` 0.7.3 (ROADMAP, `docs/DESIGN-KINDS.md`), read
2026-10-17; nothing here was built or run against them except the mailbox spike below. The ruling: every project
adopts auth-framework for authentication and PersistAnt for persistence. This note says what that means for
Synapse, in what order, at what risk. No code or dependency change.

## 1. Authentication: what Synapse has

| Piece | Where | What it does |
|---|---|---|
| Account key, role keys, role certificate | `keystore.rs`, `certificate.rs` | Ed25519 account key signs a 24 h role certificate (renewed under 12 h) with coarse permissions (`send`, `request-ack`, `ack`, extensions) |
| Claim | `roles.rs`, `/v1/claim` | The role signs a domain-separated text with a fresh nonce; verified against the certificate chain, freshness window and a replay set |
| Session token | `synapsed` `sessions` map | Random token, stored only as a SHA-256 hash, **in memory** (lost on daemon restart; clients re-claim) |
| Bearer check, failure limiter, security log | `synapsed` | Per-route bearer lookup; delayed failure answers, no lockout; owner-only JSONL log |
| Sender attestation | `sender_auth.rs`, `email_server/auth.rs` | Per-message sender checks for the mail path |
| Key manager | `synapse/auth/utils.rs` | Block-signing keys. The old `SynapseAuth` (passwords, JWT) was deleted in brute-force hardening P2 |

Synapse authenticates **machine roles with keys it holds**, not people with passwords, and there is no OAuth by design
(an earlier ruling: no OAuth).

## 2. Authentication: what auth-framework offers

JWT/PASETO token management, sessions, OAuth 2.x with PKCE and **DPoP** (proof of possession), token introspection,
MFA/WebAuthn, rate limiting, RBAC, SAML/LDAP/Kerberos and more, behind its own API server. It is a release candidate and
says it should be validated per deployment. It is a user-and-credential framework first.

## 3. Fit, by piece

- **Session tokens, bearer lookup, rate limiting, RBAC for the daemon's routes:** direct fit. auth-framework's token
  manager, session management, limiter and RBAC would replace `sessions`, the bearer check and `FailureLimiter`. **Gap
  to close before swapping:** Synapse's rule "a failed answer is delayed, never different, and nothing locks out" is a
  design choice with tests; auth-framework's limiter semantics must be shown to satisfy it, or the rule is changed by
  a CireSnave decision. Also, sessions would stop being "hash only, in memory" unless its storage is configured so.
- **Claim by role signature over a certificate chain:** no equivalent found in the docs read. DPoP is the nearest
  concept (proof of possession of a key) but is an HTTP-token binding, not a role-certificate claim. **Unverified:**
  whether auth-framework can verify a caller-supplied Ed25519 signature plus a certificate chain as a credential (a
  custom authentication method). Until that is shown, the claim stays Synapse's and auth-framework supplies what
  follows it.
- **Keystore and certificates:** key custody (0600 files, hard-link no-clobber publish, 0700 directory) is not an
  authentication-framework concern. It belongs to PersistAnt (section 5), not auth-framework.
- **Sender attestation (`sender_auth.rs`, 1,502 lines):** not assessed in this note. Reading it against
  auth-framework is its own task.

Net: adopt auth-framework for the daemon's **token/session/limit/RBAC layer**; keep the claim and certificate model as
a credential type fed into it. If auth-framework cannot take a custom credential, the cost is a small upstream change
in auth-framework (same owner), not a Synapse workaround.

## 4. Persistence: what Synapse stores

| Store | Where | Contract needed |
|---|---|---|
| Mailbox (queues, leases, acks, retention) | `mailbox.rs`, redb 4.4 (`mailbox-redb`) | One atomic multi-key write transaction per operation, ordered `(role, seq)` scan, time-index sweep, 0600 file |
| Keystore (account/role keys, certs) | `keystore.rs` | Create-only no-clobber publish (hard link), replace-by-rename for certs, fsync, 0600, dir 0700 |
| Announce file, client session file | `synapsed`, `synapsed-client` | tmp + fsync + rename, 0600 |
| Security log | `synapse-security` | Append-only JSONL, rotate by rename, 0600 |
| Role lock | `RoleLock` | Process-lifetime exclusive lock |

The legacy Postgres/redis layer that PersistAnt's inventory lists for Synapse is **gone at this ref**: a search for
`sqlx::` and `redis::` over `src` and `crates` finds nothing. PersistAnt `DESIGN-KINDS.md` rows 67, 95 and 582 are
stale on that point (routed to the PersistAnt lane).

## 5. Persistence: fit, and the remaining gaps

- **Mailbox contract: shown for three backends, once.** `conformance::run_all` (21 cases) passed on a `MailStore` over
  `KvOnRelational` for SQLite, PostgreSQL 16 and MySQL 8.4 with PersistAnt 0.7.0 (spike `C:/s/spike-kvmail`, outside
  the repo; 0 deadlocks on the last rerun). That proves **semantics**, not the properties below.
- **Gap 1, durability class.** redb commits are durable at its default setting (not re-verified for this note). The
  PersistAnt `kv` contract verifies `kv.durability.process` only. **`Machine` (power loss) is declared and unverified**
  in PersistAnt, and `blob.durable` has no OpenDAL implementation (no parent-directory fsync). Moving the mailbox off
  redb could give up a guarantee PersistAnt cannot yet verify. Required before any move: Synapse names the class it
  needs (`Machine` for mail acknowledged to a sender) and PersistAnt verifies it, or CireSnave accepts `Process`.
- **Gap 2, performance.** Not measured. redb is in-process; every PersistAnt backend that passed is SQLite or a server
  database behind a relational layer. Rerun `benches/mailbox_depth_cost` and `mailbox_fetch_cost` against the adapter
  first.
- **Gap 3, native-fs properties.** `blob.create_exclusive_atomic` (hard link) and `blob.file_mode` (0600) are not in
  OpenDAL; PersistAnt says they stay native. The keystore therefore cannot move wholesale: it would adopt
  `replace_atomic` for the certificate and keep native create-only and mode handling, or PersistAnt grows a native fs
  blob.
- **Gap 4, deployment shape.** Synapse is one daemon per user per machine with no server. Postgres and MySQL add a
  service to run; only SQLite keeps that install story. The adapter would be SQLite-first.
- **Gap 5, migration off redb.** The file format is redb's; there is no in-place path. A one-shot copy from `RedbStore`
  to the new store, run offline by `synapsed` on first start behind a stamp, keeping the redb file until a verified
  read-back. Needs a migration test with live leases and unacked mail. Any on-disk change needs this (ROADMAP row 18).
- **Gap 6, crash tests.** `tests/mailbox_crash.rs` aborts a child mid-transaction against redb. The equivalent must
  pass against the new store before it ships.
- **Gap 7, dependencies.** PersistAnt's own minimum Rust version and its UniSQL dependency must be crates.io versions
  (EXPECTATIONS 6.8).

## 6. Order, risk, effort (estimates, not measurements)

1. **Now, no risk:** keep using PersistAnt as a test oracle; rerun the spike on each release (standing task).
2. **Adapter crate (SQLite only), off by default.** Implement `MailStore` over PersistAnt's kv; gate it with `run_all`,
   the crash test and the two benches. Risk: Gap 2. Effort: small (the spike already has the adapter).
3. **Decision needed from CireSnave:** the durability class for mail, and whether a service-backed store is acceptable
   (Gaps 1 and 4). Nothing past step 2 should start without it.
4. **Keystore and small files** onto `replace_atomic`, keeping native create-only and mode (Gap 3). Low risk, moderate
   effort; it touches key custody, so it needs its own spec and PM review.
5. **Auth layer onto auth-framework:** first prove a custom Ed25519-claim credential (section 3), then swap sessions,
   bearer and limiter. Highest risk (security behaviour, the delayed-answer rule, a release-candidate dependency); do
   it last, with the existing brute-force tests as the oracle.
6. **Cutover of the mailbox** with the offline migration (Gap 5), only after steps 2 and 3 pass and the pilot's 24 h
   report is in.

Not done here: the sender-attestation read-through, any performance number, any auth-framework build. The pilot (T-0)
is unaffected: nothing here changes the running daemon.
