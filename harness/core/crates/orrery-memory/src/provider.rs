//! Two doors, and the split is not cosmetic.
//!
//! [`MemoryProvider::recall`] contributes to `context.build`, alongside skills,
//! under a profile allowance. [`MemoryProvider::write`] and
//! [`MemoryProvider::forget`] take a [`LifecycleWitness`], so they can only be
//! reached from a lifecycle handler. An interceptor cannot produce one; see
//! [`crate::witness`].

use async_trait::async_trait;
use orrery_proto::TokenBudget;
use serde::{Deserialize, Serialize};

use crate::error::MemError;
use crate::scope::{MemScope, ScopeKind};
use crate::witness::LifecycleWitness;

/// One thing worth remembering.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MemEntry {
    /// How the provider names it, so the same entry can be recognised later and
    /// so a `forget` can select it.
    pub key: String,
    /// The text itself. This is what goes into the context, verbatim.
    pub text: String,
    /// How relevant the provider thought it was, when it says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
    /// When it was written, in unix milliseconds. Zero when the provider does
    /// not keep one.
    #[serde(default)]
    pub at_ms: i64,
}

impl MemEntry {
    /// A keyed entry.
    #[must_use]
    pub fn new(key: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            text: text.into(),
            score: None,
            at_ms: 0,
        }
    }

    /// Say how relevant it is.
    #[must_use]
    pub const fn with_score(mut self, score: f64) -> Self {
        self.score = Some(score);
        self
    }

    /// Say when it was written.
    #[must_use]
    pub const fn at(mut self, at_ms: i64) -> Self {
        self.at_ms = at_ms;
        self
    }

    /// The same entry, as the turn tree records it.
    #[must_use]
    pub fn to_recalled(&self) -> orrery_session::RecalledEntry {
        orrery_session::RecalledEntry {
            key: self.key.clone(),
            text: self.text.clone(),
            score: self.score,
        }
    }
}

/// What the kernel asks a provider for.
///
/// `scopes` is already filtered: it holds only the scopes this actor may read,
/// that are still alive, and that this provider said it keeps. A provider does
/// not decide visibility, and is never told about a scope it must not answer
/// from.
#[derive(Clone, Debug, PartialEq)]
pub struct RecallQuery {
    /// Where to look.
    pub scopes: Vec<MemScope>,
    /// What to look for. An empty string means "whatever you think is
    /// relevant", not "nothing".
    pub query: String,
    /// The ceiling. Advisory to the provider and **enforced by the kernel**:
    /// whatever comes back is clipped to it anyway.
    pub budget: TokenBudget,
}

/// Which entries a `forget` is about.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "kebab-case")]
pub enum MemSelector {
    /// Everything in the scope.
    All,
    /// One entry, by key.
    Key(String),
    /// Every entry whose text contains this, case-insensitively.
    Contains(String),
}

impl MemSelector {
    /// Whether this selector picks an entry.
    #[must_use]
    pub fn matches(&self, entry: &MemEntry) -> bool {
        match self {
            MemSelector::All => true,
            MemSelector::Key(k) => &entry.key == k,
            MemSelector::Contains(needle) => {
                entry.text.to_lowercase().contains(&needle.to_lowercase())
            }
        }
    }
}

/// A store, its content and its retrieval strategy. Not the kernel's business.
///
/// Zero or many can be active.
#[async_trait]
pub trait MemoryProvider: Send + Sync {
    /// What this provider calls itself. Goes in the `Recalled` row, so a replay
    /// says which store answered.
    fn id(&self) -> &str;

    /// Which scopes it keeps.
    ///
    /// **Declared, not discovered.** A provider asked for a scope outside this
    /// list returns [`MemError::UnsupportedScope`]; it never succeeds silently,
    /// because a write that appears to have worked and did not is worse than a
    /// refusal.
    fn scopes(&self) -> &[ScopeKind];

    /// How to divide an allowance when several providers are active. Equal
    /// shares by default; see open question 1 in `12-memory.md`.
    fn priority(&self) -> u32 {
        1
    }

    /// Contributes to `context.build`, alongside skills, under a profile
    /// allowance.
    ///
    /// # Errors
    ///
    /// [`MemError`] when the store cannot be read. Memory is never
    /// load-bearing: the kernel records the failure and builds the context
    /// without it.
    async fn recall(&self, query: RecallQuery) -> Result<Vec<MemEntry>, MemError>;

    /// Lifecycle handlers only. Enforced by the type of the witness.
    ///
    /// # Errors
    ///
    /// [`MemError::UnsupportedScope`] for a scope this provider does not keep,
    /// [`MemError::Backend`] when the store fails.
    async fn write(
        &self,
        witness: &LifecycleWitness,
        scope: MemScope,
        entry: MemEntry,
    ) -> Result<(), MemError>;

    /// Lifecycle handlers only. Returns how many entries went.
    ///
    /// # Errors
    ///
    /// As [`write`](MemoryProvider::write).
    async fn forget(
        &self,
        witness: &LifecycleWitness,
        scope: MemScope,
        selector: MemSelector,
    ) -> Result<u64, MemError>;
}
