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
/// TODO(plan-10): profile config picks this. The enum is here so the facade has
/// one thing to match on and the features have one place to be read.
#[non_exhaustive]
#[derive(Clone)]
pub enum ProviderChoice {
    /// Replay committed streams, one per pass. No key, no network.
    Fixture {
        /// The `.jsonl` files, one per pass, the last repeating.
        passes: Vec<PathBuf>,
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

/// Everything a harness needs to exist.
///
/// TODO(plan-10): this is `ResolvedConfig`'s kernel-shaped subset. Plan 10 owns
/// parsing, layering and defaults; this owns what the running system needs once
/// that is done.
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
    /// The permission rules, as TOML. `None` takes the workspace default:
    /// read, write and spawn inside the workspace, and nothing outside it.
    pub policy_toml: Option<String>,
    /// What the loop runs under.
    pub kernel: KernelConfig,
    /// Where to record what happened.
    pub audit: Audit,
}

impl ResolvedConfig {
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
            policy_toml: None,
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

    // 5 · The extension table, and the native bundle.
    let table = orrery_host::table::ExtensionTable::with_broker_source(facade);
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

    // 6 · The provider, and the kernel over all of it.
    let provider: Arc<dyn Provider> = match &config.provider {
        ProviderChoice::Custom(provider) => provider.clone(),
        ProviderChoice::Fixture { passes } => crate::features::fixture_provider(passes)?,
    };
    let kernel = Kernel::new(
        store.clone(),
        provider,
        Arc::new(registry),
        config.kernel.clone(),
    )
    .with_audit(config.audit.clone())
    .with_revoker(LedgerRevoker::new(engine.ledger().clone()));

    Ok(Assembled {
        kernel: Arc::new(kernel),
        store,
        engine,
        ledger: table.ledger().clone(),
        session,
        branch,
        scope,
    })
}

/// The per-call ceiling a profile has not overridden.
#[must_use]
pub fn default_tool_budget() -> ToolBudget {
    ToolBudget::new(30_000, 1 << 20)
}
