//! `orrery replay` - re-emit a stored session as events.
//!
//! Implementation plan: `harness/docs/plans/17-cli.md` task 8.

use crate::exit::not_implemented;

/// Replay a stored session into the selected renderer.
pub fn dispatch(_session: &str) -> ! {
    not_implemented("17-cli.md")
}
