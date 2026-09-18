//! The clap tree. Every command in `17-cli.md` is present from phase 0; the
//! ones that have not landed yet exit 2 naming their plan file.

use clap::{Parser, Subcommand, ValueEnum};

use crate::ui::Ui;

/// One binary that starts a kernel, attaches a renderer, and explains itself.
#[derive(Debug, Parser)]
#[command(name = "orrery", version, about, long_about = None)]
pub struct Cli {
    /// Which profile to resolve.
    #[arg(long, global = true, value_name = "NAME")]
    pub profile: Option<String>,

    /// The workspace root. Defaults to the current directory.
    #[arg(long, global = true, value_name = "PATH")]
    pub workspace: Option<std::path::PathBuf>,

    /// Renderer. Defaults to ratatui on a tty, json otherwise.
    #[arg(long, global = true, value_name = "UI")]
    pub ui: Option<Ui>,

    /// Shorthand for `--ui json`.
    #[arg(long, global = true, conflicts_with = "ui")]
    pub json: bool,

    /// Override the profile's consent mode, within the managed clamp.
    #[arg(long, global = true, value_name = "MODE")]
    pub consent: Option<Consent>,

    /// Log level on stderr. Never mixed into `--json` stdout. Repeatable.
    #[arg(short = 'v', global = true, action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// The command to run. With none, start an interactive session.
    #[command(subcommand)]
    pub command: Option<Command>,
}

/// When the harness asks before acting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Consent {
    /// Ask every time.
    Always,
    /// Ask once per matching call, then remember for the session.
    Once,
    /// Never ask: deny instead, so CI cannot hang.
    Never,
}

/// Which harness to import configuration from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ImportFrom {
    /// Claude Code.
    ClaudeCode,
    /// Codex.
    Codex,
}

/// The top-level command tree.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Run one turn, print the final text, and exit.
    Run {
        /// The prompt.
        #[arg(short = 'p', long, value_name = "PROMPT")]
        prompt: String,
    },
    /// Start a kernel, print its endpoint, and keep running without a client.
    Serve {
        /// The address or pipe name to listen on.
        #[arg(long, value_name = "ADDR")]
        listen: Option<String>,
    },
    /// Attach a renderer to a running kernel.
    Attach {
        /// The endpoint `serve` printed, or `$ORRERY_ENDPOINT`.
        #[arg(value_name = "ENDPOINT")]
        endpoint: String,
        /// Replay from this sequence number instead of the start.
        #[arg(long, value_name = "SEQ")]
        since: Option<u64>,
    },
    /// Re-emit a stored session as events, so any renderer can draw any past session.
    Replay {
        /// The session id.
        #[arg(value_name = "SESSION")]
        session: String,
    },
    /// Inspect stored sessions.
    Session {
        /// What to do with them.
        #[command(subcommand)]
        command: SessionCommand,
    },
    /// Inspect and manage extensions.
    Ext {
        /// What to do with them.
        #[command(subcommand)]
        command: ExtCommand,
    },
    /// Explain a permission decision.
    Permissions {
        /// What to explain.
        #[command(subcommand)]
        command: PermissionsCommand,
    },
    /// Explain resolved configuration.
    Config {
        /// What to explain.
        #[command(subcommand)]
        command: ConfigCommand,
    },
    /// Write a starter configuration into the workspace.
    Init {
        /// Which profile to scaffold.
        #[arg(long, value_name = "NAME")]
        profile: Option<String>,
    },
    /// Import configuration from another harness.
    Import {
        /// The harness to import from.
        #[arg(long = "from", value_name = "HARNESS")]
        from: Option<ImportFrom>,
    },
    /// Run, compare and replay evaluation suites.
    Eval {
        /// What to do.
        #[command(subcommand)]
        command: EvalCommand,
    },
    /// Query the decision ledger.
    Ledger,
    /// Query the telemetry stream.
    Telemetry,
}

/// `orrery session ...`
#[derive(Debug, Subcommand)]
pub enum SessionCommand {
    /// List stored sessions.
    List,
    /// Show one session.
    Show {
        /// The session id.
        #[arg(value_name = "ID")]
        id: String,
    },
    /// Delete one session. History cannot be reconstructed, so this asks first.
    Rm {
        /// The session id.
        #[arg(value_name = "ID")]
        id: String,
        /// Skip the confirmation. Required in non-interactive use.
        #[arg(long)]
        yes: bool,
    },
}

/// `orrery ext ...`
#[derive(Debug, Subcommand)]
pub enum ExtCommand {
    /// List loaded extensions, including degraded and skipped ones with reasons.
    List,
    /// Install an extension from the signed registry.
    Install {
        /// The extension name.
        #[arg(value_name = "NAME")]
        name: String,
    },
    /// Remove an installed extension.
    Remove {
        /// The extension name.
        #[arg(value_name = "NAME")]
        name: String,
    },
    /// Run an extension's tests against the mock broker, with no model and no network.
    Test {
        /// The extension directory. Defaults to the current one.
        #[arg(value_name = "PATH")]
        path: Option<std::path::PathBuf>,
    },
}

/// `orrery permissions ...`
#[derive(Debug, Subcommand)]
pub enum PermissionsCommand {
    /// Explain what would happen to a call, naming the rule, layer, file and line.
    Explain {
        /// The call to explain, for example `builtin.write:$WORKSPACE/src/**`.
        #[arg(value_name = "CALL")]
        call: String,
    },
}

/// `orrery config ...`
#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    /// Explain where a key's value came from, naming the winning layer.
    Explain {
        /// The configuration key.
        #[arg(value_name = "KEY")]
        key: String,
    },
}

/// `orrery eval ...`
#[derive(Debug, Subcommand)]
pub enum EvalCommand {
    /// Run a suite.
    Run {
        /// The suite to run.
        #[arg(value_name = "SUITE")]
        suite: String,
        /// Profiles to run it against.
        #[arg(long, value_name = "NAMES", value_delimiter = ',')]
        profile: Vec<String>,
        /// Models to run it against.
        #[arg(long, value_name = "MODELS", value_delimiter = ',')]
        model: Vec<String>,
        /// Report format.
        #[arg(long, value_name = "FORMAT")]
        format: Option<String>,
    },
    /// Compare two runs.
    Compare {
        /// The baseline run.
        #[arg(value_name = "RUN-A")]
        run_a: String,
        /// The run to compare against it.
        #[arg(value_name = "RUN-B")]
        run_b: String,
    },
    /// Re-open one case of a past run.
    Replay {
        /// The run id.
        #[arg(value_name = "RUN")]
        run: String,
        /// The case id.
        #[arg(long, value_name = "ID")]
        case: String,
    },
}
