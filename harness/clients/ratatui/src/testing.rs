//! Helpers the tests share. Compiled into the library on purpose: the
//! conformance harness in `tests/` and the widget snapshots both need them, and
//! a `tests/common.rs` would be compiled once per test binary.

use orrery_agui::Frame;
use orrery_client::conformance::{Scenario, Step};
use orrery_client::{SurfaceView, TurnView};
use orrery_proto::Surface;

/// Every frame of a scenario, checkpoints dropped.
#[must_use]
pub fn frames_of(scenario: &Scenario) -> Vec<Frame> {
    scenario
        .steps
        .iter()
        .filter_map(|step| match step {
            Step::Event(frame) => Some(frame.as_ref().clone()),
            _ => None,
        })
        .collect()
}

/// A store's view of a surface, as the widgets want it.
///
/// [`SurfaceView`] and [`Surface`] differ only in how the id is typed: on the
/// AG-UI wire it is a message id, which is not required to be a uuid.
#[must_use]
pub fn as_surface(view: &SurfaceView) -> Surface {
    Surface {
        id: None,
        status: view.status,
        kind: view.kind.clone(),
    }
}

/// The first turn of a replayed scenario, whatever it is called.
#[must_use]
pub fn only_turn(turns: &[TurnView]) -> &TurnView {
    turns.first().expect("the scenario opened a turn")
}
