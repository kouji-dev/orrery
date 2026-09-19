//! `orrery import` - bring configuration over from another harness.
//!
//! One-way and explicit: this reads a Claude Code or Codex setup **once**, when
//! a person runs the command. Nothing in the harness reads a foreign config
//! while a session runs, and `orrery_config::import`'s own tests prove it.
//!
//! **The config goes to stdout, the notes go to stderr**, so
//! `orrery import --from codex > .orrery/config.toml` writes a file that
//! parses, and what could not be mapped is still said out loud rather than
//! dropped.
//!
//! Implementation plan: `harness/docs/plans/10-config-layers.md` task 8,
//! surfaced by `harness/docs/plans/17-cli.md` task 9.

use std::path::PathBuf;

use orrery_config::Imported;

use crate::args::{Cli, ImportFrom};
use crate::exit::{Exit, fail};

/// Import configuration from another harness.
pub fn dispatch(cli: &Cli, from: Option<ImportFrom>) -> ! {
    let Some(from) = from else {
        fail(
            Exit::Usage,
            "say what to import from: `--from claude-code` or `--from codex`",
        );
    };

    let workspace = cli.workspace.clone().unwrap_or_else(|| {
        std::env::current_dir().unwrap_or_else(|e| fail(Exit::Usage, format!("no workspace: {e}")))
    });
    let candidates = candidates(from, &workspace);
    let Some((path, text)) = first_readable(&candidates) else {
        fail(
            Exit::Usage,
            format!(
                "nothing to import: no {} in {}",
                what(from),
                candidates
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join(" or ")
            ),
        );
    };

    let imported: Imported = match from {
        ImportFrom::ClaudeCode => orrery_config::import::claude_code(&path, &text),
        ImportFrom::Codex => orrery_config::import::codex(&path, &text),
    }
    .unwrap_or_else(|e| fail(Exit::Usage, format!("{}: {e}", path.display())));

    eprintln!("orrery: imported from {}", path.display());
    for note in &imported.notes {
        eprintln!("orrery: not imported — {note}");
    }
    eprintln!("orrery: review it before you use it");

    // `--json` has nothing extra to say here: the config *is* the data.
    print!("{}", imported.to_toml());
    Exit::Ok.exit()
}

/// Where each harness keeps the file, closest first.
fn candidates(from: ImportFrom, workspace: &std::path::Path) -> Vec<PathBuf> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from);
    match from {
        ImportFrom::ClaudeCode => [
            Some(workspace.join(".claude/settings.json")),
            Some(workspace.join(".claude/settings.local.json")),
            home.map(|h| h.join(".claude/settings.json")),
        ]
        .into_iter()
        .flatten()
        .collect(),
        ImportFrom::Codex => [
            Some(workspace.join(".codex/config.toml")),
            home.map(|h| h.join(".codex/config.toml")),
        ]
        .into_iter()
        .flatten()
        .collect(),
    }
}

fn what(from: ImportFrom) -> &'static str {
    match from {
        ImportFrom::ClaudeCode => "Claude Code `settings.json`",
        ImportFrom::Codex => "Codex `.codex/config.toml`",
    }
}

fn first_readable(paths: &[PathBuf]) -> Option<(PathBuf, String)> {
    paths
        .iter()
        .find_map(|p| std::fs::read_to_string(p).ok().map(|t| (p.clone(), t)))
}
