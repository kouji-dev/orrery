//! `cargo xtask <task>`.
//!
//! `deps-check` enforces the dependency-direction rule for real; `typegen`
//! regenerates `harness/protocol`; `agui-drift` checks the vendored AG-UI enum
//! against upstream. `wit-check` is still a stub that exits 0 naming its plan.

#![deny(missing_docs)]
#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

/// Repo automation for the Orrery harness.
#[derive(Debug, Parser)]
#[command(name = "xtask", about, long_about = None)]
struct Cli {
    /// The workspace root. Defaults to the repo root above this crate.
    #[arg(long, global = true, value_name = "PATH")]
    root: Option<PathBuf>,

    /// The task to run.
    #[command(subcommand)]
    task: Task,
}

#[derive(Debug, Subcommand)]
enum Task {
    /// Check the core / extensions / clients dependency direction.
    DepsCheck,
    /// Regenerate `harness/protocol` from the `orrery-proto` schemars derives.
    Typegen,
    /// Check `harness/wit/orrery-extension.wit` against the host bindings.
    WitCheck,
    /// Diff the vendored AG-UI event enum against the published upstream schema.
    ///
    /// Reads the installed `@ag-ui/core` when there is one, which is a file on
    /// disk. `--fetch` additionally allows a network read, and is meant for CI;
    /// without it and without the package, the job compares against the pinned
    /// list and says so rather than failing for being offline.
    AguiDrift {
        /// Allow a network read of upstream's published schema. CI only.
        #[arg(long)]
        fetch: bool,
        /// Fail when no upstream list can be read at all.
        #[arg(long)]
        strict: bool,
    },
}

fn repo_root() -> PathBuf {
    // harness/xtask -> harness -> repo root
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let root = cli.root.unwrap_or_else(repo_root);

    match cli.task {
        Task::DepsCheck => match xtask::deps_check::check(&root) {
            Err(e) => {
                eprintln!("deps-check: {e}");
                ExitCode::from(2)
            }
            Ok(violations) if violations.is_empty() => {
                println!("deps-check: ok");
                ExitCode::SUCCESS
            }
            Ok(violations) => {
                for v in &violations {
                    eprintln!("deps-check: {v}");
                }
                eprintln!(
                    "deps-check: {} violation(s) — see harness/docs/plans/00-overview.md",
                    violations.len()
                );
                ExitCode::FAILURE
            }
        },
        Task::Typegen => match xtask::typegen::run(&root) {
            Ok(dir) => {
                println!("typegen: wrote {}", dir.display());
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("typegen: {e}");
                ExitCode::from(2)
            }
        },
        Task::WitCheck => {
            println!("wit-check: not implemented — see harness/docs/plans/14-wasm-wit.md");
            ExitCode::SUCCESS
        }
        Task::AguiDrift { fetch, strict } => match xtask::agui_drift::run(&root, fetch, strict) {
            Err(e) => {
                eprintln!("agui-drift: {e}");
                ExitCode::from(2)
            }
            Ok(drift) => {
                println!("agui-drift: compared against {}", drift.source);
                for name in &drift.invented {
                    eprintln!("agui-drift: `{name}` is not an AG-UI event");
                }
                for name in &drift.unaccounted {
                    println!("agui-drift: upstream added `{name}`; neither emitted nor skipped");
                }
                for name in &drift.stale {
                    println!("agui-drift: `{name}` is skipped but upstream dropped it");
                }
                if drift.is_failure() {
                    eprintln!("agui-drift: the vendored enum has a name AG-UI does not");
                    return ExitCode::FAILURE;
                }
                if drift.is_clean() {
                    println!("agui-drift: ok");
                }
                ExitCode::SUCCESS
            }
        },
    }
}
