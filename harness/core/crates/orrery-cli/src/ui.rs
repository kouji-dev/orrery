//! Renderer selection. Every renderer is an AG-UI subscriber; none is
//! privileged, the in-binary one included.
//!
//! Implementation plan: `harness/docs/plans/17-cli.md` task 2.

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
