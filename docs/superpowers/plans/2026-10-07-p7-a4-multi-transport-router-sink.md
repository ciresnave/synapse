# P7 follow-up (a4): a security sink for `MultiTransportRouter`'s email transport

**Ask (PM, 2026-10-07, answering the a3 [FINDING]):** `MultiTransportRouter::new`
(`src/transport/router.rs:83`) builds its own Direct-mode `EmailTransportImpl` with no sink. Take it as
its own slice: red first, a refusal through that path must reach a sink; removing the wiring must fail
the test by name.

**Finding at origin/main e1468f7.** `new_with_provider` gets all four transports from a
`TransportProvider` and holds them in plain `Option<Arc<dyn Transport>>` fields. Nothing ever calls
`attach_security_sink` on them. `start_background_services` does not call `start()` on the email
transport either, so its Direct-mode listener is bound (in `EmailTransportImpl::new`) but never served.
The gap is real but latent.

## Changes

1. `MultiTransportRouter::set_security_sink(&self, sink)` passes the sink to each transport it holds,
   as `TransportManager` does. The fields never change after construction, so no shared slot is needed.
2. `SynapseRouter::set_security_sink` passes its sink on to `multi_transport` when one is present.
   `new()` always leaves that field `None`, as it does `email_server`.

## Tests (red first)

- `tests/email_security_events.rs::multi_transport_router_passes_its_sink_to_the_email_transport`:
  a real Direct-mode `EmailTransportImpl` (one inbound connection per minute) is injected through
  `TestTransportProvider`, the router is given a capture sink, and the test starts the transport.
  Positive control: the first connection is greeted `220`. Negative control: the second is refused
  `421`, and the router's sink holds one `RateLimited` event on `email/smtp-inbound`.
- `router_merged::tests::the_multi_transport_routers_email_transport_gets_the_sink`: a
  `SynapseRouter` holding a `MultiTransportRouter` passes its sink to that router's email transport
  (pointer equality on a recording transport).

Mutation check: emptying `MultiTransportRouter::set_security_sink`'s loop fails both tests; deleting
the forwarding line in `SynapseRouter::set_security_sink` fails the second.

## Out of scope

- Starting the email transport from `start_background_services`. That would serve a listener on
  port 2525 that nothing serves today, a behaviour change outside this slice.
- Alert DELIVERY is still only a seam: `AlertTransport` has no implementation, pending board 131.
