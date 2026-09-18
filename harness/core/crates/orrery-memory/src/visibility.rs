//! Visibility copies §4.10's sub-agent rule, and it is enforced **here**.
//!
//! Three sentences, and the third is the one that matters:
//!
//! 1. Read down your own chain.
//! 2. Never across siblings.
//! 3. **Never write to a scope wider than the one you run in** — otherwise a
//!    sub-agent denied `write` puts a secret in `global` for its parent to read
//!    back, and the denial bought nothing.
//!
//! None of this is asked of the provider. A provider is handed a scope list
//! that has already been filtered, and a write it must not accept never reaches
//! it. A store that would happily do the wrong thing is therefore harmless,
//! which is the only assumption worth making about somebody else's code.

use orrery_proto::{AgentScope, BranchId, RunId, SessionId, Subject, TurnId};

use crate::scope::{Lifetimes, MemScope, ScopeKind};

/// Who is reading or writing, and where they sit in the tree.
///
/// Built by the kernel from the branch it is about to run, never from anything
/// an extension said about itself.
#[derive(Clone, Debug)]
pub struct Actor {
    /// Who they are, for a permission rule.
    pub subject: Subject,
    /// The scope they run under, for the grant half of the permission check.
    pub agent: AgentScope,
    /// Which session.
    pub session: SessionId,
    /// The branch chain, **root first and self last**. Reading "down your own
    /// chain" is reading anything in here.
    pub ancestry: Vec<BranchId>,
    /// The workspace root, when there is one.
    pub workspace: Option<String>,
    /// The project folder, when there is one.
    pub project: Option<String>,
    /// The workflow run, when this is one.
    pub workflow: Option<RunId>,
    /// The turn in play, when there is one.
    pub turn: Option<TurnId>,
    /// The widest scope this actor may write to.
    ///
    /// Set by the kernel: [`ScopeKind::Global`] for the main agent, whose rules
    /// then decide, and [`ScopeKind::Branch`] for anything running on a
    /// sub-agent branch. Never taken from a manifest.
    pub run_scope: ScopeKind,
}

impl Actor {
    /// The main agent: one branch, no ancestors, and free to write as wide as
    /// its permission rules allow.
    #[must_use]
    pub fn root(subject: Subject, agent: AgentScope, session: SessionId, branch: BranchId) -> Self {
        Self {
            subject,
            agent,
            session,
            ancestry: vec![branch],
            workspace: None,
            project: None,
            workflow: None,
            turn: None,
            run_scope: ScopeKind::Global,
        }
    }

    /// A sub-agent, on a branch, under a chain. **Pinned to `branch` width**:
    /// this is the constructor that makes rule 3 structural.
    #[must_use]
    pub fn sub_agent(
        subject: Subject,
        agent: AgentScope,
        session: SessionId,
        chain: impl IntoIterator<Item = BranchId>,
    ) -> Self {
        let ancestry: Vec<BranchId> = chain.into_iter().collect();
        Self {
            subject,
            agent,
            session,
            ancestry,
            workspace: None,
            project: None,
            workflow: None,
            turn: None,
            run_scope: ScopeKind::Branch,
        }
    }

    /// Name the workspace root.
    #[must_use]
    pub fn in_workspace(mut self, workspace: impl Into<String>) -> Self {
        self.workspace = Some(workspace.into());
        self
    }

    /// Name the project folder.
    #[must_use]
    pub fn in_project(mut self, project: impl Into<String>) -> Self {
        self.project = Some(project.into());
        self
    }

    /// Name the workflow run.
    #[must_use]
    pub const fn in_workflow(mut self, run: RunId) -> Self {
        self.workflow = Some(run);
        self
    }

    /// Name the turn in play.
    #[must_use]
    pub const fn in_turn(mut self, turn: TurnId) -> Self {
        self.turn = Some(turn);
        self
    }

    /// The branch it runs on: the last link of the chain.
    #[must_use]
    pub fn branch(&self) -> Option<BranchId> {
        self.ancestry.last().copied()
    }

    /// Whether this actor may read from a scope.
    ///
    /// Sibling branches fail the [`MemScope::Branch`] arm: a sibling's id is not
    /// on this actor's chain, and there is no arm that would let it be.
    #[must_use]
    pub fn can_read(&self, scope: &MemScope) -> bool {
        match scope {
            MemScope::Global => true,
            MemScope::Workspace(p) => self.workspace.as_deref() == Some(p.as_str()),
            MemScope::Project(p) => self.project.as_deref() == Some(p.as_str()),
            MemScope::Session(s) => *s == self.session,
            MemScope::Workflow(r) => self.workflow == Some(*r),
            MemScope::Branch(b) => self.ancestry.contains(b),
            MemScope::Turn(t) => self.turn == Some(*t),
        }
    }

    /// Whether this actor may write to a scope.
    ///
    /// Readable **and** no wider than [`run_scope`](Self::run_scope). The width
    /// half is checked first so the refusal names the real reason.
    #[must_use]
    pub fn can_write(&self, scope: &MemScope) -> bool {
        scope.width() <= self.run_scope.width() && self.can_read(scope)
    }

    /// Why a write was refused, in words a person can act on.
    #[must_use]
    pub fn refuse_write(&self, scope: &MemScope) -> String {
        if scope.width() > self.run_scope.width() {
            format!(
                "`{}` runs at `{}` scope and may not write to `{}`, which is wider",
                self.agent.agent,
                self.run_scope,
                scope.kind(),
            )
        } else {
            format!("`{}` is not on `{}`'s own chain", scope, self.agent.agent)
        }
    }

    /// Every scope this actor may read from, widest first, filtered to the ones
    /// still alive.
    ///
    /// Widest first so that a clamp spends its allowance on the durable notes
    /// before the ephemeral ones, and so two providers are asked in the same
    /// order.
    #[must_use]
    pub fn readable_scopes(&self, lifetimes: &Lifetimes) -> Vec<MemScope> {
        let mut out = vec![MemScope::Global];
        if let Some(ws) = &self.workspace {
            out.push(MemScope::Workspace(ws.clone()));
        }
        if let Some(p) = &self.project {
            out.push(MemScope::Project(p.clone()));
        }
        out.push(MemScope::Session(self.session));
        if let Some(r) = self.workflow {
            out.push(MemScope::Workflow(r));
        }
        // Root first, so a parent's notes outrank a child's own scratch.
        out.extend(self.ancestry.iter().map(|b| MemScope::Branch(*b)));
        if let Some(t) = self.turn {
            out.push(MemScope::Turn(t));
        }
        out.retain(|s| lifetimes.is_live(s));
        out
    }
}
