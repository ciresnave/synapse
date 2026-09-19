# The Transport Contract (pre-2.0)

**Status: APPROVED by CireSnave, 2026-09-18** (selecting *"Approved — write the plan and build it"*).
**Branch:** `design/transport-contract`, from `main` at `bbd4bf0` (after P2 slices a–e and f1).
**Precedes:** QUIC, then email (authenticated by construction), then f2 (certificates-only trust).
**Ruled into the 2.0 set** (CireSnave, 2026-09-18, EXPECTATIONS §5.1b): *"Receiving messages needs to
be unified and that should be done sooner rather than later. QUIC shoud not report "delivered" until
delivery."*

## 1. The problem

Measured at `bbd4bf0`:

- **26 receive functions in 23 files, returning five different types.** Exactly one —
  `TransportManager::receive_messages` — returns messages carrying a sender verdict. The other 25 hand
  back unverified messages, and several return an empty list unconditionally.
- **Two traits are both named `Transport`**, with incompatible signatures. Only the newer one
  (`transport/abstraction.rs`) is used: the manager, every factory and `synapse-mcp` hold it. The older
  one (`transport/mod.rs`) is implemented by three transports nothing can reach.
- **QUIC exists twice.** The real implementation, on `quinn`, is commented out of the build as
  *"Temporarily disabled due to trait mismatch"* — because it implements the old trait. The one that is
  compiled performs no networking at all, yet is the only transport that claims `Delivered`.
- **`protocol_version` does not exist on messages.** It is a string on mDNS service discovery,
  advertised as `"1.0"` by one path and `"1.1.0"` by another, and compared nowhere.

## 2. Decisions (CireSnave, 2026-09-18)

He answered by selecting options; the quotes are the options he chose.

1. **Receive:** *"Only the manager is public; it returns verified messages."* Transports become private
   plumbing. No application can receive an unverified message by calling a transport directly.
2. **QUIC:** *"Restore the real one, delete the fake."* The quinn implementation is ported to the new
   trait; `quic_unified` is deleted.
3. **Transports that cannot receive:** *"Fix every one to actually receive."* (The recommendation was
   to declare them send-only; he chose otherwise. §7 asks him to confirm the shape this takes.)
4. **The old trait:** *"Delete it and its duplicates; port real QUIC to the new trait."* The old
   `Transport` trait goes, with `tcp_enhanced`, `udp` and `websocket`.

**`protocol_version`** (CireSnave, 2026-09-18): one integer, not a major/minor pair, starting at 1 in
2.0, bumped only on wire-incompatible changes; additive optional fields do not bump it.

## 3. The receive API

**One public entry point:** `TransportManager::receive_messages(&self) -> Result<Vec<ReceivedMessage>>`,
which already exists and already returns, per message: the sender verdict, the opened payload, the
freshness mark, and the certificate summary.

**The `Transport` trait's receive becomes crate-private.** It keeps returning raw
`Vec<IncomingMessage>` — the transport's job is to produce bytes from the wire and say where they came
from — but it can no longer be called from outside the crate. The trait itself stays public so a
downstream crate can *implement* a transport; it cannot *read* one past the manager.

**The mechanism is a write-only inbox, not a token** (revised 2026-09-19, during review of the first
implementation). The receive method moves to a supertrait whose signature is
`async fn receive_raw(&self, inbox: &mut RawInbox) -> Result<()>`. `RawInbox` can be pushed into by
anyone who holds one, but only this crate can construct one or read from it. The first design passed
implementors a by-value `Token` instead, and review found it leaks: the manager must hand every
transport a fresh token on every poll, so an external transport registered with the manager is simply
given one, and can store it and then call `receive_raw` on any other transport to read raw, unverified
messages. With an inbox there is nothing to steal: forwarding the inbox to another transport only
puts that transport's messages into the manager's inbox, where they are verified. Decorating and
wrapping transports still work.

**Why the transport does not verify.** Verification needs the trust store, the replay guard, the
delivery gate and the sealing key — all of which the manager owns and threads one clock through.
Putting that in each transport would duplicate the one path the P2 reviews spent a week hardening,
and every review on this branch found a defect exactly where two routes were meant to check the same
thing and one didn't.

## 4. What each delivery status may claim

`DeliveryConfirmation` keeps its four variants. What changes is that each one is a **claim with a
precondition**, and a transport may only construct it when the precondition held:

| variant | means | a transport may return it only when |
|---|---|---|
| `Sent` | the bytes left this process | a write to a real socket or a real client library returned success |
| `Delivered` | the peer's transport stack confirmed receipt | the protocol itself acknowledged it — a QUIC stream finish acknowledged by the peer, an HTTP 2xx, an SMTP 250 for the final data command |
| `Acknowledged` | the peer *application* processed it | produced only by the manager, from a verified signed ack (slice b) — never by a transport |
| `Expired` | no acknowledgement within the tracking window | produced only by the manager (slice e) — never by a transport |

The rule a reviewer checks: **every `Delivered` construction must name the protocol event it rests
on, in a comment beside it.** A `Delivered` with no such event is the bug this section exists to kill.

## 5. `protocol_version` on the wire

- `SecureMessage` gains `protocol_version: u16`, and 2.0 writes `1`.
- **It is inside the signed bytes** (`canonical_input`). Otherwise a relay could rewrite it and push
  two peers into an older wire format — the same class of hole as the sub-microsecond timestamp digits
  closed in f1, where an unsigned field let a relay change a message's meaning.
- **A receiver that does not support the version refuses it as `Unverifiable { UnsupportedVersion }`**,
  a new reason, before attempting to interpret anything else. It is never a parse failure: an operator
  must see "unsupported protocol version 2" in the knock record, not a malformed-message mystery.
- **Peers exchange their supported versions at introduction**, so the introduction app records it
  alongside the account key and certificate.
- The mDNS `"version"` TXT record is **renamed** to `synapse_protocol` and carries the same integer, so
  discovery and the wire can no longer disagree about what "version" means.
- **This changes the signed bytes** (sender-authentication spec §4, amended): `protocol_version` was
  inserted as a signed field, so messages and acks signed before this change do not verify after it.
  Acceptable as part of 2.0's single wire break; there is no in-place upgrade path for a signature
  produced under the old canonical input.
- **A value that does not fit a `u16`** (out of range, negative, or not a number at all) is a malformed
  message, not an unsupported version: it is rejected by serde at deserialisation, before `verify_at`
  runs, and the transport drops it silently the same way it drops any other malformed datagram. Only a
  value that parses as a `u16` and is absent from `SUPPORTED_PROTOCOL_VERSIONS` reaches
  `Unverifiable { UnsupportedVersion }`.

## 6. What actually works today

Measured at `bbd4bf0` by a survey that verified its branch before reading. This section corrects an
earlier claim of mine and should be read before §7.

**Exactly one transport works end to end through the manager: UDP.** `udp_unified` binds one real
socket, runs a real receive loop, and is the only transport `synapse-mcp` enables — every integration
test registers it and nothing else. Everything below is either not compiled, not reachable from the
manager, or not networked.

| protocol | compiled implementation | reachable from the manager? | receive | send |
|---|---|---|---|---|
| UDP | `udp_unified` | **yes** — the only one | real | real, honest `Sent` |
| TCP | `tcp_simple` and `tcp_unified` | only by accident (below) | `tcp_simple`: always empty | real, `Sent` |
| HTTP | `http_unified` (`production_http` is orphaned) | no factory registered | queue nothing fills — no server is ever bound | real `reqwest` POST |
| WebSocket | `websocket_unified` | factory exists, never registered | **double bind unfixed**: the accept loop never starts | handshake skipped, yet claims `Delivered` |
| Email | `email_simple` | only via a separate provider, never the manager | always empty, "no actual checking performed" | **no network at all** — logs, sleeps 500 ms, reports `Sent` |
| QUIC | `quic_unified` | factory never registered | always empty | fabricated, claims `Delivered` |
| NAT traversal | `nat_traversal` | no factory at all | its own double bind | real |

**Nine transport files are not compiled at all** — `tcp.rs`, `tcp_enhanced.rs`, `udp.rs`, `quic.rs`,
`nat_traversal_clean.rs`, `email_enhanced.rs`, `email_unified.rs`, `websocket.rs`, `mdns.rs` — and six
of the seven files under `src/wasm/` are likewise never compiled in any configuration.

**The TCP trap.** `mod.rs` re-exports `abstraction::*`, and `abstraction.rs` defines its own
`TcpTransportFactory` which builds `tcp_simple` — the implementation whose receive is always empty.
The explicit re-export of the good one, `tcp_unified::TcpTransportFactory` (whose double bind *is*
fixed), is commented out. So every caller of `synapse::transport::TcpTransportFactory`, including the
crate's own demo, gets the TCP transport that can never receive.

**Correction to what I told CireSnave about QUIC.** I said the real implementation "largely exists".
It does not, usefully: `quinn` is **not a dependency** in `Cargo.toml`, the file targets an old `quinn`
API, and it calls circuit-breaker methods that no longer exist (`can_proceed` without `.await`, and a
`record_result` that was renamed). It would fail to compile for at least three independent reasons.
Restoring "the real one" means writing a QUIC transport on current `quinn` with that file as a
reference, not re-enabling it.

**The long-standing red test is the email bug.** `test_transport_error_handling`, the one test that has
failed on `main` throughout this work, asserts that `"invalid@"` is not reachable. `email_simple`'s
`test_connectivity` checks only for an `@`, so `"invalid@"` passes and the assertion fails. Fixing the
email validator turns `main` green for the first time.

## 7. Making every transport receive — the question this forces

CireSnave's decision 3 was *"Fix every one to actually receive."* With §6 in hand, "every one" means:

| protocol | what "actually receive" takes | size |
|---|---|---|
| TCP | re-export `tcp_unified`'s factory and delete `tcp_simple` and the shadowing factory | small |
| WebSocket | fix the double bind, perform the real handshake, stop claiming `Delivered` | medium |
| HTTP | bind a real server; today there is none | medium |
| NAT traversal | fix its double bind and give it a factory | medium |
| QUIC | write it on `quinn` | large |
| Email | write real SMTP sending and IMAP receiving; neither exists | large |

Two of the six are new transports, not repairs.

**Decided (CireSnave, 2026-09-18), selecting:** *"Repair four now; QUIC and email as their own slices
before 2.0."* So **this slice repairs TCP, WebSocket, HTTP and NAT traversal.** QUIC and email each get
their own spec and slice, both still before publishing. Until they land, **QUIC refuses to construct**, and
**email constructs but refuses to operate** — its send, receive and connectivity check each return an
error naming the email slice. Email still constructs because its address validator is real, and is
what `test_transport_error_handling` exercises. Either way, no build of this branch ever reports a
delivery that did not happen.

**Email authentication folds into the email slice** (CireSnave, selecting *"Fold it into the email
transport slice"*). The approved order had email authentication next, on the premise that email would
inherit verification once on the unified path; there is no email transport that sends or receives
yet, so email is built on the unified path from the start and is authenticated by construction.

## 8. The email `invalid@` defect

`email_simple::test_connectivity` accepts any address containing `@`, while its own `can_reach`
requires both `@` and `.`. The fix is one validator used by all three paths (`can_reach`,
`test_connectivity`, `send_message`). The unit test `test_email_format_validation` never exercises
the weak path — it tests only `can_reach`, with addresses that have no `@` at all — so it is replaced
with one that drives `test_connectivity` and `send_message` with `"invalid@"` and asserts they refuse.
The integration test's `Ok(None) | Err(_)` arm, which treats "the transport failed to construct" as a
pass, is removed: a test that can pass without reaching its assertion is not a test. Never `#[ignore]`.

## 9. WASM

The only WASM code that compiles is `src/wasm/simple.rs`: a `WasmSynapseNode` whose `send_message`
writes to the browser console. There is no browser transport, no WebCrypto, and no sender
verification in any WASM build. The other six files under `src/wasm/` (2,838 lines, including a
`WebCrypto` wrapper and a WebSocket transport) are not declared as modules and never compile.

So a WASM transport is a new build, not a repair: a browser WebSocket or WebTransport client that goes
through the same manager, verification and sealing as every other transport, with `ed25519-dalek` and
HPKE compiled to wasm (both are pure Rust, so this is feasible). **This slice deletes the six
uncompiled WASM files** — they cannot be built and are not a foundation to keep — and leaves
`simple.rs` as it is. **Whether a real WASM transport is pre-2.0 is the one scope question still open
for CireSnave** (§12).

## 10. Testing

- **One end-to-end test per repaired transport** — TCP, WebSocket, HTTP, NAT traversal, alongside the
  existing UDP ones — that sends a signed, sealed message through a real loopback socket and receives
  it through `TransportManager::receive_messages` with a `Verified` verdict. This is the test none of
  them has today, and it is the one that would have caught every defect in §6.
- **A receive-path test per transport that a message arrives with no application call to the
  transport itself**: proves receive is plumbing, not a public API (§3).
- **A compile-time test that the transport receive cannot be called from outside the crate**, as a
  doc-test marked `compile_fail`.
- **`Delivered` audit**: a source scan asserting every `DeliveryConfirmation::Delivered` construction
  has a comment naming its protocol event (§4), with a control that the scan finds the constructions.
- **`protocol_version`**: a message with version 2 is `Unverifiable { UnsupportedVersion }`; a relay
  rewriting the version invalidates the signature; the mDNS record carries the integer.
- **The email validator**: `"invalid@"` is refused by `can_reach`, `test_connectivity` and
  `send_message` alike. This makes `test_transport_error_handling` pass — `main`'s first fully green run.
- **QUIC and email refuse to construct** with an error naming their slice.
- **Every test binds loopback only**; the full run happens in a fresh target directory with the
  Windows Firewall event 2097 count reported, as for every slice so far.

## 11. What this slice deletes

The old `Transport` trait in `transport/mod.rs`; the nine uncompiled transport files (§6); the
fabricated `quic_unified.rs`; `tcp_simple.rs` and the shadowing `TcpTransportFactory` in
`abstraction.rs`; the orphaned `production_http.rs`; the dead `create_standard_factories`; and the six
uncompiled files under `src/wasm/`. Each deletion is checked by building, since an uncompiled file can
only be deleted safely once nothing refers to it.

## 12. Decided at review

1. **WASM — decided:** *"Its own slice, after 2.0."* Nothing in the 2.0 set depends on it. This slice
   still deletes the six WASM files that never compile.
2. **Everything else in this document** — the receive API (§3), the delivery claims (§4) and
   `protocol_version` on the wire (§5) — is proposed within decisions he has already made, and needs
   only his review of the spec as a whole.
