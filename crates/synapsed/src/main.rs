// SPDX-License-Identifier: MIT OR Apache-2.0
//! `synapsed`: serve the Synapse mailbox on loopback until Ctrl-C.
//!
//! Home: `SYNAPSE_HOME` (else the platform default, as for `synapse id`). Address: `SYNAPSE_ADDR`
//! (default `127.0.0.1:7920`), loopback only. Logs never carry tokens or key material.

use std::net::SocketAddr;
use std::process::ExitCode;

use synapsed::{Daemon, DaemonConfig};

const DEFAULT_ADDR: &str = "127.0.0.1:7920";

#[tokio::main]
async fn main() -> ExitCode {
    let home = match synapse::keystore::default_home() {
        Ok(home) => home,
        Err(e) => {
            eprintln!("synapsed: {e}");
            return ExitCode::from(2);
        }
    };
    let addr: SocketAddr = match std::env::var("SYNAPSE_ADDR")
        .unwrap_or_else(|_| DEFAULT_ADDR.to_string())
        .parse()
    {
        Ok(addr) => addr,
        Err(_) => {
            eprintln!("synapsed: SYNAPSE_ADDR is not an address");
            return ExitCode::from(2);
        }
    };
    let config = DaemonConfig {
        home,
        addr,
        sweep_every: std::time::Duration::from_secs(3600),
    };
    let daemon = match Daemon::open(&config) {
        Ok(daemon) => daemon,
        Err(e) => {
            eprintln!("synapsed: {e}");
            return ExitCode::from(2);
        }
    };
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(listener) => listener,
        Err(e) => {
            eprintln!("synapsed: cannot listen on {addr}: {}", e.kind());
            return ExitCode::from(2);
        }
    };
    eprintln!("synapsed: listening on {addr}");
    let shutdown = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    match daemon.serve(listener, shutdown).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("synapsed: {}", e.kind());
            ExitCode::from(1)
        }
    }
}
