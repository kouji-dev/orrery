//! The turn tree's rows.
//!
//! A turn is **immutable**. Nothing in this crate mutates one: `compact` writes
//! a [`TurnKind::Summary`] and a watermark, a retry writes a new branch, an
//! edited turn forks. That is what makes `materialise` a pure function of the
//! rows and what makes a session replayable at all.

use orrery_proto::{
    BranchId, CallId, CancelReason, ContentBlock, Event, Outcome, Seq, SessionId, ToolRef, TurnId,
    Usage, UserInput,
};
use serde::{Deserialize, Serialize};

/// What a turn *is*.
///
/// Six kinds, and the interesting ones are the last three: a
/// [`BranchResult`](TurnKind::BranchResult) is how a sub-agent's answer lands
/// in its parent without the child ever writing to the parent's rows, a
/// [`Summary`](TurnKind::Summary) is compaction that never destroys what it
/// compacted, and [`Recalled`](TurnKind::Recalled) is why memory is replayable:
/// the memory store moves on, the tree still shows what the model saw.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "kebab-case")]
pub enum TurnKind {
    /// A person submitted something.
    User {
        /// What they submitted.
        input: UserInput,
    },
    /// The model answered.
    Assistant {
        /// What it said.
        content: Vec<ContentBlock>,
        /// What it cost.
        usage: Usage,
    },
    /// A tool call settled.
    ToolResult {
        /// Which call.
        call: CallId,
        /// Which tool.
        r#ref: ToolRef,
        /// How it went.
        outcome: Outcome,
    },
    /// Written by the **parent** when a child branch closes. The child's turns
    /// stay exactly where they are; this row is the parent's record of the
    /// join. See the invariant at the top of [`crate::lease`].
    BranchResult {
        /// The branch that closed.
        child: BranchId,
        /// How it went.
        outcome: BranchOutcome,
    },
    /// Written by [`compact`](crate::SessionStore::compact). Never replaces
    /// anything; a watermark row points at it.
    Summary {
        /// The inclusive span of sequence numbers this stands in for.
        covers: (Seq, Seq),
        /// The summary itself.
        text: String,
        /// What producing it cost.
        usage: Usage,
    },
    /// Memory recalled at `context.build`, recorded as **resolved content**
    /// (§4.3) rather than as a query to be re-run.
    Recalled {
        /// Which memory provider answered.
        provider: String,
        /// What it returned, verbatim.
        entries: Vec<RecalledEntry>,
    },
}

impl TurnKind {
    /// The stable wire discriminant, used as the `kind` column so a backend can
    /// filter without deserialising every payload.
    #[must_use]
    pub fn tag(&self) -> &'static str {
        match self {
            TurnKind::User { .. } => "user",
            TurnKind::Assistant { .. } => "assistant",
            TurnKind::ToolResult { .. } => "tool-result",
            TurnKind::BranchResult { .. } => "branch-result",
            TurnKind::Summary { .. } => "summary",
            TurnKind::Recalled { .. } => "recalled",
        }
    }

    /// What this turn cost, when it cost anything.
    #[must_use]
    pub fn usage(&self) -> Usage {
        match self {
            TurnKind::Assistant { usage, .. } | TurnKind::Summary { usage, .. } => *usage,
            _ => Usage::default(),
        }
    }
}

/// One thing a memory provider returned.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RecalledEntry {
    /// How the provider names it, so the same entry can be recognised later.
    pub key: String,
    /// The text that actually went into the context.
    pub text: String,
    /// How relevant the provider thought it was, when it says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
}

/// How a branch ended.
///
/// The value a parent records in its [`TurnKind::BranchResult`], and the value
/// [`close_branch`](crate::SessionStore::close_branch) is handed.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "kebab-case")]
pub enum BranchOutcome {
    /// It finished and has something to say.
    Completed {
        /// What it concluded.
        summary: String,
    },
    /// It ran and went wrong.
    Failed {
        /// A stable, machine-readable code.
        code: String,
        /// What went wrong, in words.
        message: String,
    },
    /// It was stopped.
    Cancelled {
        /// Why.
        reason: CancelReason,
    },
}

impl BranchOutcome {
    /// The stable discriminant, for the `branches.state` column.
    #[must_use]
    pub fn tag(&self) -> &'static str {
        match self {
            BranchOutcome::Completed { .. } => "completed",
            BranchOutcome::Failed { .. } => "failed",
            BranchOutcome::Cancelled { .. } => "cancelled",
        }
    }
}

/// A turn on its way in: no id and no sequence number yet, because only the
/// branch's lease may hand those out.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NewTurn {
    /// What the turn is.
    pub kind: TurnKind,
}

impl NewTurn {
    /// Wrap a kind.
    #[must_use]
    pub fn new(kind: TurnKind) -> Self {
        Self { kind }
    }
}

impl From<TurnKind> for NewTurn {
    fn from(kind: TurnKind) -> Self {
        Self::new(kind)
    }
}

/// A turn as it is stored: the row `materialise` is handed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TurnRow {
    /// Its id.
    pub id: TurnId,
    /// The branch it sits on.
    pub branch: BranchId,
    /// Where it sits on that branch. One-based, contiguous, never reused.
    pub seq: Seq,
    /// What it is.
    pub kind: TurnKind,
    /// Unix milliseconds.
    pub created_at: i64,
}

/// What [`open`](crate::SessionStore::open) hands back.
#[derive(Clone, Debug, PartialEq)]
pub struct SessionHandle {
    /// The session.
    pub session: SessionId,
    /// Its first branch — the one a turn lands on when nobody says otherwise.
    pub root: BranchId,
    /// The workspace root it was created against.
    pub workspace: String,
    /// The profile it was created from.
    pub profile: String,
    /// Every branch in the session, root first.
    pub branches: Vec<BranchId>,
}

/// An event as the store kept it, for `session.attach(since)` and
/// `orrery replay`.
#[derive(Clone, Debug, PartialEq)]
pub struct StoredEvent {
    /// Where it sits in the session's order. Contiguous and gap-free: a client
    /// detects a lost frame by arithmetic.
    pub seq: Seq,
    /// The event itself.
    pub event: Event,
}

/// What [`compact`](crate::SessionStore::compact) did.
#[derive(Clone, Debug, PartialEq)]
pub struct CompactResult {
    /// The summary turn that was written. It is a row like any other.
    pub summary: TurnId,
    /// The watermark that now sits over the branch.
    pub watermark: Seq,
    /// How many rows the watermark stands in front of. They are all still
    /// there — `compact` writes, it never mutates.
    pub covered: u64,
}
