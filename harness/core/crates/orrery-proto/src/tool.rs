//! What the model is shown about a tool.

use serde::{Deserialize, Serialize};

/// One tool, as it appears in a model request.
///
/// A wire type, and therefore this crate's: the registry produces it from
/// [`AgentScope`](crate::AgentScope), a provider serialises it into the prompt,
/// and the model is shown it. Both sides naming the same struct is what stops
/// the two halves drifting — there used to be a copy in `orrery-provider` and a
/// second in `orrery-tools`, with no conversion between them.
///
/// It is produced only by `Registry::visible`, so the list the model sees is a
/// real subset of what exists rather than a promise in a prompt.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ToolDescriptor {
    /// The form the model should emit: the short name where it is unambiguous
    /// within this scope, the full `ext.name` where it is not.
    pub name: String,
    /// What it does, in the model's context window.
    pub description: String,
    /// JSON Schema for the input. Validated at the dispatch boundary, never by
    /// the provider.
    pub input_schema: serde_json::Value,
    /// Whether the tool's effect is all-or-nothing.
    pub atomic: bool,
}
