//! `Harness::build`: a config in, a running kernel out.
//!
//! Everything the harness assembles is assembled here, in one order, so that
//! "how is this wired" has one answer:
//!
//! ```text
//! rules ─> PolicyEngine ─┬─> TokenLedger ─> LocalBroker ─> PolicyBroker
//!                        │                                     │
//!                        │                              ExtensionTable
//!                        │                                     │
//!                        └─> EngineGate ─────> Registry <── native extensions
//!                                                 │
//!                          SessionStore ─────> Kernel <── Provider
//! ```
//!
//! Two of those arrows are the ones that matter. `EngineGate` is the registry's
//! policy check, so no tool call can reach a host without one; and
//! `PolicyBroker` holds the same engine, so no *effect* can happen without a
//! token minted by it. Plan 04 and plan 06 both shipped allow-all stubs in those
//! two places; neither stub is in the dispatch path any more.

use std::path::PathBuf;
use std::sync::Arc;

use orrery_audit::Audit;
use orrery_broker::{EngineGate, LocalBroker};
use orrery_ext_api::ExtensionManifest;
use orrery_host::{NativeHost, NativeRegistry};
use orrery_kernel::{Kernel, KernelConfig};
use orrery_policy::{PolicyBuilder, PolicyEngine};
use orrery_proto::{
    AgentScope, Aspect, BranchId, Capability, Consent, Grant, Layer, LoadOutcome, SessionId,
    Subject,
};
use orrery_provider::Provider;
use orrery_session::{SessionError, SessionStore};
use orrery_tools::{Registry, ToolBudget};

use crate::broker::{LedgerRevoker, PolicyBroker};

/// Where the model comes from.
///
/// Profile config picks this: [`crate::config::provider_choice`] reads
/// `[provider]` off the layers in force, and this is the one thing the facade
/// matches on so that every feature is read in one place.
#[non_exhaustive]
#[derive(Clone)]
pub enum ProviderChoice {
    /// Replay committed streams, one per pass. No key, no network.
    Fixture {
        /// The `.jsonl` files, one per pass, the last repeating.
        passes: Vec<PathBuf>,
    },
    /// Anthropic's Messages API, behind the `anthropic` feature.
    ///
    /// The feature is **off by default** and off in CI: it links a TLS stack,
    /// and nothing in this repository may reach the network. The code path
    /// exists and compiles regardless, which is the difference between a
    /// product that can be pointed at a model and one that structurally cannot
    /// — `ProviderChoice` had no variant for a real model at all, so no
    /// configuration, key or flag could have reached one.
    ///
    /// `base_url` is the injectable transport: a test points it at an
    /// in-process loopback server and never leaves the machine.
    Anthropic {
        /// The model id, in the provider's own vocabulary.
        model: String,
        /// The credential name the broker holds the key under.
        credential: String,
        /// Where the API lives. `None` is the real origin.
        base_url: Option<String>,
    },
    /// An OpenAI-compatible chat-completions endpoint, behind the
    /// `openai-compat` feature.
    ///
    /// This is the one provider that needs no credential to exist: point
    /// `base_url` at ollama or vllm on localhost and the binary is talking to a
    /// real model with no key anywhere in the system.
    ///
    /// It is still **off by default**, like `anthropic`, because its `http`
    /// feature links reqwest and rustls and the default build must pull no TLS
    /// stack. "Needs no key" and "links no TLS" are different properties and
    /// only the second decides the default set. The variant exists in every
    /// build regardless, so configuration can name it and the error can say
    /// which flag to turn on.
    ///
    /// `base_url` is also the seam a test uses - an in-process server, and
    /// nothing leaves the machine.
    OpenAiCompat {
        /// The model id, in whatever vocabulary that server uses.
        model: String,
        /// The credential name, for a hosted endpoint that wants one. A local
        /// server wants none and an unset grant is `Anonymous`, not a prompt.
        credential: String,
        /// The server root: `http://localhost:11434/v1` for ollama,
        /// `http://localhost:8000/v1` for vllm. No default, because guessing
        /// which local server somebody runs is worse than asking.
        base_url: String,
    },
    /// Something the caller built.
    Custom(Arc<dyn Provider>),
}

/// Where the turn tree lives.
#[non_exhaustive]
#[derive(Clone)]
pub enum StoreChoice {
    /// The default backend, under the state directory.
    Sqlite,
    /// Something the caller built.
    Custom(Arc<dyn SessionStore>),
}

impl std::fmt::Debug for ProviderChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProviderChoice::Fixture { passes } => {
                f.debug_struct("Fixture").field("passes", passes).finish()
            }
            ProviderChoice::Anthropic {
                model,
                credential,
                base_url,
            } => f
                .debug_struct("Anthropic")
                .field("model", model)
                .field("credential", credential)
                .field("base_url", base_url)
                .finish(),
            ProviderChoice::OpenAiCompat {
                model,
                credential,
                base_url,
            } => f
                .debug_struct("OpenAiCompat")
                .field("model", model)
                .field("credential", credential)
                .field("base_url", base_url)
                .finish(),
            ProviderChoice::Custom(p) => f.debug_tuple("Custom").field(&p.id()).finish(),
        }
    }
}

impl std::fmt::Debug for StoreChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreChoice::Sqlite => f.write_str("Sqlite"),
            StoreChoice::Custom(_) => f.write_str("Custom"),
        }
    }
}

/// An extension discovery found on disk, ready to be loaded.
///
/// Discovery is `orrery-config`'s; loading is the host's. This carries what sits
/// between the two, and nothing else — in particular not a parsed manifest,
/// because the manifest must go through the same parser a first-party bundle
/// goes through, in the same place, so that one broken third-party `orrery.toml`
/// is reported the way a broken first-party one would be.
#[derive(Clone, Debug)]
pub struct ExtensionSource {
    /// Where its `orrery.toml` is.
    pub manifest_path: PathBuf,
    /// The directory its files live in. A `[process]` command is relative to it.
    pub root: PathBuf,
    /// Which layer contributed it.
    pub layer: Layer,
}

/// Every extension discovery found, in the shape a host wants.
///
/// Public because the composition root needs it too: `orrery-cli` builds its
/// `ResolvedConfig` field by field rather than through
/// [`ResolvedConfig::from_layers`], and an installed extension that the binary
/// lists but cannot dispatch is exactly the failure this round exists to end.
#[must_use]
pub fn extension_sources(resolved: &orrery_config::ResolvedConfig) -> Vec<ExtensionSource> {
    resolved
        .manifest
        .extensions
        .iter()
        .map(|found| {
            let (root, manifest_path) = if found.source.is_dir() {
                (found.source.clone(), found.source.join("orrery.toml"))
            } else {
                (
                    found
                        .source
                        .parent()
                        .map_or_else(|| found.source.clone(), std::path::Path::to_path_buf),
                    found.source.clone(),
                )
            };
            ExtensionSource {
                manifest_path,
                root,
                layer: found.layer,
            }
        })
        .collect()
}

/// Everything a harness needs to exist.
///
/// This is the **kernel-shaped subset** of `orrery_config::ResolvedConfig`:
/// that crate owns parsing, layering, trust and defaults, and this owns what
/// the running system needs once that is done.
/// [`from_layers`](Self::from_layers) is the one bridge between the two, and
/// [`crate::config::kernel_config`] is where each key is read.
#[derive(Clone, Debug)]
pub struct ResolvedConfig {
    /// The workspace root. Every relative path a tool names resolves here.
    pub workspace: PathBuf,
    /// Where the session database and the audit stream live.
    pub state_dir: PathBuf,
    /// Which profile this was built from.
    pub profile: String,
    /// Where the model comes from.
    pub provider: ProviderChoice,
    /// Where the turn tree lives.
    pub store: StoreChoice,
    /// The extensions discovery found, beyond the compiled-in set.
    ///
    /// Empty is the honest default for a caller that built this by hand; a
    /// caller that resolved config gets whatever the layers in force declare.
    pub extensions: Vec<ExtensionSource>,
    /// The MCP servers configuration declares.
    ///
    /// Here rather than only in `orrery mcp list` because phase 7's criterion
    /// is about a **turn**: `orrery-cli` depended on `orrery-mcp` and this
    /// crate depended on neither it nor `orrery-skills`, so the inspection
    /// command performed a real handshake and the run path had never heard of
    /// the server. See [`crate::mcp`].
    pub mcp_servers: Vec<orrery_mcp::ServerSpec>,
    /// The skills discovery found, already scoped to the agent that will run.
    ///
    /// Rendered into the system prompt at assemble time: `KernelConfig::skills`
    /// has existed since plan 05 and nothing filled it. See [`crate::skills`].
    pub skills: Vec<orrery_skills::SkillRef>,
    /// The permission rules, already resolved and compiled.
    ///
    /// `None` takes the workspace default ([`DEFAULT_RULES`]): read, write and
    /// spawn inside the workspace, and nothing outside it. That is the right
    /// answer for a caller that built this by hand — a test, an embedder — and
    /// the wrong one for a composition root that read the layers, which is why
    /// this is the *resolved* rule set rather than a TOML fragment. Rules carry
    /// their layer, file and line; a fragment could only be re-parsed as one
    /// layer, and the engine would then disagree with `permissions explain`
    /// about precedence.
    pub policy: Option<Arc<orrery_policy::ResolvedRules>>,
    /// Where the patches the kernel-side differ produces go.
    ///
    /// `None` still runs the differ; its output simply goes nowhere. A
    /// composition root with a client attached passes something that turns each
    /// patch into a frame — which is how a surface the *kernel* produced
    /// reaches a renderer instead of being described and thrown away.
    pub surfaces: Option<Arc<dyn crate::surfaces::SurfacePatches>>,
    /// The routing rules, as the `[[route]]` list of a configuration layer.
    /// `None` is an empty set, which is a router that decides on its own
    /// ladder rather than one that refuses.
    pub routing_toml: Option<String>,
    /// What the loop runs under.
    pub kernel: KernelConfig,
    /// Where to record what happened.
    pub audit: Audit,
}

impl ResolvedConfig {
    /// Build one from the layers `orrery-config` resolved.
    ///
    /// Everything a file did not mention keeps the value `base` gave it, so a
    /// workspace with no `.orrery` at all behaves exactly as it did before this
    /// existed. The provider and the store are the caller's: resolving config
    /// cannot decide what to talk to, because the same layers describe a fixture
    /// replay and a real model.
    #[must_use]
    pub fn from_layers(
        resolved: &orrery_config::ResolvedConfig,
        provider: ProviderChoice,
        store: StoreChoice,
    ) -> Self {
        let workspace = resolved.root.clone();
        let profile = resolved.profile.name.clone();
        // A `[provider]` table in force wins over the caller's default. Without
        // this the enum has a real-model variant nothing can select, which is
        // the same dead end one level up.
        let provider =
            crate::config::provider_choice(&resolved.values, &profile).unwrap_or(provider);
        let kernel = crate::config::kernel_config(
            &resolved.values,
            &profile,
            orrery_kernel::KernelConfig::default(),
        );
        Self {
            state_dir: workspace.join(".orrery"),
            workspace,
            profile: if profile.is_empty() {
                "default".to_owned()
            } else {
                profile
            },
            provider,
            store,
            extensions: extension_sources(resolved),
            mcp_servers: Vec::new(),
            skills: Vec::new(),
            policy: None,
            routing_toml: None,
            surfaces: None,
            kernel,
            audit: orrery_audit::null(),
        }
    }

    /// A config rooted at a workspace, with a fixture provider and the default
    /// store.
    #[must_use]
    pub fn fixture(workspace: impl Into<PathBuf>, passes: Vec<PathBuf>) -> Self {
        let workspace = workspace.into();
        Self {
            state_dir: workspace.join(".orrery"),
            workspace,
            profile: "default".to_owned(),
            provider: ProviderChoice::Fixture { passes },
            store: StoreChoice::Sqlite,
            extensions: Vec::new(),
            mcp_servers: Vec::new(),
            skills: Vec::new(),
            policy: None,
            routing_toml: None,
            surfaces: None,
            kernel: KernelConfig::default(),
            audit: orrery_audit::null(),
        }
    }
}

/// The permissions a workspace has by default.
///
/// Read, write and spawn **inside the workspace**, and every tool that is
/// registered. Deliberately not "everything": a path outside the root does not
/// match `./**`, so the first thing a misbehaving tool tries is the first thing
/// that is refused.
///
/// Re-exported from `orrery-config`, which is where configuration resolution
/// falls back to it. One constant, so the rules a run dispatches through and
/// the rules `permissions explain` prints cannot drift apart.
pub use orrery_config::DEFAULT_PERMISSIONS as DEFAULT_RULES;

/// A harness that could not be built.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    /// The tokio runtime would not start.
    #[error("could not start the runtime: {0}")]
    Runtime(#[source] std::io::Error),
    /// The session store would not open. §4.7: this one cannot degrade.
    #[error(transparent)]
    Store(#[from] SessionError),
    /// No session backend is compiled in.
    #[error("this build has no session store: enable the `sqlite` feature")]
    NoStore,
    /// A `[[route]]` list that will not parse. A person wrote it, so it names
    /// the file and the rule.
    #[error(transparent)]
    Routing(#[from] orrery_router::RuleError),
    /// The named provider is not compiled in.
    #[error("this build has no `{which}` provider: check the cargo features")]
    NoProvider {
        /// Which one was asked for.
        which: String,
    },
    /// A provider would not load.
    #[error("the provider would not load: {message}")]
    Provider {
        /// What went wrong.
        message: String,
    },
    /// The permission rules would not compile.
    #[error(transparent)]
    Policy(#[from] orrery_policy::PolicyError),
    /// A first-party extension did not load.
    ///
    /// Not every load failure: a *degraded* extension is a reportable state and
    /// the harness still starts. This is for an extension that contributed
    /// nothing at all, which in a first-party bundle means the build is wrong.
    #[error("`{ext}` did not load: {message}")]
    Extension {
        /// Which extension.
        ext: String,
        /// What went wrong.
        message: String,
    },
}

/// What `build` assembled, before it is wrapped in a runtime.
pub(crate) struct Assembled {
    pub kernel: Arc<Kernel>,
    pub registry: Arc<Registry>,
    pub router: orrery_router::Router,
    pub surfaces: crate::surfaces::KernelSurfaces,
    pub store: Arc<dyn SessionStore>,
    pub engine: Arc<PolicyEngine>,
    pub ledger: orrery_ext_api::Ledger,
    pub session: SessionId,
    pub branch: BranchId,
    pub scope: AgentScope,
}

/// Everything, in the order the diagram shows.
pub(crate) async fn assemble(config: &ResolvedConfig) -> Result<Assembled, BuildError> {
    // 1 · The rules, and the engine that mints against them. A composition root
    //     that resolved the configuration layers hands its own compiled set
    //     over — the same one `permissions explain` prints — and a caller that
    //     built this by hand gets the workspace default.
    let rules: Arc<orrery_policy::ResolvedRules> = match &config.policy {
        Some(rules) => Arc::clone(rules),
        None => Arc::new(
            PolicyBuilder::new(&config.workspace)
                .layer_toml(
                    DEFAULT_RULES,
                    config.workspace.join("orrery.toml"),
                    Layer::Project,
                    true,
                )?
                .build()?,
        ),
    };
    let engine = Arc::new(
        PolicyEngine::new(rules)
            .with_audit(config.audit.clone())
            .with_consent(orrery_policy::ConsentMode::Never),
    );

    // 2 · The store, and a session on it. History is the one thing that cannot
    //     be reconstructed, so this is the one failure that is fatal.
    let store: Arc<dyn SessionStore> = match &config.store {
        StoreChoice::Custom(store) => store.clone(),
        StoreChoice::Sqlite => crate::features::open_store(&config.state_dir)?,
    };
    let session = store
        .create(&config.workspace.display().to_string(), &config.profile)
        .await?;
    let branch = store.open(session).await?.root;

    // 3 · What the **agent** is allowed to be. This is the main agent's own
    //     ceiling, and it is deliberately not narrowed: the rules decide what
    //     the agent may do, and this grant exists so a *sub*-agent can be given
    //     less.
    //
    //     It is **not** what an extension is loaded under. It used to be, and
    //     that is precisely how section 8 phase 3 came to be unreachable: every
    //     extension was handed `Capability::all` for all four aspects, so
    //     "denied `spawn`" was a state the loader could not be in. What an
    //     extension gets is `gate::grant_for`, below, asked of the same engine.
    let grant = Grant {
        capabilities: vec![
            Capability::all(Aspect::Tool),
            Capability::all(Aspect::Read),
            Capability::all(Aspect::Write),
            Capability::all(Aspect::Spawn),
        ],
        consent: Consent::Always,
    };
    let scope = AgentScope {
        agent: "main".to_owned(),
        branch,
        tools: vec!["*".to_owned()],
        grant,
    };

    // 4 · The broker an extension actually reaches, behind the same engine.
    let broker =
        Arc::new(LocalBroker::new(engine.ledger().clone()).with_audit(config.audit.clone()));
    let facade = PolicyBroker::new(
        engine.clone(),
        broker,
        config.workspace.clone(),
        Subject::Agent,
        scope.clone(),
        config.kernel.tool_budget,
    );

    // 5 · The extension table, the native bundle, and whatever discovery found.
    let table = orrery_host::table::ExtensionTable::with_broker_source(facade.clone());
    // Every `ctx.ui.*` an extension makes now reaches a differ keyed on the
    // call that made it. Before this the table held one session-wide sink that
    // threw everything away, so the surfaces a client saw were the ones the
    // client had minted itself.
    let surfaces = crate::surfaces::KernelSurfaces::new(
        config
            .surfaces
            .clone()
            .unwrap_or_else(|| Arc::new(crate::surfaces::NoPatches)),
    );
    table.set_surface_source(Arc::new(surfaces.clone()));
    let mut native = NativeRegistry::new();
    crate::features::register_native(&mut native);
    let host = Arc::new(NativeHost::new(native));

    // 5a · The MCP servers configuration declares. Discovery starts nothing;
    //      what it builds is the host an `mcp.<server>` reference is routed to,
    //      which has to exist before the registry does because a registry has
    //      exactly one host. Everything else about an MCP tool - the namespace,
    //      the policy gate, the audit line - is the ordinary path.
    let mcp = if config.mcp_servers.is_empty() {
        None
    } else {
        let watch = Arc::new(orrery_mcp::ListChangedWatch::new());
        let servers = Arc::new(
            orrery_mcp::Servers::discover(config.mcp_servers.clone())
                .with_handler(watch.clone()),
        );
        Some((servers, watch))
    };
    let tool_host: Arc<dyn orrery_tools::ToolHost> = match &mcp {
        Some((servers, _)) => Arc::new(crate::mcp::RoutingHost::new(
            table.clone(),
            Arc::new(orrery_mcp::McpHost::new(servers.clone())),
        )),
        None => table.clone(),
    };
    let gate = Arc::new(EngineGate::new(engine.clone()));
    let mut registry = Registry::with_host(tool_host).with_policy(gate.clone());
    for manifest in host.manifests() {
        let ext = manifest.name.clone();
        // The policy gate, at load. A compiled-in bundle is an ordinary
        // extension (translation #14), so it is asked the same question a
        // third-party one is: an operator who denies `spawn` denies it to the
        // builtin tools as well, and `builtin.bash` is then never offered.
        let granted = extension_grant(&engine, &manifest, &scope);
        let outcome = table
            .load(host.clone(), manifest, Layer::Project, granted)
            .await;
        record_load(&config.audit, &outcome);
        match &outcome {
            LoadOutcome::Ok { .. } | LoadOutcome::Degraded { .. } => {
                table.register_into(&mut registry, &ext);
            }
            LoadOutcome::Failed { message, .. } => {
                return Err(BuildError::Extension {
                    ext: ext.to_string(),
                    message: message.clone(),
                });
            }
            other => {
                return Err(BuildError::Extension {
                    ext: ext.to_string(),
                    message: format!("{other:?}"),
                });
            }
        }
    }

    // 5b · The installed set. Same table, same policy gate, same ledger: an
    //      extension a person installed reaches the model through the identical
    //      path a compiled-in one does, and the only difference is which host
    //      runs its code.
    //
    //      A third-party failure is **not** a build failure. A first-party
    //      bundle that contributes nothing means this build is wrong; somebody
    //      else's extension that will not start means their extension will not
    //      start, and a harness that refused to open over it would be unusable.
    //      It lands in the ledger, which is what `orrery ext list` reads.
    let managed = crate::plan::managed_registry();
    for source in &config.extensions {
        let Ok(text) = std::fs::read_to_string(&source.manifest_path) else {
            tracing::warn!(
                target: "orrery.harness.build",
                path = %source.manifest_path.display(),
                "an extension was discovered but its manifest could not be read"
            );
            continue;
        };
        let manifest = match ExtensionManifest::from_toml_str(
            &text,
            source.manifest_path.display().to_string(),
        ) {
            Ok(manifest) => Arc::new(manifest),
            Err(e) => {
                tracing::warn!(
                    target: "orrery.harness.build",
                    path = %source.manifest_path.display(),
                    error = %e,
                    "an extension's manifest does not parse"
                );
                continue;
            }
        };
        let ext = manifest.name.clone();

        // What the run path decides is what `orrery ext list` reports, because
        // both ask `plan::skip_for`. A skip that is only a log line is a skip
        // the listing cannot see, which is how `ext list` came to print `ok`
        // for extensions this loop had already passed over.
        if let Some(skip) = crate::plan::skip_for(manifest.runtime, &source.root, managed.as_ref())
        {
            tracing::warn!(
                target: "orrery.harness.build",
                ext = %ext,
                runtime = %manifest.runtime,
                why = %skip.why,
                "an installed extension is skipped"
            );
            table.ledger().record(skip.outcome(&ext));
            // ...and into the stream, not only the in-memory ledger. A refusal
            // to load is a decision, and section 8 phase 3 says every decision
            // is logged: a fresh run under `unpinned = "refuse"` left
            // `audit/*.jsonl` holding two `model.request` lines and nothing
            // about the extension it had just refused, so `orrery ledger` was
            // empty about the one thing that had happened.
            config.audit.append(orrery_audit::AuditEvent::ExtensionLoad {
                ext: ext.clone(),
                status: "skipped".to_owned(),
                contributions: Vec::new(),
                problems: vec![skip.why.clone()],
            });
            continue;
        }

        let Some(host) = crate::features::host_for(
            manifest.runtime,
            &ext,
            &source.root,
            facade.clone() as Arc<dyn orrery_ext_api::BrokerFacade>,
        ) else {
            // Unreachable while `plan::no_host_reason` and `features::host_for`
            // agree, which `plan::tests::the_two_agree` holds them to.
            tracing::warn!(
                target: "orrery.harness.build",
                ext = %ext,
                runtime = %manifest.runtime,
                "this build has no host for that runtime, so the extension is skipped"
            );
            continue;
        };
        let granted = extension_grant(&engine, &manifest, &scope);
        let outcome = table.load(host, manifest, source.layer, granted).await;
        record_load(&config.audit, &outcome);
        match outcome {
            LoadOutcome::Ok { .. } | LoadOutcome::Degraded { .. } => {
                table.register_into(&mut registry, &ext);
            }
            other => tracing::warn!(
                target: "orrery.harness.build",
                ext = %ext,
                outcome = ?other,
                "an installed extension did not load"
            ),
        }
    }

    // 5c · What each declared MCP server offers, in the same registry behind the
    //      same gate. This is the step whose absence made phase 7 PARTIAL: the
    //      tools were listed by an inspection command and no turn could reach
    //      one, because nothing ever put them in a registry a turn dispatches
    //      through.
    if let Some((servers, watch)) = &mcp {
        let admitted = crate::mcp::install(
            &mut registry,
            servers,
            &Subject::Agent,
            gate.as_ref(),
            &config.audit,
        )
        .await;
        crate::mcp::watch_for_growth(
            servers.clone(),
            watch.clone(),
            config.audit.clone(),
            admitted,
        );
    }

    // 6 · The router. Declared rules, or none at all: an empty set is not a
    //     router that refuses, it is one that decides on its own ladder, so a
    //     workspace with no `[[route]]` list behaves exactly as it did before
    //     the router was in the binary.
    let router = orrery_router::Router::new(orrery_router::RouterProfile {
        rules: match &config.routing_toml {
            Some(text) => orrery_router::RuleSet::parse_toml(
                text,
                &config.workspace.join("orrery.toml").display().to_string(),
            )?,
            None => orrery_router::RuleSet::empty(),
        },
        ..orrery_router::RouterProfile::default()
    })
    .with_audit(config.audit.clone());

    // 7 · The provider, and the kernel over all of it. The skills discovery
    //     found are rendered into section 4 of the system prompt here, which is
    //     the only place that can do it: `KernelConfig` is taken by value and
    //     the kernel is built once.
    let provider = provider_for(&config.provider, &config.state_dir)?;
    let registry = Arc::new(registry);
    let mut kernel_config = config.kernel.clone();
    kernel_config
        .skills
        .extend(crate::skills::render(&config.skills));
    let kernel = Kernel::new(store.clone(), provider, registry.clone(), kernel_config)
    .with_audit(config.audit.clone())
    .with_revoker(LedgerRevoker::new(engine.ledger().clone()));

    Ok(Assembled {
        kernel: Arc::new(kernel),
        registry,
        router,
        surfaces,
        store,
        engine,
        ledger: table.ledger().clone(),
        session,
        branch,
        scope,
    })
}

/// What one extension is loaded under: its manifest's ask, put to the policy
/// engine.
///
/// One line, in one place, called from both load loops — the compiled-in bundle
/// and the installed set — because "what may this extension do" answered twice
/// is how a `native` bundle came to be privileged over a `node` one.
fn extension_grant(
    engine: &PolicyEngine,
    manifest: &ExtensionManifest,
    scope: &AgentScope,
) -> Grant {
    orrery_broker::grant_for(
        engine,
        &manifest.name,
        &manifest.capabilities(),
        Consent::Always,
        scope,
    )
}

/// Put a load in the audit's `load` stream, not only in the in-memory ledger.
///
/// `orrery ledger --stream load` reads the stream; the table's ledger dies with
/// the process. Until this was here, a degrade the loader had decided was
/// visible to nothing a person could run — the same defect the skip path had
/// already been fixed for, one branch over.
fn record_load(audit: &orrery_audit::Audit, outcome: &LoadOutcome) {
    let (ext, status, problems) = match outcome {
        LoadOutcome::Ok { ext, .. } => (ext.clone(), "ok", Vec::new()),
        LoadOutcome::Degraded { ext, problems, .. } => {
            (ext.clone(), "degraded", problems.clone())
        }
        LoadOutcome::Skipped { ext, reason } => (ext.clone(), "skipped", vec![format!("{reason:?}")]),
        LoadOutcome::Failed { ext, message, .. } => (ext.clone(), "failed", vec![message.clone()]),
        _ => return,
    };
    audit.append(orrery_audit::AuditEvent::ExtensionLoad {
        ext,
        status: status.to_owned(),
        contributions: outcome.contributions().iter().map(|c| c.name.clone()).collect(),
        problems,
    });
}

/// Build the provider a [`ProviderChoice`] names.
///
/// `state_dir` is where the `creds` grant lives: what `orrery auth login`
/// wrote. It is a parameter rather than a constant because the state directory
/// is `--state-dir`'s to name, and a provider reading a *different* store from
/// the one the login wrote to is the shape of defect this whole round is about.
///
/// The one place a choice becomes a `Provider`, so a composition root that
/// assembles its own config — `orrery-cli` does, because it wraps the provider
/// in a narrator before the kernel sees it — selects from exactly the same
/// list [`assemble`] does. Two matches on this enum is how a flag comes to
/// reach fewer providers than a config file.
///
/// # Errors
///
/// [`BuildError::NoProvider`] when the variant's cargo feature is off, and
/// [`BuildError::Provider`] when it is on and the provider will not load.
pub fn provider_for(
    choice: &ProviderChoice,
    state_dir: &std::path::Path,
) -> Result<Arc<dyn Provider>, BuildError> {
    Ok(match choice {
        ProviderChoice::Custom(provider) => provider.clone(),
        ProviderChoice::Fixture { passes } => crate::features::fixture_provider(passes)?,
        ProviderChoice::Anthropic {
            model,
            credential,
            base_url,
        } => crate::features::anthropic_provider(
            model,
            credential,
            base_url.as_deref(),
            state_dir,
        )?,
        ProviderChoice::OpenAiCompat {
            model,
            credential,
            base_url,
        } => crate::features::openai_compat_provider(model, credential, base_url)?,
    })
}

/// The per-call ceiling a profile has not overridden.
#[must_use]
pub fn default_tool_budget() -> ToolBudget {
    ToolBudget::new(30_000, 1 << 20)
}
