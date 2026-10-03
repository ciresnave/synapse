// SPDX-License-Identifier: MIT OR Apache-2.0
//! `synapse claim|send|inbox|ack|list` against a real `synapsed` that the CLI starts itself (M5b).
//!
//! Every test gets its own `SYNAPSE_HOME` and `SYNAPSE_ADDR=127.0.0.1:0`: the daemon binds a free
//! port and the CLI learns it from the announce file. A guard kills the announced daemon on drop.

use std::path::Path;
use std::process::{Command, Output};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use serde_json::Value;

/// `cargo test -p synapse-cli` does not build another package's binary, so build `synapsed` into
/// the directory the CLI looks in first: next to the `synapse` executable.
fn ensure_synapsed_built() {
    static BUILT: OnceLock<()> = OnceLock::new();
    BUILT.get_or_init(|| {
        let dir = Path::new(env!("CARGO_BIN_EXE_synapse")).parent().unwrap();
        let mut cmd = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
        cmd.args(["build", "-p", "synapsed", "--bin", "synapsed"]);
        if dir.file_name().is_some_and(|n| n == "release") {
            cmd.arg("--release");
        }
        let status = cmd.status().expect("cargo runs");
        assert!(status.success(), "building synapsed failed");
        let exe = dir.join(format!("synapsed{}", std::env::consts::EXE_SUFFIX));
        assert!(exe.exists(), "synapsed is not at {}", exe.display());
    });
}

struct Home {
    dir: tempfile::TempDir,
}

impl Home {
    fn new() -> Home {
        ensure_synapsed_built();
        let home = Home {
            dir: tempfile::tempdir().unwrap(),
        };
        let init = home.run(&["id", "init", "--account", "acct"]);
        assert_eq!(init.status.code(), Some(0), "{init:?}");
        home
    }

    fn path(&self) -> &Path {
        self.dir.path()
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_synapse"));
        cmd.args(args)
            .env("SYNAPSE_HOME", self.path())
            .env("SYNAPSE_ADDR", "127.0.0.1:0")
            .env_remove("SYNAPSE_ROLE");
        cmd
    }

    fn run(&self, args: &[&str]) -> Output {
        self.command(args)
            .output()
            .expect("the synapse binary runs")
    }

    /// Run and require success.
    fn ok(&self, args: &[&str]) -> String {
        let out = self.run(args);
        assert_eq!(
            out.status.code(),
            Some(0),
            "synapse {args:?} failed: {out:?}"
        );
        String::from_utf8(out.stdout).unwrap()
    }

    fn announced_pid(&self) -> Option<u32> {
        let text = std::fs::read_to_string(self.path().join("synapsed.json")).ok()?;
        let v: Value = serde_json::from_str(&text).ok()?;
        v["pid"].as_u64().map(|p| p as u32)
    }

    fn kill_daemon(&self) {
        if let Some(pid) = self.announced_pid() {
            kill(pid);
            wait_dead(pid);
        }
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        self.kill_daemon();
    }
}

fn kill(pid: u32) {
    #[cfg(windows)]
    let _ = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/F"])
        .output();
    #[cfg(unix)]
    let _ = Command::new("kill").args(["-9", &pid.to_string()]).output();
}

fn alive(pid: u32) -> bool {
    #[cfg(windows)]
    {
        let out = Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"])
            .output()
            .expect("tasklist runs");
        String::from_utf8_lossy(&out.stdout).contains(&format!("\"{pid}\""))
    }
    #[cfg(unix)]
    {
        Command::new("kill")
            .args(["-0", &pid.to_string()])
            .output()
            .is_ok_and(|o| o.status.success())
    }
}

fn wait_dead(pid: u32) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while alive(pid) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The pids the CLI said it spawned (`synapse: started synapsed (pid N)` on stderr).
fn spawned_pids(out: &Output) -> Vec<u32> {
    String::from_utf8_lossy(&out.stderr)
        .lines()
        .filter_map(|l| l.strip_prefix("synapse: started synapsed (pid "))
        .filter_map(|rest| rest.strip_suffix(')'))
        .map(|n| n.parse().expect("a pid"))
        .collect()
}

fn json_lines(text: &str) -> Vec<Value> {
    text.lines()
        .map(|l| serde_json::from_str(l).unwrap_or_else(|_| panic!("not JSON: {l}")))
        .collect()
}

fn role_epoch(home: &Home, as_role: &str, global_id: &str) -> u64 {
    let roles = json_lines(&home.ok(&["list", "--role", as_role]));
    roles
        .iter()
        .find(|r| r["global_id"] == global_id)
        .unwrap_or_else(|| panic!("{global_id} not listed: {roles:?}"))["epoch"]
        .as_u64()
        .unwrap()
}

fn line<'a>(text: &'a str, label: &str) -> &'a str {
    text.lines()
        .find_map(|l| l.strip_prefix(&format!("{label}: ")))
        .unwrap_or_else(|| panic!("no `{label}:` line in:\n{text}"))
}

#[test]
fn a_command_against_a_stopped_daemon_starts_it() {
    let home = Home::new();
    assert!(
        home.announced_pid().is_none(),
        "no daemon before the first command"
    );

    let out = home.run(&["claim", "--role", "alpha"]);
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let text = String::from_utf8_lossy(&out.stdout);
    assert_eq!(line(&text, "identity"), "alpha@acct");
    assert_eq!(line(&text, "epoch"), "1");

    let pid = home.announced_pid().expect("the CLI started a daemon");
    assert_eq!(spawned_pids(&out), vec![pid]);
    assert!(alive(pid));
}

/// Start both commands back to back (not via `output()`, which runs each to completion in turn
/// on its thread), so their auto-starts overlap. Several fresh homes, since a race need not
/// happen on any one try; how many tries really raced is printed.
#[test]
fn two_concurrent_first_commands_leave_one_daemon() {
    let mut raced = 0;
    for _ in 0..5 {
        let home = Home::new();
        let piped = |mut cmd: Command| {
            cmd.stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .unwrap()
        };
        let a = piped(home.command(&["claim", "--role", "alpha"]));
        let b = piped(home.command(&["claim", "--role", "beta"]));
        let out_a = a.wait_with_output().unwrap();
        let out_b = b.wait_with_output().unwrap();
        assert_eq!(out_a.status.code(), Some(0), "{out_a:?}");
        assert_eq!(out_b.status.code(), Some(0), "{out_b:?}");

        let winner = home.announced_pid().expect("a daemon announced itself");
        let spawned: Vec<u32> = spawned_pids(&out_a)
            .into_iter()
            .chain(spawned_pids(&out_b))
            .collect();
        assert!(!spawned.is_empty(), "someone started the daemon");
        assert!(
            spawned.contains(&winner),
            "{spawned:?} vs announced {winner}"
        );
        if spawned.len() > 1 {
            raced += 1;
        }
        for pid in spawned.iter().filter(|&&p| p != winner) {
            wait_dead(*pid);
            assert!(!alive(*pid), "a second daemon (pid {pid}) is still running");
        }
        assert!(alive(winner));
        // Both roles were claimed on the one daemon.
        assert_eq!(role_epoch(&home, "alpha", "beta@acct"), 1);
    }
    eprintln!("both commands started a daemon in {raced} of 5 tries");
}

/// Two first commands for one role, at once, on a running daemon: one claim, not two that
/// supersede each other (review I4).
#[test]
fn concurrent_first_commands_for_one_role_claim_once() {
    let home = Home::new();
    home.ok(&["claim", "--role", "beta"]); // the daemon is up
    for role in ["g1", "g2", "g3"] {
        let start = |args: &[&str]| {
            home.command(args)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .unwrap()
        };
        let a = start(&["send", "--role", role, "--to", role, "one"]);
        let b = start(&["inbox", "--role", role]);
        let out_a = a.wait_with_output().unwrap();
        let out_b = b.wait_with_output().unwrap();
        assert_eq!(out_a.status.code(), Some(0), "{out_a:?}");
        assert_eq!(out_b.status.code(), Some(0), "{out_b:?}");
        assert_eq!(role_epoch(&home, "beta", &format!("{role}@acct")), 1);
    }
}

#[test]
fn a_round_trip_between_two_roles() {
    let home = Home::new();
    home.ok(&["claim", "--role", "alpha"]);
    home.ok(&["claim", "--role", "beta"]);

    let sent = home.ok(&["send", "--role", "alpha", "--to", "beta", "hello beta"]);
    assert_eq!(line(&sent, "outcome"), "queued");
    let message_id = line(&sent, "message id").to_string();
    assert!(message_id.starts_with("alpha@acct/"), "{message_id}");

    let inbox = json_lines(&home.ok(&["inbox", "--role", "beta"]));
    assert_eq!(inbox.len(), 1, "{inbox:?}");
    assert_eq!(inbox[0]["from"], "alpha@acct");
    assert_eq!(inbox[0]["body"], "hello beta");
    assert_eq!(inbox[0]["message_id"], message_id.as_str());

    let acked = home.ok(&["ack", "--role", "beta", &message_id]);
    assert_eq!(line(&acked, "outcome"), "removed");
    assert!(
        json_lines(&home.ok(&["inbox", "--role", "beta"])).is_empty(),
        "an acked message is not redelivered"
    );
}

#[test]
fn a_message_can_come_from_stdin() {
    use std::io::Write;
    let home = Home::new();
    home.ok(&["claim", "--role", "beta"]);
    let mut child = home
        .command(&["send", "--role", "beta", "--to", "beta@acct"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"from stdin")
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let inbox = json_lines(&home.ok(&["inbox", "--role", "beta"]));
    assert_eq!(inbox[0]["body"], "from stdin");
}

#[test]
fn a_cached_session_is_reused() {
    let home = Home::new();
    home.ok(&["claim", "--role", "alpha"]);
    home.ok(&["send", "--role", "alpha", "--to", "alpha", "one"]);
    home.ok(&["send", "--role", "alpha", "--to", "alpha", "two"]);
    home.ok(&["inbox", "--role", "alpha"]);
    assert_eq!(
        role_epoch(&home, "alpha", "alpha@acct"),
        1,
        "commands after a claim must not re-claim"
    );
}

#[test]
fn a_superseded_session_is_reported_not_stolen() {
    let home = Home::new();
    home.ok(&["claim", "--role", "alpha"]);
    let cache = home.path().join("sessions").join("alpha.json");
    let stale = std::fs::read(&cache).expect("the session is cached");
    home.ok(&["claim", "--role", "alpha"]); // a deliberate takeover: epoch 2
    std::fs::write(&cache, stale).unwrap(); // a holder still on epoch 1

    let out = home.run(&["send", "--role", "alpha", "--to", "alpha", "late"]);
    assert_ne!(out.status.code(), Some(0), "{out:?}");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("superseded"), "{err}");

    // Nothing re-claimed behind the user's back. (`list` itself must use a current session.)
    home.ok(&["claim", "--role", "beta"]);
    assert_eq!(role_epoch(&home, "beta", "alpha@acct"), 2);
}

/// The daemon forgets sessions more than a few takeovers old, so their calls get 401, not 409.
/// That must not be answered by re-claiming either (review I1).
#[test]
fn a_long_superseded_session_is_not_reclaimed_on_401() {
    let home = Home::new();
    home.ok(&["claim", "--role", "alpha"]);
    let cache = home.path().join("sessions").join("alpha.json");
    let stale = std::fs::read(&cache).expect("the session is cached");
    for _ in 0..10 {
        home.ok(&["claim", "--role", "alpha"]);
    }
    std::fs::write(&cache, stale).unwrap();

    let out = home.run(&["send", "--role", "alpha", "--to", "alpha", "late"]);
    assert_ne!(out.status.code(), Some(0), "{out:?}");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("unauthorized"), "{err}");
    home.ok(&["claim", "--role", "beta"]);
    assert_eq!(role_epoch(&home, "beta", "alpha@acct"), 11);
}

#[test]
fn a_dead_daemon_is_restarted_and_the_role_reclaimed() {
    let home = Home::new();
    home.ok(&["claim", "--role", "alpha"]);
    let first = home.announced_pid().unwrap();
    home.kill_daemon();
    assert!(!alive(first));

    let out = home.run(&["send", "--role", "alpha", "--to", "alpha", "again"]);
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let second = home.announced_pid().unwrap();
    assert_ne!(first, second);
    assert_eq!(spawned_pids(&out), vec![second]);
    let inbox = json_lines(&home.ok(&["inbox", "--role", "alpha"]));
    assert_eq!(inbox.len(), 1);
    assert_eq!(inbox[0]["body"], "again");
}

#[test]
fn a_role_is_required() {
    let home = Home::new();
    let out = home.run(&["inbox"]);
    assert_ne!(out.status.code(), Some(0));
    assert!(
        home.announced_pid().is_none(),
        "a usage error starts no daemon"
    );
}
