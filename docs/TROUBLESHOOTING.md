# Synapse troubleshooting

**Scope.** Symptoms of the daemon-era surface (`synapsed`, `synapse`, the MCP adapters) at 6.0.0-rc.99, each
tied to a message or behaviour documented in `docs/API_REFERENCE.md`, `docs/CONFIGURATION_GUIDE.md` or the
code. The former guide (email routing, discovery and name-resolution scenarios; about 545 lines) was replaced in
rc.100 because most of its method names no longer exist (`python .github/docs_staleness.py` measured 61 of 64
unknown). It is in git: `git show 519bd52:docs/TROUBLESHOOTING.md`.

First look at three things: `synapse list --role <role>` (who is online, what is pending), the daemon's
`<home>/security-events.jsonl`, and `<home>/synapsed-start.err` (stderr of a daemon started on demand). Home is
`SYNAPSE_HOME`, else the platform default (`docs/CONFIGURATION_GUIDE.md`).

## The daemon will not start

| you see | cause | fix |
|---|---|---|
| `synapsed exited (exit code: 2) and no daemon answered`, followed by the daemon's last stderr lines and the home path | the daemon refused to run; its reason is in those lines | read the reason and fix it, below |
| `permissions_too_open` with a path | the home, account directory, key, session or announce file is readable by others | restrict the file or directory to its owner; the daemon will not use it |
| `no account` | the home has no account key | `synapse id init --account <name>` |
| `SYNAPSE_ADDR is not an address` (exit 2) | the variable does not parse | give `ip:port`, loopback only |
| `cannot listen on <addr>` (exit 2) | the port is taken (often by a daemon already running) | `synapse list --role <role>`; if it answers, a daemon is running; otherwise choose another `SYNAPSE_ADDR` |

A kill (`SIGKILL`, `taskkill /F`) leaves the announce file behind; that is harmless, since clients decide by the
health proof. Never stop daemons by image name (`taskkill /IM synapsed.exe` also kills other people's daemons);
stop one by its pid.

## A call is refused

| you see | cause | fix |
|---|---|---|
| HTTP 401 | a missing or unrecognised bearer | `synapse claim --role <role>`; every call except `health` needs the bearer from a claim |
| `superseded ... run synapse claim` (adapter tool error) | another holder took the role; one holder per (home, role) | claim again if you mean to take it back; an adapter never re-claims by itself |
| HTTP 400 on a field the client sent | bodies deny unknown fields, so an older daemon rejects a field it does not know | match the daemon and client versions |
| `mailbox_full` (HTTP 413) | the role's queue is at 10,000 messages or 64 MiB | the receiver must `ack` or the dead set must be handled; nothing was dropped |
| a role name is refused | names are 1 to 64 characters of letters, digits, `-`, `_` | rename |

Repeated failed bearers and claims are rate-limited, answered after a delay, and recorded in
`security-events.jsonl`.

## A message does not arrive

1. `synapse list --role <role>`: is the recipient `online` (heard from within 90 s) and what is its `pending`?
2. A fetched message stays **leased** until acked or the lease ends (default 60 s). `synapse inbox` fetches, so
   running it as the receiving role hides messages from that role's own adapter for the lease. Use a role nothing
   else drains.
3. A message leased 10 times without an ack moves to the **dead set**, never dropped:
   `synapse dead --role <role>` lists it, `synapse requeue --role <role> <message_id>` puts it back with its count
   reset, `synapse ack` discards it. `requeue` answers `mailbox_full` (it stays dead) if the queue is full.
4. Delivery is at-least-once: the same `message_id` can be shown again. That is expected; act once per id.
5. `outcome: duplicate` on a send means that id was already queued or acked within the 7-day history; nothing was
   queued twice.

`synapse stats --role <role>` gives `sent`, `acked`, `redelivered`, `expired` since the daemon started; a restart
begins again from zero (see `since`).

## Windows build problems

- `linker lld-link not found`: the repository no longer forces `lld-link`; see the note in `.cargo/config.toml`.
- `LNK1318 Unexpected PDB error` in a debug build: `docs/WINDOWS_LINKER_WORKAROUND.md`.
