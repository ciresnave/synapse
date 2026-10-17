<!-- SPDX-License-Identifier: MIT OR Apache-2.0 -->
# Pilot T-0 runbook (PM runs this, in order)

Spec: `docs/superpowers/specs/2026-10-15-daemon-contract-and-pilot.md` (section 3 = measures).
Written 2026-10-16 against `origin/main@993abf3` (6.0.0-rc.94). **Nothing here was run**; "Expect" lines
come from reading source and the machine's current state, and say so where they are a guess.
Subject: lanes `synapse`, `agentlife`, `overmind`. Pilot home: `C:\Users\cires\AppData\Local\synapse-pilot`
(exists, empty as of 2026-10-16; the default `%LOCALAPPDATA%\synapse` does not exist). Address `127.0.0.1:7920`.

## 0. Read first

- **The Claude Code adapter is `synapse-claude-channel`, not `synapse-mcp-server`.** `server:synapse` needs the
  `claude/channel` capability, which only `synapse-claude-channel` declares. `synapse-mcp-server` (bin
  `synapse-mcp`) is the generic MCP adapter: it would load, never push. Install the channel crate.
- `synapsectl`'s binary is `synapse.exe`. agentlife's `-Exe` help text says "beside synapsectl.exe and
  synapse-mcp-server.exe": wrong names, harmless (only the file path matters), fix in agentlife.
- Blockers that are not mine: **G5** (CireSnave's yes + OverMind's approval record for the channel string
  `server:claude-peers, server:synapse`; without it every relaunch stops at a dialog nobody answers) and a
  decision on the **account name** (`<ACCT>` below).

## 1. Install rc.97 and stop the rc.33 daemon

```powershell
cargo install synapsed synapsectl synapse-claude-channel --version 6.0.0-rc.97 --locked
```
Needs the MSVC toolchain env your shell already has. Prerelease: `--version` must be exact.
Expect: `Installed package ... synapsed`, `... synapsectl`, `... synapse-claude-channel`; in `C:\Users\cires\.cargo\bin`:
`synapsed.exe`, `synapse.exe`, `synapse-claude-channel.exe` (side by side: the client looks next to its own exe).
Check: neither `synapsed` nor `synapse` has a `--version` flag; `Get-Item` the exe, and read the daemon's `version` from `synapse list` output or the pilot log.
Rollback: `cargo uninstall synapsed synapsectl synapse-claude-channel`.

Stop the old daemon (restarttest, rc.33, **pid 28280**, `...\synapse-restarttest-bin\synapsed.exe`, port 7931,
started 2026-10-12):
```powershell
$p = Get-CimInstance Win32_Process -Filter "ProcessId=28280"
$p.ExecutablePath    # must be ...\synapse-restarttest-bin\synapsed.exe, else STOP: the pid was reused
Stop-Process -Id 28280
Get-NetTCPConnection -State Listen -LocalPort 7931 -ErrorAction SilentlyContinue   # expect: nothing
```
Never `taskkill /IM synapsed.exe` (kills the new daemon too). Breaks: only the disposable restarttest home on
7931 (`...\synapse-restarttest`, kept on disk, nothing else uses it; no `restarttest` lane runs). rc.33 does not
touch the pilot home or port. In-memory counters and presence die with it (expected).
Rollback: `Start-Process "C:\Users\cires\AppData\Local\synapse-restarttest-bin\synapsed.exe"` with
`SYNAPSE_HOME=...\synapse-restarttest` and `SYNAPSE_ADDR=127.0.0.1:7931` set first.

## 2. Identity, then the scheduled task (agentlife `scripts/synapsed-task.ps1`)

```powershell
$env:SYNAPSE_HOME = "C:\Users\cires\AppData\Local\synapse-pilot"; $env:SYNAPSE_ADDR = "127.0.0.1:7920"
synapse id init --account <ACCT>
synapse id show --role synapse; synapse id show --role agentlife; synapse id show --role overmind
```
Expect: `account: <ACCT>`, `account key id: ...`, then `identity: <role>@<ACCT>` per role; never key material.
`id init` refuses a second run. Rollback: delete the pilot home (step 5).

Dry run (prints the wrapper and the task XML, changes nothing), then register:
```powershell
cd C:\Projects\agentlife
.\scripts\synapsed-task.ps1 -Exe C:\Users\cires\.cargo\bin\synapsed.exe -SynapseHome $env:SYNAPSE_HOME -Addr 127.0.0.1:7920
.\scripts\synapsed-task.ps1 -Exe C:\Users\cires\.cargo\bin\synapsed.exe -SynapseHome $env:SYNAPSE_HOME -Addr 127.0.0.1:7920 -Register
schtasks /Run /TN synapsed-cires
synapse list
```
Expect: dry run starts `DRY RUN (nothing written...`; register ends `registered task synapsed-cires (read back: logon
trigger, restart-on-failure, wrapper ...)`. The task name is `synapsed-` + the Windows user name (`cires`; guess,
read it from the output). `synapse list` prints one JSON object per role, empty before any lane connects; no
error means the daemon answered and proved its instance id. Log: `%SYNAPSE_HOME%\task\synapsed.log`.
Exit 2 from `synapsed` is a config or lock problem; the wrapper reports it and does not restart.
Rollback: `.\scripts\synapsed-task.ps1 -Remove` (leaves logs and home), then
`Stop-Process -Id (Get-Content "$env:SYNAPSE_HOME\synapsed.json" | ConvertFrom-Json).pid`
(`pid` is a field of the announce file in the client's reader; confirm with `Get-NetTCPConnection -LocalPort 7920`).

## 3. Per-lane config

Same for all three lanes; only `--role` differs. Role = the lane's name: `synapse`, `agentlife`, `overmind`.

**a. `C:\Projects\.lane-state\<role>.mcp.json`** (new file; server name MUST be `synapse`):
```json
{ "mcpServers": { "synapse": {
  "command": "C:/Users/cires/.cargo/bin/synapse-claude-channel.exe",
  "args": ["--role", "<role>", "--home", "C:/Users/cires/AppData/Local/synapse-pilot"],
  "env": { "SYNAPSE_HOME": "C:/Users/cires/AppData/Local/synapse-pilot", "SYNAPSE_ADDR": "127.0.0.1:7920" } } } }
```
`SYNAPSE_HOME` in `env` is required: `--home` does not reach a daemon the adapter auto-starts (soak procedure 2.2).

**b. `C:\Projects\.lane-state\<role>.launch.json`**, field `launch_args` (today
`["--dangerously-load-development-channels","server:claude-peers"]`) becomes:
```json
"launch_args": ["--dangerously-load-development-channels","server:claude-peers","server:synapse",
                "--mcp-config","C:\\Projects\\.lane-state\\<role>.mcp.json"]
```
Both flags are on `lane-restart`'s carry-over allowlist. **Editing this spec alone does not survive a restart**
(measured by OverMind from `agentlife@e609da5` code, not run: `docs/PILOT-OVERMIND-LANE-CONFIG.md` in the OverMind
repo, section 3). A restart launches from the state file `.lane-state/<role>.json` (the original launch's full
argv), then overwrites `<role>.launch.json` from it. So **the hand launch below is required**: it is what puts the
new flags in the state file. After it, read back `<role>.json` and `<role>.launch.json`: both must hold the new
flags, and only then does a normal restart carry them. Editing the spec first only matters for stop-then-`start`.

First launch of each lane (hand, in the lane's cwd; Sonnet only):
```
claude --model sonnet --mcp-config C:\Projects\.lane-state\<role>.mcp.json --dangerously-load-development-channels server:claude-peers server:synapse
```
Answer the dialog "I am using this for local development" (option 1) **only if** its text matches the G5 record;
an unfamiliar dialog = STOP. A session at the dialog still fires `SessionStart`: confirm a real prompt is accepted.
Cwds: synapse `C:\Projects\synapse`, agentlife `C:\Projects\agentlife`, overmind `C:\Projects\OverMind`.
Rollback per lane: restoring the spec and restarting reverts nothing (a restart rebuilds from the state file).
Hand-launch once with the old flags (`claude --model sonnet --dangerously-load-development-channels server:claude-peers`)
so `SessionStart` re-records them, confirm `<role>.json` and `<role>.launch.json` show the two-element form, delete
`<role>.mcp.json`, then restart normally and confirm a prompt.

## 4. First message and the measures

```powershell
synapse list      # expect the three roles with "online": true (heartbeat at start, then every 30 s; window 90 s)
synapse send --role overmind --to synapse --id pilot-overmind-0001 "smoke"
```
Expect: the synapse session shows a `<channel source="synapse">` event, text starting `UNTRUSTED message from`,
within ~1-2 s. Then `synapse list` shows `pending` for `synapse` at `queued:0, leased:0` after the adapter's bounded
attempts (up to 3 shows, ~30 s apart; same `message_id`; act once).
Record at T-0 in the pilot log: `git rev-parse` of the rc.97 tag/commit (crates.io rc.97 = the commit it was
published from), `synapse list` output, the task XML, and each lane's `--role`. Then start the 10 measures
(spec section 3, table): sender log lines `<id> <to-role> <utc>` with ids `pilot-<role>-<n>`; `synapse list` every 5 min;
`security-events.jsonl` and `synapse dead <role>` at the end. Do not `curl` the port without a session
(it writes `security-events.jsonl` lines).

## 5. STOP, and rollback to claude-peers-only

STOP on any of: a lost id (sent, never in the receiver's transcript); a lane stuck at a dialog or unable to
receive on claude-peers; `pending.oldest_enqueued_at` > 5 min for an `online` role that does not clear on the
next sample; an unexplained `security-events.jsonl` line; `synapsed` exits or its pid/`started_at` changes
(other than the deliberate restart); CireSnave says stop.

Rollback (nothing on claude-peers was ever switched off, so no path needs rebuilding):
1. Per lane: hand-launch once with the old flags (`claude --model sonnet --dangerously-load-development-channels
   server:claude-peers`) so the state file records them (restoring `launch_args` in the spec and restarting is
   overwritten: section 3b), confirm `<role>.json` and `<role>.launch.json` show the two-element form, delete
   `<role>.mcp.json`, then restart normally (`lane-restart --role <role>`). Confirm each reaches a prompt.
2. `.\scripts\synapsed-task.ps1 -Remove`, then stop the daemon by pid (step 2 rollback). Never by image name.
3. Keep `$env:SYNAPSE_HOME` until the pilot report is written (the mailbox and `security-events.jsonl` are the
   evidence). Then `Remove-Item -Recurse "C:\Users\cires\AppData\Local\synapse-pilot"` (keystore included).
4. `cargo uninstall synapsed synapsectl synapse-claude-channel` if rc.97 should leave the machine too.
