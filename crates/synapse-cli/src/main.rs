// SPDX-License-Identifier: MIT OR Apache-2.0
//! `synapse`: the plain command line adapter (plan adapter (c)). M2 adds the identity commands:
//! `synapse id init --account <name>` and `synapse id show [--role <role>]`.
//!
//! Output carries names, key ids and validity windows only: never key material, never the
//! keystore's path (spec `docs/superpowers/specs/2026-10-01-m2-keystore-design.md` §3.3).
//!
//! M5b adds the mail commands over `synapsed`, which they start if it is not running:
//! `synapse claim|send|inbox|ack|list --role <role>` (or `SYNAPSE_ROLE`). `inbox` and `list` print
//! one JSON object per line. Session tokens are never printed.

mod mail;

use std::io::{IsTerminal, Read};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use serde_json::json;
use synapse::certificate::Permission;
use synapse::keystore::{Keystore, default_home, valid_name};

use mail::{Daemon, MailError};

#[derive(Parser)]
#[command(name = "synapse", about = "Synapse command line")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Identity: the account key and per-role keys kept in the keystore.
    #[command(subcommand)]
    Id(IdCommand),
    /// Claim a role on the daemon (taking it over if another holder has it).
    Claim {
        #[command(flatten)]
        role: RoleArg,
    },
    /// Send a message: the MESSAGE argument, else standard input.
    Send {
        #[command(flatten)]
        role: RoleArg,
        /// The recipient: `<role>` on this account, or `<role>@<account>`.
        #[arg(long)]
        to: String,
        /// A message id of your own, for idempotent resends (default: a fresh UUID).
        #[arg(long)]
        id: Option<String>,
        message: Option<String>,
    },
    /// Fetch waiting messages, one JSON object per line. Each stays leased until acked.
    Inbox {
        #[command(flatten)]
        role: RoleArg,
        #[arg(long)]
        max: Option<usize>,
        #[arg(long)]
        lease_secs: Option<i64>,
    },
    /// Acknowledge a fetched message, removing it from the inbox.
    Ack {
        #[command(flatten)]
        role: RoleArg,
        message_id: String,
    },
    /// List every role the daemon knows, with presence, one JSON object per line.
    List {
        #[command(flatten)]
        role: RoleArg,
    },
}

#[derive(clap::Args)]
struct RoleArg {
    /// The role to act as.
    #[arg(long, env = "SYNAPSE_ROLE")]
    role: String,
}

#[derive(Subcommand)]
enum IdCommand {
    /// Create the account key. Refuses if an account already exists.
    Init {
        #[arg(long)]
        account: String,
    },
    /// Show the account, and with --role, that role's identity (creating its keys on first use).
    Show {
        #[arg(long)]
        role: Option<String>,
    },
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(2)
        }
    }
}

fn run(cli: Cli) -> Result<(), MailError> {
    let home = default_home()?;
    match cli.command {
        Command::Id(IdCommand::Init { account }) => {
            let summary = Keystore::init_account(&home, &account)?;
            println!("account: {}", summary.account);
            println!("account key id: {}", summary.key_id);
        }
        Command::Id(IdCommand::Show { role }) => {
            let store = Keystore::open(&home)?;
            let summary = store.account();
            println!("account: {}", summary.account);
            println!("account key id: {}", summary.key_id);
            if let Some(role) = role {
                let identity = store.role(&role, chrono::Utc::now())?;
                println!("identity: {}", identity.global_id);
                println!("signing key id: {}", identity.signing_key_id);
                println!("sealing key id: {}", identity.sealing_key_id);
                println!(
                    "certificate valid: {} to {}",
                    identity.not_before.to_rfc3339(),
                    identity.not_after.to_rfc3339()
                );
                let permissions: Vec<String> = identity
                    .permissions
                    .iter()
                    .map(Permission::as_str)
                    .collect();
                println!("permissions: {}", permissions.join(", "));
            }
        }
        Command::Claim { role } => {
            let daemon = connect(&home, &role)?;
            let claimed = daemon.claim(&role.role)?;
            println!("identity: {}", claimed.global_id);
            println!("epoch: {}", claimed.epoch);
        }
        Command::Send {
            role,
            to,
            id,
            message,
        } => {
            check_role(&role)?;
            let to = if to.contains('@') {
                to
            } else {
                format!("{to}@{}", Keystore::open(&home)?.account().account)
            };
            let body = match message {
                Some(text) => text.into_bytes(),
                None if std::io::stdin().is_terminal() => {
                    return Err(MailError::Usage(
                        "give the message as an argument, or pipe it on standard input".into(),
                    ));
                }
                None => {
                    let mut bytes = Vec::new();
                    std::io::stdin().read_to_end(&mut bytes)?;
                    bytes
                }
            };
            // Chosen here, not by the daemon, so resending with `--id` is a duplicate, not a copy.
            let id = id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
            let daemon = connect(&home, &role)?;
            let reply = daemon.call(
                &role.role,
                "/v1/send",
                Some(&json!({"to": to, "message_id": id, "body_b64": mail::encode_body(&body)})),
            )?;
            println!("message id: {}", reply["message_id"].as_str().unwrap_or(""));
            println!("outcome: {}", reply["outcome"].as_str().unwrap_or(""));
        }
        Command::Inbox {
            role,
            max,
            lease_secs,
        } => {
            let daemon = connect(&home, &role)?;
            let reply = daemon.call(
                &role.role,
                "/v1/fetch",
                Some(&json!({"max": max, "lease_secs": lease_secs})),
            )?;
            for message in reply["messages"].as_array().into_iter().flatten() {
                println!("{}", mail::inbox_line(message));
            }
        }
        Command::Ack { role, message_id } => {
            let daemon = connect(&home, &role)?;
            let reply = daemon.call(
                &role.role,
                "/v1/ack",
                Some(&json!({"message_id": message_id})),
            )?;
            println!("outcome: {}", reply["outcome"].as_str().unwrap_or(""));
        }
        Command::List { role } => {
            let daemon = connect(&home, &role)?;
            let reply = daemon.call(&role.role, "/v1/list", None)?;
            for entry in reply["roles"].as_array().into_iter().flatten() {
                println!("{entry}");
            }
        }
    }
    Ok(())
}

/// Validate the role before it names a file or starts a daemon, then connect.
fn connect(home: &std::path::Path, role: &RoleArg) -> Result<Daemon, MailError> {
    check_role(role)?;
    Daemon::connect(home)
}

fn check_role(role: &RoleArg) -> Result<(), MailError> {
    if valid_name(&role.role) {
        Ok(())
    } else {
        Err(MailError::Usage(format!(
            "`{}` is not a role name: use [A-Za-z0-9_-], at most 64 characters",
            role.role
        )))
    }
}
