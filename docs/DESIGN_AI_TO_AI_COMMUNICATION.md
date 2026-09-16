# Design proposal: negotiated AI-to-AI communication in Synapse

**Status: PROPOSAL. Stops for CireSnave's review before any prototype.** No code accompanies this
document.

**Base:** `ciresnave/synapse` `main` `1e6898bd` (2026-09-16), and FAM's
`DESIGN-SYNAPSE-HANDOVER.md` at `ciresnave/FAM` `main` `5111f2b`. Every "today" statement below
cites the measurement behind it, mostly in `CAPABILITY_INVENTORY.md`.

**Owner:** Synapse. FAM is being retired into it, so this is where the protocol lives.

**Literature:** §4.3 and §9 draw on the OverMind lane's summaries of 17 papers
(`OverMind/research/ai-to-ai-communication-papers.md`, OverMind#28). The papers' figures are relayed
from those summaries, not reproduced here.

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
| **L2** | latent | both parties on Fuel, with access to internal layers | a party outside the coupled group | The coupled models count as one agent. Needs a binary frame and compatible latent spaces (§9). |

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

### 4.3 What the literature says about L1

From the same summaries as §9, with the same caveat: the papers' own figures, relayed, not reproduced.

- **Agora (#1) is the closest existing design to §4.2 and §5.** Agents negotiate plain-text protocol
  documents identified by a **hash**, generate code routines that process structured JSON **without
  calling an LLM**, and fall back to natural language. It reports about **5×** lower cost than natural
  language in a 100-agent network. Negotiating costs about twice one natural-language query (0.043 vs
  0.020 USD), so it **pays off after about three uses**.
  - ⚠️ **Two cautions for this design.** First, Agora's routines are **written by the negotiating
    model**. A generated decoder is a program, but it is **not independent** of its author until someone
    else checks it against the spec. Second, Agora reports **duplicate protocols** emerging in partly
    isolated networks — an argument for one shared registry of specs (§11, question 2).
- **CLSR (#9):** agents evolve Language Symbolism Frameworks — textual lexicons, grammars and usage
  constraints. Those are **filed specs** in this design's sense. It reports **3–6×** fewer completion
  tokens than chain-of-thought, and notes the output is harder for humans to read.
- ⚠️ **BabelTele (#13) is NOT L1 under this design.** It is compact non-standard text written by one
  model and read back by another, with **no spec and no program decoder** — its only decoder is a model.
  It reports text at **27.9%** of original length, but also a quality drop of **10.75–14.95 points** on
  QuALITY.
- **GlossoGen (#5):** emergent shorthand appeared only when agents had a deliberation channel, and the
  open-weight models tested (Qwen3-32B, Llama-3.3-70B-Instruct) **did not form one at all**. The
  portfolio's non-Claude fleet leans on open-weight models, so **prefer designed schemas over emergent
  languages** for it.
- **AutoForm (#17):** letting models choose structured non-prose formats cut multi-agent tokens by up to
  **72.7%** (GPT-4 with GPT-3.5, HotpotQA). Weaker models sometimes produced over-compressed or
  hallucinated output.
- **#2:** emergent languages drifted toward longer messages and degenerate vocabularies — more support for
  requiring a filed spec.

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

For negotiated languages (§4.2) the decoder may be generated from the filed spec, but ⚠️ **a decoder
written by one of the negotiating parties is not independent of them** (§4.3, Agora). It must be
checked against the spec by someone outside the negotiation before it counts as the auditor.

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

**It exists and is in use.** `C:\Projects\MESSAGE-FORMATS.md` defines one-line `[TYPE] <subject>
key=value ...` headers for routine messages and `set_summary`, with prose only on `note:` lines. It was
introduced to cut token spend after the 2026-09-16 budget outage. **This design builds on it rather than
defining a second format.**

v1 is already readable by any person, so its L0 auditor exists. L1 adds a second auditor that is a
**program**. That needs a grammar, a version on each message, and a decoder.

### 7.1 Already adopted (2026-09-16)

Re-read from the current `MESSAGE-FORMATS.md`, not from the proposal:

- **Version tag:** `[READY v2]`; an untagged header is v1.
- **`<x|y>` in a template means "one of"**, and a real value never contains `|`. `[ASK]` options are
  now comma-separated.
- **A `[FINDING]` type:** `[FINDING] <topic> ref=<sha> by=<lane> status=<open|fixed|retracted>
  basis=<measured|inferred|relayed>`, with the finding itself on `note:` lines.

`basis=` was added after this section first noted its absence. With it, a finding's header carries its
ref *and* whether it was measured, inferred or relayed — the portfolio rule *"relay a measurement with
its predicate and ref"* is structural rather than a matter of discipline. The instrument itself still
lives in the `note:` prose.

### 7.2 Grammar

For the PM to link from `MESSAGE-FORMATS.md`. It settles the points the prose leaves to a reader.

```abnf
message    = header *( LF note-line )
header     = "[" type [ SP version ] "]" [ SP subject ] *( SP pair )
type       = 1*( %x41-5A )                 ; A-Z
version    = "v" 1*DIGIT                   ; absent => v1
subject    = token                         ; the first token, only if it contains no "="
pair       = key "=" value                 ; split on the FIRST "=" only
key        = 1*( ALPHA / DIGIT / "-" )
value      = "-" / token                   ; "-" means none
token      = 1*( %x21-7E )                 ; visible ASCII, no space
note-line  = "note: " text
```

Rules the grammar cannot express:

1. **Subject.** `[STATUS]` has none. For every other type, the first token after the tag is the subject
   if and only if it contains no `=`.
2. **`=` inside a value** is allowed; only the first `=` in a pair splits it.
3. **`|` inside a value is invalid.** A decoder rejects it (adopted rule).
4. **Sub-structure is defined per key, never globally:**

| key | sub-structure |
|---|---|
| `board`, `red`, `files`, `options`, `why` | a `,`-separated list |
| `waiting`, `need` | `who:what`, with a list of pairs `,`-separated |
| `affects` | `repo:files`, with `files` a `,`-separated list |
| `checks` | `pass/total`, two integers |
| every other key | an opaque token: `;`, `+`, `:`, `/` inside it are literal text |

The last row is deliberate. v1 examples use `;` (`action=…Err;use-…`) and `+`
(`need=FUEL3:fix+answer-threads`) as prose-like joiners. Parsing them would invent structure the authors did not declare.

5. **Required keys** are the ones in each type's template, in template order. Extra keys may follow them.

### 7.3 The decoder

A program that, for each message:

1. parses the header by §7.2;
2. **rejects** a message with an unknown type, an unknown version, a missing required key, or a `|` in a
   value, rather than guessing;
3. renders it to L0 by fixed rules: one sentence per header, keys in template order, `-` rendered as
   "none". The same input always produces the same text.

Because it is a program, it cannot share an author's misreading. That is what makes it the L1 auditor.

### 7.4 Out of scope here

No size saving is claimed. Token counts depend on the tokenizer, so measure with the consuming models'
tokenizers before claiming one.

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

## 9. Latent mode (L2)

**Source.** The OverMind lane's summaries of 17 papers, `OverMind/research/ai-to-ai-communication-papers.md`
(OverMind#28, head `f8483a23`). They were written by non-Claude models, and each quoted figure was checked
mechanically against the paper text. ⚠️ **Every number below is the paper's own claim, relayed through
that summary. None was reproduced here.** Paper numbers (#N) follow that file.

### 9.1 What gets exchanged

| representation | papers | what crosses the wire | access needed |
|---|---|---|---|
| hidden states | #7 LMNet, #15 RecursiveMAS | dense vectors from internal layers | internals, plus **training** of per-pair adapters |
| state deltas alongside tokens | #10 SDE | natural-language tokens **plus** a per-token difference between adjacent hidden states at chosen layers | internals; same base model |
| KV cache | #16 DroidSpeak | KV cache for non-critical layers, embedding cache at transition layers | internals; **same foundation model** |
| (survey of the above, plus input embeddings) | #14 Beyond Tokens | — | — |

### 9.2 The deciding constraint: are the two latent spaces compatible?

- **Same base model:** training-free (#14). #16 says outright that it does not work across different
  foundation models. #10 assumes agents share a base model.
- **Different models:** need a **trained adapter for each pair** — #7's trainable edges, #15's
  RecursiveLink per pair of agents, #14's learned projections.

So L2 has two sub-modes:

| sub-mode | requires | notes |
|---|---|---|
| **L2-same** | both parties run the same base weights, with internal access | This is what CireSnave's *"access to those deeper layers"* enables. |
| **L2-bridged** | a trained adapter for the pair | Also needs **training** capability. The adapter is trained on both models, so it belongs to the coupled group (§6.3). |

### 9.3 The space identifier, concretely

Negotiation must name enough for both sides to check compatibility before exchanging anything:

- **base weights**, by content hash — the same idea as Agora's protocol hash (#1);
- **representation kind:** hidden state, state delta, KV cache or embedding;
- **layer set** (§9.4);
- **dtype and dimensions**;
- **tokenizer**, by content hash, since #10 aligns deltas to tokens;
- for L2-bridged, the **adapter**, by content hash and version.

Two parties whose identifiers differ in any field do not use L2 with each other.

### 9.4 Layer selection is part of the mode

- #10: injecting deltas into **all** layers degrades generation, so layers must be chosen.
- #16: on average only **11%** of layers are "critical" and must be recomputed. Quality degrades when real
  data drifts from the offline profiling data.

So the layer set is **negotiated and versioned**, and the profile it came from has a version too. A
profile invalidated by drift is a reason to renegotiate, and the downgrade is marked (§5.4).

### 9.5 Transport

- #14 reports high transport cost for full KV caches; #10 reports added bandwidth.
- Synapse today: byte fields cost **4.58×** raw as JSON, and a UDP payload caps near **14 KB** (§2).

So L2 needs the **binary frame** (§8) and a **stream transport with receiver acknowledgement**. UDP does
not fit L2 payloads. QUIC cannot be the L2 transport while it remains a simulation (§2).

### 9.6 Integrity

#14 lists tampered or untrusted latent states as a security risk. ⚠️ **A latent payload is injected
into the receiver's forward pass**, so tampering manipulates the receiving model directly, more directly
than text can. Therefore:

- L2 is limited to **authenticated peers** (§5.5);
- every L2 payload is **integrity-checked before injection**, and rejected otherwise.

#12 (LACP) is one existing signed-envelope design: a JWS envelope with transaction IDs for idempotency. Its
reported cost is **+30%** payload size on realistic messages and **up to +500%** on tiny ones such as
heartbeats.

### 9.7 Audit

The coupled-agent rule (§6.3) applies unchanged. #10 is worth noting: SDE keeps the natural-language
tokens alongside the deltas, so the coupled group's token stream exists. That helps debugging. **It is
still not an audit** of what the deltas carried.

### 9.8 Reported gains

Relayed from the summaries, not reproduced:

| paper | reported gain | compared with | conditions |
|---|---|---|---|
| #16 DroidSpeak | up to **4×** throughput; about **3.1×** faster prefill | no cross-model sharing | same foundation model, eight model pairs |
| #15 RecursiveMAS | **1.2–2.4×** end-to-end speed-up; **34.6–75.6%** fewer tokens; **+8.3%** accuracy | recursive LMs and multi-agent baselines | 9 benchmarks; trained RecursiveLink |
| #10 SDE | **+0.3–17.3%**, by task family | the best of natural language or CIPHER | same base model |
| #7 LMNet | **+30.5%** relative | prompting | Qwen2.5-0.5B; trained |
| #14 | e.g. up to 24× speed-up (Interlat) | — | ⚠️ **a survey: these are other papers' results, second-hand** |

### 9.9 Not yet known

- **Whether any of this works on the models Fuel will run.** No paper here used them.
- **Hosted API models cannot take part in L2 at all.** #7, #10 and #15 each say so. The free-tier
  providers the portfolio routes routine work to are API-only.
- **Gibberlink (#8) was not fetched**, and #4's ID resolves to an unrelated survey. Both are on
  CireSnave's board.

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
   auditor, so it needs a home neither negotiating party controls. Agora (#1) also reports duplicate
   protocols emerging without one shared registry.
3. **When does a coupling stop counting?** §6.3 treats coupled models as one agent. If two models
   were coupled and later work separately, when — if ever — are they independent again for review?
4. **Is training in scope for Fuel?** L2 between *different* models needs a trained adapter for each
   pair (§9.2). L2 between identical models does not. Which should Synapse plan for first?
