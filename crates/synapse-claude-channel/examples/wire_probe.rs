// SPDX-License-Identifier: MIT OR Apache-2.0
//! The wire probe (plan M7 test 4), repeatable: `cargo run -p synapse-claude-channel --example wire_probe`.
//!
//! Puts a byte-for-byte tee between the REAL Claude Code client and the adapter, then checks what the
//! client sent and what the adapter answered. It is a program, not a test, because it needs the
//! `claude` CLI on PATH (or `SYNAPSE_PROBE_CLAUDE`) and a logged-in account, which CI does not have.
//! No model request is made: it stops the client as soon as the adapter has answered `initialize`. It
//! never kills by image name; it kills only the child it spawned, and the tee, adapter and daemon
//! exit when their pipes close or when this program kills the daemon it started.
//!
//! Exit 0 and `wire probe: ok` on success; exit 1 with the reason otherwise. Add `--release` to probe
//! the release binaries.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitCode, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use synapse::keystore::Keystore;

struct Daemon(Child);

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn exe_dir() -> PathBuf {
    // target/<profile>/examples/wire_probe -> target/<profile>
    let exe = std::env::current_exe().expect("current exe");
    exe.parent()
        .and_then(Path::parent)
        .expect("target dir")
        .to_path_buf()
}

fn cargo_build(args: &[&str], release: bool) -> Result<(), String> {
    let mut cmd = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    cmd.arg("build").args(args);
    if release {
        cmd.arg("--release");
    }
    match cmd.status() {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => Err(format!("cargo build {args:?} exited {s}")),
        Err(e) => Err(format!("cargo build {args:?} did not start: {e}")),
    }
}

fn path(p: &Path) -> Result<&str, String> {
    p.to_str()
        .ok_or_else(|| format!("non-UTF-8 path {}", p.display()))
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => {
            println!("wire probe: ok");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("wire probe FAILED: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let dir = exe_dir();
    let release = dir.file_name().is_some_and(|n| n == "release");
    let suffix = std::env::consts::EXE_SUFFIX;
    cargo_build(&["-p", "synapsed", "--bin", "synapsed"], release)?;
    cargo_build(
        &[
            "-p",
            "synapse-claude-channel",
            "--bin",
            "synapse-claude-channel",
            "--example",
            "tee_mcp",
        ],
        release,
    )?;
    let tee = dir.join("examples").join(format!("tee_mcp{suffix}"));
    let adapter = dir.join(format!("synapse-claude-channel{suffix}"));
    let synapsed = dir.join(format!("synapsed{suffix}"));
    for p in [&tee, &adapter, &synapsed] {
        if !p.exists() {
            return Err(format!("{} is not built", p.display()));
        }
    }

    let home = tempfile::tempdir().map_err(|e| e.to_string())?;
    Keystore::init_account(home.path(), "acct").map_err(|e| format!("init account: {e}"))?;
    let _daemon = Daemon(
        Command::new(&synapsed)
            .current_dir(home.path())
            .env("SYNAPSE_HOME", home.path())
            .env("SYNAPSE_ADDR", "127.0.0.1:0")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("synapsed did not start: {e}"))?,
    );
    let announce = home.path().join("synapsed.json");
    let deadline = Instant::now() + Duration::from_secs(15);
    while !announce.exists() {
        if Instant::now() > deadline {
            return Err("synapsed never announced".into());
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    let log = home.path().join("wire.log");
    let config = home.path().join("mcp.json");
    std::fs::write(
        &config,
        json!({"mcpServers": {"synapse": {
            "command": path(&tee)?,
            "args": [path(&log)?, path(&adapter)?, "--role", "probe", "--home", path(home.path())?],
        }}})
        .to_string(),
    )
    .map_err(|e| e.to_string())?;

    let claude_cmd = std::env::var("SYNAPSE_PROBE_CLAUDE").unwrap_or_else(|_| "claude".into());
    let mut claude = Command::new(&claude_cmd)
        .args([
            "-p",
            "hi",
            "--model",
            "haiku",
            "--strict-mcp-config",
            "--mcp-config",
        ])
        .arg(&config)
        .current_dir(home.path())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("{claude_cmd} did not start: {e}"))?;

    let deadline = Instant::now() + Duration::from_secs(90);
    let mut text = String::new();
    while Instant::now() < deadline {
        text = std::fs::read_to_string(&log).unwrap_or_default();
        if text
            .lines()
            .any(|l| l.starts_with("S>C") && l.contains("serverInfo"))
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    let _ = claude.kill();
    let _ = claude.wait();

    println!("--- wire log (C>S = real Claude Code, S>C = adapter) ---\n{text}---");
    let replies: Vec<Value> = text
        .lines()
        .filter_map(|l| l.strip_prefix("S>C "))
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    let init = replies
        .iter()
        .find(|v| v["result"]["serverInfo"].is_object())
        .ok_or("the adapter never answered initialize")?;
    let check = |ok: bool, what: &str| {
        if ok {
            Ok(())
        } else {
            Err(format!("{what}: {init}"))
        }
    };
    check(
        init["result"]["protocolVersion"] == "2025-11-25",
        "protocolVersion is not 2025-11-25",
    )?;
    check(
        init["result"]["serverInfo"]["name"] == "synapse",
        "serverInfo.name is not synapse",
    )?;
    check(
        init["result"]["capabilities"]["tools"].is_object(),
        "no tools capability",
    )?;
    check(
        init["result"]["capabilities"]["experimental"]["claude/channel"].is_object(),
        "no claude/channel capability",
    )?;
    // If the client opens with server/discover, it must have been refused with -32022.
    if text.contains("\"server/discover\"") && !replies.iter().any(|v| v["error"]["code"] == -32022)
    {
        return Err("server/discover was not refused with -32022".into());
    }
    Ok(())
}
