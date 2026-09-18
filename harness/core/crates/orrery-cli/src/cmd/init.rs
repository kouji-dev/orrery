//! `orrery init` - write a starter configuration into the workspace.
//!
//! Implementation plan: `harness/docs/plans/10-config-layers.md`.

use crate::exit::not_implemented;

/// Scaffold configuration for a profile.
pub fn dispatch(_profile: Option<&str>) -> ! {
    not_implemented("10-config-layers.md")
}
