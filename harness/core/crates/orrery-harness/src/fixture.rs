//! A fixture provider that answers a *turn* rather than a pass.
//!
//! `orrery-ext-provider-fixture` replays one file from the start every time it
//! is asked, which is exactly right for a one-pass test and wrong for a loop:
//! pass two would ask for the same tool again, and again. A real turn is a
//! sequence — ask for a tool, see the result, answer — so a fixture that can
//! stand in for one has to be a sequence too.
//!
//! One file per pass, in order, with the last repeating. Still no network, still
//! no key, and still `ModelEvent`s: the format *is* the type.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use futures_util::stream::BoxStream;
use orrery_ext_provider_fixture::FixtureProvider;
use orrery_provider::{
    Capabilities, ModelEvent, ModelRequest, Provider, ProviderAuth, ProviderError, TokenCounter,
};
use tokio_util::sync::CancellationToken;

use crate::build::BuildError;

/// One fixture per pass.
pub struct Sequence {
    passes: Vec<FixtureProvider>,
    next: AtomicUsize,
    capabilities: Capabilities,
}

impl std::fmt::Debug for Sequence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Sequence")
            .field("passes", &self.passes.len())
            .field("served", &self.next.load(Ordering::SeqCst))
            .finish()
    }
}

impl Sequence {
    /// Load one or more `.jsonl` streams.
    ///
    /// # Errors
    ///
    /// [`BuildError::Provider`] when a file will not open or a line will not
    /// parse — at load, naming the line, never in the middle of a turn.
    pub fn load(paths: &[PathBuf]) -> Result<Arc<Self>, BuildError> {
        if paths.is_empty() {
            return Err(BuildError::Provider {
                message: "a fixture provider needs at least one stream".to_owned(),
            });
        }
        let mut passes = Vec::with_capacity(paths.len());
        for path in paths {
            passes.push(Self::one(path)?);
        }
        let capabilities = *passes[0].capabilities();
        Ok(Arc::new(Self {
            passes,
            next: AtomicUsize::new(0),
            capabilities,
        }))
    }

    fn one(path: &Path) -> Result<FixtureProvider, BuildError> {
        FixtureProvider::load(path).map_err(|e| BuildError::Provider {
            message: e.to_string(),
        })
    }

    /// How many passes have been served.
    #[must_use]
    pub fn served(&self) -> usize {
        self.next.load(Ordering::SeqCst)
    }
}

impl Provider for Sequence {
    fn id(&self) -> &str {
        "fixture"
    }

    fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    fn stream(
        &self,
        req: ModelRequest,
        cancel: CancellationToken,
    ) -> BoxStream<'static, Result<ModelEvent, ProviderError>> {
        let i = self.next.fetch_add(1, Ordering::SeqCst);
        self.passes[i.min(self.passes.len() - 1)].stream(req, cancel)
    }

    fn counter(&self) -> Arc<dyn TokenCounter> {
        self.passes[0].counter()
    }

    fn auth(&self) -> Arc<dyn ProviderAuth> {
        self.passes[0].auth()
    }
}
