// SPDX-License-Identifier: MIT OR Apache-2.0
//! `synapse-mcp`: the MCP stdio surface (P2 slice c).
//!
//! Usage: `synapse-mcp --config <path>`, or set `SYNAPSE_MCP_CONFIG`. stdout carries the MCP
//! protocol; logs go to stderr. No output ever names the private key's path (spec §6).

use std::path::PathBuf;
use std::process::ExitCode;

use rmcp::ServiceExt;
use rmcp::transport::stdio;
use synapse::mcp_server::{McpConfig, SynapseMcpServer};

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
    let server = match SynapseMcpServer::start(config).await {
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

fn config_path() -> Option<PathBuf> {
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--config" {
            return args.next().map(PathBuf::from);
        }
    }
    std::env::var_os("SYNAPSE_MCP_CONFIG").map(PathBuf::from)
}
