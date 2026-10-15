# Daemon start contract (G7), the delivery statement, and the 24 h pilot checklist

**Status:** for PM and AgentLife review. Written 2026-10-15 against `origin/main@7d2df10` (6.0.0-rc.81).
**Source of the decisions below:** PM `[ASK-ANSWER]` 2026-10-15T22:58Z, relaying board 217. They are the PM's
statements, not CireSnave's words, and are quoted as such. Nothing here was run.

## 1. Who starts `synapsed` (G7): the contract for AgentLife

Decision: AgentLife starts `synapsed` once, from Task Scheduler, before any lane launches.

**One fact changes the size of this job.** `synapsed-client` already starts a daemon on demand:
`Daemon::connect(home)` reads the announce file, and if no daemon proves itself it spawns `synapsed`
(detached, no console, its stderr in `<home>/` for the failure text, found next to the client's own
executable else on `PATH`). So the first adapter or CLI call on a machine starts one. What the scheduler
adds is: a daemon that is up **before** the first lane, with a **fixed** environment, that comes back
after a crash or logoff without waiting for a lane to need it. An auto-started daemon inherits the
environment of whichever lane happened to start it, which is the wrong source for `SYNAPSE_HOME` and
`SYNAPSE_LAUNCHER_ROLES`.

| Item | Contract |
|---|---|
| Binary | `synapsed` (release build of the workspace at the pilot's commit; later the crates.io `synapsed` the PM publishes). One path, recorded in the task. Keep `synapsed.exe` beside `synapsectl.exe`/`synapse-mcp-server.exe`: the client looks next to its own executable first |
| Home | `SYNAPSE_HOME` if set, else `%LOCALAPPDATA%\synapse` on Windows. **Set it explicitly in the task and for every lane** (lane launch must pass the same value): a lane with a different home talks to a different daemon or starts a second one. Owner-only; holds the keystore, `mailbox.redb`, `synapsed.json` (announce), `security-events.jsonl` |
| Bind | `SYNAPSE_ADDR`, default `127.0.0.1:7920`. Loopback only: a non-loopback address makes the daemon exit 2. Must be the same for every lane |
| Launcher roles | `SYNAPSE_LAUNCHER_ROLES` **empty** until the launch poller service exists (spec `2026-10-15-cutover-and-lane-launch.md` C2a); then `agentlife-launcher` |
| Single instance | A second `synapsed` on the same home exits 2 (redb file lock). So the scheduler task and a lane's auto-start cannot both run; the later one fails harmlessly |
| Run as | the interactive user (the keystore is owner-only; a different account cannot read it) |
| Exit codes | `0`/signal: shutdown, announce file removed (best effort on Windows). `2`: refused to start (not an address, not loopback, cannot bind, keystore or store error, second instance). Message on stderr |
| Restart policy | `synapsed` does **not** restart itself. Task Scheduler: restart on failure, at most 3 times, 1 minute apart. **Do not loop on exit 2**: it is a configuration or lock problem and restarting cannot fix it |
| Liveness check | `synapsectl list` against the same home (it connects and proves the daemon with the instance id; `GET /v1/health` alone is unauthenticated and proves nothing about *which* process answered). Do not probe the port with bare `curl`: it adds `security-events.jsonl` lines (the soak's single `auth_failure` was this) |
| What a restart loses | Presence and launch state are in memory: every role looks offline until its next heartbeat (up to 90 s), and launch signals are suppressed for one online window after start. Mail is not lost: the mailbox is durable, and a role's next claim receives everything queued |
| Logs | stderr. The task should redirect it to a file in the home or elsewhere; the daemon never logs tokens or key material |

Open for AgentLife: whether the task runs at logon (then a reboot with no one logged in has no
daemon, which is acceptable for a desktop lane host) or at startup.

## 2. Delivery statement (to be read as the contract, not the aspiration)

PM, 2026-10-15T22:58Z: *"delivery = bounded push attempts + mailbox as source of truth (fetch acks),
ACCEPTED, state it in the spec."*

Stated:

1. **The mailbox is the source of truth.** Every message sent is stored durably (redb) and stays until it
   is acked or dead-lettered after `max_deliveries` (default 10). Nothing is dropped silently.
2. **`fetch` is how a role takes mail, and a fetch acks.** A message the role fetched and acknowledged is
   delivered. Reading the mailbox (`fetch`, `list`'s `pending`) always shows the true state.
3. **Push is a notification with bounded attempts, not a delivery guarantee.** The channel adapter pushes a
   message to the session up to `max_pushes` (3) times, about `lease_secs` apart, and then acks it itself,
   because the model does not call `ack` (0 of 5 trials on rc.32). If Claude Code drops every push, the
   message is acked without being shown. This was observed once (phase-1 check 7, rc.29, n=1).
4. **Therefore** a daemon-side `acked` or an empty `pending` does not prove a session saw a message; only
   the session transcript does. A lane that must not miss a message does not rely on push alone: on
   (re)start and periodically it calls `fetch`, and a sender that needs confirmation asks for a reply.
5. **Duplicates are possible** (up to 3 shows of one `message_id`); a receiver acts once per `message_id`.

Where the change goes: the adapter instructions already say act once per `message_id`. The onboarding line
"on start, call `fetch`" is a docs PR (spec C item 4).

## 3. Pilot checklist: synapse, agentlife, OverMind lanes, side by side, 24 h, current main

**Subject:** the three lanes, each loaded with both channels (`server:claude-peers` and `server:synapse`),
against one `synapsed` built from `origin/main` at a commit recorded at start. Everything else stays on
claude-peers only.

### Before start (T-0)

- [ ] Commit sha, the workspace version it was built at (`synapsed` takes no flags), and the scheduler task's env recorded in the pilot log.
- [ ] G5 record exists for `server:synapse` (and the overlap value) and its handler test pins exact match.
- [ ] One daemon, one home, one address; each of the three lanes launched with the same
      `SYNAPSE_HOME`/`SYNAPSE_ADDR` and `--role <role>`; `synapsectl list` shows all three `online`.
- [ ] A role-name table (lane cwd to role) written down and given to all three lanes.
- [ ] Sender log in place: each lane records every Synapse send as `<id> <to-role> <utc>` (use `--id`;
      ids are unique and carry the sender role, e.g. `pilot-<role>-<n>`).
- [ ] Baseline for cost: each lane's turns and cache-read tokens per hour on claude-peers-only traffic
      from the previous 24 h, if its transcript has them.

### Traffic

- [ ] Real work traffic runs over Synapse as the primary path for these three lanes, with claude-peers as
      the fallback only when a Synapse message is not answered in 5 minutes.
- [ ] At least one deliberate **queued-while-down** case per lane: send to a role whose session is stopped,
      restart it, check the message is shown (this is the phase-1 check 7 case; the rc.33 soak may not
      have covered it).
- [ ] One deliberate **daemon restart** during traffic, via the scheduler's restart path; check every role
      re-claims and nothing sent before or during is lost.
- [ ] At least 10 replies in which a **model** called `send` (not only received).

### Measure

| # | Measure | How | Pass |
|---|---|---|---|
| 1 | **Loss count by id** | sender log ids minus ids present as `<channel source="synapse">` events in the receiving transcript. Only the transcript decides | 0 |
| 2 | **Wake latency** | transcript event time minus sender log time, for messages to an idle session; report p50, p95, max | p95 within a few seconds (poll is 1 s; the number is the finding) |
| 3 | Shows per id | count of events per `message_id` in the transcript | max 3; one action per id |
| 4 | Queue age | `synapsectl list` every 5 min: `pending.oldest_enqueued_at` for online roles | under 5 min |
| 5 | Dead set | `synapsectl dead <role>` at the end | empty, or each entry explained |
| 6 | Security events | lines in `security-events.jsonl`, each classified | 0 unexplained |
| 7 | Daemon stability | pid and `started_at` at each sample | unchanged except the deliberate restart |
| 8 | Dialog stalls | count of times a lane sat at a dialog or lost claude-peers because of the second channel | 0 |
| 9 | Cost | turns and cache-read per message per lane, against the baseline | recorded, no pass bar (this is the missing number) |
| 10 | Redelivery after restart | ids queued during the daemon restart and shown after | all |

### Stop (restore claude-peers-only launches and write up the cause)

Any lost id (measure 1); a lane stuck at a dialog; queue age over 5 minutes for an online role that does not
clear on the next sample; an unexplained security event; the daemon exiting unplanned; CireSnave says stop.

### Output

A pilot report from the synapse lane with each measure's number, the ids of any loss, the commit under
test, and what the pilot did not cover (compaction, more than three lanes, reboot). The PM decides the
full cutover from that report.
