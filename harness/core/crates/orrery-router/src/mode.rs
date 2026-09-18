//! Modes are permissions, and a mode switch is an ordinary capability request.
//!
//! `mode(plan)` and `mode(execute)` are grantable like anything else (§4.8), so
//! "this profile may never enter execute" is one line in a rule file:
//!
//! ```toml
//! [permissions]
//! allow = ["mode(plan)", "mode(review)"]
//! deny  = ["mode(execute)"]
//! ```
//!
//! **Nothing in this module decides anything.** [`switch`] builds the request;
//! `orrery-policy` answers it. A router that could refuse a mode switch on its
//! own would be a second, undocumented permission system — and the one thing
//! §4.8 is for is that there is only one.

use orrery_policy::PendingCall;
use orrery_proto::Aspect;

use crate::signals::Mode;

/// The capability request that entering `mode` is.
///
/// Hand it to [`PolicyEngine::check`](orrery_policy::PolicyEngine::check). The
/// answer is the engine's, not this crate's.
#[must_use]
pub fn switch(mode: Mode) -> PendingCall {
    PendingCall::new(Aspect::Mode, mode.as_str())
}

/// One tool, as far as mode filtering is concerned: its name and what it cannot
/// work without.
///
/// The same `requires` list an extension's `ToolDef` declares — named here as a
/// plain pair so that this crate, which sits below the host, does not have to
/// depend on it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolAspects<'a> {
    /// The fully-qualified tool name.
    pub name: &'a str,
    /// What it needs in order to do anything.
    pub requires: &'a [Aspect],
}

impl<'a> ToolAspects<'a> {
    /// A tool and its requirements.
    #[must_use]
    pub const fn new(name: &'a str, requires: &'a [Aspect]) -> Self {
        Self { name, requires }
    }
}

/// The tools a mode leaves visible.
///
/// A tool that is not returned here is **not offered to the model at all** —
/// the same meaning [`AgentScope::tools`](orrery_proto::AgentScope::tools)
/// carries. In `plan` and `review`, a tool that needs `write`, `mem.write` or
/// `spawn` is gone; it is not offered and then refused, because a model that is
/// shown a tool it cannot use will spend a pass finding that out.
#[must_use]
pub fn visible<'a>(tools: &'a [ToolAspects<'a>], mode: Mode) -> Vec<&'a str> {
    tools
        .iter()
        .filter(|t| t.requires.iter().all(|a| mode.permits(*a)))
        .map(|t| t.name)
        .collect()
}
