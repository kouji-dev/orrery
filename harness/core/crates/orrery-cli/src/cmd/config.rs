//! `orrery config explain` - which layer won, and where it is written.
//!
//! Implementation plan: `harness/docs/plans/10-config-layers.md` task 7.

use crate::args::ConfigCommand;
use crate::exit::not_implemented;

/// Dispatch a `config` subcommand.
pub fn dispatch(_command: &ConfigCommand) -> ! {
    not_implemented("10-config-layers.md")
}
