//! The trait itself.

use std::sync::Arc;

use futures_core::stream::BoxStream;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::auth::ProviderAuth;
use crate::counter::TokenCounter;
use crate::error::ProviderError;
use crate::event::ModelEvent;
use crate::request::ModelRequest;

/// What a provider can do.
///
/// Not decoration. A provider without `tools` is never sent tool descriptors;
/// a `max_context` below the assembled context triggers compaction instead of a
/// rejected request. The context assembler reads this before it builds
/// anything.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capabilities {
    /// Accepts tool descriptors and emits tool calls.
    pub tools: bool,
    /// Accepts image content blocks.
    pub images: bool,
    /// Has a prompt cache that
    /// [`ModelRequest::cache_breakpoint`](crate::ModelRequest::cache_breakpoint)
    /// can key on.
    pub cache: bool,
    /// The context window, in tokens.
    pub max_context: u64,
    /// The largest output it will produce, in tokens.
    pub max_output: u64,
}

/// Model I/O, and nothing else.
///
/// Object-safe without `async_trait`: `stream` is a plain fn returning a
/// `'static` boxed stream, which is what lets the kernel hold
/// `Arc<dyn Provider>` in a registry.
///
/// **Implementations never sleep and never retry.** They classify; the kernel
/// spends. Otherwise budgets stop meaning anything.
pub trait Provider: Send + Sync + 'static {
    /// The id this provider is selected by, e.g. `anthropic`.
    fn id(&self) -> &str;

    /// What it can do.
    fn capabilities(&self) -> &Capabilities;

    /// Start a completion.
    ///
    /// Dropping the returned stream — or cancelling `cancel` — must abort the
    /// underlying request. That is what makes cancellation stop costing money
    /// rather than stop showing it.
    fn stream(
        &self,
        req: ModelRequest,
        cancel: CancellationToken,
    ) -> BoxStream<'static, Result<ModelEvent, ProviderError>>;

    /// How to count tokens for this provider's models.
    fn counter(&self) -> Arc<dyn TokenCounter>;

    /// How to sign in.
    fn auth(&self) -> Arc<dyn ProviderAuth>;
}
