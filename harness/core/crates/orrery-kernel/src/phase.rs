//! Ten phases, each with the one payload an interceptor at it may rewrite.
//!
//! # A gate, or a span
//!
//! §4.1's naming rule: a **gate** (`.before`, `.after`, `.resolve`) may return a
//! verdict that changes the outcome; a **span** (`.start`, `.end`) only
//! brackets. A phase exists only where a verdict is possible — everything else
//! is an event on the stream, and ten phases is the whole list rather than the
//! first thirty somebody thought of.
//!
//! # The associated type is the point (translation #2)
//!
//! [`Phase`] is a trait with an associated `Payload`, not a string enum, so
//! [`Verdict<P>`](orrery_proto::Verdict) is generic over what the phase is
//! about. An interceptor registered for [`ToolBefore`] cannot hand back a
//! [`ModelRequest`](orrery_provider::ModelRequest), because those are different
//! `P`s and the compiler says so:
//!
//! ```compile_fail
//! # use orrery_kernel::{InterceptCtx, Interceptor, ToolBefore};
//! # use orrery_provider::ModelRequest;
//! # use orrery_proto::Verdict;
//! struct Sneaky;
//!
//! impl Interceptor<ToolBefore> for Sneaky {
//!     // `ToolBefore::Payload` is `ToolInput`. Returning a model request from
//!     // a tool phase does not compile, which is translation #2's whole value.
//!     fn run(
//!         &self,
//!         _ctx: &InterceptCtx<'_>,
//!         _payload: &<ToolBefore as orrery_kernel::Phase>::Payload,
//!     ) -> Verdict<ModelRequest> {
//!         Verdict::Continue
//!     }
//! }
//! ```
//!
//! The same shape written honestly compiles, so the failure above is a type
//! error and not a typo:
//!
//! ```
//! # use orrery_kernel::{InterceptCtx, Interceptor, Phase, ToolBefore, ToolInput};
//! # use orrery_proto::Verdict;
//! struct Honest;
//!
//! impl Interceptor<ToolBefore> for Honest {
//!     fn run(&self, _ctx: &InterceptCtx<'_>, _payload: &ToolInput) -> Verdict<ToolInput> {
//!         Verdict::Continue
//!     }
//! }
//! # assert_eq!(ToolBefore::NAME, "tool.before");
//! ```

use orrery_proto::Outcome;
use orrery_provider::ModelRequest;

use crate::context::{CompactPlan, ContextDraft};
use crate::turn::{PassResult, PendingCall, ResolvedManifest, ToolInput, TurnInput, TurnSummary};

/// How long a phase's decision lives.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum PhaseScope {
    /// Once per session.
    Session,
    /// Once per turn.
    Turn,
    /// Once per pass through the loop.
    Pass,
}

/// One point in the loop where an interceptor may have a say.
pub trait Phase: 'static {
    /// What an interceptor at this phase may rewrite.
    type Payload: Send + 'static;

    /// The dotted name, which is also the registration key and what the audit
    /// prints.
    const NAME: &'static str;

    /// How long a decision here lives.
    const SCOPE: PhaseScope;

    /// Whether a [`Deny`](orrery_proto::Verdict::Deny) means anything here.
    ///
    /// False for `context.build`: there is nothing to refuse, the turn is
    /// already happening, and an interceptor that returned `Deny` there would
    /// be asking for something the kernel has no way to honour. Registration
    /// refuses it rather than discovering it mid-turn.
    const ALLOWS_DENY: bool = true;
}

/// Declare a marker type and its `Phase` impl in one line each.
macro_rules! phases {
    ($( $(#[$m:meta])* $name:ident : $payload:ty = $wire:literal, $scope:ident $(, deny = $deny:literal)? ;)*) => {
        $(
            $(#[$m])*
            #[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
            pub struct $name;

            impl Phase for $name {
                type Payload = $payload;
                const NAME: &'static str = $wire;
                const SCOPE: PhaseScope = PhaseScope::$scope;
                $( const ALLOWS_DENY: bool = $deny; )?
            }
        )*

        /// Every phase's wire name, in the order the loop reaches them.
        ///
        /// Open question 4, decided: registration is **type-driven** —
        /// [`InterceptorSet::register`](crate::InterceptorSet::register) takes
        /// `P: Phase` and the key is `P::NAME`, so an unknown phase cannot be
        /// named at all and there is nothing to validate at `session.start`.
        /// This list exists for the ledger and for `orrery explain`, not for
        /// lookup.
        pub const ALL_PHASES: &[&str] = &[ $( $wire ),* ];
    };
}

phases! {
    /// A session opened. An interceptor here gets a verdict on the resolved
    /// manifest; a lifecycle handler at the same point does the I/O of opening
    /// a store. Both, on purpose.
    SessionStart: ResolvedManifest = "session.start", Session;
    /// A turn was submitted.
    TurnStart: TurnInput = "turn.start", Turn;
    /// A turn is about to settle.
    TurnEnd: TurnSummary = "turn.end", Turn;
    /// The context is assembled and not yet sent. **Rewrite only.**
    ContextBuild: ContextDraft = "context.build", Pass, deny = false;
    /// The context does not fit and is about to be shrunk. The one phase
    /// allowed I/O (translation #8) — through the compactor, not through here.
    ContextCompact: CompactPlan = "context.compact", Pass;
    /// The request is built and not yet sent. Auth is checked here.
    ProviderBefore: ModelRequest = "provider.before", Pass;
    /// The stream is finished and not yet acted on.
    ProviderAfter: PassResult = "provider.after", Pass;
    /// A name the model emitted is about to become a tool.
    ToolResolve: PendingCall = "tool.resolve", Pass;
    /// A resolved call is about to be checked and dispatched.
    ToolBefore: ToolInput = "tool.before", Pass;
    /// A call has settled and is about to be appended.
    ToolAfter: Outcome = "tool.after", Pass;
}
