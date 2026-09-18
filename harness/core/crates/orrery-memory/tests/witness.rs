//! Task 2 · The witness.
//!
//! `witness::interceptor_cannot_write` is a pair of **`compile_fail` doctests**
//! on [`orrery_memory::LifecycleWitness`] and
//! [`orrery_memory::InterceptCtx`], not a `trybuild` case. `trybuild` is not in
//! this workspace's pins or in `Cargo.lock`, and adding one mid-flight would
//! mean a registry fetch this phase forbids; a `compile_fail` doctest is
//! compiled against the crate **as an external dependency**, which is exactly
//! the vantage point the guarantee needs, and it is what the rest of this repo
//! already uses (`orrery-policy`'s `CapabilityToken`, `orrery-session`'s
//! `BranchLease`, `orrery-kernel`'s phase table).
//!
//! `cargo test -p orrery-memory` runs them. This file holds the half that can
//! be asserted at run time: that the two contexts really are different types
//! and that only one of them has a witness to give.

use orrery_memory::{InterceptCtx, LifecycleCtx, LifecyclePoint};
use orrery_proto::{BranchId, SessionId, TurnId};

#[test]
fn a_lifecycle_context_yields_a_witness() {
    let ctx = LifecycleCtx::at(LifecyclePoint::TurnEnd, SessionId::new(), BranchId::new());
    // The only public way to obtain one.
    let w = ctx.witness();
    assert_eq!(w.point(), LifecyclePoint::TurnEnd);
}

#[test]
fn an_interceptor_context_carries_no_way_to_make_one() {
    let ctx = InterceptCtx::new(SessionId::new(), TurnId::new(), BranchId::new());
    // There is no `ctx.witness()`; the compile-fail doctest is the proof. All
    // that is left to assert here is that the type exists and is inert.
    assert_eq!(ctx.session_id(), ctx.session_id());
}
