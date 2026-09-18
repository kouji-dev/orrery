//! `orrery session` - list, show and remove stored sessions.
//!
//! Implementation plan: `harness/docs/plans/02-session-store.md`.

use crate::args::SessionCommand;
use crate::exit::not_implemented;

/// Dispatch a `session` subcommand.
pub fn dispatch(_command: &SessionCommand) -> ! {
    not_implemented("02-session-store.md")
}
