//! `orrery import` - bring configuration over from another harness.
//!
//! Implementation plan: `harness/docs/plans/10-config-layers.md`.

use crate::args::ImportFrom;
use crate::exit::not_implemented;

/// Import configuration from another harness.
pub fn dispatch(_from: Option<ImportFrom>) -> ! {
    not_implemented("10-config-layers.md")
}
