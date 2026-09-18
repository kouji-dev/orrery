//! The `orrery` command tree. Links the ratatui and json clients.
//!
//! Implementation plan: `harness/docs/plans/17-cli.md`
//!
//! The tree is complete: every subcommand parses, and the ones that have not
//! landed yet exit 2 naming the plan file that will implement them. `run`,
//! `serve` and `attach` are wired to a real kernel.
//!
//! # stdout is data, stderr is narration
//!
//! Logging is installed on **stderr**, always, whatever `-v` says. That is what
//! makes `orrery run --json -vv | jq` a working line rather than a trap, and
//! there is a test for it.
//!
//! # Nothing here is a back door
//!
//! The interactive path builds a kernel, puts a [`Hub`](orrery_transport::Hub)
//! in front of it and subscribes a renderer to that hub. `orrery attach` does
//! the same thing over a pipe. Neither can reach the kernel except through
//! frames and the control RPC.

#![deny(missing_docs)]
#![forbid(unsafe_code)]

mod args;
mod cmd;
mod control;
mod exit;
mod render;
mod session;
mod term;
mod ui;

use clap::Parser;

use args::{Cli, Command};

fn main() {
    let cli = Cli::parse();
    install_logging(cli.verbose);
    term::install_panic_hook();

    match &cli.command {
        // Bare `orrery` starts an interactive session: the kernel in-process,
        // with a renderer attached over the in-process transport.
        None => cmd::interactive::dispatch(&cli),
        Some(Command::Run { prompt }) => cmd::run::dispatch(&cli, prompt),
        Some(Command::Serve { listen }) => cmd::serve::dispatch(&cli, listen.as_deref()),
        Some(Command::Attach {
            endpoint,
            since,
            submit,
        }) => cmd::attach::dispatch(&cli, endpoint, *since, submit.as_deref()),
        Some(Command::Replay { session }) => cmd::replay::dispatch(session),
        Some(Command::Session { command }) => cmd::session::dispatch(command),
        Some(Command::Ext { command }) => cmd::ext::dispatch(&cli, command),
        Some(Command::Permissions { command }) => cmd::permissions::dispatch(command),
        Some(Command::Config { command }) => cmd::config::dispatch(command),
        Some(Command::Init { profile }) => cmd::init::dispatch(profile.as_deref()),
        Some(Command::Import { from }) => cmd::import::dispatch(*from),
        Some(Command::Eval { command }) => cmd::eval::dispatch(command),
        Some(Command::Ledger) => cmd::ledger::ledger(),
        Some(Command::Telemetry) => cmd::ledger::telemetry(),
    }
}

/// Narration on stderr, never on stdout.
///
/// `RUST_LOG` still wins when it is set, because a person debugging one module
/// should not have to discover that `-vv` is the only dial.
fn install_logging(verbosity: u8) {
    use tracing_subscriber::EnvFilter;
    let default = match verbosity {
        0 => "warn",
        1 => "info",
        2 => "debug",
        _ => "trace",
    };
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .try_init();
}
