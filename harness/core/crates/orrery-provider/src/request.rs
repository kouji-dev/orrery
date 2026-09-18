//! What the kernel hands a provider.

use std::sync::Arc;

use orrery_proto::Message;
use serde::{Deserialize, Serialize};

/// A tool as the *model* sees it.
///
/// The same shape plan 04's registry produces from `visible(scope)`. It is
/// declared here rather than in `orrery-tools` so that a provider crate — which
/// has no business knowing about dispatch, budgets or policy — can be built
/// against the provider crate alone. `orrery-tools` converts into it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolDescriptor {
    /// The form the model should emit, `<ext>.<name>`.
    pub name: String,
    /// What it does, in the model's context window.
    pub description: String,
    /// JSON Schema. Validated at the dispatch boundary, not here.
    pub input_schema: serde_json::Value,
}

/// One model call.
///
/// Owns `Arc<[Message]>` rather than borrowing so that the stream it produces
/// is `'static` and can outlive the caller's frame — which is what lets
/// [`Provider::stream`](crate::Provider::stream) be a plain non-async fn.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelRequest {
    /// The model id, in the provider's own vocabulary.
    pub model: String,
    /// The system prompt, when there is one.
    pub system: Option<Arc<str>>,
    /// The conversation.
    pub messages: Arc<[Message]>,
    /// The tools the model may call. **Empty** when
    /// [`Capabilities::tools`](crate::Capabilities::tools) is false — a
    /// provider never has to decide whether to drop them.
    pub tools: Arc<[ToolDescriptor]>,
    /// The output ceiling.
    pub max_output_tokens: u64,
    /// Sampling temperature, when the caller wants one.
    pub temperature: Option<f32>,
    /// Stop sequences.
    pub stop: Vec<String>,
    /// Where the stable prefix ends: an index into `messages`, computed by the
    /// context assembler rather than guessed per provider. Providers with
    /// [`Capabilities::cache`](crate::Capabilities::cache) key on it; providers
    /// without it ignore it, and that degradation is a no-op, never an error.
    pub cache_breakpoint: Option<usize>,
}

impl ModelRequest {
    /// A request with no tools, no system prompt and no cache breakpoint.
    #[must_use]
    pub fn new(model: impl Into<String>, messages: Arc<[Message]>, max_output_tokens: u64) -> Self {
        Self {
            model: model.into(),
            system: None,
            messages,
            tools: Arc::from([] as [ToolDescriptor; 0]),
            max_output_tokens,
            temperature: None,
            stop: Vec::new(),
            cache_breakpoint: None,
        }
    }
}
