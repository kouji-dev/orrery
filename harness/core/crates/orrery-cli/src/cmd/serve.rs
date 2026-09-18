//! `orrery serve` - kernel only; print the endpoint; keep running.
//!
//! Implementation plan: `harness/docs/plans/17-cli.md` task 4.

use crate::exit::not_implemented;

/// Start a kernel with no client attached.
pub fn dispatch(_listen: Option<&str>) -> ! {
    not_implemented("17-cli.md")
}
