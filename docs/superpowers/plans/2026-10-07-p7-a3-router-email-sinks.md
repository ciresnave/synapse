# P7 follow-up (a3): security sinks for `router_merged`'s email paths

**Ask (PM order, item a3; #88 body):** "Two paths enforce the limits but have no sink yet:
`router_merged`'s email transport and its `SynapseEmailServer`."

**Finding at origin/main 79b4533.** Both paths already have a sink *setter* from #88
(`Transport::attach_security_sink`, which `EmailTransportImpl` forwards to its Direct-mode SMTP
server; `SynapseEmailServer::with_security_sink`). What is missing is the wiring: `SynapseRouter`
has no way to accept a sink, `ensure_email_transport` never calls `attach_security_sink`, and the
`email_server` field (always `None` today) is held in an `Arc`, which a consuming builder cannot reach.

## Changes

1. `SynapseEmailServer::set_security_sink(&self, sink)`, matching the SMTP and IMAP servers' own
   setters. `with_security_sink` calls it.
2. `SynapseRouter` gains a sink slot that its clones share (`Arc<OnceLock<..>>`; the first call wins,
   as in `EmailSecurity`) and `async fn set_security_sink(&self, sink)`. It passes the sink to
   `email_server` if one is present (nothing populates that field today) and to an email transport
   that is already built.
3. `ensure_email_transport` attaches the sink after construction and before `start`, the order
   `TransportManager` uses. It reads the sink, starts the transport and stores it under one write lock,
   so a concurrent `set_security_sink` cannot miss it. Construction moves behind a private
   `ensure_email_transport_with(build)` so a test can inject a transport that records what it was given.

The first draft was a consuming builder with a field each clone copied. Review finding 1 showed that a
clone made earlier could build the transport without the sink, so the slot became shared.

## Tests (red first)

- `the_email_transport_gets_the_sink_before_it_starts`:
  - A recording transport sees `attach` and then `start`. The sink it was given is the router's
    (pointer equality), even when a clone made before the sink was set builds the transport.
  - A transport built before the sink is set sees `start` and then `attach`.
  - Negative control: with no sink set, it sees only `start`.
- `the_email_server_records_to_the_routers_sink`: a router holding a `SynapseEmailServer` is given a
  capture sink, and the sink records a failed SMTP login's `AuthFailure` event.

Mutation check: deleting either wiring line fails its test.

## Out of scope (reported, not fixed)

- `MultiTransportRouter` builds its own Direct-mode `EmailTransportImpl` through the same provider and
  gives it no sink either. That is a third path with no sink, outside the #88 wording.
- Alert DELIVERY is still only a seam: `AlertTransport` has no implementation, pending board 131.
