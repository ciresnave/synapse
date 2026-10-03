// SPDX-License-Identifier: MIT OR Apache-2.0
//! `synapsed`: serve the Synapse mailbox on loopback until Ctrl-C or a termination signal, then
//! remove its announce file (#74). On Unix that is SIGTERM or SIGHUP; on Windows, Ctrl-Break and the
//! console's close, logoff and shutdown events. A kill (SIGKILL, `taskkill /F`) runs no handler.
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
    match daemon.serve(listener, shutdown_signal()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("synapsed: {}", e.kind());
            ExitCode::from(1)
        }
    }
}

/// Resolves on the first stop request. A handler that cannot be installed just never fires.
async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut term = signal(SignalKind::terminate()).ok();
        let mut hup = signal(SignalKind::hangup()).ok();
        tokio::select! {
            Ok(()) = tokio::signal::ctrl_c() => {}
            Some(()) = async { term.as_mut()?.recv().await } => {}
            Some(()) = async { hup.as_mut()?.recv().await } => {}
            else => std::future::pending::<()>().await,
        }
    }
    #[cfg(windows)]
    {
        use tokio::signal::windows;
        let mut brk = windows::ctrl_break().ok();
        let mut close = windows::ctrl_close().ok();
        let mut logoff = windows::ctrl_logoff().ok();
        let mut down = windows::ctrl_shutdown().ok();
        tokio::select! {
            Ok(()) = tokio::signal::ctrl_c() => {}
            Some(()) = async { brk.as_mut()?.recv().await } => {}
            Some(()) = async { close.as_mut()?.recv().await } => {}
            Some(()) = async { logoff.as_mut()?.recv().await } => {}
            Some(()) = async { down.as_mut()?.recv().await } => {}
            else => std::future::pending::<()>().await,
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
