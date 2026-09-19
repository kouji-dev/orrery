//! A real store, a real provider, a real registry.
//!
//! Nothing here is a mock of the thing under test. The store is the sqlite
//! backend that passes the session conformance suite, the provider replays a
//! committed `.jsonl` through `orrery-ext-provider-fixture`, and the registry is
//! the one every tool call in the system goes through. What is stubbed is only
//! what sits *past* the kernel: the tool host, because a kernel test should not
//! also be a filesystem test.
//!
//! **No test in this crate makes a network request or needs a key.**

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use futures_util::stream::BoxStream;
use orrery_ext_provider_fixture::FixtureProvider;
use orrery_ext_session_sqlite::SqliteSessionStore;
use orrery_proto::{AgentScope, BranchId, Grant, Layer, Outcome, SessionId, ToolRef};
use orrery_provider::{
    Capabilities, ModelEvent, ModelRequest, Provider, ProviderAuth, ProviderError, TokenCounter,
};
use orrery_session::{SessionStore, TurnKind, TurnRow};
use orrery_tools::{CallCtx, Registry, ToolError, ToolHost, ToolSpec};
use parking_lot::Mutex;
use tokio_util::sync::CancellationToken;

/// The committed model streams, from this crate's directory.
#[must_use]
pub fn stream_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../clients/conformance/streams")
        .join(name)
}

/// One of the six committed streams.
#[must_use]
pub fn fixture(name: &str) -> FixtureProvider {
    FixtureProvider::load(&stream_path(name))
        .unwrap_or_else(|e| panic!("the committed fixture `{name}` loads: {e}"))
}

/// A stream written for one test, beside its temporary directory.
///
/// Hand-written rather than recorded, for the shapes the six committed streams
/// do not cover. Still `ModelEvent`s: the format *is* the type.
#[must_use]
pub fn write_stream(dir: &Path, name: &str, lines: &[&str]) -> FixtureProvider {
    let path = dir.join(name);
    std::fs::write(&path, format!("{}\n", lines.join("\n"))).expect("the fixture is writable");
    FixtureProvider::load(&path).expect("a hand-written fixture parses")
}

/// A provider that replays a different stream on each pass.
///
/// The fixture provider replays one file from the start every time it is asked,
/// which is right for a one-pass test and wrong for a loop: pass two would ask
/// for the same tool again, forever. This hands out the streams in order and
/// repeats the last one, so a test says what the model does on each pass.
pub struct Passes {
    streams: Vec<FixtureProvider>,
    next: AtomicUsize,
    capabilities: Capabilities,
}

impl std::fmt::Debug for Passes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Passes")
            .field("streams", &self.streams.len())
            .field("served", &self.next.load(Ordering::SeqCst))
            .finish()
    }
}

impl Passes {
    /// Replay these, in this order.
    #[must_use]
    pub fn of(streams: Vec<FixtureProvider>) -> Arc<Self> {
        let capabilities = *streams.first().expect("at least one stream").capabilities();
        Arc::new(Self {
            streams,
            next: AtomicUsize::new(0),
            capabilities,
        })
    }

    /// One stream, replayed on every pass.
    #[must_use]
    pub fn repeating(stream: FixtureProvider) -> Arc<Self> {
        Self::of(vec![stream])
    }

    /// Declare different capabilities: a provider without tools, a tiny window.
    #[must_use]
    pub fn with_capabilities(self: Arc<Self>, capabilities: Capabilities) -> Arc<Self> {
        Arc::new(Self {
            streams: self.streams.clone(),
            next: AtomicUsize::new(self.next.load(Ordering::SeqCst)),
            capabilities,
        })
    }

    /// How many passes have been served.
    #[must_use]
    pub fn served(&self) -> usize {
        self.next.load(Ordering::SeqCst)
    }
}

impl Provider for Passes {
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
        let which = i.min(self.streams.len() - 1);
        self.streams[which].stream(req, cancel)
    }

    fn counter(&self) -> Arc<dyn TokenCounter> {
        self.streams[0].counter()
    }

    fn auth(&self) -> Arc<dyn ProviderAuth> {
        self.streams[0].auth()
    }
}

/// A provider that is signed out.
///
/// Wraps a real one so that every other answer — capabilities, the counter, the
/// stream — is the real one's, and only the auth state differs. A test for
/// "refused early" has to be able to tell "we never streamed" from "we could
/// not stream".
pub struct SignedOut {
    inner: Arc<dyn Provider>,
    streamed: Arc<AtomicUsize>,
}

impl SignedOut {
    /// A signed-out view of a provider.
    #[must_use]
    pub fn over(inner: Arc<dyn Provider>) -> Arc<Self> {
        Arc::new(Self {
            inner,
            streamed: Arc::new(AtomicUsize::new(0)),
        })
    }

    /// How many times anybody asked for a completion.
    #[must_use]
    pub fn streamed(&self) -> usize {
        self.streamed.load(Ordering::SeqCst)
    }
}

impl Provider for SignedOut {
    fn id(&self) -> &str {
        self.inner.id()
    }

    fn capabilities(&self) -> &Capabilities {
        self.inner.capabilities()
    }

    fn stream(
        &self,
        req: ModelRequest,
        cancel: CancellationToken,
    ) -> BoxStream<'static, Result<ModelEvent, ProviderError>> {
        self.streamed.fetch_add(1, Ordering::SeqCst);
        self.inner.stream(req, cancel)
    }

    fn counter(&self) -> Arc<dyn TokenCounter> {
        self.inner.counter()
    }

    fn auth(&self) -> Arc<dyn ProviderAuth> {
        Arc::new(NeedsLogin)
    }
}

/// An auth that always says "sign in".
pub struct NeedsLogin;

#[async_trait]
impl ProviderAuth for NeedsLogin {
    fn methods(&self) -> &[orrery_provider::AuthMethod] {
        &[orrery_provider::AuthMethod::ApiKey]
    }

    async fn state(&self) -> Result<orrery_provider::AuthState, ProviderError> {
        Ok(orrery_provider::AuthState::NeedsLogin {
            reason: "no api key is configured".to_owned(),
        })
    }

    async fn login(
        &self,
        _ctx: &dyn orrery_provider::AuthCtx,
    ) -> Result<orrery_provider::AuthState, ProviderError> {
        Err(ProviderError::Auth("not in a test".to_owned()))
    }

    async fn refresh(&self) -> Result<orrery_provider::AuthState, ProviderError> {
        Err(ProviderError::Auth("not in a test".to_owned()))
    }

    async fn logout(&self) -> Result<(), ProviderError> {
        Ok(())
    }
}

/// What a tool host was asked to do, and what it answered.
#[derive(Debug, Default)]
pub struct Recorder {
    calls: Mutex<Vec<(String, serde_json::Value)>>,
}

impl Recorder {
    /// Every call, in order, as `ext.name` and its input.
    #[must_use]
    pub fn calls(&self) -> Vec<(String, serde_json::Value)> {
        self.calls.lock().clone()
    }

    /// How many calls were dispatched.
    #[must_use]
    pub fn len(&self) -> usize {
        self.calls.lock().len()
    }

    /// Whether nothing was dispatched.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.calls.lock().is_empty()
    }
}

/// How a test's tool host behaves.
#[derive(Clone, Debug)]
pub enum Answer {
    /// Answer at once, echoing the input.
    Echo,
    /// Wait to be cancelled. The kernel's own token is what ends the call, so
    /// this never actually returns in a passing test.
    Hang,
}

/// A tool host a test can watch.
pub struct TestHost {
    pub recorder: Arc<Recorder>,
    answer: Answer,
}

impl TestHost {
    /// A host that echoes.
    #[must_use]
    pub fn echoing() -> Arc<Self> {
        Arc::new(Self {
            recorder: Arc::new(Recorder::default()),
            answer: Answer::Echo,
        })
    }

    /// A host that never answers.
    #[must_use]
    pub fn hanging() -> Arc<Self> {
        Arc::new(Self {
            recorder: Arc::new(Recorder::default()),
            answer: Answer::Hang,
        })
    }
}

#[async_trait]
impl ToolHost for TestHost {
    async fn call(
        &self,
        r#ref: &ToolRef,
        input: serde_json::Value,
        _ctx: &CallCtx,
    ) -> Result<Outcome, ToolError> {
        self.recorder
            .calls
            .lock()
            .push((r#ref.to_string(), input.clone()));
        match self.answer {
            Answer::Echo => Ok(Outcome::Ok {
                surface: None,
                value: Some(input),
            }),
            Answer::Hang => {
                // Longer than any test's patience, so only a cancellation ends
                // it.
                tokio::time::sleep(std::time::Duration::from_secs(120)).await;
                Ok(Outcome::ok())
            }
        }
    }
}

/// A registry holding `builtin.read` over a watchable host.
#[must_use]
pub fn registry(host: Arc<dyn ToolHost>) -> Registry {
    let mut registry = Registry::with_host(host);
    registry.register(
        &"builtin".parse().expect("`builtin` is an ext id"),
        Layer::Project,
        ToolSpec::new("read").described("Read a file."),
    );
    registry
}

/// A store, a session and a branch, in a temporary directory.
///
/// Holds the concrete backend as well as the trait object: the kernel talks to
/// `Arc<dyn SessionStore>` like everything else, and a test reads the rows back
/// through the backend's own reader, because asserting on *kinds* is what a
/// transcript test is about and `materialise` renders them away.
pub struct Rig {
    _dir: tempfile::TempDir,
    /// The backend, for reading rows back.
    pub sqlite: Arc<SqliteSessionStore>,
    /// The same thing, as the kernel holds it.
    pub store: Arc<dyn SessionStore>,
    /// The session.
    pub session: SessionId,
    /// Its root branch.
    pub branch: BranchId,
    /// The workspace root, which is the temporary directory.
    pub workspace: PathBuf,
}

impl Rig {
    /// Open a store and create one session in it.
    pub async fn open() -> Self {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let workspace = dir.path().to_path_buf();
        let sqlite = Arc::new(
            SqliteSessionStore::open(workspace.join("sessions.db")).expect("the store opens"),
        );
        let store: Arc<dyn SessionStore> = sqlite.clone();
        let session = store
            .create(&workspace.display().to_string(), "test")
            .await
            .expect("a session");
        let branch = store.open(session).await.expect("the session opens").root;
        Self {
            _dir: dir,
            sqlite,
            store,
            session,
            branch,
            workspace,
        }
    }

    /// The scope of the main agent, allowed to see everything registered.
    #[must_use]
    pub fn scope(&self) -> AgentScope {
        AgentScope {
            agent: "main".to_owned(),
            branch: self.branch,
            tools: vec!["*".to_owned()],
            grant: Grant::nothing(),
        }
    }

    /// Every row on the root branch, oldest first.
    pub async fn rows(&self) -> Vec<TurnRow> {
        self.sqlite
            .reader()
            .turns_on(self.branch)
            .await
            .expect("the branch reads back")
    }

    /// The transcript as tags: `user`, `assistant`, `tool-result`, `summary`.
    pub async fn transcript(&self) -> Vec<String> {
        self.rows()
            .await
            .into_iter()
            .map(|r| r.kind.tag().to_owned())
            .collect()
    }

    /// The transcript in the shape plan 05's acceptance criterion names:
    /// `user`, `assistant(tool_use)`, `tool_result`, `assistant(text)`.
    pub async fn shaped(&self) -> Vec<String> {
        self.rows().await.into_iter().map(shape).collect()
    }

    /// The branch as the model would be shown it: every row rendered, in order.
    pub async fn materialised(&self) -> String {
        let counter = orrery_session::CharsOverFour;
        let view = self
            .store
            .materialise(
                self.branch,
                orrery_proto::TokenBudget {
                    max: 1_000_000,
                    reserve: 0,
                },
                &counter,
            )
            .await
            .expect("the branch materialises");
        view.messages
            .iter()
            .flat_map(|m| m.content.iter())
            .filter_map(|b| match b {
                orrery_proto::ContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(
                "
",
            )
    }

    /// A lease on the root branch.
    pub async fn lease(&self) -> orrery_session::BranchLease {
        self.store.lease(self.branch).await.expect("a lease")
    }
}

/// One row, as the acceptance criterion spells it.
#[must_use]
pub fn shape(row: TurnRow) -> String {
    match &row.kind {
        TurnKind::User { .. } => "user".to_owned(),
        TurnKind::Assistant { content, .. } => {
            let kinds: Vec<&str> = content
                .iter()
                .map(|b| match b {
                    orrery_proto::ContentBlock::Text { .. } => "text",
                    orrery_proto::ContentBlock::ToolUse { .. } => "tool_use",
                    orrery_proto::ContentBlock::Thinking { .. } => "thinking",
                    orrery_proto::ContentBlock::ToolResult { .. } => "tool_result",
                    orrery_proto::ContentBlock::Image { .. } => "image",
                    _ => "?",
                })
                .collect();
            format!("assistant({})", kinds.join("+"))
        }
        TurnKind::ToolResult { .. } => "tool_result".to_owned(),
        other => other.tag().to_owned(),
    }
}

/// Whether a tool result carries a denial.
#[must_use]
pub fn is_denied(kind: &TurnKind) -> bool {
    matches!(
        kind,
        TurnKind::ToolResult {
            outcome: Outcome::Denied { .. },
            ..
        }
    )
}

/// The outcome of the first tool result on a branch.
#[must_use]
pub fn first_outcome(rows: &[TurnRow]) -> Option<&Outcome> {
    rows.iter().find_map(|r| match &r.kind {
        TurnKind::ToolResult { outcome, .. } => Some(outcome),
        _ => None,
    })
}
