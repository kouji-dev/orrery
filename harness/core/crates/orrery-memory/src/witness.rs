//! "Writes only from lifecycle handlers", enforced by the type system.
//!
//! [`MemoryProvider::write`](crate::MemoryProvider::write) and
//! [`MemoryProvider::forget`](crate::MemoryProvider::forget) take a
//! `&LifecycleWitness`. A [`LifecycleWitness`] has no public constructor and no
//! public field; the **only** way to obtain one is [`LifecycleCtx::witness`],
//! and a `LifecycleCtx` is what a lifecycle handler is handed. An interceptor is
//! handed an [`InterceptCtx`], which has no `witness` method and cannot be
//! turned into a `LifecycleCtx`.
//!
//! So memory physically cannot write from inside the loop. That is the whole of
//! translation #1 for this crate, and it is checked by the compiler rather than
//! by a reviewer.
//!
//! # `witness::interceptor_cannot_write`
//!
//! An interceptor context cannot produce a witness:
//!
//! ```compile_fail
//! use orrery_memory::{InterceptCtx, LifecycleWitness};
//! use orrery_proto::{BranchId, SessionId, TurnId};
//!
//! let ctx = InterceptCtx::new(SessionId::new(), TurnId::new(), BranchId::new());
//! // error[E0599]: no method named `witness` found for struct `InterceptCtx`
//! let _w: LifecycleWitness = ctx.witness();
//! ```
//!
//! …and it cannot route around the method by building one itself:
//!
//! ```compile_fail
//! use orrery_memory::{LifecyclePoint, LifecycleWitness};
//!
//! // error[E0451]: field `point` of struct `LifecycleWitness` is private
//! let _w = LifecycleWitness { point: LifecyclePoint::TurnEnd };
//! ```
//!
//! The positive case, for contrast, compiles:
//!
//! ```
//! use orrery_memory::{LifecycleCtx, LifecyclePoint};
//! use orrery_proto::{BranchId, SessionId};
//!
//! let ctx = LifecycleCtx::at(LifecyclePoint::TurnEnd, SessionId::new(), BranchId::new());
//! let w = ctx.witness();
//! assert_eq!(w.point(), LifecyclePoint::TurnEnd);
//! ```

use orrery_proto::{BranchId, SessionId, TurnId};

/// Where a lifecycle handler fires.
///
/// The same five points `orrery-kernel` publishes, named here so that this
/// crate stays publishable and does not depend on the kernel.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum LifecyclePoint {
    /// A session opened.
    SessionStart,
    /// A session closed.
    SessionEnd,
    /// A turn settled, whatever it settled as.
    TurnEnd,
    /// A branch closed.
    BranchClose,
    /// A workflow finished.
    WorkflowEnd,
}

impl LifecyclePoint {
    /// The dotted name, as a manifest writes it and the ledger prints it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            LifecyclePoint::SessionStart => "session.start",
            LifecyclePoint::SessionEnd => "session.end",
            LifecyclePoint::TurnEnd => "turn.end",
            LifecyclePoint::BranchClose => "branch.close",
            LifecyclePoint::WorkflowEnd => "workflow.end",
        }
    }
}

impl std::fmt::Display for LifecyclePoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

/// Proof that the caller is a lifecycle handler.
///
/// Unforgeable from outside this crate: the field is private and there is no
/// constructor. Not `Clone` and not `Copy`, so it cannot be squirrelled away by
/// a handler and used from somewhere else later — it is borrowed for the length
/// of one call and nothing else.
#[derive(Debug)]
pub struct LifecycleWitness {
    point: LifecyclePoint,
}

impl LifecycleWitness {
    /// Where the handler that produced it fired.
    #[must_use]
    pub const fn point(&self) -> LifecyclePoint {
        self.point
    }
}

/// What a lifecycle handler is told, and the one thing that can mint a witness.
#[derive(Clone, Debug)]
pub struct LifecycleCtx {
    /// Where it fired.
    pub point: LifecyclePoint,
    /// Which session.
    pub session: SessionId,
    /// Which branch.
    pub branch: BranchId,
    /// Which turn, when one is in play.
    pub turn: Option<TurnId>,
}

impl LifecycleCtx {
    /// A context for a point, a session and a branch.
    #[must_use]
    pub const fn at(point: LifecyclePoint, session: SessionId, branch: BranchId) -> Self {
        Self {
            point,
            session,
            branch,
            turn: None,
        }
    }

    /// Name the turn this fired for.
    #[must_use]
    pub const fn for_turn(mut self, turn: TurnId) -> Self {
        self.turn = Some(turn);
        self
    }

    /// The witness. The only public constructor there is.
    #[must_use]
    pub const fn witness(&self) -> LifecycleWitness {
        LifecycleWitness { point: self.point }
    }
}

/// What an interceptor is told: the same three ids, and **no way to write**.
///
/// Deliberately a separate type with a deliberately smaller surface. It exists
/// in this crate so the compile-fail proof has something to point at without
/// `orrery-memory` depending on `orrery-kernel`; the kernel's own
/// `InterceptCtx` is the same shape and, like this one, has no `witness`.
#[derive(Copy, Clone, Debug)]
pub struct InterceptCtx {
    session: SessionId,
    turn: TurnId,
    branch: BranchId,
}

impl InterceptCtx {
    /// What an interceptor gets to see.
    #[must_use]
    pub const fn new(session: SessionId, turn: TurnId, branch: BranchId) -> Self {
        Self {
            session,
            turn,
            branch,
        }
    }

    /// Which session.
    #[must_use]
    pub const fn session_id(&self) -> SessionId {
        self.session
    }

    /// Which turn.
    #[must_use]
    pub const fn turn_id(&self) -> TurnId {
        self.turn
    }

    /// Which branch.
    #[must_use]
    pub const fn branch_id(&self) -> BranchId {
        self.branch
    }
}
