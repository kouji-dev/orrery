//! `orrery eval` - run, compare and replay evaluation suites.
//!
//! Implementation plan: `harness/docs/plans/16-eval-runner.md`.

use crate::args::EvalCommand;
use crate::exit::not_implemented;

/// Dispatch an `eval` subcommand.
pub fn dispatch(_command: &EvalCommand) -> ! {
    not_implemented("16-eval-runner.md")
}
