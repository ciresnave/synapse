// SPDX-License-Identifier: MIT OR Apache-2.0
//! `synapse-mcp`: the MCP stdio surface (P2 slice c).
//!
//! Usage: `synapse-mcp --config <path>`, or set `SYNAPSE_MCP_CONFIG`. stdout carries the MCP
//! protocol; logs go to stderr. No output ever names the private key's path (spec §6).
//!
//! Security events (hardening P6) go to `<config stem>-security-events.jsonl` beside the config
//! file: owner-only, append-only, rotated once at 4 MiB. Alert delivery is not wired (board 131).

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use rmcp::ServiceExt;
use rmcp::transport::stdio;
use synapse_mcp_server::{McpConfig, SynapseMcpServer};

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    let Some(config_path) = config_path() else {
        eprintln!("synapse-mcp: usage: synapse-mcp --config <path> (or set SYNAPSE_MCP_CONFIG)");
        return ExitCode::from(2);
    };
    let Ok(text) = std::fs::read_to_string(&config_path) else {
        eprintln!("synapse-mcp: cannot read the config file");
        return ExitCode::from(2);
    };
    let config = match McpConfig::from_toml(&text) {
        Ok(config) => config,
        Err(e) => {
            eprintln!("synapse-mcp: {e}");
            return ExitCode::from(2);
        }
    };
    let sink = match synapse_security::FileSink::open(
        security_events_path(&config_path),
        SECURITY_EVENTS_MAX_BYTES,
    ) {
        Ok(sink) => Arc::new(sink),
        Err(_) => {
            // Fixed text: the path sits beside the config, which may name the key's directory.
            eprintln!(
                "synapse-mcp: cannot open the security event file beside the config \
                 (it must be owner-only)"
            );
            return ExitCode::FAILURE;
        }
    };
    let server = match SynapseMcpServer::start_with_sink(config, sink).await {
        Ok(server) => server,
        Err(e) => {
            eprintln!("synapse-mcp: {e}");
            return ExitCode::FAILURE;
        }
    };
    let service = match server.serve(stdio()).await {
        Ok(service) => service,
        Err(e) => {
            eprintln!("synapse-mcp: MCP initialization failed: {e}");
            return ExitCode::FAILURE;
        }
    };
    match service.waiting().await {
        Ok(_) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("synapse-mcp: the server task failed: {e}");
            ExitCode::FAILURE
        }
    }
}

/// The size at which the event file rotates to `<file>.1`.
const SECURITY_EVENTS_MAX_BYTES: u64 = 4 * 1024 * 1024;

/// `<config stem>-security-events.jsonl` in the config file's directory, so two servers whose
/// configs share a directory keep separate files.
fn security_events_path(config_path: &Path) -> PathBuf {
    let stem = config_path
        .file_stem()
        .map_or_else(|| "synapse-mcp".into(), |s| s.to_string_lossy());
    config_path.with_file_name(format!("{stem}-security-events.jsonl"))
}

fn config_path() -> Option<PathBuf> {
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--config" {
            return args.next().map(PathBuf::from);
        }
    }
    std::env::var_os("SYNAPSE_MCP_CONFIG").map(PathBuf::from)
}
