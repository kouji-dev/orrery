//! The kernel as a server: a `Harness`, a [`Hub`], and the two wrappers that
//! turn a running turn into events.
//!
//! # Why wrappers and not a hook
//!
//! `orrery run` has to emit AG-UI events *while* the turn runs, and the kernel
//! does not publish an event stream — it returns a [`TurnOutcome`] and writes
//! rows. Rather than reach inside it, this module wraps the two collaborators
//! the turn already talks to:
//!
//! - the **provider**, where the model's text arrives as it is generated
//!   ([`Narrating`]), and
//! - the **session store**, where every settled tool call is appended
//!   ([`Recording`]).
//!
//! Both are ordinary `Arc<dyn …>` values handed to
//! [`ResolvedConfig`](orrery_harness::ResolvedConfig), so nothing in `core/`
//! changed shape to make the CLI work, and an embedder that wants its own
//! stream does exactly this.
//!
//! # The run id is the CLI's, not the kernel's
//!
//! `Kernel::run_turn` mints a `TurnId` it never hands out. The id in
//! `RUN_STARTED` / `RUN_FINISHED` is therefore minted here, at submit, and is
//! stable for the whole turn. Everything downstream — the `SurfaceStore`, both
//! renderers — keys off that one id, so the stream is internally consistent;
//! it is simply not the same number the session database stores.
//!
//! Implementation plan: `harness/docs/plans/17-cli.md`

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;
use futures_util::StreamExt;
use futures_util::stream::BoxStream;
use orrery_harness::{Harness, ProviderChoice, ResolvedConfig, StoreChoice};
use orrery_kernel::{KernelConfig, TurnOutcome};
use orrery_provider::{
    Capabilities, ModelEvent, ModelRequest, Provider, ProviderAuth, ProviderError, TokenCounter,
};
use orrery_proto::{
    BranchId, Event, Outcome, Seq, SessionId, Surface, SurfaceId, SurfaceKind, SurfacePatch,
    TokenBudget, TurnId, Usage,
};
use orrery_session::{
    BranchLease, BranchOutcome, CompactResult, Materialised, NewTurn, SessionError, SessionHandle,
    SessionStore, SessionSummary, StoredEvent, TokenCounter as SessionTokenCounter, TurnKind,
};
use orrery_transport::Hub;
use tokio_util::sync::CancellationToken;

/// What a turn emitted, and how it ended.
#[derive(Debug)]
pub struct Completed {
    /// How the kernel says it went.
    pub outcome: TurnOutcome,
    /// Every tool outcome the turn settled, in order.
    pub tools: Vec<Outcome>,
}

/// Everything a turn publishes, in one place.
///
/// Shared by the provider wrapper, the store wrapper and the command that
/// submitted the turn, because all three are describing one stream.
pub struct Publisher {
    hub: Hub,
    seq: AtomicU64,
    state: std::sync::Mutex<Live>,
}

#[derive(Default)]
struct Live {
    /// The surface the assistant's prose is streaming into, if any is open.
    surface: Option<SurfaceId>,
    /// Every tool outcome this turn has settled.
    tools: Vec<Outcome>,
}

impl std::fmt::Debug for Publisher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Publisher")
            .field("seq", &self.seq.load(Ordering::SeqCst))
            .finish_non_exhaustive()
    }
}

impl Publisher {
    /// A publisher over one session's hub.
    #[must_use]
    pub fn new(hub: Hub) -> Self {
        Self {
            hub,
            seq: AtomicU64::new(0),
            state: std::sync::Mutex::new(Live::default()),
        }
    }

    /// The hub every client subscribes to.
    #[must_use]
    pub fn hub(&self) -> &Hub {
        &self.hub
    }

    fn next(&self) -> Seq {
        Seq(self.seq.fetch_add(1, Ordering::SeqCst) + 1)
    }

    fn publish(&self, event: Event) {
        self.hub.publish(&event);
    }

    /// A turn began.
    pub fn turn_started(&self, turn: TurnId) {
        let mut live = self.state.lock().expect("publisher");
        live.surface = None;
        live.tools.clear();
        drop(live);
        self.publish(Event::TurnStarted {
            seq: self.next(),
            turn,
        });
    }

    /// A run of assistant prose, as the provider produced it.
    pub fn text(&self, text: &str) {
        if text.is_empty() {
            return;
        }
        let open = self.state.lock().expect("publisher").surface;
        let patch = match open {
            Some(id) => SurfacePatch::Append {
                id,
                text: text.to_owned(),
            },
            None => {
                let id = SurfaceId::new();
                self.state.lock().expect("publisher").surface = Some(id);
                SurfacePatch::Replace {
                    id,
                    value: Surface::new(SurfaceKind::Markdown {
                        value: text.to_owned(),
                        complete: false,
                    }),
                }
            }
        };
        self.publish(Event::Delta {
            seq: self.next(),
            surface: patch_id(&patch),
            patch,
        });
    }

    /// The model asked for a tool.
    pub fn tool_started(&self, call: orrery_proto::CallId, name: &str) {
        let Ok(r#ref) = name.parse() else {
            // An unresolvable name is still a real call the person should see;
            // it settles as `no-such-tool` and `tool.settled` carries that.
            return;
        };
        self.publish(Event::ToolStarted {
            seq: self.next(),
            call,
            r#ref,
        });
    }

    /// A fragment of a tool call's arguments, as the model produced it.
    ///
    /// Published as a patch on the surface **whose id is the call's id**, which
    /// is what `orrery-agui` encodes as `TOOL_CALL_ARGS`. Our frames have no
    /// argument event of their own; the call's surface is where its arguments
    /// belong anyway, so nothing had to be invented to carry them.
    pub fn tool_args(&self, call: orrery_proto::CallId, fragment: &str) {
        if fragment.is_empty() {
            return;
        }
        let id = SurfaceId::from_uuid(*call.as_uuid());
        self.publish(Event::Delta {
            seq: self.next(),
            surface: id,
            patch: SurfacePatch::Append {
                id,
                text: fragment.to_owned(),
            },
        });
    }

    /// A tool call settled.
    pub fn tool_settled(&self, call: orrery_proto::CallId, outcome: Outcome) {
        self.state
            .lock()
            .expect("publisher")
            .tools
            .push(outcome.clone());
        self.publish(Event::ToolSettled {
            seq: self.next(),
            call,
            outcome,
        });
    }

    /// The turn is over. Closes any open message first, so no client is left
    /// holding an unterminated one.
    pub fn turn_settled(&self, turn: TurnId, usage: Usage) -> Vec<Outcome> {
        let mut live = self.state.lock().expect("publisher");
        let open = live.surface.take();
        let tools = live.tools.clone();
        drop(live);
        if let Some(id) = open {
            let patch = SurfacePatch::Set {
                id,
                path: vec!["complete".to_owned()],
                value: serde_json::Value::Bool(true),
            };
            self.publish(Event::Delta {
                seq: self.next(),
                surface: id,
                patch,
            });
        }
        self.publish(Event::TurnSettled {
            seq: self.next(),
            turn,
            usage,
        });
        tools
    }

    /// Something went wrong, at whatever scope.
    pub fn error(&self, code: &str, message: &str, retryable: bool) {
        self.publish(Event::Error {
            seq: self.next(),
            scope: orrery_proto::ErrorScope::Turn,
            detail: orrery_proto::ErrorDetail {
                code: code.to_owned(),
                message: message.to_owned(),
                retryable,
                data: None,
            },
        });
    }
}

fn patch_id(patch: &SurfacePatch) -> SurfaceId {
    match patch {
        SurfacePatch::Replace { id, .. }
        | SurfacePatch::Append { id, .. }
        | SurfacePatch::Set { id, .. }
        | SurfacePatch::Remove { id } => *id,
        _ => SurfaceId::new(),
    }
}

/// A provider that narrates what it streams.
///
/// Wraps another provider and republishes its text as surface patches. It
/// changes nothing about the stream itself: every event is passed through
/// untouched, in order, which is what keeps this a renderer concern and not a
/// second model path.
struct Narrating {
    inner: Arc<dyn Provider>,
    publisher: Arc<Publisher>,
    capabilities: Capabilities,
}

impl Provider for Narrating {
    fn id(&self) -> &str {
        self.inner.id()
    }

    fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    fn stream(
        &self,
        req: ModelRequest,
        cancel: CancellationToken,
    ) -> BoxStream<'static, Result<ModelEvent, ProviderError>> {
        let publisher = self.publisher.clone();
        self.inner
            .stream(req, cancel)
            .map(move |item| {
                if let Ok(event) = &item {
                    match event {
                        ModelEvent::TextDelta { text } => publisher.text(text),
                        ModelEvent::ToolUseStart { call, name } => {
                            publisher.tool_started(*call, name);
                        }
                        ModelEvent::ToolUseDelta {
                            call,
                            json_fragment,
                        } => publisher.tool_args(*call, json_fragment),
                        _ => {}
                    }
                }
                item
            })
            .boxed()
    }

    fn counter(&self) -> Arc<dyn TokenCounter> {
        self.inner.counter()
    }

    fn auth(&self) -> Arc<dyn ProviderAuth> {
        self.inner.auth()
    }
}

/// A session store that reports every tool call it is asked to record.
///
/// Every other method delegates. `append` is the one that matters: a
/// [`TurnKind::ToolResult`] row is a call that has settled, which is exactly
/// `tool.settled` on the wire.
struct Recording {
    inner: Arc<dyn SessionStore>,
    publisher: Arc<Publisher>,
}

#[async_trait]
impl SessionStore for Recording {
    async fn create(&self, workspace: &str, profile: &str) -> Result<SessionId, SessionError> {
        self.inner.create(workspace, profile).await
    }

    async fn open(&self, session: SessionId) -> Result<SessionHandle, SessionError> {
        self.inner.open(session).await
    }

    async fn list_sessions(&self) -> Result<Vec<SessionSummary>, SessionError> {
        self.inner.list_sessions().await
    }

    async fn lease(&self, branch: BranchId) -> Result<BranchLease, SessionError> {
        self.inner.lease(branch).await
    }

    async fn delete(&self, session: SessionId) -> Result<(), SessionError> {
        self.inner.delete(session).await
    }

    async fn append(&self, lease: &BranchLease, turn: NewTurn) -> Result<TurnId, SessionError> {
        if let TurnKind::ToolResult { call, outcome, .. } = &turn.kind {
            self.publisher.tool_settled(*call, outcome.clone());
        }
        self.inner.append(lease, turn).await
    }

    async fn branch(&self, from: TurnId, label: &str) -> Result<BranchId, SessionError> {
        self.inner.branch(from, label).await
    }

    async fn close_branch(
        &self,
        lease: BranchLease,
        outcome: BranchOutcome,
    ) -> Result<(), SessionError> {
        self.inner.close_branch(lease, outcome).await
    }

    async fn materialise(
        &self,
        branch: BranchId,
        budget: TokenBudget,
        counter: &dyn SessionTokenCounter,
    ) -> Result<Materialised, SessionError> {
        self.inner.materialise(branch, budget, counter).await
    }

    async fn compact(
        &self,
        lease: &BranchLease,
        upto: Seq,
        summary: NewTurn,
    ) -> Result<CompactResult, SessionError> {
        self.inner.compact(lease, upto, summary).await
    }

    async fn events_since(
        &self,
        session: SessionId,
        since: Option<Seq>,
    ) -> Result<Vec<StoredEvent>, SessionError> {
        self.inner.events_since(session, since).await
    }
}

/// How to build the kernel this process serves.
#[derive(Clone, Debug)]
pub struct Setup {
    /// The workspace root. Every relative path a tool names resolves here.
    pub workspace: PathBuf,
    /// Where the session database lives.
    pub state_dir: PathBuf,
    /// Which profile this was built from.
    pub profile: String,
    /// One `.jsonl` per pass, the last repeating. Empty is an error: phase 1
    /// has no provider that does not need to be named.
    pub fixtures: Vec<PathBuf>,
    /// What the loop runs under, as the layers on disk resolved it.
    ///
    /// Not built here: `cmd::setup` folds the five configuration layers into
    /// this, because a ceiling a person wrote in a file has to reach the kernel
    /// or it is decoration. See `orrery_harness::kernel_config`.
    pub kernel: KernelConfig,
}

/// A built kernel with a hub in front of it.
pub struct Session {
    harness: Arc<Harness>,
    publisher: Arc<Publisher>,
}

/// A session that could not be built.
#[derive(Debug, thiserror::Error)]
pub enum SetupError {
    /// No provider was named.
    #[error(
        "no model. Pass `--provider fixture:<path-to.jsonl>`; \
         this build has no provider that needs no configuration"
    )]
    NoProvider,
    /// The harness would not assemble.
    #[error(transparent)]
    Build(#[from] orrery_harness::BuildError),
}

impl Session {
    /// Assemble the kernel, the store and the hub.
    ///
    /// # Errors
    ///
    /// [`SetupError`] when no provider was named or the harness would not
    /// build — a fixture that will not parse, a database that will not open.
    pub fn build(setup: &Setup) -> Result<Self, SetupError> {
        if setup.fixtures.is_empty() {
            return Err(SetupError::NoProvider);
        }
        let publisher = Arc::new(Publisher::new(Hub::new(&setup.profile)));

        let provider = orrery_harness::features::fixture_provider(&setup.fixtures)?;
        let capabilities = *provider.capabilities();
        let provider: Arc<dyn Provider> = Arc::new(Narrating {
            inner: provider,
            publisher: publisher.clone(),
            capabilities,
        });

        let store = orrery_harness::features::open_store(&setup.state_dir)?;
        let store: Arc<dyn SessionStore> = Arc::new(Recording {
            inner: store,
            publisher: publisher.clone(),
        });

        let mut config = ResolvedConfig::fixture(&setup.workspace, setup.fixtures.clone());
        config.state_dir = setup.state_dir.clone();
        config.profile = setup.profile.clone();
        config.provider = ProviderChoice::Custom(provider);
        config.store = StoreChoice::Custom(store);
        config.kernel = setup.kernel.clone();

        Ok(Self {
            harness: Arc::new(Harness::build(config)?),
            publisher,
        })
    }

    /// The hub every renderer attaches to.
    #[must_use]
    pub fn hub(&self) -> Hub {
        self.publisher.hub().clone()
    }

    /// The publisher, for a command that wants to narrate something itself.
    #[must_use]
    pub fn publisher(&self) -> Arc<Publisher> {
        self.publisher.clone()
    }

    /// The session the kernel opened.
    #[must_use]
    pub fn id(&self) -> SessionId {
        self.harness.session()
    }

    /// The harness, for a command that needs the store or the policy engine.
    #[must_use]
    pub fn harness(&self) -> Arc<Harness> {
        self.harness.clone()
    }

    /// The runtime the kernel is on.
    #[must_use]
    pub fn handle(&self) -> tokio::runtime::Handle {
        self.harness.handle()
    }

    /// Run one turn to its end, publishing as it goes.
    ///
    /// # Errors
    ///
    /// [`orrery_kernel::KernelError`] when the harness itself broke. A ceiling,
    /// a refusal and a login prompt are [`TurnOutcome`] values.
    pub async fn submit(
        harness: Arc<Harness>,
        publisher: Arc<Publisher>,
        turn: TurnId,
        prompt: String,
        cancel: CancellationToken,
    ) -> Result<Completed, orrery_kernel::KernelError> {
        publisher.turn_started(turn);
        match harness.submit(prompt, cancel).await {
            Ok(outcome) => {
                let tools = publisher.turn_settled(turn, outcome.usage());
                match &outcome {
                    TurnOutcome::Failed { code, message, .. } => {
                        publisher.error(code, message, false);
                    }
                    TurnOutcome::NeedsLogin { reason } => {
                        publisher.error("provider.needs-login", reason, false);
                    }
                    _ => {}
                }
                Ok(Completed { outcome, tools })
            }
            Err(e) => {
                publisher.error("kernel", &e.to_string(), false);
                publisher.turn_settled(turn, Usage::default());
                Err(e)
            }
        }
    }
}
