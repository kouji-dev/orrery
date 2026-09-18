//! `mem.read` and `mem.write` are **per-scope capabilities**.
//!
//! Scoped by [`MemScope`], not by path, so an organisation can forbid `global`
//! writes outright while leaving `session` alone:
//!
//! ```toml
//! [permissions."agent:critic"]
//! deny = ["mem.write(global)", "mem.write(workspace)"]
//! ```
//!
//! # Why this is a trait and not `orrery-policy`
//!
//! `orrery-memory` is `publish = true`, because `orrery-ext-memory-file`
//! depends on it and `deps-check` rule 2 says an extension reaches core only
//! through published crates. `orrery-policy` is `publish = false`. So the check
//! is a trait here and the real engine is wired in by the host — and by this
//! crate's own `tests/perm.rs`, which drives the rules above through the actual
//! `PolicyEngine` rather than a stub.

use orrery_proto::{AgentScope, Aspect, Subject};

use crate::scope::MemScope;

/// Whether a subject may read or write a memory scope.
pub trait MemPermissions: Send + Sync {
    /// Decide.
    ///
    /// `aspect` is [`Aspect::MemRead`] or [`Aspect::MemWrite`]; the target a
    /// rule matches is the scope's **kind** — `global`, `session`, `branch` —
    /// not the handle, because a rule cannot name a uuid that did not exist
    /// when it was written.
    ///
    /// # Errors
    ///
    /// The reason, in words a person can act on, when the answer is no.
    fn check(
        &self,
        subject: &Subject,
        agent: &AgentScope,
        aspect: Aspect,
        scope: &MemScope,
    ) -> Result<(), String>;
}

/// The permission check of a kernel that has not been given one.
///
/// Deliberately named for what it does. A harness that wires no rules in has no
/// rules; it does not have a quietly closed default that would make the
/// visibility tests pass for the wrong reason.
#[derive(Copy, Clone, Debug, Default)]
pub struct AllowAll;

impl MemPermissions for AllowAll {
    fn check(
        &self,
        _subject: &Subject,
        _agent: &AgentScope,
        _aspect: Aspect,
        _scope: &MemScope,
    ) -> Result<(), String> {
        Ok(())
    }
}

/// The rule-grammar form of a memory request: `mem.write(global)`.
#[must_use]
pub fn request_text(aspect: Aspect, scope: &MemScope) -> String {
    let word = match aspect {
        Aspect::MemRead => "mem.read",
        Aspect::MemWrite => "mem.write",
        _ => "mem",
    };
    format!("{word}({})", scope.kind().name())
}
