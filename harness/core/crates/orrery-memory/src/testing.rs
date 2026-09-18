//! An in-test provider, so the conformance suite is exercised before the file
//! provider exists.
//!
//! In `src/` rather than in `tests/`, for the same reason
//! [`crate::conformance`] is: a `tests/` binary cannot be linked by another
//! crate, and `orrery-ext-memory-file` runs exactly this suite.
//!
//! [`InMemoryProvider`] is deliberately **permissive**: it enforces nothing but
//! the scopes it declared. Every visibility test in this crate asserts against
//! a store that would happily have done the wrong thing, which is the only
//! assumption worth making about somebody else's code.

use async_trait::async_trait;
use parking_lot::Mutex;

use crate::error::MemError;
use crate::provider::{MemEntry, MemSelector, MemoryProvider, RecallQuery};
use crate::scope::{MemScope, ScopeKind};
use crate::witness::LifecycleWitness;

/// A store in a `Vec`. Substring match, newest first.
#[derive(Debug)]
pub struct InMemoryProvider {
    id: String,
    scopes: Vec<ScopeKind>,
    priority: u32,
    rows: Mutex<Vec<(MemScope, MemEntry)>>,
}

impl InMemoryProvider {
    /// A provider that keeps exactly the scopes named.
    #[must_use]
    pub fn new(id: impl Into<String>, scopes: &[ScopeKind]) -> Self {
        Self {
            id: id.into(),
            scopes: scopes.to_vec(),
            priority: 1,
            rows: Mutex::new(Vec::new()),
        }
    }

    /// A provider that keeps every scope there is.
    #[must_use]
    pub fn anything(id: impl Into<String>) -> Self {
        Self::new(id, &ScopeKind::ALL)
    }

    /// Declare a priority, for the multi-provider share.
    #[must_use]
    pub const fn with_priority(mut self, priority: u32) -> Self {
        self.priority = priority;
        self
    }

    /// How many entries it holds, across every scope.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rows.lock().len()
    }

    /// Whether it holds nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// How many entries it holds in one scope.
    #[must_use]
    pub fn len_in(&self, scope: &MemScope) -> usize {
        self.rows.lock().iter().filter(|(s, _)| s == scope).count()
    }

    fn supports(&self, scope: &MemScope) -> Result<(), MemError> {
        if self.scopes.contains(&scope.kind()) {
            Ok(())
        } else {
            Err(MemError::UnsupportedScope {
                provider: self.id.clone(),
                scope: scope.kind().name().to_owned(),
            })
        }
    }
}

#[async_trait]
impl MemoryProvider for InMemoryProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn scopes(&self) -> &[ScopeKind] {
        &self.scopes
    }

    fn priority(&self) -> u32 {
        self.priority
    }

    async fn recall(&self, query: RecallQuery) -> Result<Vec<MemEntry>, MemError> {
        let needle = query.query.to_lowercase();
        let rows = self.rows.lock();
        Ok(rows
            .iter()
            .rev()
            .filter(|(scope, _)| query.scopes.contains(scope))
            .filter(|(_, entry)| {
                needle.is_empty()
                    || entry.text.to_lowercase().contains(&needle)
                    || entry.key.to_lowercase().contains(&needle)
            })
            .map(|(_, entry)| entry.clone())
            .collect())
    }

    async fn write(
        &self,
        _witness: &LifecycleWitness,
        scope: MemScope,
        entry: MemEntry,
    ) -> Result<(), MemError> {
        self.supports(&scope)?;
        self.rows.lock().push((scope, entry));
        Ok(())
    }

    async fn forget(
        &self,
        _witness: &LifecycleWitness,
        scope: MemScope,
        selector: MemSelector,
    ) -> Result<u64, MemError> {
        self.supports(&scope)?;
        let mut rows = self.rows.lock();
        let before = rows.len();
        rows.retain(|(s, e)| !(s == &scope && selector.matches(e)));
        Ok((before - rows.len()) as u64)
    }
}
