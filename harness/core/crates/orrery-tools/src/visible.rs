//! The tool list the model is actually given.

use orrery_proto::{AgentScope, ToolRef};

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
