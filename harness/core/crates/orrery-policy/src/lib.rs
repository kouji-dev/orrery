//! Rules, the selector matcher, check/consent/explain, and the capability-token minter. Fully sync.
//!
//! # The rule grammar, at a glance
//!
//! `Tool` or `Tool(specifier)`, three lists — `deny`, `ask`, `allow` —
//! evaluated in that order, **first match wins, specificity deliberately
//! irrelevant**, and an allow can never carve an exception out of a deny.
//!
//! ```toml
//! [permissions]                       # the agent itself
//! allow = ["tool(git.*)", "read(./**)"]
//! ask   = ["write(./**)"]
//! deny  = ["net(domain: *)"]
//!
//! [permissions."ext:buildgraph"]      # one extension
//! allow = ["spawn(bazel *)", "read(./**)"]
//! deny  = ["creds(*)"]
//! ```
//!
//! A subject's effective set is its own rules **intersected with its parent's**:
//! a rule file narrows a sub-agent, it never widens one. Across config layers,
//! **deny is a union and the managed layer's deny cannot be relaxed**; allow and
//! ask resolve by layer precedence.
//!
//! # Rules decide whether to ask; the broker and the token decide what can be touched
//!
//! Claude Code's docs are candid that a `Bash(curl *)` deny stops
//! `curl https://x` but not `/usr/bin/curl https://x` or
//! `sh -c 'curl https://x'`. **Patterns describe intent; they do not enforce.**
//! A `spawn` grant is enforced where the process is created — in `orrery-broker`,
//! against a [`CapabilityToken`] — and not by the string that matched. Anything
//! that reads a rule as a guarantee has misread it.
//!
//! # Why `check` is not async
//!
//! [`PolicyEngine::check`] is a sync, pure `fn`. Not "fast enough": being
//! non-async makes "policy performs no I/O" a **compile-time property**.
//!
//! Implementation plan: `harness/docs/plans/07-policy-broker-audit.md`

#![deny(missing_docs)]
#![forbid(unsafe_code)]

pub mod call;
pub mod engine;
pub mod error;
pub mod explain;
pub mod handler;
pub mod r#match;
pub mod parse;
pub mod rule;
pub mod token;

pub use call::PendingCall;
pub use engine::{
    no_rule, ConsentAnswer, ConsentMode, Decision, PolicyBuilder, PolicyEngine, ResolvedRules,
    Verdict,
};
pub use error::{HandlerError, ParseError, PolicyError, TokenError, Warning};
pub use explain::{Explanation, RuleMatch};
pub use handler::{review_narrowing, InertMinter, PermissionHandler};
pub use rule::{Rule, RuleList, Selector, SelectorKind, Source};
pub use token::{CapabilityToken, ResolvedScope, TokenLedger, TokenMinter, DEFAULT_TTL};
