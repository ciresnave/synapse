# Design proposal: negotiated AI-to-AI communication in Synapse

**Status: PROPOSAL. Stops for CireSnave's review before any prototype.** No code accompanies this
document.

**Base:** `ciresnave/synapse` `main` `1e6898bd` (2026-09-16), and FAM's
`DESIGN-SYNAPSE-HANDOVER.md` at `ciresnave/FAM` `main` `5111f2b`. Every "today" statement below
cites the measurement behind it, mostly in `CAPABILITY_INVENTORY.md`.

**Owner:** Synapse. FAM is being retired into it, so this is where the protocol lives.

⚠️ **Section 9 is incomplete by design.** The latent-mode technique depends on a survey of 17
papers that the OverMind lane is summarising with non-Claude models. Everything else here stands
without it.

---

## 1. The requirement

### 1.1 CireSnave's constraint, verbatim

> *"some of our models (those running under Fuel eventually) will have access to those deeper
> layers and thus would be able to further optimize in those ways. I would say that would need to be
> one of the potential communication languages negotiated between models or between a model and
> Synapse/FAM."*

### 1.2 The deliverables

1. A **negotiated mode stack**: latent (both ends on Fuel with internal access) → compact
   structured tokens (API-only models) → natural language (fallback, and for findings).
2. **How negotiation works**, building on FAM's negotiated-capability pattern.
3. **Every mode auditable by a party independent of its author, and convertible to readable text on
   demand.**
4. A **near-term compact format for routine lane updates**, usable before any of the rest exists.
   That format now exists as `MESSAGE-FORMATS` v1; §7 builds on it.

### 1.3 Why "independent" is the operative word

A model auditing a language it helped create shares the author's reading, so its audit is not an
independent check. This is the portfolio's existing rule about measurements — *two readings agree
as evidence only if their methods differ* — applied to languages. The design below gives each mode
an auditor that does not share the author's method:

| mode | the independent auditor |
|---|---|
| natural language | any reader |
| compact structured | a **program**: a versioned spec plus a deterministic decoder, which cannot misread |
| negotiated / invented language | an **outside auditor** holding a written spec the negotiation was required to produce |
| latent | a party **outside the coupled group**, auditing the group's outputs. Latent state has no independent decoder, so the coupled models count as **one agent** (§6.3). |

---

## 2. What exists today — the ground this stands on

Measured, not assumed. Section references are to `CAPABILITY_INVENTORY.md`.

| fact | consequence for this design | ref |
|---|---|---|
| The wire format is **plain self-describing JSON**, one `SecureMessage` per datagram or TCP connection. A CPython process with no Rust linkage has sent to and received from Synapse. | A non-Rust agent can speak Synapse today. Every mode here must keep that true. | §2.2 Probes B, D |
| Byte fields go on the wire as **JSON arrays of integers, not base64** — 4.58× raw size. The largest payload that fits a UDP datagram is **~14 KB**. | Latent tensors cannot ride the current encoding. **A binary frame is a prerequisite for latent mode**, not an optimisation. | §2.2 |
| **UDP delivers; TCP delivers since PR #11.** QUIC is a simulation. WebSocket shares TCP's former defect. | Negotiation must be able to pick the transport, and must not believe a transport's name. | §2.2 |
| **No message's sender is authenticated.** `SecureMessage.signature` is written in one place and read in none. | ⚠️ **Negotiation is a downgrade and upgrade attack surface until this is fixed.** See §5.5. | §2.2 |
| **Every delivery confirmation is sender-side.** `DeliveryConfirmation::Received` and `::Acknowledged` are declared and never constructed. | Mode switches need a receiver-side acknowledgement, or a switch can happen on one side only. | §2.2 |
| `metadata` is a free-form `HashMap<String, String>`. | ⚠️ Putting the mode in `metadata` makes it **optional**, and an optional mode field reproduces the "silently absent" state. The mode must be a mandatory field (§8). | §2.2 |
| Receiving is a **poll** (`receive_messages`), with inconsistent semantics across 26 implementations. | There is no push channel to inject into. Mode negotiation must not assume one. | §2.2 |

---

## 3. Principles

Each is taken from a measured failure, here or in FAM.

1. **A capability is declared at handshake and discoverable; it is never a private name a peer must
   be told out of band.** *(FAM handover §2.)*
2. **Also advertise out of band.** The Python MCP SDK fixes its notification bindings before the
   handshake, so discovery alone forces a reconnect before a client can act on what it discovered.
   *(FAM handover §2.)*
3. **The receiver acknowledges.** A mode is in effect only when the receiver has said so. *(FAM §4a;
   Synapse §2.2.)*
4. **A mandatory field, not an optional one.** A message whose mode cannot be determined fails to
   parse. It never arrives looking ordinary. *(Synapse §2.2, the sender-identity finding.)*
5. **Failure is never silent.** A mode that cannot be used produces a *marked* downgrade, not a
   quiet one. *(Synapse's TCP silent drop; FAM's SDK dropping unknown notifications at DEBUG.)*
6. **Name the payload for the message, not the pipe.** *(FAM §3: `WebSocketMessagePush` drifted from
   the wire.)*
7. **One code path per decision.** Mode selection, downgrade and logging each have exactly one
   implementation that every transport calls. *(FAM §4b: four guards written on one branch and not
   its sibling, in one day.)*
8. **Test with a foreign client from the start.** The native client's habits hide divergence.
   *(FAM §4b.)*

---

## 4. The mode stack

Three levels. A pair of parties uses the highest level both support; every level can fall back to
the one below it.

| level | name | who can use it | auditor | notes |
|---|---|---|---|---|
| **L0** | natural language | everyone | any reader | Always available. Findings are carried here, or in L1 with their predicate and ref (§7.4). |
| **L1** | compact structured | any model that can emit a schema | a deterministic decoder | Versioned schemas. Includes **negotiated languages** (§4.2). |
| **L2** | latent | both parties on Fuel, with access to internal layers | a party outside the coupled group | The coupled models count as one agent. Requires a binary frame (§2). Technique pending §9. |

### 4.1 L1 is a family, not one format

L1 is any content type with **a published, versioned schema and a deterministic decoder**. The
portfolio's `MESSAGE-FORMATS` v1 headers are the first (§7). Others can be added without changing
the protocol, because negotiation names the schema and its version.

### 4.2 Negotiated or invented languages are L1, under one extra rule

Two models may agree on a compressed vocabulary of their own. That is allowed only if **the
negotiation produces a written spec**, filed where an outside auditor can read it, before the
language is used. A language with no filed spec cannot be selected. The spec is an artefact with an
identifier and a version, like any other L1 schema.

⚠️ **Meaning drifts between two parties, and a third reader can misread a private language with
confidence.** The filed spec is what an auditor holds the exchange against. If the spec and the
usage disagree, the usage is wrong.

---

## 5. Negotiation

### 5.1 Advertisement

Each party advertises the modes it supports **at handshake**, in a mandatory capability block, and
**also publishes the same block out of band** (principle 2):

```json
{
  "modes": [
    { "level": "L0" },
    { "level": "L1", "schema": "message-formats", "versions": ["1"] },
    { "level": "L1", "schema": "lang.8f3a…", "spec": "<spec id>", "versions": ["2"] },
    { "level": "L2", "space": "<latent space id>", "frame": "binary/1" }
  ]
}
```

L0 is always present. A party that omits it is malformed.

### 5.2 Selection

The initiator proposes an ordered list; the responder picks one and **acknowledges it**. The mode is
in effect only after the acknowledgement arrives (principle 3). Until then both sides use L0.

Negotiation messages themselves are **always L0 or L1**, never latent — the bootstrap must be
readable by an auditor.

### 5.3 Who negotiates

- **Model ↔ model**, for a conversation between two agents.
- **Model ↔ Synapse**, where Synapse acts as a translator or relay, for example rendering L1 to L0
  for a party that only supports L0.

Both use the same messages. Synapse is a party like any other.

### 5.4 Downgrade and renegotiation

Any party may renegotiate at any time. A failed decode, an unknown schema version, or a missing
spec triggers a **marked downgrade**: the receiver replies in the next lower mode and says why. A
downgrade is logged (§6) and is never silent.

### 5.5 ⚠️ Negotiation is an attack surface until senders are authenticated

Synapse does not authenticate senders today (§2). Until it does:

- A forged peer can **force a downgrade**. Mostly a cost problem.
- ⚠️ **A forged peer can select an unauditable mode**, such as a private language with no spec or a
  latent exchange, to move content out of the audit log's reach. **That is the more serious one.**

**Proposal:** L0 and the portfolio's published L1 schemas may be used unauthenticated. **Negotiated
languages (§4.2) and L2 require an authenticated peer.** Sender authentication is a prerequisite for
those modes, not a later hardening step. FAM's voucher chain is the existing design for it
(handover §3).

---

## 6. Auditability

The requirement: every mode is auditable by a party independent of its author, and convertible to
readable text on demand.

### 6.1 L0

The message is the rendering.

### 6.2 L1

The decoder is a **program**. It renders any L1 message to L0 deterministically, from the schema
alone. The same input always produces the same text, and the decoder cannot "interpret". A message
the decoder cannot render is invalid, and the receiver rejects it rather than guessing.

For negotiated languages (§4.2) the decoder is generated from, or checked against, the filed spec.

### 6.3 L2: coupled models are one agent

Latent state has no independent decoder. A readable rendering of it would be a lossy translation
produced by a model — the same kind of author-shared reading §1.3 rules out — so **a rendering is not
an audit.** Instead:

1. **Log the coupling.** Every L2 exchange records which models were coupled, when the coupling
   started and ended, and in which mode. That record is what makes it visible which outputs came from
   a combined agent.
2. **Audit the combined agent's outputs**, exactly as any single agent's outputs are audited.
3. ⚠️ **A coupled group counts as ONE agent for independence.** Its members must never review each
   other's work, because they are not independent of each other.

A readable rendering of latent state remains **allowed as a debugging aid**, stored separately and
labelled as not an audit.

An L2 exchange with no coupling record is **rejected** (principle 4): without it, a combined agent's
output is indistinguishable from an independent one's, which is exactly the confusion this rule
exists to prevent.

### 6.4 The log

- Append-only, one entry per exchange, keyed by `message_id`.
- Each entry states which mode was used. For L0 and L1 the readable text is a **decoding**. For L2 the
  entry is the **coupling record** (§6.3); any debugging rendering lives elsewhere and is labelled as
  not an audit.
- Downgrades and rejections are entries too.
- A missing rendering is an entry that says so. It is never an absent row.

---

## 7. Near-term: `MESSAGE-FORMATS` v1 is L1's first schema

**It already exists and is in use.** `C:\Projects\MESSAGE-FORMATS.md` (v1, 2026-09-16) defines
one-line `[TYPE] <subject> key=value ...` headers for routine messages and `set_summary`, with prose
only on `note:` lines. It was introduced to cut token spend after the 2026-09-16 budget outage.
**This design builds on it rather than defining a second format.**

v1 is already readable by any person, so its L0 auditor exists. What L1 adds is the second auditor —
a **program** — and that needs three things v1 does not yet have.

### 7.1 A formal grammar

v1 is specified in prose. Reading it as a parser would, these are the points a decoder has to guess
at today (all taken from the text of v1):

| where | the ambiguity |
|---|---|
| `[TYPE] <subject> key=value` | `[STATUS]` has no subject; the other types do. A subject is presumably the first token without `=`, but that rule is unstated. |
| list separators | The rules say lists use `,`, but `[ASK]` writes `options=ship-wasm\|drop-step` with `\|`. |
| templates vs values | Templates use `\|` to mean "one of" (`board=<open PRs\|0>`), while `[ASK]` values use it as a separator. A reader of a template cannot tell which. |
| `=` inside a value | Unstated. "Split on the first `=`" is the obvious rule, but it is not written down. |
| sub-structure | `waiting=<who:what>` and `id-from=worktree:…` use `:`; nothing says whether `:` is structural. |
| `-` | Means "none". Unambiguous only while no real value is `-`. |

**Proposal:** add a short grammar to `MESSAGE-FORMATS.md` that settles each row, and make a value's
sub-structure part of each key's definition rather than a general rule.

### 7.2 A version on every message

v1 is versioned as a document, but a message does not say which version it follows. Once v2 exists, a
decoder cannot tell which grammar applies. By principle 4 the version must be carried, not assumed.

**Proposal:** `[STATUS/2] …` carries its version in the tag. **An untagged header means v1**, so every
message already sent stays valid.

### 7.3 A deterministic decoder

A program that, for each message:

1. parses the header by the grammar (§7.1);
2. **rejects** a message with an unknown type, an unknown version, or a missing required key, rather
   than guessing;
3. renders it to a sentence of L0 by fixed rules — same input, same text, every time.

The decoder is the L1 auditor. Because it is a program, it cannot share an author's misreading.

### 7.4 A candidate v2 type: `[FINDING]`

v1 carries findings as prose on `note:` lines. That is fine for reasoning, but it leaves the
portfolio's rule *"relay a measurement with its predicate and its ref"* to discipline. `[FIX]` already
does the equivalent for retractions structurally (`reached=`, `fixed-in=`). A matching type would do
it for claims:

    [FINDING] <topic> claim=<what> ref=<sha> by=<instrument> result=<value> status=<measured|inferred|relayed> from=<lane, when relayed>

    [FINDING] synapse-clippy claim=main-passes-clippy ref=1e6898bd by=cargo-clippy--D-warnings result=exit-0 status=measured

⚠️ **`status=inferred` is allowed and must be stated.** An inference written in a measurement's
grammar is the failure this key exists to prevent. `from=` is required when `status=relayed`.

This is a **candidate**, not a change. `MESSAGE-FORMATS.md` belongs to the portfolio PM.

### 7.5 What stays out of scope here

No size saving is claimed for any of this. Token counts depend on the tokenizer, so any claim should be
measured with the consuming models' tokenizers first.

---

## 8. Mapping onto Synapse's wire

- Add a **mandatory** `mode` field to `SecureMessage`, beside `security_level`, carrying the level,
  schema and version. Do not put it in `metadata` (§2).
- Add a **binary frame** for L2, negotiated as part of the mode. The current JSON encoding of byte
  arrays is ~4.6× raw and caps a UDP payload near 14 KB (§2).
- Add a **receiver acknowledgement message**, and have `DeliveryConfirmation::Received` and
  `::Acknowledged` actually constructed from it (§2). Mode selection depends on it (§5.2).
- Add the **capability block** (§5.1) as a message type, exchanged at session start.
- The audit log (§6.4) is a Synapse component, called from one place by every transport
  (principle 7).

---

## 9. ⚠️ PENDING: latent-mode technique

**Waiting on the OverMind lane's summaries of 17 papers.** This section is deliberately empty rather
than guessed.

What the negotiation already requires of any L2 technique, independent of the papers:

- a **space identifier**, so two parties can tell whether their latent spaces are compatible;
- the **frame**: binary, with the dtype and dimensions stated;
- the **coupling record** (§6.3), so the coupled models can be treated as one agent;
- **authenticated peers only** (§5.5).

To be filled from the summaries: which layers are exchanged, whether spaces must match exactly or
can be bridged, what a "space identifier" concretely is, and what the measured gains are.

---

## 10. What must not be inherited

From FAM's handover and Synapse's inventory, restated as constraints on the implementation:

- **The ack belongs to the receiver.** *(FAM §4a.)*
- **No optional field where absence and a negative value would read the same.** *(Synapse §2.2.)*
- **No type that advertises a guarantee the implementation never produces.**
  `DeliveryConfirmation::Received` is the example: declared, never constructed. *(Synapse §2.2.)*
- **No function named for a check it does not perform.** `verify_message_sender` performs
  authorisation on an unauthenticated claim. *(Synapse §2.2.)*
- **The value FAM has above its transport** — sealing, identity resolution, replay suppression and
  the voucher chain — must be carried over, not assumed. *(FAM §3.)*

---

## 11. Questions for CireSnave

1. **Authentication first?** §5.5 proposes that negotiated languages and L2 require authenticated
   peers, which makes sender authentication a prerequisite for them. Is that the right order?
2. **Who may file a negotiated-language spec, and where?** A spec must be readable by an outside
   auditor; it needs a home that neither negotiating party controls.
3. **When does a coupling stop counting?** §6.3 treats coupled models as one agent. If two models
   were coupled and later work separately, when — if ever — are they independent again for review?
4. **Extend `MESSAGE-FORMATS`?** §7 proposes a grammar, a version tag and a `[FINDING]` type. None
   needs a Synapse change; the file is the portfolio PM's.
