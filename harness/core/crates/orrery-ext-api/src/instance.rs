//! Generation ids and the instance state machine.
//!
//! Translation #10's other half: a manifest is what was *declared*, an instance
//! is what is *running*, and the thing that keeps the two apart at runtime is
//! the generation id.

use serde::{Deserialize, Serialize};

/// A monotonic counter that makes a stale reference unresolvable.
///
/// Reload an extension and the new instance gets a new generation. A
/// [`ToolRef`](orrery_proto::ToolRef) resolved before the reload therefore
/// cannot resurrect the old one: the table answers
/// [`Outcome::Unloaded`](orrery_proto::Outcome), which is a settled tool
/// outcome the model can re-plan around, and never a session failure.
#[derive(
    Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct Generation(pub u64);

impl Generation {
    /// The generation after this one.
    #[must_use]
    pub const fn next(self) -> Self {
        Self(self.0 + 1)
    }
}

impl std::fmt::Display for Generation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "#{}", self.0)
    }
}

/// Where an instance is in its life.
///
/// ```text
/// Loading ──> Live ──unload()──> Draining ──grace elapsed──> Dead
///    │          │                    │
///    │          └──> Degraded ───────┘
///    └──> Dead (it never came up)
/// ```
#[non_exhaustive]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InstanceState {
    /// Being brought up. No calls yet.
    #[default]
    Loading,
    /// Up, and everything it promised works.
    Live,
    /// Up, and some of what it promised does not work.
    Degraded,
    /// Finishing what it has. No new calls.
    Draining,
    /// Gone. A reference to it settles `Unloaded`.
    Dead,
}

impl InstanceState {
    /// Whether a **new** call may be started against this instance.
    ///
    /// `Degraded` still serves: half an extension is usable, and that is the
    /// entire reason the state exists rather than being folded into `Dead`.
    #[must_use]
    pub const fn accepts_calls(self) -> bool {
        matches!(self, InstanceState::Live | InstanceState::Degraded)
    }

    /// Whether this instance is finished with.
    #[must_use]
    pub const fn is_dead(self) -> bool {
        matches!(self, InstanceState::Dead)
    }

    /// Whether this transition is one the machine allows.
    ///
    /// Written down rather than implied, because "how did it get to `Live`
    /// from `Dead`" is the kind of question that only ever gets asked at three
    /// in the morning.
    #[must_use]
    pub const fn can_transition_to(self, next: Self) -> bool {
        use InstanceState::{Dead, Degraded, Draining, Live, Loading};
        matches!(
            (self, next),
            (Loading, Live)
                | (Loading, Degraded)
                | (Loading, Dead)
                | (Live, Degraded)
                | (Degraded, Live)
                | (Live, Draining)
                | (Degraded, Draining)
                | (Draining, Dead)
                // A child that vanished is dead wherever it was.
                | (Live, Dead)
                | (Degraded, Dead)
        )
    }
}

impl std::fmt::Display for InstanceState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            InstanceState::Loading => "loading",
            InstanceState::Live => "live",
            InstanceState::Degraded => "degraded",
            InstanceState::Draining => "draining",
            InstanceState::Dead => "dead",
        };
        f.write_str(s)
    }
}

#[cfg(test)]
mod tests {
    use super::{Generation, InstanceState};

    #[test]
    fn a_generation_only_goes_up() {
        let g = Generation::default();
        assert!(g.next() > g);
        assert!(g.next().next() > g.next());
    }

    #[test]
    fn dead_is_final() {
        for state in [
            InstanceState::Loading,
            InstanceState::Live,
            InstanceState::Degraded,
            InstanceState::Draining,
        ] {
            assert!(
                !InstanceState::Dead.can_transition_to(state),
                "dead must not come back as {state}"
            );
        }
    }

    #[test]
    fn draining_does_not_take_new_calls() {
        assert!(InstanceState::Live.accepts_calls());
        assert!(InstanceState::Degraded.accepts_calls());
        assert!(!InstanceState::Draining.accepts_calls());
        assert!(!InstanceState::Dead.accepts_calls());
        assert!(!InstanceState::Loading.accepts_calls());
    }
}
