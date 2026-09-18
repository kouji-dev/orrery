//! What the model is shown about a tool.

use serde::{Deserialize, Serialize};

/// One tool, as it appears in a model request.
///
/// Produced only by [`Registry::visible`](crate::Registry::visible), so the
/// list the model sees is a real subset of what exists rather than a promise in
/// a prompt.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolDescriptor {
    /// The form the model should emit: the short name where it is unambiguous
    /// within this scope, the full `ext.name` where it is not.
    pub name: String,
    /// What it does.
    pub description: String,
    /// JSON Schema for the input, validated at the boundary by `dispatch`.
    pub input_schema: serde_json::Value,
    /// Whether the tool's effect is all-or-nothing.
    pub atomic: bool,
}
