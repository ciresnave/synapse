# synapsed

The Synapse daemon: one per user, on loopback, serving a durable role mailbox (lease, ack, redelivery, dead set) to any agent over an authenticated HTTP/JSON API.

Part of [Synapse](https://github.com/ThinkersJournal/synapse), a model-agnostic communication system for agents. See the repository README and `docs/` for the design, the HTTP API and the security model.

This crate is released in lockstep with the rest of the workspace: every crate carries the same version number.

## Licence

Licensed under either of the Apache License, Version 2.0 (`LICENSE-APACHE`) or the MIT licence (`LICENSE-MIT`), at your option.
