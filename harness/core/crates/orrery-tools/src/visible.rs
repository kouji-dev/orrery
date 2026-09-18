//! The tool list the model is actually given.

use orrery_proto::{AgentScope, ToolRef};

use crate::registry::{Entry, ExtState, Registry};
use orrery_proto::ToolDescriptor;

/// Whether a scope's glob list names this tool.
///
/// The scope's `tools` are matched against both the full `ext.name` and the
/// short name, so `git.*` and `status` both work and neither is special-cased
/// elsewhere. An **empty** list is empty: a tool that is not named is not
/// offered to the model at all.
#[must_use]
pub(crate) fn in_scope(scope: &AgentScope, r#ref: &ToolRef) -> bool {
    let full = r#ref.to_string();
    scope.tools.iter().any(|pattern| {
        globset::Glob::new(pattern)
            .map(|g| {
                let m = g.compile_matcher();
                m.is_match(&full) || m.is_match(&r#ref.name)
            })
            .unwrap_or(false)
    })
}

impl Registry {
    /// What this scope's model may be told about.
    ///
    /// This is what makes a sub-agent's tool list real rather than a sentence in
    /// a prompt. It intersects four things:
    ///
    /// 1. the tools that exist,
    /// 2. the scope's `tools` glob list,
    /// 3. what the policy engine would not categorically deny for this subject,
    /// 4. extensions currently [`ExtState::Live`].
    ///
    /// # The order is stable, and that is load-bearing
    ///
    /// Layers in precedence order — project first, managed last — and
    /// registration order within a layer. The backing store is an
    /// [`IndexMap`](indexmap::IndexMap) and the sort is stable, so the same
    /// registrations always produce the same bytes. An unstable prompt prefix
    /// silently kills provider caching, which is the kind of regression nobody
    /// notices until the bill arrives; `tests/visible.rs` asserts it twenty
    /// times over.
    #[must_use]
    pub fn visible(&self, scope: &AgentScope) -> Vec<ToolDescriptor> {
        let subject = orrery_proto::Subject::SubAgent(scope.agent.clone());

        let mut offered: Vec<&Entry> = self
            .entries
            .values()
            .filter(|e| e.state == ExtState::Live)
            .filter(|e| in_scope(scope, &e.r#ref))
            .filter(|e| !self.policy.categorically_denies(&subject, &e.r#ref))
            .collect();
        offered.sort_by_key(|e| (std::cmp::Reverse(e.layer), e.order));

        offered
            .iter()
            .map(|e| ToolDescriptor {
                name: self.offered_name(e, &offered),
                description: e.spec.description.clone(),
                input_schema: e.spec.input_schema.clone(),
                atomic: e.spec.atomic,
            })
            .collect()
    }

    /// The short name where it is unambiguous **within this list**, the full
    /// `ext.name` where it is not.
    ///
    /// Ambiguity is judged against what this scope can see, not against the
    /// whole registry: a sub-agent that was given only `ripgrep.*` has no
    /// reason to be shown a qualified name for a clash it cannot reach.
    #[allow(clippy::unused_self)]
    fn offered_name(&self, entry: &Entry, offered: &[&Entry]) -> String {
        let claimants = offered
            .iter()
            .filter(|o| o.spec.name == entry.spec.name)
            .count();
        if claimants == 1 {
            entry.spec.name.clone()
        } else {
            entry.r#ref.to_string()
        }
    }
}
