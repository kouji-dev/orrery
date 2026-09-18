//! `context.build`: the assembly order is fixed, and the reason is cost.
//!
//! ```text
//! system, in this order and no other:
//!   1. base system prompt
//!   2. agent prompt (role binding)
//!   3. tool descriptors      ← registry.visible(scope), index-stable
//!   4. skills
//!   ────────────────── cache_breakpoint
//! then the volatile suffix:
//!   recalled memory
//!   history
//!   this input
//! ```
//!
//! Stable prefix first. A memory provider injecting near the top would
//! invalidate the provider's prompt cache on **every pass**, which is the kind
//! of regression nobody notices until the bill arrives. The breakpoint handed to
//! the provider is computed here and nowhere else.
//!
//! Nothing reaches the model without passing through here: the visible tool set,
//! the active skills and anything recalled are settled at this phase rather than
//! at the provider.

use async_trait::async_trait;
use orrery_proto::{
    BranchId, Message, MessageRole, Seq, TokenBudget, ToolDescriptor, Usage, UserInput,
};
use orrery_session::RecalledEntry;
use tokio_util::sync::CancellationToken;

/// One block of the system prompt, named so that a rewrite can find the one it
/// meant.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section {
    /// `base`, `agent`, `tools`, `skills`, or whatever an interceptor added.
    pub name: String,
    /// The text.
    pub text: String,
}

impl Section {
    /// A named block.
    #[must_use]
    pub fn new(name: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            text: text.into(),
        }
    }
}

/// Everything that is about to be sent, before it is a request.
#[derive(Clone, Debug, PartialEq)]
pub struct ContextDraft {
    /// The stable prefix, in assembly order.
    pub system: Vec<Section>,
    /// How many leading `system` sections are the cached prefix. Everything
    /// above this index is stable from pass to pass.
    pub cache_breakpoint: usize,
    /// The volatile suffix begins here.
    pub recalled: Vec<Message>,
    /// The conversation so far.
    pub history: Vec<Message>,
    /// What was submitted this turn.
    pub input: Message,
    /// The tools the model may call, as the registry offered them.
    pub tools: Vec<ToolDescriptor>,
}

impl ContextDraft {
    /// The system prompt as one string.
    #[must_use]
    pub fn system_text(&self) -> String {
        self.system
            .iter()
            .map(|s| s.text.as_str())
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    /// The conversation, oldest first: recalled, then history, then the input.
    #[must_use]
    pub fn messages(&self) -> Vec<Message> {
        let mut out = Vec::with_capacity(self.recalled.len() + self.history.len() + 1);
        out.extend(self.recalled.iter().cloned());
        out.extend(self.history.iter().cloned());
        out.push(self.input.clone());
        out
    }

    /// Where the provider's prompt cache should key, as an index into
    /// [`messages`](Self::messages).
    ///
    /// Everything before this pass's own input has been sent before and will be
    /// sent again unchanged; the input has not. A provider without a cache
    /// ignores it, and that degradation is a no-op rather than an error.
    #[must_use]
    pub fn message_breakpoint(&self) -> usize {
        self.recalled.len() + self.history.len()
    }

    /// Which section holds the tool descriptors, if any does.
    #[must_use]
    pub fn section_index(&self, name: &str) -> Option<usize> {
        self.system.iter().position(|s| s.name == name)
    }
}

/// What a compaction is being asked to do.
#[derive(Clone, Debug, PartialEq)]
pub struct CompactPlan {
    /// The branch whose history is too long.
    pub branch: BranchId,
    /// The last sequence number the summary will stand in for.
    pub upto: Seq,
    /// What is being summarised.
    pub messages: Vec<Message>,
    /// What it has to fit into afterwards.
    pub target: TokenBudget,
    /// Which attempt this is, one-based.
    pub attempt: u32,
}

/// What a compaction produced.
#[derive(Clone, Debug, PartialEq)]
pub struct Compacted {
    /// The summary that stands in for what it covered.
    pub summary: String,
    /// What producing it cost. Charged to the turn, like anything else.
    pub usage: Usage,
}

/// A compaction that could not be done.
#[non_exhaustive]
#[derive(Debug, Clone, thiserror::Error)]
pub enum CompactError {
    /// Nothing is configured to compact with.
    #[error("no compactor is configured")]
    Unavailable,
    /// The compactor tried and failed.
    #[error("compaction failed: {message}")]
    Failed {
        /// What went wrong.
        message: String,
    },
    /// It was cancelled, like anything else charged to the turn.
    #[error("compaction was cancelled")]
    Cancelled,
}

/// `context.compact` — **the one phase allowed I/O** (translation #8).
///
/// Named as an exception rather than left implicit: it is async, it takes the
/// turn's cancellation token, and what it costs is charged to the turn that
/// triggered it. Everything else in the phase pipeline is sync and cannot reach
/// anything.
#[async_trait]
pub trait Compactor: Send + Sync {
    /// Shrink what a plan names.
    ///
    /// # Errors
    ///
    /// [`CompactError`] when it cannot. The kernel's answer is to stop with
    /// [`BudgetKind::Tokens`](orrery_proto::BudgetKind), not to fail the turn.
    async fn compact(
        &self,
        plan: &CompactPlan,
        cancel: &CancellationToken,
    ) -> Result<Compacted, CompactError>;
}

/// The compactor of a harness that has not been given one.
///
/// Refuses, so a context that does not fit stops the turn with a budget reason
/// rather than being silently truncated into a model that will then answer
/// about half a conversation.
#[derive(Copy, Clone, Debug, Default)]
pub struct NoCompactor;

#[async_trait]
impl Compactor for NoCompactor {
    async fn compact(
        &self,
        _plan: &CompactPlan,
        _cancel: &CancellationToken,
    ) -> Result<Compacted, CompactError> {
        Err(CompactError::Unavailable)
    }
}

/// Where recalled memory comes from.
///
/// A typed hole for plan 12. The kernel calls it at `context.build` and records
/// what came back as a [`TurnKind::Recalled`](orrery_session::TurnKind) row, so
/// the tree shows what the model actually saw rather than a query to re-run.
#[async_trait]
pub trait MemoryRecall: Send + Sync {
    /// What this provider calls itself, for the `Recalled` row.
    fn name(&self) -> &str;

    /// What is worth remembering about this input, within a ceiling.
    async fn recall(&self, input: &UserInput, budget: TokenBudget) -> Vec<RecalledEntry>;

    /// The same answer, split by the store that gave it.
    ///
    /// The kernel writes one [`TurnKind::Recalled`](orrery_session::TurnKind)
    /// row per pair, so a replay says **which** store said what — which is why
    /// this and not `recall` is what the turn calls. The default is the single
    /// store [`name`](MemoryRecall::name) describes; an implementation over
    /// several providers overrides it and keeps them apart.
    async fn recall_rows(
        &self,
        input: &UserInput,
        budget: TokenBudget,
    ) -> Vec<(String, Vec<RecalledEntry>)> {
        let entries = self.recall(input, budget).await;
        if entries.is_empty() {
            Vec::new()
        } else {
            vec![(self.name().to_owned(), entries)]
        }
    }
}

/// The memory of a harness that has not been given one.
#[derive(Copy, Clone, Debug, Default)]
pub struct NoMemory;

#[async_trait]
impl MemoryRecall for NoMemory {
    fn name(&self) -> &str {
        "none"
    }

    async fn recall(&self, _input: &UserInput, _budget: TokenBudget) -> Vec<RecalledEntry> {
        Vec::new()
    }
}

/// Render recalled entries as the one message the volatile suffix starts with.
///
/// One message rather than one each, so that eliding memory is one decision.
#[must_use]
pub fn recalled_message(provider: &str, entries: &[RecalledEntry]) -> Option<Message> {
    if entries.is_empty() {
        return None;
    }
    Some(Message::text(
        MessageRole::User,
        entries
            .iter()
            .map(|e| format!("[{provider}:{}] {}", e.key, e.text))
            .collect::<Vec<_>>()
            .join("\n"),
    ))
}

/// The user's input as a message.
#[must_use]
pub fn input_message(input: &UserInput) -> Message {
    let mut content = vec![orrery_proto::ContentBlock::Text {
        text: input.text.clone(),
    }];
    content.extend(input.attachments.iter().cloned());
    Message {
        role: MessageRole::User,
        content,
    }
}

/// The tool descriptors, as the stable section of the system prompt.
///
/// Rendered here rather than at the provider so that two providers are shown the
/// same list in the same order, and so that `context::tool_order_is_stable` can
/// compare bytes.
#[must_use]
pub fn tools_section(tools: &[ToolDescriptor]) -> String {
    if tools.is_empty() {
        return String::new();
    }
    let mut out = String::from("# Tools\n");
    for tool in tools {
        out.push_str(&format!("\n## {}\n{}\n", tool.name, tool.description));
        out.push_str(&format!(
            "input: {}\n",
            serde_json::to_string(&tool.input_schema).unwrap_or_else(|_| "{}".to_owned())
        ));
        if tool.atomic {
            out.push_str("atomic: all-or-nothing\n");
        }
    }
    out
}
