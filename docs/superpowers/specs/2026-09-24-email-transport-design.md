# The email transport slice

**Status: AWAITING PM SPEC REVIEW** — brainstormed and approved by CireSnave (design questions answered directly; see §1a for the four decisions that shaped this document — "It's fine. Route it."), routed to the PM for readiness review and, at plan time, the spec-versus-plan drift check. Written in a worktree branched fresh from `main` at `93f9fad4` (after #54's merge); the implementation plan should branch fresh from `main` at whatever SHA is current when planning starts.

**Follows:** the transport contract (PR A #44 + PR B #47) and the QUIC slice (#50), which established the shape every transport here follows: `TransportReceive::receive_raw` into a sealed `RawInbox`, honest delivery claims, `SynapseError::MessageRefused` for oversize sends kept out of the circuit breaker, a byte-budget queue, measured (not aspirational) `capabilities()`/`estimate_metrics()`, and — new as of #54 — `TransportCapabilities::unmeasured_metrics` declaring which `TransportMetrics` fields are not real observations.

**Precedes:** f2 (agent-certificate tooling), then the 2.0.0 publish — **with CireSnave's approval only.**

**QUIC's own forward note called email "authenticated by construction" without defining the phrase. §3 below retracts that claim precisely: it is false as a general statement, and this document does not repeat it.**

## 1. What exists today

- `src/transport/email_simple.rs`'s `SimpleEmailTransport` implements `abstraction::Transport` + `TransportReceive` honestly: it validates addresses, but every `send_message`/`test_connectivity`/`receive_raw` path returns an explicit "not implemented yet; it arrives in the email slice" error rather than a faked success. This is the honest state this slice builds from, the same pattern QUIC's stub used before its slice landed.
- `src/email.rs`'s `EmailTransport` is a **real**, `lettre`-based SMTP client (`SmtpTransport::relay`, `Credentials::new`) — not simulated — but it does not implement `abstraction::Transport` at all; it has its own `send_message(&SimpleMessage)`/`receive_messages()` methods, and its `receive_messages_imap` is a documented simulation (CAPABILITY_INVENTORY.md §found via the FAM lane: "an IMAP simulation whose body is a comment describing what a real implementation would do"). It is feature-gated behind the optional `email` Cargo feature (pulls in `lettre`, `async-smtp`, `async-imap`, `mail-parser`), which is not enabled by default. **Reference for the send path** (§4): its lettre wiring is real and should be reused/adapted where it holds up, per CireSnave's answer in §1a.
- `src/email_server/` (`smtp_server.rs`, `imap_server.rs`, `auth.rs`, `connectivity.rs`) is a substantially real SMTP+IMAP server pair: both bind real `TcpListener`s, both implement their protocols beyond trivial stubs (auth, message storage). `connectivity.rs`'s `ConnectivityDetector`/`ServerRecommendation` already models exactly the three-way choice this slice needs (§2) but is currently unused by anything — CAPABILITY_INVENTORY.md: "Compiles. `email_server_demo` example compiles. No test binds a port or speaks SMTP/IMAP." This slice is the first thing to wire it to the `Transport` trait and to test it end-to-end.
- `EmailConfig`/`SmtpConfig`/`ImapConfig` (`types.rs`) store `password: String` with `#[derive(Debug, Serialize, Deserialize)]` on the containing structs — a `{:?}` print, a log line, or a serialized config dump all leak it today. No credential has ever been put through these structs in anger (every existing email transport is either honestly non-functional or feature-gated off by default); this slice is what first makes that live. Fixed in this slice per CireSnave's answer in §1a.
- `TransportType::Email` exists; no factory currently constructs anything but `SimpleEmailTransport`'s honest refusal.

## 1a. Decisions from brainstorming (CireSnave, 2026-09-24)

Quoted from the design conversation, not paraphrased:

1. **Receive path:** *"Run our own SMTP server"* — not IMAP-polling an external mailbox as the primary mechanism.
2. **Reuse vs. rewrite:** *"Either. Start by attempting reuse and verify but if that fails to support all of our needs, write fresh."*
3. **Credential exposure:** *"Yes, fix it in this slice."*
4. On the roles of SMTP vs. IMAP, verbatim, and this is the decision that sets §2's three-mode shape: *"SMTP and IMAP serve different purposes. SMTP is for directly sending and receiving mail. IMAP, on the other hand, is used to retrieve mail from remote servers or to allow remote clients to retrieve mail from our server. Obviously, SMTP serves our needs best but there are situations where a computer is unable to directly send or receive mail due to network limitations, corporate rules, or other limitations. That means both are needed but not so much to allow humans to read the messages that pass between agents but to allow one server to send and receive messages on behalf of another as a sort of proxy."*

Consequence of (4): **IMAP is not for a human to read agent traffic.** It exists solely for the case where this transport cannot run its own inbound SMTP listener (firewalled network, corporate policy, no public port 25) and must instead have another mail server hold and forward messages on its behalf, retrieved by IMAP. `email_server::SynapseImapServer`'s human-facing-mailbox capability is unrelated to this transport and is not wired into it.

## 2. Scope

One slice, staged the way QUIC's was:

1. **Skeleton:** `EmailTransportImpl` + a real `EmailTransportFactory` (replacing `SimpleEmailTransport`'s permanent refusal). Direct mode only (§3): send via `lettre`, receive via `email_server::SynapseSmtpServer`'s message store, over loopback. Reuse `src/email.rs`'s lettre wiring per §1a(2); write fresh whatever doesn't fit the `Transport` contract's shape.
2. **Credential handling:** the redacting wrapper (§5) for `SmtpConfig`/`ImapConfig`'s `password` field, applied before any other task puts a real credential through these structs.
3. **Relay-out and external modes:** wire `ConnectivityDetector`'s existing `RelayOnly`/`ExternalProvider` assessment (§3) to actually select behavior — smart-host relay for send, IMAP polling (via `async-imap`) of a configured proxy mailbox for receive.
4. **Size limits & backpressure:** `max_message_size`, a byte-budget queue, `MessageRefused` for oversize sends refused before any SMTP handshake — the pattern every prior transport in this contract established.
5. **Honest capabilities & metrics:** real `capabilities()`/`metrics()`/`estimate_metrics()` per §6, including `unmeasured_metrics` (#54) for whatever this slice genuinely cannot measure.
6. **Verification:** mutation check with an execution-time-bounded timeout, full run in a target directory that has only built current source, firewall event count with a positive control, fmt/clippy, docs/breaking-change list — the same shape as QUIC's Task 5.

**Out of scope for this slice:** IMAP as a human-facing mailbox (§1a), SPF/DKIM/DMARC verification of inbound mail's envelope sender (§3 — irrelevant given this transport never trusts the envelope sender at all), running `email_server`'s IMAP server for anything but the relay-out/external proxy case, WASM.

## 3. Sender authentication — precisely, retracting "authenticated by construction"

**Claim retracted:** email is not authenticated by construction, and no general statement of that shape is true for it. This section replaces that phrase with the actual mechanism.

**What this transport trusts:** exactly what every other transport in this crate trusts — the `SecureMessage`'s own signature, verified by `TransportManager::receive_messages` after `receive_raw` returns, identically regardless of which transport carried the bytes. Email adds no additional trust and needs none: `send_message` embeds the whole signed `SecureMessage` as JSON in the email body (§4), and `receive_raw` extracts and passes through that same JSON, untouched, for the manager to verify exactly as it would for TCP, QUIC, or NAT traversal.

**What this transport explicitly does *not* trust:** the SMTP envelope. `MAIL FROM`, the `From:` header, and the connecting client's IP are all easily forged by an unauthenticated remote party without SPF/DKIM/DMARC (which this slice does not implement) — a "message arrived at our own mail server" is not "message came from who it claims." This design **never reads or relies on the SMTP-level sender identity for anything security-relevant.** The only sender identity that matters is the one signed inside the JSON body, checked the same way as any other transport's.

**A second, unrelated control this slice does add: SMTP submission authentication.** `email_server::auth`'s `AuthHandler`/`SynapseAuthHandler` requires `AUTH` before our own `SynapseSmtpServer` accepts a message for relay from a local client — this is an anti-open-relay control (who may use our server to send mail), not a claim about the content's sender. **These two facts must not be conflated:** a message can arrive over an authenticated SMTP submission and still carry a `SecureMessage` whose signature fails verification (the submitting client lied about the payload), and a message can arrive over an unauthenticated inbound connection from the public Internet and still verify perfectly (the envelope is irrelevant; the signature is what's checked). "Authenticated by construction" as a phrase invites exactly this conflation — that is why it is retracted rather than defined more carefully. **What would have to be true for the retracted claim to hold:** every SMTP hop between sender and receiver enforcing SPF+DKIM+DMARC and this transport actually checking those results before trusting the envelope — none of which this slice does, and none of which would be sufder even then, since a compromised or malicious *authenticated* relay could still forge an envelope its own DKIM key covers.

## 4. Wire format and delivery claim

Same pattern as NAT traversal's PR B repair and every transport since: serialize the whole `SecureMessage` via `serde_json::to_vec`, never hand-pick fields into a template string (which corrupts binary `encrypted_content` by lossy-UTF-8-converting it — the exact bug NAT traversal had). The JSON becomes the email body; the subject and headers carry no security-relevant information and exist only for human-readable debugging of raw mail if someone inspects a queue by hand.

`send_message` returns `DeliveryConfirmation::Sent` once the SMTP transaction completes (a `250 OK` from the immediate next hop — our own relay or the recipient's MX, depending on mode), **never** `Delivered`. This is weaker evidence than TCP/HTTP's in-band confirmation and weaker than QUIC's transport-ACK: a `250 OK` from an intermediate relay says only that *that hop* accepted responsibility for further relay, not that the message reached its final destination's mailbox, let alone that anything read it. `delivery_ack.rs`'s existing transport-agnostic application-level acknowledgement (§4 of the QUIC spec) is what closes this gap, and email needs zero new ack code for the same reason QUIC didn't: an ack is just another `SecureMessage`, carried over whatever transport is available, including this one.

## 5. Credential handling

`SmtpConfig`/`ImapConfig` (`types.rs`) gain a `SecretString`-style wrapper type around `password`:

- `Debug` prints a fixed redaction (`"[redacted]"`), never the value.
- **`Serialize`/`Deserialize` must round-trip the real value — measured, not guessed.** `Config::to_file`/`Config::from_file` (`src/config.rs:101-114`) serialize the *entire* `Config`, including `email: EmailConfig`, to and from a TOML file via `toml::to_string_pretty`/`toml::from_str` — this is the real, load-bearing path an operator uses to persist an entity's configuration (with real SMTP/IMAP credentials filled in) to disk and reload it on next run. A blanket redact-on-serialize would make `to_file` write `"[redacted]"` in place of the real password, and `from_file` would then load that literal string back as the credential, silently corrupting every saved config. **Positive control that this search isn't vacuous:** the same file's `EntityConfig` (`config.rs:12`) is confirmed to serialize/deserialize normally via the same derive, so the mechanism the search is checking for demonstrably exists and is found where it applies. So: the wrapper's `Serialize`/`Deserialize` impls pass the real value through unchanged (this is what makes `Config::to_file`/`from_file` keep working) — the protection this wrapper adds is against *accidental* exposure via `Debug`/logging, not against deliberate, intended persistence to a config file the operator controls. `test_config_file_operations` (`config.rs:436-448`) does not currently assert the password itself survives the round-trip (only `entity.local_name`); this slice should extend that test to assert `loaded_config.email.smtp.password == config.email.smtp.password`, closing the gap that let this go unverified until now.
- The wrapped value is still a plain `String` in memory once deserialized, and is written to disk in plain text inside the TOML file (no encryption-at-rest) — this slice does not add memory-zeroing-on-drop, OS-level secret storage, or config-file encryption. That is a larger, portfolio-wide credential-handling question (every project with a password field has the same gap), not something to solve inside one transport's slice.

**Who can read it:** whatever process holds the `EmailTransportImpl` in memory — same as any other in-process config value. `SecretString`'s job is to stop *accidental* exposure through logging/debug-printing/serialization, not to defend against an attacker who already has code execution in the same process.

## 6. Capabilities and metrics

Following #54's contract:

- `max_message_size`: a configured limit refused at construction if unparseable/zero, same pattern as every prior transport. Default should be measured against what an email-body-embedded JSON payload actually costs (base64/JSON overhead over the "wire" bytes), not assumed equal to TCP's.
- `features`: describe what each configured mode actually does; do not advertise `store_and_forward`-style language implying guaranteed eventual delivery this transport cannot promise (SMTP relays can and do drop mail silently on misconfiguration).
- `estimate_metrics()`: a real probe per mode — Direct mode can attempt a connection to the target's MX; Relay-out/External modes report based on the configured relay/mailbox's last-known reachability, not a fixed guess.
- `metrics()`: real counters for messages/bytes sent and received, and send/receive failures, matching QUIC's `AtomicU64` pattern. `reliability_score` computed from real attempts (QUIC's exact formula: `messages_sent / (messages_sent + send_failures)`, `1.0` when untried — do not invent a second formula, per the standing rule from #54's review). **`average_latency_ms` should be declared via `unmeasured_metrics` unless this slice actually instruments it** (email's latency is dominated by relay hops this transport doesn't control end-to-end, so a plausible-looking figure is exactly the fabrication #54 exists to prevent — measure it for real from this transport's own send-to-`250-OK` time, or declare it absent; do not estimate).

## 7. Testing and verification

The hard constraint: no test may depend on a live external mail provider (Gmail, etc.) — that is both a CI-reliability risk and, per §1a, not even the primary path this slice builds.

- **Direct mode:** the existing `node`/`round_trip`/`Pair` harness (`tests/transport_repairs.rs`), adding `TransportType::Email` to `port_key`/`socket_of` and an `email()` factory helper — alice's `EmailTransportImpl` sends via `lettre` to bob's own `SynapseSmtpServer` on loopback; bob's `receive_raw` drains its message store. Same shape as every other transport's end-to-end test.
- **Relay-out/External modes:** point the transport's IMAP receive path at our own `SynapseImapServer` on loopback with its message store pre-seeded (standing in for "the external mailbox already has mail"), rather than any real provider. Send-side for these modes still goes through our own `SynapseSmtpServer` acting as the "smart host," not a real external relay.
- **Credential redaction:** a test asserting `format!("{:?}", config)` never contains the literal password string, mirroring how other security-relevant invariants in this crate are tested (e.g., `security_claims_match_the_tree.rs`'s pattern of testing a *property* rather than one code path).
- **Verification (final task), same shape as QUIC's Task 5:** mutation check with a bounded timeout, full run in a target directory that has only built current source, firewall event count with a positive control, `cargo fmt --check`/`cargo clippy -- -D warnings` (CI's actual invocation, plus `--lib --tests --all-targets` given issue #53's documented gap), and a breaking-changes list for the eventual PR body.

## 8. Open questions for the PM/implementation plan

None blocking — every decision in this document reflects an explicit answer from brainstorming (§1a) or an explicit precedent already established by a merged slice (QUIC, #54). The implementation plan should:

- Confirm current `lettre`/`async-imap`/`mail-parser` version pins at plan time (CireSnave's standing rule: dependencies on their most recent versions).
- ~~Decide whether any existing call site needs `EmailConfig`'s real password to round-trip through serialization~~ — **answered by measurement, not left to the plan:** yes, `Config::to_file`/`from_file` (`config.rs:101-114`) do, and §5 now specifies the wrapper accordingly (`Serialize`/`Deserialize` pass the real value through; only `Debug` redacts).
- Measure (not assume) the `max_message_size` default and the adversarial parsing factor for this transport's actual send/receive path, per §6 and the standing discipline (CLAUDE.md §7: state bounds as measured vs. derived).
