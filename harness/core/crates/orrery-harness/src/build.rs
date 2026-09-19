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
    /// The permission rules, as TOML. `None` takes the workspace default:
    /// read, write and spawn inside the workspace, and nothing outside it.
    pub policy_toml: Option<String>,
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
            policy_toml: None,
            routing_toml: None,
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
            policy_toml: None,
            routing_toml: None,
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
pub const DEFAULT_RULES: &str = "\
[permissions]
allow = [\"tool(*)\", \"read(./**)\", \"write(./**)\", \"spawn(*)\"]
";

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
    pub store: Arc<dyn SessionStore>,
    pub engine: Arc<PolicyEngine>,
    pub ledger: orrery_ext_api::Ledger,
    pub session: SessionId,
    pub branch: BranchId,
    pub scope: AgentScope,
}

/// Everything, in the order the diagram shows.
pub(crate) async fn assemble(config: &ResolvedConfig) -> Result<Assembled, BuildError> {
    // 1 · The rules, and the engine that mints against them.
    let rules = PolicyBuilder::new(&config.workspace)
        .layer_toml(
            config.policy_toml.as_deref().unwrap_or(DEFAULT_RULES),
            config.workspace.join("orrery.toml"),
            Layer::Project,
            true,
        )?
        .build()?;
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

    // 3 · What the agent is allowed to be. The grant is the ceiling the tools
    //     are offered under: a bundle whose `spawn` is missing here loads
    //     `Degraded` with the tool that needed it disabled, rather than failing.
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
        grant: grant.clone(),
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
    let mut native = NativeRegistry::new();
    crate::features::register_native(&mut native);
    let host = Arc::new(NativeHost::new(native));
    let mut registry =
        Registry::with_host(table.clone()).with_policy(Arc::new(EngineGate::new(engine.clone())));
    for manifest in host.manifests() {
        let ext = manifest.name.clone();
        let outcome = table
            .load(host.clone(), manifest, Layer::Project, grant.clone())
            .await;
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
        let Some(host) = crate::features::host_for(
            manifest.runtime,
            &ext,
            &source.root,
            facade.clone() as Arc<dyn orrery_ext_api::BrokerFacade>,
        ) else {
            tracing::warn!(
                target: "orrery.harness.build",
                ext = %ext,
                runtime = %manifest.runtime,
                "this build has no host for that runtime, so the extension is skipped"
            );
            continue;
        };
        match table
            .load(host, manifest, source.layer, grant.clone())
            .await
        {
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

    // 7 · The provider, and the kernel over all of it.
    let provider = provider_for(&config.provider)?;
    let registry = Arc::new(registry);
    let kernel = Kernel::new(
        store.clone(),
        provider,
        registry.clone(),
        config.kernel.clone(),
    )
    .with_audit(config.audit.clone())
    .with_revoker(LedgerRevoker::new(engine.ledger().clone()));

    Ok(Assembled {
        kernel: Arc::new(kernel),
        registry,
        router,
        store,
        engine,
        ledger: table.ledger().clone(),
        session,
        branch,
        scope,
    })
}

/// Build the provider a [`ProviderChoice`] names.
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
pub fn provider_for(choice: &ProviderChoice) -> Result<Arc<dyn Provider>, BuildError> {
    Ok(match choice {
        ProviderChoice::Custom(provider) => provider.clone(),
        ProviderChoice::Fixture { passes } => crate::features::fixture_provider(passes)?,
        ProviderChoice::Anthropic {
            model,
            credential,
            base_url,
        } => crate::features::anthropic_provider(model, credential, base_url.as_deref())?,
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
