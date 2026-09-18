//! The instance table: what is loaded, at which generation, in what state.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;
use indexmap::IndexMap;
use orrery_ext_api::{
    BrokerFacade, CallCtx, DeniesEverything, ExtensionManifest, Generation, HostError,
    InstanceState, Ledger, SingletonSlot, SurfaceSink, ToolDef,
};
use orrery_proto::{
    Contribution, ExtId, Grant, Layer, LoadOutcome, LoadStage, Outcome, RuleId, ToolRef,
};
use orrery_tools::{Registry, ToolError, ToolHost, ToolSpec};
use parking_lot::RwLock;
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::host::{ExtensionHost, budget_of, ceiling_of};
use crate::state::{CallGuard, InFlight, StateCell, call_token};

/// One loaded extension.
///
/// Translation #10's runtime half: the manifest says what was declared, the
/// instance is what is running, and the generation is what keeps a reference to
/// one from reaching the other.
pub struct ExtensionInstance {
    /// What it declared.
    pub manifest: Arc<ExtensionManifest>,
    /// Which load this is. Monotonic across the whole table.
    generation: Generation,
    /// Which layer contributed it.
    layer: Layer,
    /// What it was actually granted.
    grant: Grant,
    state: StateCell,
    host: Arc<dyn ExtensionHost>,
    cancel: CancellationToken,
    inflight: Arc<InFlight>,
    tools: Vec<ToolDef>,
    disabled: Vec<String>,
    contributions: RwLock<Vec<Contribution>>,
}

impl std::fmt::Debug for ExtensionInstance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExtensionInstance")
            .field("ext", &self.manifest.name.as_str())
            .field("generation", &self.generation)
            .field("state", &self.state.get())
            .field("disabled", &self.disabled)
            .finish_non_exhaustive()
    }
}

impl ExtensionInstance {
    /// Which extension this is.
    #[must_use]
    pub fn ext(&self) -> &ExtId {
        &self.manifest.name
    }

    /// Which load this is.
    #[must_use]
    pub fn generation(&self) -> Generation {
        self.generation
    }

    /// Which layer contributed it.
    #[must_use]
    pub fn layer(&self) -> Layer {
        self.layer
    }

    /// What it was granted.
    #[must_use]
    pub fn grant(&self) -> &Grant {
        &self.grant
    }

    /// Where it is in its life.
    #[must_use]
    pub fn state(&self) -> InstanceState {
        self.state.get()
    }

    /// What it contributes, as the ledger reports it.
    #[must_use]
    pub fn contributions(&self) -> Vec<Contribution> {
        self.contributions.read().clone()
    }

    /// The tools it actually offers, disabled ones included.
    #[must_use]
    pub fn tools(&self) -> &[ToolDef] {
        &self.tools
    }

    /// The tools that cannot work, and are therefore not offered to the model.
    #[must_use]
    pub fn disabled(&self) -> &[String] {
        &self.disabled
    }

    /// Whether a named tool is one of the disabled ones.
    #[must_use]
    pub fn is_disabled(&self, tool: &str) -> bool {
        self.disabled.iter().any(|d| d == tool)
    }

    /// How many calls it is carrying.
    #[must_use]
    pub fn in_flight(&self) -> usize {
        self.inflight.count()
    }

    /// The token that stops every call this instance is carrying.
    #[must_use]
    pub fn cancel_token(&self) -> &CancellationToken {
        &self.cancel
    }

    /// The runtime this instance is served by.
    #[must_use]
    pub fn host_ref(&self) -> &Arc<dyn ExtensionHost> {
        &self.host
    }

    /// Move to a new state, if the machine allows it.
    pub fn set_state(&self, next: InstanceState) -> bool {
        self.state.set(next)
    }

    /// Finish. Allowed from anywhere: a child that vanished is dead wherever it
    /// was.
    pub fn mark_dead(&self) {
        self.state.set(InstanceState::Dead);
    }

    fn enter(&self) -> CallGuard {
        self.inflight.enter()
    }
}

/// Everything loaded, and the decisions taken while loading it.
///
/// Held as an `Arc` because it is the registry's [`ToolHost`]: `Registry::with_host`
/// takes an `Arc<dyn ToolHost>`, and the table is the thing that knows whether
/// the extension behind a reference is still there.
pub struct ExtensionTable {
    instances: RwLock<IndexMap<ExtId, Arc<ExtensionInstance>>>,
    singletons: RwLock<BTreeMap<SingletonSlot, (ExtId, Layer)>>,
    ledger: Ledger,
    next_generation: AtomicU64,
    broker: Arc<dyn BrokerFacade>,
    ui: RwLock<SurfaceSink>,
}

impl std::fmt::Debug for ExtensionTable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExtensionTable")
            .field("loaded", &self.instances.read().len())
            .finish_non_exhaustive()
    }
}

impl ExtensionTable {
    /// An empty table whose broker refuses everything.
    ///
    /// Refusing is the right default for a missing dependency: plan 07's broker
    /// is substituted in later, and until then an extension that reaches for a
    /// file is told no rather than quietly succeeding.
    #[must_use]
    pub fn new() -> Arc<Self> {
        Self::with_broker(Arc::new(DeniesEverything))
    }

    /// An empty table over a broker.
    #[must_use]
    pub fn with_broker(broker: Arc<dyn BrokerFacade>) -> Arc<Self> {
        Arc::new(Self {
            instances: RwLock::new(IndexMap::new()),
            singletons: RwLock::new(BTreeMap::new()),
            ledger: Ledger::new(),
            next_generation: AtomicU64::new(1),
            broker,
            ui: RwLock::new(SurfaceSink::discarding()),
        })
    }

    /// Send every surface an extension describes somewhere.
    pub fn set_surface_sink(&self, ui: SurfaceSink) {
        *self.ui.write() = ui;
    }

    /// What loaded, what degraded, what failed, what was skipped.
    #[must_use]
    pub fn ledger(&self) -> &Ledger {
        &self.ledger
    }

    /// The instance for an extension, whatever state it is in.
    #[must_use]
    pub fn get(&self, ext: &ExtId) -> Option<Arc<ExtensionInstance>> {
        self.instances.read().get(ext).cloned()
    }

    /// The instance for an extension *at a generation*.
    ///
    /// `None` when it was unloaded, or reloaded since. A reference held across
    /// either cannot reach the new instance, which is the whole job of the
    /// generation id.
    #[must_use]
    pub fn resolve(&self, ext: &ExtId, generation: Generation) -> Option<Arc<ExtensionInstance>> {
        self.get(ext).filter(|i| i.generation == generation)
    }

    /// Everything loaded, in load order.
    #[must_use]
    pub fn all(&self) -> Vec<Arc<ExtensionInstance>> {
        self.instances.read().values().cloned().collect()
    }

    /// Who holds a singleton slot.
    #[must_use]
    pub fn singleton_holder(&self, slot: SingletonSlot) -> Option<ExtId> {
        self.singletons.read().get(&slot).map(|(e, _)| e.clone())
    }

    /// Load one extension through one host, at one layer, under one grant.
    ///
    /// Everything that can go wrong is in the answer: this never returns an
    /// `Err` and never panics, because a session with a broken extension in it
    /// is a session that still has to start.
    pub async fn load(
        self: &Arc<Self>,
        host: Arc<dyn ExtensionHost>,
        manifest: Arc<ExtensionManifest>,
        layer: Layer,
        grant: Grant,
    ) -> LoadOutcome {
        let ext = manifest.name.clone();
        let outcome = host.load(manifest.clone(), grant.clone()).await;

        let (mut contributions, mut problems, ms) = match &outcome {
            LoadOutcome::Ok {
                contributions, ms, ..
            } => (contributions.clone(), Vec::new(), *ms),
            LoadOutcome::Degraded {
                contributions,
                problems,
                ms,
                ..
            } => (contributions.clone(), problems.clone(), *ms),
            // It did not come up. Nothing to put in the table; the ledger keeps
            // the reason.
            failed_or_skipped => {
                self.ledger.record(failed_or_skipped.clone());
                return outcome;
            }
        };

        // Cross-extension rules the host cannot see: a singleton slot has one
        // holder, and the closer layer takes it.
        self.settle_singletons(&manifest, layer, &mut contributions, &mut problems);

        let disabled = host.disabled(&ext);
        let tools = host.tools(&ext);
        let state = if problems.is_empty() {
            InstanceState::Live
        } else {
            InstanceState::Degraded
        };

        let instance = Arc::new(ExtensionInstance {
            manifest,
            generation: Generation(self.next_generation.fetch_add(1, Ordering::SeqCst)),
            layer,
            grant,
            state: StateCell::new(state),
            host,
            cancel: CancellationToken::new(),
            inflight: Arc::new(InFlight::default()),
            tools,
            disabled,
            contributions: RwLock::new(contributions.clone()),
        });
        self.instances.write().insert(ext.clone(), instance);

        let settled = if problems.is_empty() {
            LoadOutcome::Ok {
                ext,
                contributions,
                ms,
            }
        } else {
            LoadOutcome::Degraded {
                ext,
                contributions,
                ms,
                problems,
            }
        };
        self.ledger.record(settled.clone());
        settled
    }

    /// Give every slot this manifest claims to whichever layer is closer, and
    /// say so in the ledger either way.
    fn settle_singletons(
        &self,
        manifest: &ExtensionManifest,
        layer: Layer,
        contributions: &mut Vec<Contribution>,
        problems: &mut Vec<String>,
    ) {
        for slot in manifest.provides.singletons() {
            let name = manifest
                .provides
                .singleton_name(slot)
                .unwrap_or_default()
                .to_owned();
            let held = self.singletons.read().get(&slot).cloned();
            match held {
                Some((holder, _)) if holder == manifest.name => {}
                Some((holder, holder_layer)) if layer > holder_layer => {
                    // The newcomer is closer: it takes the slot, and the
                    // previous holder is demoted *and told why*.
                    self.singletons
                        .write()
                        .insert(slot, (manifest.name.clone(), layer));
                    self.demote(&holder, slot, &manifest.name, layer);
                }
                Some((holder, holder_layer)) => {
                    problems.push(format!(
                        "singleton `{slot}` went to `{holder}` ({holder_layer:?} is closer \
                         than {layer:?}); this extension's `{name}` is not used"
                    ));
                    strip(contributions, slot, &name);
                }
                None => {
                    self.singletons
                        .write()
                        .insert(slot, (manifest.name.clone(), layer));
                }
            }
        }
    }

    /// Record that a loaded extension has lost a singleton slot.
    fn demote(&self, loser: &ExtId, slot: SingletonSlot, winner: &ExtId, winner_layer: Layer) {
        let problem = format!(
            "singleton `{slot}` went to `{winner}` ({winner_layer:?} is closer); \
             this extension's is not used"
        );

        let Some(instance) = self.get(loser) else {
            return;
        };
        let name = instance
            .manifest
            .provides
            .singleton_name(slot)
            .unwrap_or_default()
            .to_owned();

        let mut contributions = instance.contributions();
        strip(&mut contributions, slot, &name);
        *instance.contributions.write() = contributions.clone();
        instance.state.set(InstanceState::Degraded);

        let mut problems = match self.ledger.of(loser) {
            Some(LoadOutcome::Degraded { problems, .. }) => problems,
            _ => Vec::new(),
        };
        problems.push(problem);
        self.ledger.record(LoadOutcome::Degraded {
            ext: loser.clone(),
            contributions,
            ms: 0,
            problems,
        });
    }

    /// Register an extension's live tools with the tool registry.
    ///
    /// Disabled tools are **not** registered: a tool that cannot work should
    /// not be in the list the model is shown, and hiding it here rather than
    /// refusing it later is what makes `visible` a real subset.
    pub fn register_into(&self, registry: &mut Registry, ext: &ExtId) {
        let Some(instance) = self.get(ext) else {
            return;
        };
        for tool in instance.tools() {
            if instance.is_disabled(&tool.name) {
                continue;
            }
            let mut spec = ToolSpec::new(&tool.name)
                .described(&tool.description)
                .with_schema(tool.input_schema.clone())
                .atomic(tool.atomic);
            if let Some(ceiling) = tool.ceiling {
                spec = spec.with_ceiling(budget_of(ceiling));
            }
            registry.register(ext, instance.layer(), spec);
        }
    }

    /// Run one tool call against whatever is loaded.
    ///
    /// This is [`ToolHost::call`] by another name, offered directly so a test —
    /// or the unload machinery — can reach it without a registry.
    ///
    /// # Errors
    ///
    /// [`ToolError`] when the harness itself could not carry the call. A stale
    /// reference is **not** one: that is `Ok(Outcome::Unloaded)`.
    pub async fn call_tool(
        &self,
        r#ref: &ToolRef,
        input: Value,
        ctx: &orrery_tools::CallCtx,
    ) -> Result<Outcome, ToolError> {
        let Some(instance) = self.get(&r#ref.ext) else {
            return Ok(Outcome::Unloaded {
                ext: r#ref.ext.clone(),
            });
        };
        if !instance.state().accepts_calls() {
            return Ok(Outcome::Unloaded {
                ext: r#ref.ext.clone(),
            });
        }
        if instance.is_disabled(&r#ref.name) {
            return Ok(Outcome::Denied {
                rule: nil_rule(),
                reason: format!(
                    "`{ref}` is disabled: {why}",
                    r#ref = r#ref,
                    why = why_disabled(&instance, &r#ref.name)
                ),
            });
        }

        let _guard = instance.enter();
        let call_ctx = CallCtx::new(
            ctx.call,
            r#ref.ext.clone(),
            r#ref.name.clone(),
            ceiling_of(ctx.budget()),
            call_token(instance.cancel_token()),
            self.broker.clone(),
        )
        .with_ui(self.ui.read().clone());

        match instance
            .host
            .call(&r#ref.ext, &r#ref.name, input, call_ctx)
            .await
        {
            Ok(outcome) => Ok(outcome),
            // The runtime is gone: the child exited, the module trapped, the
            // socket closed. The extension **degrades** and the call settles
            // `Failed`; the session lives. This is the rule for every runtime,
            // which is why it is here and not in one of them.
            Err(HostError::Transport { ext, message }) => {
                instance.set_state(InstanceState::Degraded);
                tracing::warn!(
                    target: "orrery.host",
                    %ext,
                    tool = %r#ref,
                    message,
                    "the extension's runtime failed mid-call; degrading it"
                );
                Ok(Outcome::Failed {
                    code: "extension-unreachable".to_owned(),
                    message,
                })
            }
            Err(e) => Err(ToolError::Host {
                name: r#ref.to_string(),
                message: e.to_string(),
            }),
        }
    }

    /// Drop an instance out of the table, freeing whatever it held.
    pub(crate) fn forget(&self, ext: &ExtId) -> Option<Arc<ExtensionInstance>> {
        let instance = self.instances.write().shift_remove(ext);
        self.singletons
            .write()
            .retain(|_, (holder, _)| holder != ext);
        instance
    }
}

/// The rule id a refusal carries when no configured rule is responsible.
fn nil_rule() -> RuleId {
    "00000000-0000-0000-0000-000000000000"
        .parse()
        .expect("the nil uuid is a uuid")
}

/// Why a tool is off, in the words the ledger already used.
fn why_disabled(instance: &ExtensionInstance, tool: &str) -> String {
    let needs: Vec<String> = instance
        .tools()
        .iter()
        .find(|t| t.name == tool)
        .map(|t| {
            t.requires
                .iter()
                .map(|a| format!("`{}`", orrery_ext_api::broker::aspect_name(*a)))
                .collect()
        })
        .unwrap_or_default();
    if needs.is_empty() {
        "the extension does not contribute it".to_owned()
    } else {
        format!("it needs {}, which was not granted", needs.join(" and "))
    }
}

/// Remove one singleton contribution from a list.
fn strip(contributions: &mut Vec<Contribution>, slot: SingletonSlot, name: &str) {
    let Some(kind) = slot.kind() else { return };
    contributions.retain(|c| !(c.kind == kind && c.name == name));
}

#[async_trait]
impl ToolHost for ExtensionTable {
    async fn call(
        &self,
        r#ref: &ToolRef,
        input: Value,
        ctx: &orrery_tools::CallCtx,
    ) -> Result<Outcome, ToolError> {
        self.call_tool(r#ref, input, ctx).await
    }
}

/// A load that never got as far as the code.
#[must_use]
pub fn failed(ext: &ExtId, stage: LoadStage, message: impl Into<String>) -> LoadOutcome {
    LoadOutcome::Failed {
        ext: ext.clone(),
        stage,
        message: message.into(),
    }
}

/// An unload of something that is not there.
#[must_use]
pub fn not_loaded(ext: &ExtId) -> HostError {
    HostError::NotLoaded { ext: ext.clone() }
}
