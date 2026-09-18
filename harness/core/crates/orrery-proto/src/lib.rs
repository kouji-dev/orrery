//! The wire and the shared types, and nothing else. No async, no I/O.
//!
//! Every type two other crates must agree on lives here: the ids, the grant and
//! budget shapes, the provider-neutral message, the surface vocabulary and the
//! request/event frames. The crate depends on `serde`, `serde_json`, `schemars`,
//! `uuid` and `thiserror` and on nothing else, so that anybody — the kernel, an
//! out-of-process extension, a client SDK — can name these types without
//! pulling in a runtime.
//!
//! # The three rules everything here follows
//!
//! - **Explicit wire names.** Every tagged enum names its variants one by one.
//!   Dotted tags (`turn.submit`, `mem.read`) are not what any `rename_all` rule
//!   produces, so a missing `rename` is a silently different protocol — and
//!   there is a test for exactly that.
//! - **Struct variants only**, in every tagged enum. Internal tagging cannot be
//!   applied to a newtype variant around a non-map, so a newtype variant works
//!   in JSON and breaks over CBOR. The CBOR round-trip test is the guard.
//! - **`#[non_exhaustive]`** on every enum that crosses the wire, so adding a
//!   variant is not a breaking change for anyone matching on it.
//!
//! Implementation plan: `harness/docs/plans/01-proto-shared-types.md`

#![deny(missing_docs)]
#![forbid(unsafe_code)]

pub mod budget;
pub mod expr;
pub mod frame;
pub mod grant;
pub mod ids;
pub mod load;
pub mod message;
pub mod scope;
pub mod surface;
pub mod verdict;

pub use budget::{Budget, BudgetKind, TokenBudget, Usage};
pub use expr::{CmpOp, Expr, Predicate};
pub use frame::{
    CancelReason, ConsentAnswerKind, ConsentPrompt, ErrorDetail, ErrorScope, Event, Outcome,
    QueryOf, Request, ToolRef, ToolRefError, UserInput,
};
pub use grant::{Aspect, Capability, Consent, Grant, GrantSpec};
pub use ids::{
    BranchId, CallId, ExtId, IdError, PromptId, ReqId, RuleId, RunId, Seq, SessionId, SessionRef,
    SurfaceId, TurnId,
};
pub use load::{Contribution, ContributionKind, LoadOutcome, LoadStage, SkipReason};
pub use message::{ContentBlock, Message, MessageRole};
pub use scope::{AgentScope, Layer, Role, Subject, SubjectError};
pub use surface::{
    Cell, Choice, DiffLine, DiffLineKind, Field, FieldKind, Hunk, StackDir, Status, Surface,
    SurfaceError, SurfaceKind, SurfacePatch, TaskItem, TextStyle, TreeNode,
};
pub use verdict::Verdict;
