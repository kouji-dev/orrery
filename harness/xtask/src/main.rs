//! `cargo xtask <task>`.
//!
//! `deps-check` enforces the dependency-direction rule for real. `typegen`,
//! `wit-check` and `agui-drift` are stubs that exit 0 naming their plan, so the
//! task list is complete from the scaffold and fills in plan by plan.

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
    AguiDrift,
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
        Task::AguiDrift => {
            println!(
                "agui-drift: not implemented — see harness/docs/plans/08-protocol-transport.md"
            );
            ExitCode::SUCCESS
        }
    }
}
