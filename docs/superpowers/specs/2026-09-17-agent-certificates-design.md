# Account Keys, Agent Certificates, Rotation and Revocation (P2 slice f)

**Status:** decided with CireSnave on 2026-09-17; this document is the spec he reviews.
**Branch:** `feat/agent-certificates` (slice f1), stacked on `feat/replay-suppression` (#42).
**Built in two halves (CireSnave, 2026-09-17):** see §11.
**Depends on:** slice a (sender authentication), slice d (sealing), slice e (the delivery gate and
the bounded inbound state).
**Followed by:** trust-on-first-use peer discovery, then the introduction app (its own project).

## 1. The problem

Today a receiver pins **one signing key and one sealing key per peer**, by hand, in its config file
(`TrustStore::pin_pem`, `TrustStore::pin_sealing_key_pem`). Three consequences:

- Every agent an account holder runs must be pinned separately by every peer, so the work is
  quadratic in agents and peers, and rotating a key means editing every peer's config.
- A receiver learns *which key* signed a message and nothing about **who runs that agent**, so no
  consumer can say "this action was authorised by the account holder I trust".
- There is no revocation at all. A leaked agent key is trusted until someone edits every config.

## 2. What this slice adds

An account holder keeps one long-lived **account key**. It signs **agent certificates**, each
binding an agent's signing and sealing key fingerprints to a label, a validity window, coarse
permissions and a serial. Receivers pin **account keys**, not agents. An agent attaches its
certificate chain to every message, so a receiver can verify an agent it has never seen, provided
the chain roots in an account key that receiver has pinned.

## 3. Decisions taken (CireSnave, 2026-09-17)

1. **Encoding:** the project's own canonical format — a domain tag plus length-prefixed fields,
   exactly as slice a signs messages — wrapped in PEM. No X.509, no JSON. One signing discipline,
   no new dependency, no ASN.1 parser.
2. **Delivery:** the chain is attached to **every** message, in signed metadata.
3. **Permissions:** a small fixed set Synapse understands, plus free-form `x-` strings consumers
   interpret.
4. **Minting:** a `synapse-cert` CLI, with the account key in a file, and a seam kept for
   hardware-backed signing later.
5. **An invalid certificate is treated as an unverified sender:** denied by default, delivered
   marked when `accept_unverified` is set, and recorded as a knock. It reuses slice e's gate rather
   than inventing a parallel path.
6. **Revocation:** short validity windows plus a revocation statement signed by the account key. A
   node accepts a relayed revocation **only for account keys it has already pinned**, bounded and
   deduplicated by serial.
7. **Delegation: full chains.** Permissions may only narrow; a child's validity must nest inside its
   parent's; and **each delegator sets how much further its delegates may delegate**. The format
   carries no global depth limit.
8. **Chains are never validated for a sender whose root is not pinned** (§7.3).
9. **Direct per-agent pinning is removed** — in **f2**, not f1 (§11). Certificates become the only
   trust path, with `synapse-cert import` converting an existing config.
10. **A compromised account key is out of scope here.** Recovery needs an identity check that does
    not rely on that key, which is the introduction app's job. Until then the documented recovery is
    manual re-pinning.

## 4. The certificate

```rust
pub struct AgentCertificate {
    pub version: u8,                     // 1
    pub serial: [u8; 16],                // random, unique per issuance
    pub issuer_key_id: String,           // SHA-256 of the issuing key, lowercase hex
    pub subject_label: String,           // display only: free text, never an identity, and
                                         // nothing binds it to subject_global_id (<= 128 chars)
    pub subject_global_id: String,       // the id this agent may claim
    pub subject_signing_key: [u8; 32],   // Ed25519
    pub subject_sealing_key: [u8; 32],   // X25519
    pub not_before: DateTime<Utc>,
    pub not_after: DateTime<Utc>,
    pub permissions: Vec<Permission>,
    pub may_delegate: u8,                // how many further links this subject may issue; 0 = none
    pub signature: [u8; 64],             // Ed25519 over canonical_certificate_input
}
```

**The signed bytes** mirror `sender_auth::canonical_input`'s discipline: a domain tag
`synapse/agent-cert/v1`, then every field above except `signature`, each length-prefixed with a
4-byte big-endian length, in declaration order. Timestamps are i64 microseconds, big-endian.
`permissions` is a count followed by each permission's string form, sorted.

**Permissions.** The fixed set Synapse knows is `send`, `request-ack`, `ack`. Anything beginning
`x-` is free-form, carried and reported but never interpreted. An unknown permission that does not
begin `x-` makes the certificate **invalid** — otherwise a typo would silently grant nothing while
looking fine.

**PEM label:** `SYNAPSE AGENT CERT`. A chain is a concatenation of PEM blocks, leaf first, root last.

**Note on the two bounds:** a minimal certificate PEM block is about 385 bytes, so `max_chain_bytes`
(16 KiB) admits roughly 42 blocks and `max_chain_links` (64) is therefore unreachable unless the byte
bound is raised. The link cap is a backstop for that case, not the binding constraint today.

## 5. The chain

`AgentChain` is an ordered list of certificates, leaf first. It is valid when **all** hold:

1. **Root is pinned.** The last certificate's `issuer_key_id` names an account key in this node's
   `TrustStore`, and its signature verifies against that key.
2. **Each link verifies** against its parent's `subject_signing_key`.
3. **The leaf matches the message:** `subject_signing_key` equals the key that signed the message,
   and `subject_global_id` equals `from_global_id`.
4. **Validity nests:** every certificate's `[not_before, not_after]` sits inside its parent's, and
   `now` is inside the leaf's.
5. **Permissions only narrow:** every permission in a child is present in its parent.
6. **Delegation budget:** a certificate whose `may_delegate` is 0 may not appear as an issuer. Each
   link's `may_delegate` must be strictly less than its parent's.
7. **Nothing in the chain is revoked** (§6).
8. **Identity narrows with authority.** A child's `subject_global_id` must be its parent's id with
   one label prepended: parent `agent@host` may issue `worker.agent@host`, and that worker may issue
   `task.worker.agent@host`. Without this rule a delegate could issue itself a certificate claiming
   **any** id — permissions narrow, but identity would not — and a sub-agent could impersonate its
   own account holder. The root certificate's `subject_global_id` is unconstrained; the account key
   is the authority for it.
   **A label is one component of ASCII alphanumerics, `-` and `_`, and is never empty**, and neither
   id may be empty (both tightened 2026-09-17, during the f1 review). Without the charset rule a
   label may carry an `@`, so `ciresnave@host.agent@host` narrows "under" `agent@host` while
   presenting as the account holder to anything that splits an id on `@` — which this crate already
   does in `src/identity.rs` and `src/synapse/services/privacy_manager.rs`. An empty parent id would
   make every id ending in a dot narrow "under" it, turning this rule off altogether.
9. **Every certificate declares `version` 1.** An unknown version is refused, never validated under
   v1's rules.
10. **A non-root certificate's `issuer_key_id` names the key that signed it.** Authentication rides
    on the signature, so this is belt-and-braces; the field is signature-covered and consumers may
    key diagnostics or revocation off it.

Failing any of these makes the sender `Unverifiable`, which slice e's gate denies by default.
The reason is recorded in the knock, so a misconfiguration is visible rather than silent.

## 6. Revocation

```rust
pub struct Revocation {
    pub version: u8,                 // 1
    pub serial: [u8; 16],            // the certificate being revoked
    pub issuer_key_id: String,       // the key that issued it
    pub issued_at: DateTime<Utc>,
    pub reason: String,              // <= 128 chars, free text, reported never interpreted
    pub signature: [u8; 64],         // Ed25519 by the issuing key, over synapse/agent-revocation/v1
}
```

- **Self-verifying.** Whoever relays a revocation, the receiver checks the signature itself, so a
  relay can withhold one but never forge one.
- **Sources:** a `revocations` file named in config, and revocations relayed in message metadata.
- **A relayed revocation is accepted only when its `issuer_key_id` is a pinned account key or a
  certificate already validated under one.** Everything else is dropped without verification work.
- **Scoped to its issuer.** A revocation is keyed by `(issuer_key_id, serial)`, and a certificate is
  only revoked by a revocation from the key that issued it. Serials travel in every chain, so
  without this any pinned account could silently un-trust another account's agents by publishing a
  revocation for a serial it merely observed (added 2026-09-17, during the f1 review).
- **Bounded per issuer and overall:** at most 512 revocations per account key and 4096 in total,
  deduplicated by `(issuer_key_id, serial)`, oldest `issued_at` evicted first WITHIN the issuer that
  overflowed. A per-issuer bound is what stops one account evicting another's entries by minting
  revocations with distant `issued_at` values, since `issued_at` is chosen by the issuer. **At the
  global bound the eviction comes from whichever issuer currently holds the most entries**, which may
  be a different account than the one inserting (recorded 2026-09-18, during the f1 final review).
  That is the least-bad policy available: reaching 4096 requires at least eight accounts near their
  own 512 limit, so it is not a two-party attack, and evicting the largest bucket is fairer than
  evicting the globally oldest. The store
  is in memory plus the config file; nothing is written back to disk.
- **A revoked certificate invalidates everything below it** in a chain.
- Short validity windows remain the primary mechanism. The CLI's default is **24 hours**, and
  certificates are expected to be re-minted rather than long-lived.

## 7. Behaviour on the receive path

`TransportManager::receive_messages` gains a step between slice a's signature check and slice e's
gate:

1. Verify the message signature (slice a) — unchanged.
2. **Reject a chain field larger than `max_chain_bytes` (default 16 KiB) before parsing it**, so
   an unauthenticated sender cannot make a node parse a large structure.
3. **Read the chain's claimed root `issuer_key_id` from metadata, parsing only.** If it is not a
   pinned account key, the sender is `Unverifiable { reason: UnknownIssuer }` and **no signature in
   the chain is verified**. This is decision 8: a stranger cannot make a node do chain work.
4. Otherwise validate the chain (§5), then apply slice e's gate and replay guard as today.

A backstop applies even for a pinned root: a chain longer than `max_chain_links` (default 64, a
resource guard, not a trust rule) is refused with a distinct reason. It exists so a compromised
pinned account cannot wedge a node.

**The one enforced permission:** a leaf lacking `send` is refused at the transport, so an agent
narrowed to nothing stops being carried. Every other permission is reported, never enforced —
Synapse says who an agent is and what its owner granted; the consumer decides what that allows.

**Hand-off to the discovery slice, recorded here so it is not rediscovered:** the rule in step 2
refuses exactly the case trust-on-first-use introduces, an unknown peer presenting a chain. That
slice must define how much verification an unknown peer may cost — expected to be a much shallower
allowance than a pinned root's — and must not simply relax step 2.

## 8. Surfaces

- `TrustStore`: `pin_account_key(global_id, key)`, `pin_account_key_pem`, `account_key_ids()`,
  `revoke(Revocation)`, `is_revoked(serial)`. **`pin`, `pin_pem`, `pin_sealing_key` and
  `pin_sealing_key_pem` are removed** (decision 9).
- `ReceivedMessage` gains `pub certificate: Option<VerifiedChainSummary>` — the subject label, the
  account key id, the permissions, and the chain length. Never the key bytes.
- `synapse-mcp`: config peers carry `account_public_key` **and `certificate_pem`** instead of
  `public_key_pem` and `sealing_public_key`; `list` reports each peer's account key id; `poll`
  reports each message's certificate summary; a new optional `revocations_path`.

**Where an outbound sealing key comes from (decided by CireSnave, 2026-09-18).** Removing direct
pinning removes sealing-key pinning too, and a certificate only arrives on an INBOUND message — but a
node must encrypt to a peer before it has heard from them. So **a peer's config entry carries that
peer's certificate chain** alongside the account key it roots in. The node validates the chain under
the pinned account key and takes both the signing key and the sealing key from it. One artifact is
exchanged, and it is exactly what the introduction app hands over: an account key and a current
certificate.

**An expired certificate in config refuses the send**, naming the peer and the expiry, rather than
encrypting to a key whose owner may have rotated away. The operator refreshes the certificate; short
validity windows mean that is routine, which is why `synapse-cert` makes re-minting a single command.
- `synapse-cert` binary: `init` (account key, agent keys and a certificate in one command, printing
  the config block), `mint`, `delegate`, `revoke`, `inspect`, `import` (convert a pinned-keys config).

## 9. Not in this slice

- **A compromised or lost account key** (decision 10).
- **Discovery of unknown peers**, which is the next slice.
- **The introduction app**, its own project.
- **Hardware-backed signing.** The CLI keeps a seam: signing goes through one trait with a file
  implementation.

## 11. Delivered in two slices

This document specifies both halves. They are built and reviewed separately, because together they
are about twice slice e, which itself took six tasks.

**f1 — the format and the verification** (branch `feat/agent-certificates`):
- `src/certificate.rs`: the certificate, the revocation, canonical bytes, PEM, chain validation
  (§4, §5, §6).
- `TrustStore` gains account keys and the revocation store; **`pin`, `pin_pem` and the sealing
  equivalents stay** for this slice, so nothing existing breaks.
- The receive path (§7), including the unpinned-root refusal and the size cap.
- Tests 1-11 of §10.
- At the end of f1, a node can be configured either way: pinned agent keys as today, or a pinned
  account key with certificates.

**f2 — the tooling and the changeover** (branch `feat/agent-certificates-cli`, stacked on f1):
- The `synapse-cert` binary: `init`, `mint`, `delegate`, `revoke`, `inspect`, `import`.
- The `synapse-mcp` config and tool changes (§8).
- **Removing direct pinning** and converting every test harness and example.
- Tests 12-14 of §10.

f1 ships working software on its own: certificates verify end to end, and revocation works. f2 is
what makes them the only path.

## 10. Testing

Unit, with an injected clock, in `src/certificate.rs`:

1. A valid single-link chain verifies; a control with one flipped signature byte does not.
2. Each chain rule in §5 fails on its own: wrong subject key, wrong global id, validity outside the
   parent's, a widened permission, `may_delegate` not decreasing, a zero budget used as an issuer,
   and a child claiming an id outside its parent's namespace (rule 8) — including the impersonation
   case, where a delegate issues itself a certificate for its own account holder's id.
3. An unknown non-`x-` permission makes a certificate invalid; an `x-` one is carried.
4. Expiry boundaries: exactly at `not_before` and `not_after`, and one microsecond outside each.
5. A revocation invalidates its certificate and everything below it; a control link above it stays
   valid.
6. Canonical bytes are stable: re-encoding a parsed certificate reproduces the same bytes, and the
   PEM round-trips.

Integration, over loopback UDP, in `tests/agent_certificates.rs`:

7. An agent whose chain roots in a pinned account key is `Verified` without its own key being
   pinned. **Control:** the same message with the account key unpinned is denied and knocked.
8. **No work for strangers:** a chain rooting in an unpinned key is refused, and a counter shows no
   chain signature was verified. The control is a pinned root whose chain verifies.
9. A revoked leaf is denied; the same chain before the revocation is delivered.
10. A leaf lacking `send` is refused at the transport; the same leaf with `send` is delivered.
11. Rotation: an agent re-minted under the same account key is accepted with no config change on the
    receiver. This is the slice's whole point, so it gets its own test.
12. `synapse-mcp` end to end: certificates in config, a summary in `poll`, account key ids in `list`.

CLI, in `tests/synapse_cert_cli.rs`:

13. `init` produces files that let two nodes talk with no hand-editing.
14. `import` converts a pinned-keys config, and the converted config verifies the same peers.

**Mutation check.** Predict, then run: skipping the "permissions only narrow" rule makes only the
widened-permission tests fail.

**Acceptance.** The failing set stays `{test_transport_error_handling}`; fmt and clippy exit 0; the
full run happens in a fresh target directory with the Firewall 2097 count reported; no version bump.
