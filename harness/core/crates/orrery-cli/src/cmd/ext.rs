//! `orrery ext` - list, install, remove and test extensions.
//!
//! Implementation plans: `harness/docs/plans/06-extension-host.md` for the
//! ledger and the test harness, `harness/docs/plans/15-registry-supply-chain.md`
//! for install and remove, which go through the signed registry.

use crate::args::ExtCommand;
use crate::exit::not_implemented;

/// Dispatch an `ext` subcommand.
pub fn dispatch(command: &ExtCommand) -> ! {
    match command {
        ExtCommand::Install { .. } | ExtCommand::Remove { .. } => {
            not_implemented("15-registry-supply-chain.md")
        }
        ExtCommand::List | ExtCommand::Test { .. } => not_implemented("06-extension-host.md"),
    }
}
