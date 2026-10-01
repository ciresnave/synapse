// SPDX-License-Identifier: MIT OR Apache-2.0
//! `synapse`: the plain command line adapter (plan adapter (c)). M2 adds the identity commands:
//! `synapse id init --account <name>` and `synapse id show [--role <role>]`.
//!
//! Output carries names, key ids and validity windows only: never key material, never the
//! keystore's path (spec `docs/superpowers/specs/2026-10-01-m2-keystore-design.md` §3.3).

use std::process::ExitCode;

use clap::{Parser, Subcommand};
use synapse::certificate::Permission;
use synapse::keystore::{Keystore, KeystoreError, default_home};

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

fn run(cli: Cli) -> Result<(), KeystoreError> {
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
    }
    Ok(())
}
