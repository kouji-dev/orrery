//! Renderer selection. Every renderer is an AG-UI subscriber; none is
//! privileged, the in-binary one included.
//!
//! Implementation plan: `harness/docs/plans/17-cli.md` task 2.

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

use clap::ValueEnum;

/// Which renderer attaches to the kernel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Ui {
    /// The default TUI, linked into this binary (`orrery-client-ratatui`).
    Ratatui,
    /// The React/Ink TUI, spawned as a Node process.
    Ink,
    /// Line-delimited AG-UI events on stdout.
    Json,
}

impl Ui {
    /// Which renderer to use, given what was asked for and where stdout goes.
    ///
    /// The tty rule applies to the **interactive** surface — bare `orrery`, and
    /// `attach`. `run` is a non-interactive command whose default is its human
    /// output regardless of where stdout goes; see [`Ui::for_run`].
    #[must_use]
    pub fn resolve(explicit: Option<Ui>, json: bool, tty: bool) -> Ui {
        if json {
            return Ui::Json;
        }
        match explicit {
            Some(ui) => ui,
            None if tty => Ui::Ratatui,
            None => Ui::Json,
        }
    }

    /// Which renderer `run` uses.
    ///
    /// `--json` and `--ui json` give events; everything else gives the final
    /// text. A piped `orrery run` still prints text, because a script that
    /// pipes it and wanted events says `--json` — guessing from a pipe would
    /// mean the same command printed different things under `| tee`.
    #[must_use]
    pub fn for_run(explicit: Option<Ui>, json: bool) -> Ui {
        if json || explicit == Some(Ui::Json) {
            Ui::Json
        } else {
            Ui::Ratatui
        }
    }
}

/// Whether stdout is a terminal.
#[must_use]
pub fn stdout_is_tty() -> bool {
    std::io::IsTerminal::is_terminal(&std::io::stdout())
}

/// The Ink client could not be started.
#[derive(Debug, thiserror::Error)]
pub enum InkError {
    /// There is no `node` to run it with.
    #[error(
        "`--ui ink` needs Node on PATH (the Ink client is a Node process). \
         Install Node 20.19 or newer, or drop `--ui ink` for the built-in TUI."
    )]
    NoNode,
    /// The bundle is not built.
    #[error(
        "the Ink client is not built: {0} does not exist. \
         Run `pnpm -C harness/clients/ink build`, or drop `--ui ink` for the built-in TUI."
    )]
    NotBuilt(PathBuf),
    /// Node is there and refused to start anyway.
    #[error("could not start the Ink client: {0}")]
    Spawn(#[source] std::io::Error),
}

/// Where the built Ink bundle lives, relative to this source tree.
#[must_use]
pub fn ink_bundle() -> PathBuf {
    if let Some(from_env) = std::env::var_os("ORRERY_INK_BUNDLE") {
        return PathBuf::from(from_env);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../clients/ink/dist/index.js")
}

/// Whether there is a `node` on PATH.
///
/// Asked before spawning, so the failure is a sentence rather than an
/// `ErrorKind::NotFound` from somewhere inside `std::process`.
#[must_use]
pub fn node_on_path() -> bool {
    Command::new("node")
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok()
}

/// Spawn the Ink client against an endpoint.
///
/// The endpoint travels in `ORRERY_ENDPOINT`, which is the whole contract a
/// third-party client implements — the same variable a person exports by hand
/// when they run `node dist/index.js` against `orrery serve`.
///
/// # Errors
///
/// [`InkError`] when Node is missing, the bundle is not built, or the process
/// would not start.
pub fn spawn_ink(endpoint: &str, session: &str) -> Result<Child, InkError> {
    if !node_on_path() {
        return Err(InkError::NoNode);
    }
    let bundle = ink_bundle();
    if !bundle.exists() {
        return Err(InkError::NotBuilt(bundle));
    }
    Command::new("node")
        .arg(&bundle)
        .env("ORRERY_ENDPOINT", endpoint)
        .env("ORRERY_SESSION", session)
        .spawn()
        .map_err(InkError::Spawn)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Piping the output selects the json renderer (`17-cli.md` task 2).
    #[test]
    fn defaults_to_json_without_a_tty() {
        assert_eq!(Ui::resolve(None, false, false), Ui::Json);
        assert_eq!(Ui::resolve(None, false, true), Ui::Ratatui);
    }

    /// `--json` wins over the tty, and `--ui` wins over the default.
    #[test]
    fn explicit_beats_the_default() {
        assert_eq!(Ui::resolve(None, true, true), Ui::Json);
        assert_eq!(Ui::resolve(Some(Ui::Ink), false, false), Ui::Ink);
    }

    /// `run` prints text unless it is asked for events.
    #[test]
    fn run_is_text_unless_asked() {
        assert_eq!(Ui::for_run(None, false), Ui::Ratatui);
        assert_eq!(Ui::for_run(None, true), Ui::Json);
        assert_eq!(Ui::for_run(Some(Ui::Json), false), Ui::Json);
    }
}
