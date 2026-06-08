//! The orrery command-line interface. The same executable doubles as a CLI:
//! launched with a subcommand it runs that and exits; launched bare it starts
//! the desktop app. Today the only subcommand is `hook`, which installed agent
//! hooks invoke to broker a permission/status event with the running app.

pub mod hook;

use clap::{Parser, Subcommand};

/// Subcommand names we recognise. Checked *before* clap parses so a bare desktop
/// launch (or any unexpected OS arg) never reaches the CLI parser.
const SUBCOMMANDS: &[&str] = &["hook"];

#[derive(Parser)]
#[command(name = "orrery", bin_name = "orrery", version, about = "Orrery — agent orchestration")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Broker one agent hook event with the running app (called by installed agent hooks).
    Hook(hook::HookArgs),
}

/// True when argv names one of our subcommands (or asks for top-level help/version).
/// A bare desktop launch has no such arg → start the app, never touch clap.
pub fn invoked_as_cli() -> bool {
    match std::env::args().nth(1) {
        Some(a) => {
            SUBCOMMANDS.contains(&a.as_str())
                || matches!(a.as_str(), "--help" | "-h" | "--version" | "-V")
        }
        None => false,
    }
}

/// Parse argv and run the named subcommand. clap handles --help/--version and
/// prints usage errors itself.
pub fn run() {
    match Cli::parse().command {
        Commands::Hook(args) => hook::run(args),
    }
}
