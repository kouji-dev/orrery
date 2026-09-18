//! Test doubles. **No provider, no network, no model, no API key.**
//!
//! The scripted provider replays a committed `.jsonl` of `ModelEvent`s, which
//! is the same file format `orrery-ext-provider-fixture` reads. It is
//! re-implemented here rather than depended on because a core crate may not
//! depend on an extension (`deps-check` rule 1) — the same reason
//! `orrery-orchestrator` copies its session store.
#![allow(dead_code)]

pub mod store;

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;
use futures_core::stream::BoxStream;
use orrery_provider::{
    AuthCtx, AuthMethod, AuthState, Capabilities, HeuristicCounter, ModelEvent, ModelRequest,
    Provider, ProviderAuth, ProviderError, TokenCounter,
};
use tokio_util::sync::CancellationToken;

/// Replays a committed stream, ignoring the request entirely.
pub struct ScriptedProvider {
    id: String,
    events: Arc<Vec<ModelEvent>>,
    capabilities: Capabilities,
    calls: AtomicU64,
}

impl ScriptedProvider {
    /// Replay these events.
    pub fn new(id: impl Into<String>, events: Vec<ModelEvent>) -> Self {
        Self {
            id: id.into(),
            events: Arc::new(events),
            capabilities: Capabilities {
                tools: true,
                images: false,
                cache: false,
                max_context: 200_000,
                max_output: 8_192,
            },
            calls: AtomicU64::new(0),
        }
    }

    /// Replay a committed fixture file: one `ModelEvent` per line.
    pub fn from_fixture(id: impl Into<String>, path: impl AsRef<Path>) -> Self {
        let text = std::fs::read_to_string(path.as_ref())
            .unwrap_or_else(|e| panic!("fixture {}: {e}", path.as_ref().display()));
        let events = text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .enumerate()
            .map(|(i, line)| {
                serde_json::from_str::<ModelEvent>(line)
                    .unwrap_or_else(|e| panic!("{}, line {}: {e}", path.as_ref().display(), i + 1))
            })
            .collect();
        Self::new(id, events)
    }

    /// How many times anybody streamed from it.
    pub fn calls(&self) -> u64 {
        self.calls.load(Ordering::Relaxed)
    }
}

impl Provider for ScriptedProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    fn stream(
        &self,
        _req: ModelRequest,
        _cancel: CancellationToken,
    ) -> BoxStream<'static, Result<ModelEvent, ProviderError>> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        let events = Arc::clone(&self.events);
        Box::pin(futures_util::stream::iter(
            (0..events.len()).map(move |i| Ok(events[i].clone())),
        ))
    }

    fn counter(&self) -> Arc<dyn TokenCounter> {
        Arc::new(HeuristicCounter::new())
    }

    fn auth(&self) -> Arc<dyn ProviderAuth> {
        Arc::new(NoAuth)
    }
}

/// No credential, because there is no service.
pub struct NoAuth;

#[async_trait]
impl ProviderAuth for NoAuth {
    fn methods(&self) -> &[AuthMethod] {
        &[]
    }

    async fn state(&self) -> Result<AuthState, ProviderError> {
        Ok(AuthState::Anonymous)
    }

    async fn login(&self, _ctx: &dyn AuthCtx) -> Result<AuthState, ProviderError> {
        Ok(AuthState::Anonymous)
    }

    async fn refresh(&self) -> Result<AuthState, ProviderError> {
        Ok(AuthState::Anonymous)
    }

    async fn logout(&self) -> Result<(), ProviderError> {
        Ok(())
    }
}

/// Where the committed streams live.
pub fn fixture(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}
