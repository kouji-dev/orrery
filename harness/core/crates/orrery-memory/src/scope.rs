//! Scopes are handles on things the kernel already creates and destroys.
//!
//! Never a free-form label. `global`, `workspace`, `project`, `session`,
//! `workflow`, `branch` and `turn` all name something the harness already owns
//! the lifetime of, so lifetime and cleanup come for free:
//!
//! | Scope | Lives as long as | Cleared at |
//! |---|---|---|
//! | `global` | the install | an explicit `forget` |
//! | `workspace` · `project` | the config layer it sits in | the folder leaves the config set |
//! | `session` | one session | `session.end` |
//! | `workflow` | one declared run | the workflow's budget closing |
//! | `branch` | a sub-agent or a retry branch | the branch, discarded ones included |
//! | `turn` | one user turn | `turn.end` |
//!
//! # Nobody writes cleanup code
//!
//! A scope is entered with [`Lifetimes::enter`], which hands back a
//! [`ScopeGuard`]. **Dropping the guard is what discarding the thing is**: a
//! sub-agent's branch ending, a retry being abandoned, a turn settling. From
//! that instant the scope no longer resolves, so nothing recalled from it can
//! reach a model, and an abandoned retry leaves no residue — without a single
//! call to `forget` anywhere.
//!
//! # Clearing is lazy
//!
//! Retiring a scope is a fact about *resolution*, not about bytes. The bytes go
//! on the next [`MemoryKernel::sweep`](crate::MemoryKernel::sweep), which is a
//! lifecycle handler's job and may reach a store. See the decision recorded
//! under open question 2 in `harness/docs/plans/12-memory.md`.

use std::collections::HashSet;
use std::sync::Arc;

use orrery_proto::{BranchId, RunId, SessionId, TurnId};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

/// Which *kind* of scope, without the handle.
///
/// This is what a permission rule names — `mem.write(global)` — and what a
/// provider declares it supports.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ScopeKind {
    /// One user turn.
    Turn,
    /// A sub-agent or a retry branch.
    Branch,
    /// One declared workflow run.
    Workflow,
    /// One session.
    Session,
    /// The project inside the workspace.
    Project,
    /// The workspace.
    Workspace,
    /// The install.
    Global,
}

impl ScopeKind {
    /// Every kind, narrowest first.
    pub const ALL: [ScopeKind; 7] = [
        ScopeKind::Turn,
        ScopeKind::Branch,
        ScopeKind::Workflow,
        ScopeKind::Session,
        ScopeKind::Project,
        ScopeKind::Workspace,
        ScopeKind::Global,
    ];

    /// How wide it is. `turn` is 0 and `global` is 6, so "wider than" is `>`.
    ///
    /// This is the ordering the write rule is stated in: never write to a scope
    /// wider than the one you run in.
    #[must_use]
    pub const fn width(self) -> u8 {
        match self {
            ScopeKind::Turn => 0,
            ScopeKind::Branch => 1,
            ScopeKind::Workflow => 2,
            ScopeKind::Session => 3,
            ScopeKind::Project => 4,
            ScopeKind::Workspace => 5,
            ScopeKind::Global => 6,
        }
    }

    /// The name a permission rule and the ledger both use.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            ScopeKind::Turn => "turn",
            ScopeKind::Branch => "branch",
            ScopeKind::Workflow => "workflow",
            ScopeKind::Session => "session",
            ScopeKind::Project => "project",
            ScopeKind::Workspace => "workspace",
            ScopeKind::Global => "global",
        }
    }

    /// Whether this kind retires on its own, or only on an explicit `forget`.
    ///
    /// `global` never does. `workspace` and `project` retire when the folder
    /// leaves the config set, which is a config event rather than a lifecycle
    /// point; the rest retire when the kernel object they name ends.
    #[must_use]
    pub const fn is_ephemeral(self) -> bool {
        !matches!(self, ScopeKind::Global)
    }
}

impl std::fmt::Display for ScopeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

/// A scope, with the handle of the thing whose lifetime it borrows.
///
/// The handle is the point: there is no `MemScope::Named(String)`, so there is
/// no scope whose lifetime nobody owns.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "kebab-case")]
pub enum MemScope {
    /// The install.
    Global,
    /// One workspace root.
    Workspace(String),
    /// One project folder inside a workspace.
    Project(String),
    /// One session.
    Session(SessionId),
    /// One declared workflow run.
    Workflow(RunId),
    /// One branch: a sub-agent, or a retry.
    Branch(BranchId),
    /// One user turn.
    Turn(TurnId),
}

impl MemScope {
    /// Which kind this is.
    #[must_use]
    pub const fn kind(&self) -> ScopeKind {
        match self {
            MemScope::Global => ScopeKind::Global,
            MemScope::Workspace(_) => ScopeKind::Workspace,
            MemScope::Project(_) => ScopeKind::Project,
            MemScope::Session(_) => ScopeKind::Session,
            MemScope::Workflow(_) => ScopeKind::Workflow,
            MemScope::Branch(_) => ScopeKind::Branch,
            MemScope::Turn(_) => ScopeKind::Turn,
        }
    }

    /// How wide it is. See [`ScopeKind::width`].
    #[must_use]
    pub const fn width(&self) -> u8 {
        self.kind().width()
    }
}

impl std::fmt::Display for MemScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MemScope::Global => f.write_str("global"),
            MemScope::Workspace(p) => write!(f, "workspace:{p}"),
            MemScope::Project(p) => write!(f, "project:{p}"),
            MemScope::Session(s) => write!(f, "session:{s}"),
            MemScope::Workflow(r) => write!(f, "workflow:{r}"),
            MemScope::Branch(b) => write!(f, "branch:{b}"),
            MemScope::Turn(t) => write!(f, "turn:{t}"),
        }
    }
}

/// Which scopes are still alive.
///
/// Shared, cheap to clone, and deliberately synchronous: retiring a scope
/// happens in a `Drop`, where there is no runtime to await on.
#[derive(Debug, Default)]
pub struct Lifetimes {
    /// Scopes that no longer resolve. Emptied only by re-entering the scope.
    retired: Mutex<HashSet<MemScope>>,
    /// Scopes whose bytes are still in a store. A subset of `retired` until a
    /// sweep clears it — the lazy half of the decision recorded under open
    /// question 2.
    unswept: Mutex<HashSet<MemScope>>,
}

impl Lifetimes {
    /// A fresh registry.
    #[must_use]
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Take out a scope's lifetime. Dropping what comes back retires it.
    ///
    /// Re-entering a scope that was retired revives it, because a retry branch
    /// may legitimately reuse an id the store has not swept yet.
    #[must_use]
    pub fn enter(self: &Arc<Self>, scope: MemScope) -> ScopeGuard {
        self.retired.lock().remove(&scope);
        self.unswept.lock().remove(&scope);
        ScopeGuard {
            lifetimes: Arc::clone(self),
            scope: Some(scope),
        }
    }

    /// Retire a scope without holding a guard, for a config folder leaving the
    /// set — the one case that is not a kernel object ending.
    pub fn retire(&self, scope: MemScope) {
        if scope.kind().is_ephemeral() {
            self.unswept.lock().insert(scope.clone());
            self.retired.lock().insert(scope);
        }
    }

    /// Whether this scope still resolves.
    #[must_use]
    pub fn is_live(&self, scope: &MemScope) -> bool {
        !self.retired.lock().contains(scope)
    }

    /// Everything retired and not yet swept, in a stable order.
    #[must_use]
    pub fn pending_sweep(&self) -> Vec<MemScope> {
        let mut out: Vec<MemScope> = self.unswept.lock().iter().cloned().collect();
        out.sort();
        out
    }

    /// Note that a scope's bytes are gone. It stays retired: a swept scope must
    /// not come back to life just because nothing is left of it.
    pub fn swept(&self, scope: &MemScope) {
        self.unswept.lock().remove(scope);
    }
}

/// A scope's lifetime, held for as long as the thing it names exists.
///
/// **Dropping it is the cleanup.** Nothing calls `forget`.
#[derive(Debug)]
pub struct ScopeGuard {
    lifetimes: Arc<Lifetimes>,
    scope: Option<MemScope>,
}

impl ScopeGuard {
    /// Which scope this holds open.
    ///
    /// # Panics
    ///
    /// Never: the option is `Some` for the whole life of the guard and is only
    /// taken in `Drop`.
    #[must_use]
    pub fn scope(&self) -> &MemScope {
        self.scope.as_ref().expect("a live guard holds its scope")
    }
}

impl Drop for ScopeGuard {
    fn drop(&mut self) {
        if let Some(scope) = self.scope.take() {
            self.lifetimes.retire(scope);
        }
    }
}
