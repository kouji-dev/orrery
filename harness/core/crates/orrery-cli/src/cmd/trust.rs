//! `orrery trust grant|revoke|list` — the verb the trust store never had.
//!
//! # Why this exists
//!
//! The workspace and project layers do not load until the workspace is trusted
//! ([`orrery_config::trust`]), and the store that holds the answer lives in
//! `~/.orrery/trust.toml`, deliberately outside every project-writable path.
//! [`TrustStore::record`] had **one** caller — `orrery_config::resolve`, for an
//! answer a bootstrap surface had already collected — and no command anywhere
//! reached it. So `orrery init` wrote a workspace `config.toml` that nothing
//! could activate, and the only route to a working workspace layer was to
//! hand-edit `trust.auto = true` into the user layer, which nothing documented.
//!
//! # Why it is not a `--trust` flag
//!
//! Trust is an answer that outlives the command that asked it. A flag on `run`
//! would have to be passed every time, or silently persist a decision somebody
//! made in passing; a verb is a thing a person does once, on purpose, and
//! `trust list` is then able to say what they have done.
//!
//! Implementation plan: `harness/docs/plans/10-config-layers.md` task 9.

use std::path::{Path, PathBuf};

use orrery_config::{ConfigPaths, TrustStore};

use crate::args::{Cli, TrustCommand};
use crate::cmd::layers;
use crate::exit::{Exit, fail};

/// Dispatch a `trust` subcommand.
pub fn dispatch(cli: &Cli, command: &TrustCommand) -> ! {
    let (mut store, workspace) = open(cli, target(cli, command));

    match command {
        TrustCommand::Grant { .. } => {
            set(&mut store, &workspace, true);
            eprintln!(
                "orrery: {} is trusted; its own configuration, extensions and interceptors load from now on",
                workspace.display()
            );
            eprintln!("orrery: `orrery trust revoke` takes it back, and `orrery trust list` says what is stored");
            println!("{}", workspace.display());
        }
        TrustCommand::Revoke { .. } => {
            forget(&mut store, &workspace);
            eprintln!(
                "orrery: {} is no longer trusted; the session runs on the user layer alone",
                workspace.display()
            );
            println!("{}", workspace.display());
        }
        TrustCommand::List => list(cli, &store, &workspace),
    }
    Exit::Ok.exit()
}

/// The workspace a subcommand is about: its own argument, else `--workspace`,
/// else the current directory.
fn target(cli: &Cli, command: &TrustCommand) -> Option<PathBuf> {
    let own = match command {
        TrustCommand::Grant { path } | TrustCommand::Revoke { path } => path.clone(),
        TrustCommand::List => None,
    };
    own.or_else(|| cli.workspace.clone())
}

/// The store in the user directory, and the workspace it is being asked about.
///
/// `ConfigPaths::for_workspace` is what `resolve` uses, so the directory this
/// writes to is the directory the next run reads — the two cannot drift.
fn open(cli: &Cli, path: Option<PathBuf>) -> (TrustStore, PathBuf) {
    let workspace = path.unwrap_or_else(|| {
        std::env::current_dir().unwrap_or_else(|e| fail(Exit::Usage, format!("no workspace: {e}")))
    });
    let paths = ConfigPaths::for_workspace(&workspace);
    let workspace = paths.workspace_root.clone();
    let Some(dir) = paths.user_dir.clone() else {
        fail(
            Exit::Usage,
            "there is no user directory to keep the answer in: set HOME or USERPROFILE",
        );
    };
    let store = TrustStore::open(&dir, &workspace).unwrap_or_else(|e| fail(Exit::Usage, e));
    let _ = cli;
    (store, workspace)
}

fn set(store: &mut TrustStore, workspace: &Path, trusted: bool) {
    store
        .record(workspace, trusted)
        .unwrap_or_else(|e| fail(Exit::Kernel, e));
}

fn forget(store: &mut TrustStore, workspace: &Path) {
    store
        .forget(workspace)
        .unwrap_or_else(|e| fail(Exit::Kernel, e));
}

/// Every stored answer, with this workspace marked.
fn list(cli: &Cli, store: &TrustStore, workspace: &Path) {
    let here = TrustStore::key_for(workspace);
    let entries: Vec<(String, bool)> = store
        .answers()
        .map(|(path, trusted)| (path.to_owned(), trusted))
        .collect();

    if layers::wants_json(cli) {
        println!(
            "{}",
            serde_json::json!({
                "store": store.path().display().to_string(),
                "workspace": here,
                "entries": entries
                    .iter()
                    .map(|(path, trusted)| serde_json::json!({
                        "path": path,
                        "trusted": trusted,
                        "is_workspace": *path == here,
                    }))
                    .collect::<Vec<_>>(),
            })
        );
        return;
    }

    if entries.is_empty() {
        // Not an error and not silence: an empty store is the normal state, and
        // the useful thing to say is what to do about it.
        println!("no workspace has been answered for");
        println!("  the answers would live in {}", store.path().display());
        println!("  `orrery trust grant` vouches for {}", workspace.display());
        return;
    }
    println!("{}", store.path().display());
    for (path, trusted) in entries {
        let word = if trusted { "trusted" } else { "untrusted" };
        let mark = if path == here { " <- this workspace" } else { "" };
        println!("  {word:<9} {path}{mark}");
    }
}
