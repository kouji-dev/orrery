//! `orrery permissions explain` - why a call would be allowed or denied.
//!
//! Implementation plan: `harness/docs/plans/07-policy-broker-audit.md` task 10.

use crate::args::PermissionsCommand;
use crate::exit::not_implemented;

/// Dispatch a `permissions` subcommand.
pub fn dispatch(_command: &PermissionsCommand) -> ! {
    not_implemented("07-policy-broker-audit.md")
}
