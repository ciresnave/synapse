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
2. `SynapseRouter` gains `security_sink: Option<Arc<dyn SecuritySink>>` and a builder
   `with_security_sink(self, sink) -> Self`, like `TransportManagerBuilder::security_sink`. It hands
   the sink to `email_server` if one is present.
3. `ensure_email_transport` attaches the sink after construction and before `start`, the order
   `TransportManager` uses. Construction moves behind a private `ensure_email_transport_with(build)`
   so a test can inject a transport that records what it was given.

## Tests (red first)

- `the_email_transport_gets_the_sink_before_it_starts`: a recording transport sees `attach` and then
  `start`, and the attached sink is the router's (`Arc::ptr_eq`). Negative control: with no sink set,
  it sees only `start`.
- `the_email_server_records_to_the_routers_sink`: a router holding a `SynapseEmailServer`, given a
  capture sink, records a failed SMTP login's `AuthFailure` event. Negative control: before the
  sink is set, the same failure records nothing.

## Out of scope (reported, not fixed)

- `MultiTransportRouter` builds its own Direct-mode `EmailTransportImpl` through the same provider and
  attaches no sink either. That is a third sink-less path, outside the #88 wording.
- Alert DELIVERY is still only a seam: `AlertTransport` has no implementation, pending board 131.
