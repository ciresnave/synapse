# A provider-neutral public MCP endpoint for Synapse (design)

**Status:** DRAFT for PM approval. Docs only; nothing is built and nothing is exposed.
**Why:** CireSnave on board 118, verbatim: *"My VPS hosts Listmonk and nothing else. The initial version
should be the neutral endpoint so any LLM can use it. Muse is just the first test subject. Yes...get
this going."*
**Measured at:** `ciresnave/synapse@264ceb6` (5.7.2) and `ciresnave/auth-framework@fa66c43`.
**Depends on:** `2026-10-07-brute-force-hardening-design.md`. It shares `SecurityEvent`,
`FailureLimiter` and the alert policy with that spec.

## 1. Goal and non-goals

**Goal.** Any MCP client can use a Synapse mailbox over the network, whoever its model vendor is. Each
client:
- holds its own revocable identity;
- is limited, audited and alerted on;
- is never trusted beyond the role it was issued.

The mailbox is the one `synapsed` already serves: claim, send, fetch, ack, list (M4/M5).

**Non-goals.**
- Hosting models, or running anyone's code.
- Vendor-specific features. There are no fields, headers, prompts or tool names for any one provider:
  the tool surface is plain MCP with JSON Schema (§3.6).
- Replacing the local `synapsed`. Local lanes keep their loopback daemon.

## 2. Shape

```
Internet ──TLS──► Cloudflare edge ── Access (service-token policy) ──► cloudflared (container)
                                                                         │ http://127.0.0.1:8790
                                                                         ▼
                                        synapse-mcp-http (container user `synapse`, loopback only)
                                          ├─ audit (first, before any decision)
                                          ├─ Access JWT verify ─► pre-auth limiter (failures only)
                                          ├─ client auth (auth-framework, resource server)
                                          ├─ per-client limiter + cost caps
                                          ├─ off switch
                                          └─ MCP streamable HTTP ─► mailbox (in-process synapsed lib)
                                                                     redb on its own volume
```

- **A new crate `synapse-mcp-http`**, with a binary of the same name.
  - The name returned 404 on crates.io on 2026-10-07; the control `serde` returned 200.
  - It is the only new listener: rmcp 3.4.0 `transport-streamable-http-server`, a feature of the rmcp
    we already depend on, so no new dependency family.
  - It embeds the mailbox through the `synapsed` library (`Daemon`). It has its own home and store,
    never a user's.
- **Loopback only, like everything else.**
  - It binds `127.0.0.1` through `BindScope`, refuses any non-loopback address as `synapsed` does, and
    `tests/loopback_by_default.rs` keeps passing.
  - **The only public path is Cloudflare Tunnel.** No port is opened on the VPS, and the container
    publishes none.
- **The `Host` and `Origin` guard** follows `synapsed`'s, which the MCP streamable-HTTP spec requires for
  DNS rebinding.
  - `Host` must be the tunnel hostname or loopback.
  - `Origin`, when present, must be on an allowlist (empty by default, since MCP clients are not
    browsers).

## 3. Request path, in order

### 3.1 Audit first

The audit record is created when the request arrives, **before** any check.
- **It holds:** time, `CF-Connecting-IP`, the Access identity (service-token client id from the JWT),
  method, path, MCP method or tool name, the presented API-key id (its prefix, never the secret),
  outcome, status and latency.
- **The outcome is filled in on every exit path,** rejections included (Access failure, 401, 403, 429,
  503). An exit that skips the audit is a test failure; §7 test 3.
- **Storage:** append-only JSONL on the container's own volume, rotated, owner-only. It is not shared
  with Listmonk or anything else.

### 3.2 Cloudflare Access

- **Edge:** an Access application with a **service-token** policy. Each client gets its own service
  token, so Access identifies the client before our code runs, and Cloudflare can revoke it alone.
- **Origin:** we also verify `Cf-Access-Jwt-Assertion` against the team's JWKS (audience = the
  application's AUD tag).
  - Failure is a 403, recorded first in the audit, then counted by the pre-auth limiter.
  - This defends against a misrouted tunnel or a local process reaching the loopback port directly.

### 3.3 Pre-auth limiter (failed-attempt path only)

- **Separate from the per-client limiter.** It counts **failures** only: a bad JWT, a bad or unknown API
  key, a revoked key.
- **Key:** `(Access client id, CF-Connecting-IP)`, with each half also limited on its own as a backstop.
- **Over the limit:** 429 with `Retry-After`, an exponential delay, then a temporary block.
  - Events: `AuthFailure`, `RateLimited` and `Lockout`. Alerts: the threshold rule, plus an immediate
    alert on the first `Lockout` per key.
- **State lives in the redb store,** so a restart does not reset it.

### 3.4 Client authentication: auth-framework as resource server

- Each client holds an **API key** issued by auth-framework, sent as `Authorization: Bearer <key>`.
  - It is validated with `validate_api_key` (`auth-framework src/auth.rs:1294`) and revoked with
    `revoke_api_key` (`:1300`).
  - It maps to exactly one Synapse role, e.g. `muse@pilot`. The gateway holds that role's key and makes
    the M3 claim on the client's behalf, so clients never hold Ed25519 keys.
- **The first successful use of a client id** raises a `NewClient` event and an immediate alert.
- **Blocking dependency, fixed in auth-framework, not worked around here.**
  - auth-framework stores an API key under a storage key built from the raw key:
    `format!("api_key:{}", api_key)` at `src/auth_modular/user_manager.rs:171,190,234`.
  - The fix (a keyed hash as the storage key, with an id prefix for lookup) is on the auth-framework
    lane's list, with the client registry (#90). See §5 gate G1.

### 3.5 Per-client limits, cost caps, off switch

- **Per-client limiter**, keyed on the **authenticated client id**:
  - requests per minute;
  - tool calls per day;
  - bytes in and out per day;
  - messages queued per role (M4's mailbox caps already bound the store).
  - A trip is a 429 with a `RateLimited` event and a threshold alert.
- **Global caps:**
  - a maximum number of client identities;
  - total requests per day;
  - total stored bytes.
  - When a global cap is reached the endpoint answers 503 and raises an **immediate** alert.
- **Off switch, three independent ways:**
  1. A runtime flag file, `<home>/DISABLED`, checked on every request: 503, audited, and a `Config`
     event when it appears or disappears.
  2. Disable the Access application.
  3. `docker stop`.

  Test 6 exercises the first; a runbook line covers the other two.

### 3.6 The MCP surface (provider-neutral)

The tools match the local mailbox, with plain JSON Schema arguments and results:

| tool | arguments | result |
|---|---|---|
| `send` | `to`, `body`, optional `message_id` | as `/v1/send` |
| `inbox` | optional `max`, optional `lease_secs` | messages, as `/v1/fetch` |
| `ack` | `message_id` | as `/v1/ack` |
| `list` | — | as `/v1/list`, scoped to the client's account |

- **There is no `claim` tool.** The client's role is fixed by its credential.
- **Message bodies are untrusted text.** The tool descriptions say so, as `synapse-mcp`'s `poll` does
  today.
- **MCP session ids** (`Mcp-Session-Id`) are bound to the authenticated client and are never a
  credential. A session id presented by a different client is a 404, plus an `AuthFailure` event.

## 4. Deployment on the VPS

The VPS hosts Listmonk only, in CireSnave's words, so this must not touch it.

- **Isolation:**
  - its own container, from its own image;
  - its own unprivileged user;
  - its own named volume holding redb, the audit log and the security-event file;
  - **no shared database** and no shared network with Listmonk's containers;
  - CPU and memory limits, so it cannot starve Listmonk.
- **Its own `cloudflared`:** its own tunnel credentials and its own route (hostname to be chosen by
  CireSnave). It does not reuse Listmonk's tunnel token.
- **Secrets, all deployment secrets and none in the image:**
  - `SECURITY_ALERT_EMAIL` (the PM sets it);
  - `SECURITY_ALERT_SMTP_URL`, a relay credential of its own, not Listmonk's;
  - the Access team domain and AUD;
  - the auth-framework store key.
- **It refuses to start** if `SECURITY_ALERT_EMAIL` or the alert relay is missing. An endpoint that
  cannot alert does not run.

## 5. Sequence and gates

| gate | what | who | blocks |
|---|---|---|---|
| G0 | Hardening P1–P3 merged (6.0.0) | synapse | everything below |
| G0b | Hardening P4 (`SecurityEvent`, limiter, sinks) | synapse | the build |
| G1 | auth-framework stores API keys by keyed hash, not raw key; client registry #90; audit of OAuth/introspection #91; a per-client limiter #92 (or we keep ours, §3.5); MFA/refresh fixes | auth-framework lane | real client auth |
| G1b | synapse moves from auth-framework 0.3.0 to a release with G1, which also clears RUSTSEC-2026-0258 (h2, board 102) | synapse after G1 | real client auth |
| G2 | `synapse-mcp-http` built and tested **behind loopback** (§6) | synapse | review |
| G3 | **Independent security review of the whole path**: Access policy, tunnel, container, gateway, auth-framework integration, alerting | someone who did not build it | **any exposure** |
| G4 | The pilot (§8) | PM and CireSnave | — |

**Buildable now, before G1.**

The crate can be built entirely behind loopback against a stub:

```rust
trait ClientAuthenticator { async fn authenticate(&self, presented: &str) -> Result<ClientId, AuthReject>; }
```

- The stub holds test clients as keyed hashes in memory.
- At G1b the stub is replaced by an auth-framework-backed implementation. Nothing else changes.
- Everything else in §3 can be built and tested on loopback with the stub: audit, Access JWT checks
  (against a test JWKS), both limiters, caps, the off switch and the MCP tools.
- Nothing is deployed before G3.

## 6. What the build PRs will look like (for scale; each is planned separately)

1. Crate skeleton:
   - the loopback-only listener;
   - the `Host`/`Origin` guard;
   - the off switch;
   - audit-first middleware, with a test that every exit is audited.
2. Access JWT verification against a test JWKS, and the pre-auth limiter.
3. Stub `ClientAuthenticator`, per-client limiter and caps.
4. MCP tools over the mailbox, and session binding.
5. `AlertSink` wiring, and the startup refusal without alert config.
6. After G1b: the auth-framework `ClientAuthenticator`.

## 7. Tests (each with a negative and a positive control)

1. **Loopback only.** The listener refuses a non-loopback bind address, and `loopback_by_default.rs`
   still passes.
2. **Access.** A request with no JWT, or a JWT with a bad signature, audience or expiry, gets 403. A
   valid test JWT passes.
3. **Audit completeness.** For every rejection class (403, 401, 429, 503, bad session) the audit file
   gains exactly one record with that outcome. A success gains one record too.
4. **Pre-auth limiter.** N bad keys from one `(client id, IP)` trip the limiter, and a good key from
   another client is unaffected. The block survives a restart.
5. **Per-client limiter and caps.** Client A over its limit gets 429 while client B is unaffected. A
   reached global cap gives 503 plus an immediate alert.
6. **Off switch.** Creating `DISABLED` gives 503 plus a `Config` event; removing it restores service.
7. **Alerts.** A capturing sink receives `NewClient` on first use and the threshold alerts. Coalescing
   holds a burst of 100 failures to one alert.
8. **Neutrality.** A scan asserts that no vendor or model name appears in tool names, descriptions or
   schemas. Positive control: the scan finds a planted name.
9. **Session binding.** Client B presenting client A's `Mcp-Session-Id` gets 404 plus an event.

## 8. The pilot: Muse

- **Scope:** Muse gets a throwaway client identity (`muse@pilot`) with low caps.
- **Tasks:** only non-secret, self-contained tasks, decided in advance, that need no repository access or
  credentials. For example: summarise a public document; draft prose from a given outline; transform
  given data.
- **Ending it:** revoking the key and the Access token ends it.

**Measuring whether it saves tokens.** Fix a set of about 20 such tasks in advance. Do each task two
ways:
- **A, the baseline:** a Claude lane does the task itself.
- **B, delegated:** a Claude lane writes the request, Muse does the task, and the lane reviews the
  result, reworking it if needed.

Record per task:
- the Claude input and output tokens for A and for B (from the session's usage records);
- whether B's result was accepted unchanged, accepted after rework, or rejected;
- wall-clock time.

**Savings** = Σ(A tokens) − Σ(B tokens, rework included). B pays for writing and reviewing, so a task
whose review costs as much as doing it shows **no** saving, and is counted that way.

Two numbers are reported together: savings and the acceptance rate. A large saving with a 50%
acceptance rate is not a win.

The success threshold (for example, ≥ 40% savings with ≥ 90% acceptance) is the PM's or CireSnave's to
set **before** the run, not after.

## 9. Open questions

1. **What is "Muse"?** It is not in the portfolio docs (DECISIONS, EXPECTATIONS, ROADMAP, STRATEGY). The
   design does not depend on the answer, but the pilot's client setup does.
2. **Which SMTP relay** sends the alerts? It must be a separate credential from Listmonk's (for example, a
   separate Postmark server token).
3. **Which hostname** does the tunnel route get?
4. **Who does the G3 independent review?**
