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
use orrery_proto::{
    BranchId, Event, Outcome, Seq, SessionId, Surface, SurfaceId, SurfaceKind, SurfacePatch,
    TokenBudget, TurnId, Usage,
};
use orrery_provider::{
    Capabilities, ModelEvent, ModelRequest, Provider, ProviderAuth, ProviderError, TokenCounter,
};
use orrery_session::{
    BranchLease, BranchOutcome, CompactResult, Materialised, NewTurn, SessionError, SessionHandle,
    SessionStore, SessionSummary, StoredEvent, TokenCounter as SessionTokenCounter, TurnKind,
    TurnRow,
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

    /// A surface something described, as the frame that carries it.
    ///
    /// Live, this arrives through [`SurfacePatches`](orrery_harness::SurfacePatches):
    /// the extension described it, the kernel-side differ turned it into a
    /// patch. `replay` calls this directly, off the stored outcome, so a
    /// replayed session emits the same frames the live one did — the whole
    /// point of "turns are the record; frames are a view of it" is that the
    /// view is reproducible.
    pub fn surface(&self, value: Surface) {
        let id = value.id.unwrap_or_default();
        self.publish(Event::Delta {
            seq: self.next(),
            surface: id,
            patch: SurfacePatch::Replace { id, value },
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

/// **A surface the kernel produced, reaching the client.**
///
/// Everything else this publisher mints, it mints itself: assistant prose, a
/// tool call's streaming arguments. This is the one that comes from somewhere
/// else — an extension called `ctx.ui.*`, the kernel-side differ in
/// `orrery-harness` turned it into a patch, and the patch arrives here already
/// diffed. The publisher's only job is to give it a `seq` and put it on the hub.
impl orrery_harness::SurfacePatches for Publisher {
    fn patch(&self, surface: SurfaceId, patch: SurfacePatch) {
        self.publish(Event::Delta {
            seq: self.next(),
            surface,
            patch,
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

/// Where a session's audit stream lives under the state directory.
///
/// One file per session, named by the session id. That is what makes
/// `orrery ledger --session <id>` a file open rather than a scan-and-filter,
/// and it is the only reason the CLI knows which decisions belong to which run
/// — the events themselves carry no session, on purpose: the audit schema is
/// about *what was decided*, and threading a session id through every variant
/// would be a second identity for something the file name already says.
#[must_use]
pub fn audit_dir(state_dir: &std::path::Path) -> PathBuf {
    state_dir.join("audit")
}

/// The audit sink a run writes through.
///
/// # Why it buffers
///
/// The file is named after the session, and the session does not exist until
/// the store creates it — which happens *inside* `Harness::build`, after the
/// sink has already been handed to it. So the sink starts closed, keeps what it
/// is given, and opens the moment [`Recording::create`] tells it which session
/// this is. Nothing is lost, and nothing had to be re-ordered in the facade to
/// make an operator surface possible.
#[derive(Debug)]
struct SessionAudit {
    dir: PathBuf,
    state: std::sync::Mutex<AuditState>,
}

#[derive(Debug)]
enum AuditState {
    /// Before the session id is known: hold on to the events.
    Waiting(Vec<orrery_audit::AuditEvent>),
    /// After it is: a real file.
    Open(orrery_audit::FileSink),
    /// The file would not open. A session must not die because a disk is full,
    /// so this drops records and says so once, on stderr.
    Closed,
}

impl SessionAudit {
    fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            state: std::sync::Mutex::new(AuditState::Waiting(Vec::new())),
        }
    }

    /// The session exists now. Open its file and flush what was held.
    fn bind(&self, session: SessionId) {
        let mut state = self.state.lock().expect("audit");
        let AuditState::Waiting(held) = std::mem::replace(&mut *state, AuditState::Closed) else {
            return;
        };
        match orrery_audit::FileSink::open(self.dir.join(format!("{session}.jsonl"))) {
            Ok(sink) => {
                use orrery_audit::AuditSink as _;
                for event in held {
                    sink.append(event);
                }
                *state = AuditState::Open(sink);
            }
            Err(e) => eprintln!("orrery: not recording the audit stream: {e}"),
        }
    }
}

impl orrery_audit::AuditSink for SessionAudit {
    fn append(&self, event: orrery_audit::AuditEvent) {
        let mut state = self.state.lock().expect("audit");
        match &mut *state {
            AuditState::Waiting(held) => held.push(event),
            AuditState::Open(sink) => orrery_audit::AuditSink::append(sink, event),
            AuditState::Closed => {}
        }
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
    audit: Arc<SessionAudit>,
}

#[async_trait]
impl SessionStore for Recording {
    async fn create(&self, workspace: &str, profile: &str) -> Result<SessionId, SessionError> {
        let session = self.inner.create(workspace, profile).await?;
        // The one place the session id is known early enough to name a file
        // after it. See [`SessionAudit`].
        self.audit.bind(session);
        Ok(session)
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

    /// Delegated like the rest. `orrery replay` reads here, so a wrapper that
    /// did not pass this through would make a recorded session unreplayable.
    async fn turns(&self, branch: BranchId) -> Result<Vec<TurnRow>, SessionError> {
        self.inner.turns(branch).await
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
    /// Where the model comes from, as the flag or the `[provider]` table in
    /// force named it. `None` is an error at build time: there is no provider
    /// that needs no configuration at all.
    pub provider: Option<ProviderChoice>,
    /// The workspace's own `orrery.toml`, for its `[[route]]` list. `None` when
    /// there is no such file, which is an empty rule set rather than a refusal.
    pub routing_toml: Option<String>,
    /// The extensions discovery found, beyond the compiled-in set.
    pub extensions: Vec<orrery_harness::ExtensionSource>,
    /// The MCP servers configuration declares.
    ///
    /// The same list `orrery mcp list` prints, read once in `cmd::setup`. A
    /// server an inspection command can hand-shake with and a turn cannot call
    /// is the phase-7 defect in one sentence.
    pub mcp_servers: Vec<orrery_mcp::ServerSpec>,
    /// The skills the discovery pass found, scoped to the agent that will run.
    pub skills: Vec<orrery_skills::SkillRef>,
    /// The permission rules the layers on disk resolved to, profile shorthands
    /// included.
    ///
    /// Not optional and not a default: `[permissions]` was inert in every build
    /// before this field existed, because nothing carried the resolved rules
    /// from `cmd::setup` to the engine the kernel dispatches through. It is an
    /// `Arc` because [`Setup`] is cloned and a rule set is not.
    pub policy: std::sync::Arc<orrery_policy::ResolvedRules>,
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
    /// No provider was named, by either route.
    #[error(
        "no model. Pass `--provider fixture:<path-to.jsonl>`, or put a          `[provider]` table in a config layer; this build has no provider          that needs no configuration"
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
        let Some(choice) = setup.provider.clone() else {
            return Err(SetupError::NoProvider);
        };
        let publisher = Arc::new(Publisher::new(Hub::new(&setup.profile)));

        // One selector, shared with `Harness::build`'s own. The narrator wraps
        // whatever comes back, so every provider the enum names is narrated the
        // same way and none of them is a special case here.
        let provider = orrery_harness::provider_for(&choice)?;
        let capabilities = *provider.capabilities();
        let provider: Arc<dyn Provider> = Arc::new(Narrating {
            inner: provider,
            publisher: publisher.clone(),
            capabilities,
        });

        let audit = Arc::new(SessionAudit::new(audit_dir(&setup.state_dir)));
        let store = orrery_harness::features::open_store(&setup.state_dir)?;
        let store: Arc<dyn SessionStore> = Arc::new(Recording {
            inner: store,
            publisher: publisher.clone(),
            audit: audit.clone(),
        });

        let mut config = ResolvedConfig::fixture(&setup.workspace, Vec::new());
        config.state_dir = setup.state_dir.clone();
        config.profile = setup.profile.clone();
        config.provider = ProviderChoice::Custom(provider);
        config.store = StoreChoice::Custom(store);
        // Every decision this run makes is written down, per session, under the
        // state directory. `orrery ledger` is what reads it back.
        config.audit = audit;
        config.kernel = setup.kernel.clone();
        config.extensions = setup.extensions.clone();
        // Plan 13 on the run path: the declared servers' tools go into the same
        // registry, behind the same gate, and the discovered `SKILL.md`s into
        // the same system prompt. Both were reachable only from an inspection
        // command before this line existed.
        config.mcp_servers = setup.mcp_servers.clone();
        config.skills = setup.skills.clone();
        // The rules `orrery permissions explain` prints, handed to the engine
        // every tool call is checked against. One rule set, two readers.
        config.policy = Some(setup.policy.clone());
        config.routing_toml = setup.routing_toml.clone();
        // Where a surface an extension described actually goes. Without this
        // the differ runs and its output is thrown away, which is what it did
        // for every round before this one.
        config.surfaces = Some(publisher.clone());

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
        // The store is keyed by turn, which is what makes sealing possible; the
        // harness has no other way to know which turn is running.
        harness.surfaces().begin_turn(turn);
        match harness.submit(prompt, cancel).await {
            Ok(outcome) => {
                // Sealed **before** `turn.settled` goes out: a client that has
                // closed the turn has nowhere to put a later patch, so the
                // refusal happens once, here, and goes back to the extension.
                harness.surfaces().seal_turn(turn);
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
                harness.surfaces().seal_turn(turn);
                publisher.error("kernel", &e.to_string(), false);
                publisher.turn_settled(turn, Usage::default());
                Err(e)
            }
        }
    }
}
