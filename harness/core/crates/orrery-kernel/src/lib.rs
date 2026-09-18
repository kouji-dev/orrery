//! The phase pipeline, the sync interceptor chain, context building and run_turn.
//!
//! One loop: build context, call the provider, receive tool calls, dispatch
//! them, append results, repeat until stop.
//!
//! # The four things this crate makes true
//!
//! - **Interceptors cannot do I/O** (translation #1). [`Interceptor::run`] is a
//!   sync `fn` and [`InterceptCtx`] holds no handle, so there is nothing to call
//!   and nothing to await.
//! - **A verdict is typed to its phase** (translation #2). [`Phase`] has an
//!   associated `Payload`, so an interceptor at [`ToolBefore`] cannot return a
//!   model request. The `compile_fail` doctest on [`phase`] is the proof.
//! - **`context.compact` is the one exception** (translation #8). It is async,
//!   it is cancellable, and what it costs is charged to the turn that triggered
//!   it.
//! - **Stopping is a value.** A ceiling, a cancellation and a login prompt are
//!   [`TurnOutcome`] variants, never [`KernelError`] ones.
//!
//! Implementation plan: `harness/docs/plans/05-kernel-loop.md`

#![deny(missing_docs)]
#![forbid(unsafe_code)]

pub mod budget;
pub mod context;
pub mod error;
pub mod intercept;
pub mod lifecycle;
pub mod phase;
pub mod retry;
pub mod turn;

pub use budget::{PriceTable, TurnBudget};
pub use context::{
    CompactError, CompactPlan, Compacted, Compactor, ContextDraft, MemoryRecall, NoCompactor,
    NoMemory, Section,
};
pub use error::KernelError;
pub use intercept::{
    ChainOutcome, InterceptCtx, Interceptor, InterceptorSet, MatchCtx, RegisterError,
};
pub use lifecycle::{LifecycleCtx, LifecycleError, LifecycleHandler, LifecyclePoint, LifecycleSet};
pub use phase::{
    ALL_PHASES, ContextBuild, ContextCompact, Phase, PhaseScope, ProviderAfter, ProviderBefore,
    SessionStart, ToolAfter, ToolBefore, ToolResolve, TurnEnd, TurnStart,
};
pub use retry::RetryPolicy;
pub use turn::{
    CallRevoker, Kernel, KernelConfig, NoRevoker, PassId, PassResult, PendingCall,
    ResolvedManifest, ToolInput, TurnCancel, TurnInput, TurnOutcome, TurnSummary,
};
