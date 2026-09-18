//! `orrery attach` - a renderer against a running kernel.
//!
//! Implementation plan: `harness/docs/plans/17-cli.md` task 4.

use crate::exit::not_implemented;

/// Attach the selected renderer to an endpoint, replaying from `since`.
pub fn dispatch(_endpoint: &str, _since: Option<u64>) -> ! {
    not_implemented("17-cli.md")
}
