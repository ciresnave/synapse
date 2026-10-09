// SPDX-License-Identifier: MIT OR Apache-2.0
//! A stdio tee for the wire probe: `tee_mcp <log-file> <command> [args...]`.
//!
//! Runs `<command>` with its stdin and stdout relayed unchanged, and appends every line in each
//! direction to `<log-file>` as `C>S <line>` (client to server) or `S>C <line>`. The relay is
//! byte-for-byte; only the log is derived. Used by the ignored `real_claude_code_wire_probe` test
//! in `tests/channel.rs`, which puts it between the real Claude Code client and the adapter.

use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, ExitCode, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let (Some(log), Some(cmd)) = (args.next(), args.next()) else {
        eprintln!("usage: tee_mcp <log-file> <command> [args...]");
        return ExitCode::from(2);
    };
    let log = match OpenOptions::new().create(true).append(true).open(&log) {
        Ok(f) => Arc::new(Mutex::new(f)),
        Err(e) => {
            eprintln!("tee_mcp: cannot open the log: {e}");
            return ExitCode::FAILURE;
        }
    };
    let mut child = match Command::new(cmd)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("tee_mcp: cannot start the command: {e}");
            return ExitCode::FAILURE;
        }
    };
    let mut to_child = child.stdin.take().expect("piped stdin");
    let from_child = child.stdout.take().expect("piped stdout");

    let up_log = Arc::clone(&log);
    thread::spawn(move || {
        for line in BufReader::new(std::io::stdin())
            .lines()
            .map_while(Result::ok)
        {
            let _ = writeln!(up_log.lock().unwrap(), "C>S {line}");
            if writeln!(to_child, "{line}")
                .and_then(|()| to_child.flush())
                .is_err()
            {
                break;
            }
        }
    });
    let down = thread::spawn(move || {
        let mut out = std::io::stdout();
        for line in BufReader::new(from_child).lines().map_while(Result::ok) {
            let _ = writeln!(log.lock().unwrap(), "S>C {line}");
            if writeln!(out, "{line}").and_then(|()| out.flush()).is_err() {
                break;
            }
        }
    });
    let status = child.wait();
    let _ = down.join();
    match status {
        Ok(s) if s.success() => ExitCode::SUCCESS,
        _ => ExitCode::FAILURE,
    }
}
