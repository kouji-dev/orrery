//! The `orrery` command tree. Links the ratatui and json clients.
//!
//! Implementation plan: `harness/docs/plans/17-cli.md`
//!
//! The tree is complete from phase 0: every subcommand parses, and the ones
//! that have not landed yet exit 2 naming the plan file that will implement
//! them. Nothing here starts a kernel yet.

#![deny(missing_docs)]
#![forbid(unsafe_code)]

mod args;
mod cmd;
mod exit;
mod term;
mod ui;

use clap::Parser;

use args::{Cli, Command};
use exit::not_implemented;

fn main() {
    let cli = Cli::parse();

    match &cli.command {
        // Bare `orrery` starts an interactive session: the kernel in-process,
        // with the ratatui client attached over the in-process transport.
        None => not_implemented("09b-client-ratatui.md"),
        Some(Command::Run { prompt }) => cmd::run::dispatch(prompt),
        Some(Command::Serve { listen }) => cmd::serve::dispatch(listen.as_deref()),
        Some(Command::Attach { endpoint, since }) => cmd::attach::dispatch(endpoint, *since),
        Some(Command::Replay { session }) => cmd::replay::dispatch(session),
        Some(Command::Session { command }) => cmd::session::dispatch(command),
        Some(Command::Ext { command }) => cmd::ext::dispatch(command),
        Some(Command::Permissions { command }) => cmd::permissions::dispatch(command),
        Some(Command::Config { command }) => cmd::config::dispatch(command),
        Some(Command::Init { profile }) => cmd::init::dispatch(profile.as_deref()),
        Some(Command::Import { from }) => cmd::import::dispatch(*from),
        Some(Command::Eval { command }) => cmd::eval::dispatch(command),
        Some(Command::Ledger) => cmd::ledger::ledger(),
        Some(Command::Telemetry) => cmd::ledger::telemetry(),
    }
}
