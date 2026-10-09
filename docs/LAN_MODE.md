# Synapse across two machines (LAN mode): a design note

**Status: NOTE ONLY. Nothing here is built, and no code in this repository serves off loopback.**
Written 2026-10-09 against `origin/main` at 6.0.0-rc.27 (rc.28 carries this note). It answers the
standing wish quoted below; it is not a spec and needs the PM's approval and a spec before any code.

**The wish (CireSnave, 2026-10-07, as relayed by the PM):** the PM, a Claude Code session on his
desktop (192.168.4.23), and Qwen-over-HTTP LLMs talking to each other through Synapse across two
machines.

## 1. What exists today (read at `origin/main`, rc.27)

- `synapsed` is **loopback only, twice over**: `Daemon::open` refuses a non-loopback `SYNAPSE_ADDR`
  (`DaemonError::NotLoopback`), and `serve` re-checks the address it actually bound
  (`crates/synapsed/src/lib.rs`). Every request's `Host` must also be `127.0.0.1:<port>` or
  `localhost:<port>` (421 otherwise).
- **One account key per daemon.** `/v1/claim` verifies a role key's signature over a fresh nonce, on a
  certificate chain that the daemon's own account key signed, with the claim addressed to this
  daemon's instance id. The daemon looks up exactly one account key (`shared.account_key`).
- A claim yields a random 32-byte **bearer token**, kept server-side only as a SHA-256 hash. Every
  other route needs it. A message's `from` is always the session's verified role.
- **Failure handling (P5):** failed bearers, failed claims and over-budget requests are written to
  `<home>/security-events.jsonl` and answered *later*, never differently. There is **no lockout**.
- Roles are addressed `<role>` (same account) or `<role>@<account>`. The mailbox is durable (`redb`).
- The transports (`src/transport/`: UDP, TCP, HTTP, QUIC, email) and the sender-authentication,
  sealing and agent-certificate layers exist in core but are **not wired to `synapsed`'s mailbox**: the
  daemon's only listener is the loopback HTTP API.

So today a second machine cannot reach the mailbox at all, by design.

## 2. The cases to serve

| Participant | Where | How it would join |
|---|---|---|
| PM session | laptop | the existing loopback path (CLI / `synapse-claude-channel`) |
| Claude Code session | desktop 192.168.4.23 | `synapse-claude-channel` against the laptop's daemon (needs option A, and a client change: today `synapse-client` finds a daemon only through the announce file in a local home), or its own daemon plus a peer link (option B, §4) |
| Qwen over HTTP | either | through a **harness that speaks the Synapse client** (see §5), never raw |

## 3. Option A: the laptop daemon listens on the LAN (a "hub")

One daemon, bound to a LAN address, with per-client credentials.

**What would have to change**
1. Drop the loopback refusal **only** behind an explicit opt-in (a flag that names the address; never
   the default; the `Host` check becomes "the configured names", not just `127.0.0.1`).
2. **Transport security.** Today the bearer travels over plain loopback HTTP. On a LAN it must not.
   TLS (rustls, which is already in the tree for QUIC) with a certificate the clients pin, or the
   health-proof challenge extended to be the channel binding (the proof is keyed by the instance id, which
   today only the owner-only announce file holds, so a remote client would need it distributed). Plain HTTP on a LAN is a non-starter.
3. **Per-client credentials.** Today there is one account key, so "per client" has to be defined. The
   smallest honest version: the account key issues each remote machine its own **role certificates**
   (agent certificates, f1, already exist) with a short validity and a coarse permission set; the
   daemon accepts exactly those chains. A remote machine never holds the account key. Revocation
   today is `NoRevocations` (a stub), so revoking a stolen desktop credential needs either a real
   revocation list or short certificate lifetimes, and the note recommends **both** being decided
   in the spec, not here.
4. **Pre-auth limiter and audit-first order.** Off loopback the unauthenticated surface is reachable
   by every host on the network, so the order of work on a request has to be fixed: (1) cap the body
   size and connection count before reading anything; (2) per-source-address budget *before* the
   per-claimed-identity budget that exists now (today's `claim` budget is keyed by the claimed id, which
   an attacker chooses, plus one global backstop); (3) **write the audit line before any work that could fail or be slow**, so a
   request that dies midway still leaves a record. This is a change, not today's behaviour: the emitter
   writes at most one line per kind and surface per second (the rest are counted and ride on the next
   line), and over-budget events are written only once the delay applies, so the audit-first order needs
   the emitter changed or exempted for this surface; (4) only then verify. The current limiter delays
   failures and never locks; that stays, because a lockout is a denial-of-service lever once strangers
   can reach the port.
5. `security-events.jsonl` gains the remote address (sanitized like the claimed id already is).

**Honest cost:** this is the larger option. It touches `synapsed` transport, the claim path, the
limiter, certificates and revocation. Estimate **4-6 PRs, 4-6 lane-days**, plus a security review
that I would not skip. That is an estimate from the size of P5 (security hardening) and the M5 slices,
not a measurement.

## 4. Option B: one daemon per machine, linked

Each machine keeps its loopback-only daemon (so nothing in §1 weakens), and a **link process** carries
messages between them over the existing authenticated, sealed transports (QUIC is the natural one).
`<role>@<account>` already names the other side, so addressing needs no change.

- Pro: the daemon's security story is untouched; the network surface is the QUIC listener, which has
  already been through hardening and carries sender authentication and sealing by design.
- Con: it needs the missing piece, **a daemon-to-daemon forwarder** (queue a message for a remote
  role, send it, retry, dedupe on the far side), and a trust bootstrap between two accounts or one
  account on two machines. That forwarder is a new component with its own delivery guarantees.
  Estimate **5-8 PRs**; it is the better end state and the worse first step.

## 5. A non-Claude LLM joining (Qwen over HTTP)

Qwen does not speak MCP-channel push. It needs something to hold a role for it:

- A **harness** (the OverMind local-model dispatcher is the planned one, M8) runs `synapse-client`
  as the role: claim, heartbeat every 30 s, `fetch`, hand the body to the model as **untrusted
  text**, `send` the reply, `ack`. Qwen never sees a credential and never talks to the daemon.
- The model's HTTP endpoint stays where it is; only the harness needs a route to the daemon. On
  one machine that is loopback, so **Qwen on the laptop needs nothing from this note**; only a
  harness on the desktop does (hence A or B).
- Hard rule carried from the channel adapter: a pushed body is data, not instructions, whoever
  signed it.

## 6. Tunnel plus Access, versus a simpler separate mode

- **Tunnel plus Access** (for example a Cloudflare Tunnel with Access service tokens in front of
  Option A): no inbound port on the laptop, and identity is enforced before a packet reaches
  `synapsed`. It would also serve machines off the LAN. Cost: an external dependency and an account
  for something that is two PCs on one desk, and CireSnave's standing rule is to avoid depending on
  hosted services where a plain approach works. It also does not remove the need for §3.3 and §3.4:
  the daemon must not trust the tunnel to be the only gate.
- **Simpler separate mode** (recommended starting point): Option A restricted to **one named private
  address**, TLS with pinned certificates, off by default, per-machine role certificates. No third
  party. It covers 192.168.4.23 and nothing else, which is the stated need.

## 7. Recommendation and order

1. Do **not** start this before the M9 soak result. The soak is about whether the *local* path is
   good enough to replace claude-peers; LAN mode multiplies every defect the soak would find.
2. When wanted, start with **Option A, separate mode**, as a spec first. Option B is the long-term
   shape and should be specified alongside so A does not paint it into a corner.
3. Order against the plan: M7 (merged, channel adapter) -> M9 soak and cutover -> LAN spec -> LAN build.
   Nothing in LAN mode blocks, or is blocked by, M9.

## 8. What is not decided here

Whether CireSnave wants Option A or B; the certificate lifetime and revocation mechanism; whether
the desktop runs its own daemon at all; and anything about exposure beyond the LAN. Each is a question
for CireSnave, via the PM, once a spec exists.
