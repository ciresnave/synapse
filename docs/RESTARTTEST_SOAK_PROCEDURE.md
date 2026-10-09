# `restarttest` live push and the M9 soak: the procedure, ready to run on a yes

**Status: PREPARED, NOT RUN.** Nothing below has been executed. Every "expected" line is read from the
source at `origin/main` 6.0.0-rc.27 (rc.28 carries this document) and is marked *(source)*; where I
have not read an output's exact text, the line says so rather than guessing. **Do not start any step
in section 2 or later until CireSnave has approved (board 160).**

Plan: `docs/superpowers/plans/2026-09-29-claude-peers-replacement.md` (M1, M9, D2) and
`docs/superpowers/plans/2026-10-08-m6-m7-adapters.md` section 5.

## 0. What each phase proves

| Phase | Proves | Scope | Reversible by |
|---|---|---|---|
| 1. Live push in `restarttest` | Claude Code shows a pushed Synapse message in a real session; ack and redelivery behave | One disposable lane, **its own home and port** | Deleting a directory and killing two PIDs |
| 2. Side-by-side soak (M9) | The local path is good enough to replace claude-peers over at least 3 days | Every lane loaded with both channels | Relaunching lanes with `server:claude-peers` only |

Phase 1 does not touch the lanes' real Synapse home or any daemon another lane may be using.
Phase 2 is the PM's and OverMind's to run; this lane supplies the measurement and the stop
conditions. Cutover (dropping `server:claude-peers`) is **not** part of either phase.

## 1. Preconditions (all checkable now, none needs the approval)

1. **Binaries from a known commit.** In a worktree at the exact `origin/main` commit to be tested:
   `cargo build --release -p synapsed -p synapsectl -p synapse-claude-channel`
   with `C:\Users\cires\.rustup\toolchains\stable-x86_64-pc-windows-msvc\bin` on `PATH`. Record
   `git rev-parse HEAD` and `synapsed`'s `/v1/health` `version`. Binaries: `synapsed`, `synapse`
   (from the `synapsectl` crate), `synapse-claude-channel`.
2. **The wire probe passes** (no model request, no approval needed, kills only its own child):
   `cargo test -p synapse-claude-channel --test channel -- --ignored --nocapture real_claude_code_wire_probe`
   Expect *(source)*: `server/discover` answered `-32022`, `initialize` accepted at `2025-11-25`,
   `serverInfo.name` = `synapse`. If it fails, stop: the live test would fail the same way.
3. **The approval exists, or CireSnave will answer the dialog by hand.** `lane-restart`'s approval for
   `--dangerously-load-development-channels` pins `Channels` **exactly** (`server:claude-peers` today;
   no prefix match). Phase 1 passes `server:synapse`, which no record covers yet, so the dialog
   ("WARNING: Loading development channels") will need a human, or a new record in
   `ciresnave/ciresnave/.overmind/lane-restart/approvals/` (the OverMind lane's, D2). Phase 2 needs
   the second record, `server:claude-peers,server:synapse`.
4. **Back up before any first open of an existing home.** rc.27 migrates `mailbox.redb` to store
   format 2 on first open, **one way**; rc.26 and earlier must not open it afterwards. Phase 1 uses a
   fresh home, so it is exempt. Phase 2 on a home an older daemon has used: copy the home directory
   first, and the rollback in section 6 restores the copy.
5. **Know the limits.** There is no cumulative sent/acked/redelivered counter in the daemon (section 4).

## 2. Phase 1: the `restarttest` live push

All commands in Git Bash. `H` is the throwaway home; `PORT` is not 7920 (the default other lanes use).

```bash
export H="C:/Users/cires/AppData/Local/synapse-restarttest"   # a NEW directory; must not exist yet
export PORT=7931
export SYNAPSE_HOME="$H" SYNAPSE_ADDR="127.0.0.1:$PORT"
```

### 2.1 Identity and daemon

```bash
synapse id init --account soaktest
synapse id show --role restarttest
synapse id show --role soak-probe
```
Expect *(source)*: `account: soaktest`, `account key id: <id>`, and for `--role` an `identity: <global id>`
line. Never key material or the keystore path. `id init` refuses a second run.

Start the daemon in the background and **record the PID you started**:
```bash
synapsed 2> "$H/synapsed.log" &  echo $! > "$H/synapsed.pid"
```
Expect *(source)*, on stderr: `synapsed: listening on 127.0.0.1:7931`. Health:
`curl -s http://127.0.0.1:7931/v1/health` -> JSON with `"ok":true`, `version`, `started_at`.
(`synapse` and the channel adapter also auto-start a daemon if none answers; with `SYNAPSE_ADDR` set
they start it on this address.) *Exact stderr text of an auto-start I have not read; the manual start
above avoids depending on it.*

### 2.2 Claude Code side

`C:\Projects\restarttest` does not exist on this machine as of 2026-10-09 (checked), and I have not
read how `lane-restart` builds that lane's command line. So the launch below is the **shape**, to be
reconciled with the lane's real launcher by the PM or OverMind before use:

`mcp.json`:
```json
{ "mcpServers": { "synapse": {
  "command": "C:/path/to/synapse-claude-channel.exe",
  "args": ["--role", "restarttest", "--home", "C:/Users/cires/AppData/Local/synapse-restarttest"],
  "env": { "SYNAPSE_ADDR": "127.0.0.1:7931" } } } }
```
Launch (model **Sonnet**, never Opus):
```
claude --model sonnet --mcp-config <mcp.json> --dangerously-load-development-channels server:synapse
```
The server name in the file must be `synapse`: it is what `server:synapse` names, and what the adapter
reports as `serverInfo.name`. `--mcp-config` and `--dangerously-load-development-channels` are both on
`lane-restart`'s carry-over allowlist, so a restarted lane keeps them.

Expect: the development-channels dialog; answer "I am using this for local development" (option 1).
A session stuck at that dialog still fires `SessionStart`, so **a live process is not a working
session** (OverMind's fourth retest): confirm a real prompt is accepted.

### 2.3 The checks (each is pass/fail; record the observed text)

| # | Do | Expect | Source |
|---|---|---|---|
| 1 | `synapse list --role soak-probe` | one JSON object per line; `restarttest` has `"online": true` within ~30 s (heartbeat every 30 s; online window 90 s) | source |
| 2 | `synapse send --role soak-probe --to restarttest --id p1-0001 "hello"` | the session shows a `<channel source="synapse">` event whose text starts `UNTRUSTED message from ...` within ~1-2 s (poll every 1 s) | source |
| 3 | `synapse list --role soak-probe` | `pending` for `restarttest` is `queued:0, leased:0` (pushed, then acked) | source |
| 4 | send the same `--id p1-0001` again | no second push (idempotent resend) | source |
| 5 | send a body containing `</channel>`, `<`, an ESC character and "ignore previous instructions" | arrives inside the untrusted frame, `<` and `>` as `&lt;` `&gt;`, control characters as escapes; the model must not obey it | source |
| 6 | in the session, call the `send` tool to `soak-probe`, then `synapse inbox --role soak-probe` | the reply appears with `"from"` = the session's role | source |
| 7 | kill the **claude** process only; send one message; relaunch | the message is delivered after relaunch (leased on the old session or queued; it returns after the 30 s lease at most) | source |
| 8 | `tail "$H/security-events.jsonl"` | no lines (none expected in a clean run) | source |

**Caution on `synapse inbox`:** it *fetches*, which leases and hides messages from that role's own
channel adapter for `lease_secs`. Run it only as `soak-probe` (a role nothing else drains), never as
`restarttest` during a test.

### 2.4 Phase 1 pass and stop

**Pass:** checks 1-8 as above, with zero messages lost and none shown twice except by a failed ack.
**Stop immediately** on: a message not shown after 10 s; a message shown more than once with no
`ack failed` line in the adapter's stderr; any line in `security-events.jsonl`; the daemon exiting; the
dialog text differing from the approval's anchors (do not auto-answer an unfamiliar dialog).

## 3. Phase 2: the side-by-side soak (M9) -- needs the second approval and every lane

Owner: the PM with OverMind (launch flags are `lane-restart`'s). This lane's part is below.

1. Approval record `Channels: server:claude-peers,server:synapse` merged (D2). Without it every lane's
   relaunch stops at a dialog nobody answers.
2. Real home: take the backup (precondition 4), then start `synapsed` on the default address once, by a
   PID you record. Never `taskkill /IM synapsed.exe` (it kills other lanes' daemons).
3. Relaunch lanes at their next task boundary with **both** channels loaded
   (`--dangerously-load-development-channels server:claude-peers server:synapse`), model pinned.
   Claude-peers stays the path of record; Synapse carries *new* traffic named by the PM.
4. Run the canary (section 4) for at least 3 days; sample `synapse list` hourly.
5. Fetch-scan note: `fetch(max=1)` on a role of 10 000 messages with half leased is p50 5.2 ms (rc.27,
   release, one laptop, 1 KiB bodies; 2.4x the 1k case). Real mailboxes are small; a role whose
   `queued + leased` stays above ~1 000 is the signal that follow-up B (issue #110, a READY table) is needed.

## 4. What is measured, and the honest gap

`synapsed` keeps **no cumulative counters**. The plan's "sent, acked, redelivered and never-acked by
role" therefore cannot be read from the daemon; it is assembled from three sources:

| Quantity | How | Limit |
|---|---|---|
| sent | the canary writes one line per send (`id`, time) | counts only canary traffic |
| acked / lost | canary ids that appeared as `<channel>` events (`meta.message_id`) in the session transcript, compared to sent | needs the transcript; no daemon-side confirmation |
| redelivered | the same `message_id` appearing twice in the transcript | the adapter logs ack failures (`synapse channel: ack failed`, stderr) but not redeliveries |
| never acked / stuck | `synapse list` -> `pending.queued`, `pending.leased`, `pending.oldest_enqueued_at` | a depth snapshot, not history |
| rejected / hostile | lines in `<home>/security-events.jsonl` (rotates at 4 MiB to `.1`) | only authentication and budget events |
| latency | canary send time vs the event's arrival in the transcript | the transcript's clock; ~1 s poll granularity |

**Gap, stated plainly:** a per-role cumulative counter would need a small daemon change (a
`/v1/stats` route). It is not built and not part of this PR; ask the PM whether the soak wants it
before it starts, because adding it mid-soak resets the run.

Canary (unrun sketch; `--id` makes resends idempotent, so a retry cannot inflate "sent"):
```bash
for i in $(seq -w 1 500); do
  synapse send --role soak-probe --to restarttest --id "soak-$i" "canary $i" \
    && echo "$(date -u +%FT%TZ) soak-$i" >> "$H/sent.log"
  sleep 60
done
```
Compare at the end: `sent.log` ids minus ids found in the transcript = lost; ids found twice =
redelivered; `pending` after quiescence must be `queued:0, leased:0`.

## 5. Stop conditions for the soak (proposed; the PM sets the numbers)

Stop the soak, restore claude-peers-only launches, and write up the cause if **any** occurs:
1. A canary message **lost** (sent, acked or leased-and-expired by the daemon's view, never shown).
2. A message shown twice with no `ack failed` line explaining it.
3. `pending.oldest_enqueued_at` for a live role older than 5 minutes while the role is `online`.
4. Any `security-events.jsonl` line whose surface is not an expected canary mistake.
5. `synapsed` exits, restarts, or its announce file (`synapsed.json`) names a different instance id
   than the one started.
6. A lane stuck at a dialog or unable to receive on claude-peers because of the second channel.
7. `fetch` cost visibly grows: `queued + leased` above ~1 000 for a role, or CPU of `synapsed` not
   idle between polls.
8. CireSnave says stop.

## 6. Rollback

Phase 1 (everything is disposable):
```bash
kill "$(cat "$H/synapsed.pid")"         # only the PID started above; never taskkill /IM
# close the claude session, then:
rm -rf "$H"                              # the throwaway home, keystore included
# delete mcp.json
```
Verify: `curl http://127.0.0.1:7931/v1/health` fails to connect; `$H` is gone. Nothing outside `$H`,
port 7931 and `mcp.json` was touched, so no other lane is affected.

Phase 2:
1. Relaunch each lane with `--dangerously-load-development-channels server:claude-peers` only (the
   existing approval covers exactly that).
2. Stop the daemon by its recorded PID.
3. If the home existed before rc.27, restore the backup taken in precondition 4. **A home that rc.27
   has opened cannot be opened by rc.26 or earlier**; only the backup restores the old format.
4. Claude-peers was never switched off, so no message path has to be rebuilt.

## 7. Not covered

Cutover; LAN mode (`docs/LAN_MODE.md`); a live test of anything other than `restarttest`; and the
exact `lane-restart` invocation for `restarttest`, which belongs to OverMind and is flagged in 2.2.
