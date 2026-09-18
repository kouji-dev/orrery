//! The reference memory provider: file-backed, global and session scopes only. Not in any default feature set.
//!
//! # What it is, and what it is not
//!
//! JSONL under the state directory, `global` and `session` scopes, and
//! **substring plus recency** retrieval. That is the whole strategy. It exists
//! for three reasons: the conformance suite needs a real implementation to be
//! meaningful, "memory is absent" is a bad first-run default, and shipping one
//! makes the scope rules concrete for anyone writing a better one.
//!
//! It is **off by default**, and that is not an accident either: it is not good,
//! and a mediocre default memory is worse than none for evals — which is why
//! `EvalRun.memory` defaults to `"off"` (plan 16). An embedder turns it on
//! deliberately, or installs something better.
//!
//! # What it does not decide
//!
//! Visibility, lifetime and the token clamp are the kernel's
//! ([`orrery_memory`]). This crate is handed a scope list that has already been
//! filtered and a write that has already been admitted; it declares the two
//! scopes it keeps and refuses the rest with
//! [`MemError::UnsupportedScope`](orrery_memory::MemError::UnsupportedScope),
//! **never** with a silent success.
//!
//! Implementation plan: `harness/docs/plans/12-memory.md`

#![deny(missing_docs)]
#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use orrery_memory::{
    LifecycleWitness, MemEntry, MemError, MemScope, MemSelector, MemoryProvider, RecallQuery,
    ScopeKind,
};
use tokio::sync::Mutex;

/// What this provider calls itself, in a `Recalled` row and in the ledger.
pub const ID: &str = "memory-file";

/// The two scopes it keeps. Declared, not discovered.
const SCOPES: [ScopeKind; 2] = [ScopeKind::Global, ScopeKind::Session];

/// A memory store that is a directory of JSONL files.
///
/// One file per scope, appended to and rewritten whole. A `Mutex` serialises
/// every access, which is the right shape for a reference implementation and
/// the wrong shape for a large one: read-modify-write of a whole file is how a
/// concurrent `forget` would otherwise lose an entry, and a real provider would
/// use a database instead of paying for the lock.
#[derive(Debug)]
pub struct FileMemory {
    root: PathBuf,
    lock: Mutex<()>,
}

impl FileMemory {
    /// A store under a state directory. Files live in `<state>/memory/`,
    /// matching the manifest's `read`/`write` requirement of `$STATE/memory/**`.
    ///
    /// Nothing is created until something is written.
    #[must_use]
    pub fn new(state_dir: impl AsRef<Path>) -> Self {
        Self {
            root: state_dir.as_ref().join("memory"),
            lock: Mutex::new(()),
        }
    }

    /// The directory the files live in.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Where a scope's entries are kept.
    ///
    /// `global.jsonl` and `session/<id>.jsonl`: the id is a uuid, so it is a
    /// safe file name on every platform and there is nothing to sanitise.
    fn path_of(&self, scope: &MemScope) -> Option<PathBuf> {
        match scope {
            MemScope::Global => Some(self.root.join("global.jsonl")),
            MemScope::Session(id) => Some(self.root.join("session").join(format!("{id}.jsonl"))),
            _ => None,
        }
    }

    fn supported(&self, scope: &MemScope) -> Result<PathBuf, MemError> {
        self.path_of(scope)
            .ok_or_else(|| MemError::UnsupportedScope {
                provider: ID.to_owned(),
                scope: scope.kind().name().to_owned(),
            })
    }

    /// Every entry in one scope, oldest first. A missing file is an empty
    /// scope, not an error: nothing has been written yet.
    ///
    /// A line that does not parse is **skipped and logged**, not fatal. A
    /// corrupt byte in somebody's notes must not take a turn down with it.
    async fn load(&self, path: &Path) -> Result<Vec<MemEntry>, MemError> {
        let text = match tokio::fs::read_to_string(path).await {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(MemError::backend(ID, e)),
        };
        Ok(text
            .lines()
            .filter(|l| !l.trim().is_empty())
            .filter_map(|line| match serde_json::from_str::<MemEntry>(line) {
                Ok(entry) => Some(entry),
                Err(e) => {
                    tracing::warn!(
                        target: "orrery.ext.memory-file",
                        path = %path.display(),
                        "skipping an unparseable entry: {e}",
                    );
                    None
                }
            })
            .collect())
    }

    async fn store(&self, path: &Path, entries: &[MemEntry]) -> Result<(), MemError> {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| MemError::backend(ID, e))?;
        }
        let mut out = String::new();
        for entry in entries {
            let line = serde_json::to_string(entry).map_err(|e| MemError::backend(ID, e))?;
            out.push_str(&line);
            out.push('\n');
        }
        tokio::fs::write(path, out)
            .await
            .map_err(|e| MemError::backend(ID, e))
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

#[async_trait]
impl MemoryProvider for FileMemory {
    fn id(&self) -> &str {
        ID
    }

    fn scopes(&self) -> &[ScopeKind] {
        &SCOPES
    }

    /// Substring, then recency. That is all.
    ///
    /// An empty query returns everything the scopes hold, newest first, and the
    /// kernel's clamp decides how much of it survives. The budget is looked at
    /// only as a hint to stop reading early; it is **not** enforced here,
    /// because the clamp is the kernel's and duplicating it would mean two
    /// places to get it wrong.
    async fn recall(&self, query: RecallQuery) -> Result<Vec<MemEntry>, MemError> {
        let _guard = self.lock.lock().await;
        let needle = query.query.to_lowercase();
        let mut found = Vec::new();
        for scope in &query.scopes {
            // A scope this provider does not keep is skipped rather than
            // refused: the kernel asks every provider about every scope the
            // actor may read, and answering "not mine" with an error would make
            // one narrow provider fail every recall.
            let Some(path) = self.path_of(scope) else {
                continue;
            };
            for entry in self.load(&path).await? {
                if needle.is_empty()
                    || entry.text.to_lowercase().contains(&needle)
                    || entry.key.to_lowercase().contains(&needle)
                {
                    found.push(entry);
                }
            }
        }
        // Recency. A stable sort, so entries written in the same millisecond
        // keep the order they were written in.
        found.sort_by_key(|e| std::cmp::Reverse(e.at_ms));
        Ok(found)
    }

    async fn write(
        &self,
        _witness: &LifecycleWitness,
        scope: MemScope,
        entry: MemEntry,
    ) -> Result<(), MemError> {
        let path = self.supported(&scope)?;
        let _guard = self.lock.lock().await;
        let mut entries = self.load(&path).await?;
        let mut entry = entry;
        if entry.at_ms == 0 {
            entry.at_ms = now_ms();
        }
        entries.push(entry);
        self.store(&path, &entries).await
    }

    async fn forget(
        &self,
        _witness: &LifecycleWitness,
        scope: MemScope,
        selector: MemSelector,
    ) -> Result<u64, MemError> {
        let path = self.supported(&scope)?;
        let _guard = self.lock.lock().await;
        let entries = self.load(&path).await?;
        let before = entries.len();
        let kept: Vec<MemEntry> = entries
            .into_iter()
            .filter(|e| !selector.matches(e))
            .collect();
        let removed = before - kept.len();
        if removed > 0 {
            self.store(&path, &kept).await?;
        }
        Ok(removed as u64)
    }
}
