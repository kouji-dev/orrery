//! The five configuration layers, their TOML merge, provenance, profiles and trust gating.
//!
//! # The startup order is the design
//!
//! Fixed, one-directional, and the whole point:
//!
//! 1. Read the **managed, organisation and user** layers. They need no trust
//!    decision — the developer or the administrator wrote them.
//! 2. Resolve **trust** from those layers alone, plus the stored answer for
//!    this path. An untrusted workspace stops here and the session runs with
//!    user-level config only.
//! 3. **Discover** — one pass over extensions, skills, prompts and MCP servers
//!    across the layers now in force.
//! 4. **Load and validate**: agent parameter schemas, role bindings, singleton
//!    conflicts. A failure here names the file and the line.
//! 5. Fire `session.start`.
//!
//! An extension therefore cannot influence the trust decision that governs
//! whether it loads, and a project cannot vote itself trusted.
//!
//! Implementation plan: `harness/docs/plans/10-config-layers.md`

#![deny(missing_docs)]
#![forbid(unsafe_code)]

pub mod discover;
pub mod error;
pub mod explain;
pub mod layer;
pub mod merge;
pub mod profile;
pub mod provenance;
pub mod trust;
pub mod validate;

use std::path::{Path, PathBuf};

use orrery_policy::ResolvedRules;
use orrery_proto::Layer;

pub use discover::{
    Discovered, DiscoveryManifest, FsWalk, ItemKind, LayerRoot, LedgerEntry, LoadLedger, Skipped,
    Status, Walk,
};
pub use error::ConfigError;
pub use explain::Explanation;
pub use layer::{CONFIG_DIR, CONFIG_FILE, ConfigPaths, LayerFile};
pub use merge::{IgnoredClaim, MergeReport, Relaxation};
pub use profile::{AgentDef, Assembled, Profile, Shorthand};
pub use provenance::{Fold, Origin, Provenanced, Slot};
pub use trust::{TrustDecision, TrustSource, TrustState, TrustStore};
pub use validate::Validation;

/// One step of the startup order, recorded as it runs.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Step {
    /// 1 · the managed, organisation and user layers.
    Base,
    /// 2 · the trust decision.
    Trust,
    /// 3 · one discovery pass.
    Discover,
    /// 4 · load and validate.
    Validate,
    /// 5 · `session.start`.
    SessionStart,
}

impl Step {
    /// The order, which every run follows.
    #[must_use]
    pub const fn order() -> [Step; 5] {
        [
            Step::Base,
            Step::Trust,
            Step::Discover,
            Step::Validate,
            Step::SessionStart,
        ]
    }
}

/// What [`resolve`] is handed.
pub struct StartupCtx {
    /// Where the layers live.
    pub paths: ConfigPaths,
    /// The profile to assemble, when one was asked for.
    pub profile: Option<String>,
    /// A fresh trust answer from the bootstrap surface, to be stored.
    ///
    /// Open question 2, decided: the decision is rendered by a **minimal
    /// bootstrap surface on the control channel**, before any extension has
    /// loaded, and the answer arrives here. `resolve` never prompts: it is
    /// handed an answer or it is not, because a function that can block on a
    /// human cannot be the fixed first thing a session does.
    pub answer: Option<bool>,
    /// How discovery reaches the filesystem.
    pub walk: Box<dyn Walk>,
}

impl StartupCtx {
    /// A context over some paths, with the real filesystem.
    #[must_use]
    pub fn new(paths: ConfigPaths) -> Self {
        Self {
            paths,
            profile: None,
            answer: None,
            walk: Box::new(FsWalk),
        }
    }

    /// Assemble a named profile.
    #[must_use]
    pub fn with_profile(mut self, name: impl Into<String>) -> Self {
        self.profile = Some(name.into());
        self
    }

    /// Carry an answer from the bootstrap surface.
    #[must_use]
    pub fn with_answer(mut self, trusted: bool) -> Self {
        self.answer = Some(trusted);
        self
    }

    /// Use a different walker — a counting one, in tests.
    #[must_use]
    pub fn with_walk(mut self, walk: Box<dyn Walk>) -> Self {
        self.walk = walk;
        self
    }
}

impl std::fmt::Debug for StartupCtx {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StartupCtx")
            .field("paths", &self.paths)
            .field("profile", &self.profile)
            .field("answer", &self.answer)
            .finish_non_exhaustive()
    }
}

/// One effective configuration, and proof of where every value came from.
#[derive(Debug)]
pub struct ResolvedConfig {
    /// The profile in force.
    pub profile: Profile,
    /// Every value carries its layer, file and line.
    pub values: Provenanced,
    /// Whether the local layers loaded.
    pub trust: TrustState,
    /// Why trust was decided that way.
    pub trust_why: TrustDecision,
    /// What the ledger reports.
    pub manifest: DiscoveryManifest,
    /// What the session will answer `query extensions` with.
    pub ledger: LoadLedger,
    /// The layers actually in force.
    pub layers: Vec<LayerFile>,
    /// Attempts to relax a managed deny.
    pub relaxations: Vec<Relaxation>,
    /// The compiled rules those layers make.
    pub rules: ResolvedRules,
    /// What validation had to say.
    pub validation: Validation,
    /// The workspace root everything resolved against.
    pub root: PathBuf,
    steps: Vec<Step>,
}

impl ResolvedConfig {
    /// Build the kernel's inputs from the profile in force.
    ///
    /// # Errors
    ///
    /// When a rule does not parse or a pattern does not compile.
    pub fn assemble(&self) -> Result<Assembled, ConfigError> {
        profile::assemble(&self.profile, &self.root, &self.layers)
    }

    /// Where one key's value came from, and what it beat.
    #[must_use]
    pub fn explain(&self, key: &str) -> Explanation {
        explain::explain(&self.values, key)
    }

    /// The startup steps, in the order they ran.
    #[must_use]
    pub fn steps(&self) -> &[Step] {
        &self.steps
    }
}

/// Resolve the layers into one effective configuration.
///
/// Runs the five steps in [`Step::order`], every time, including when trust
/// fails at step 2 — steps 3 to 5 then run with the reduced layer set rather
/// than being skipped, because a session with user-level config is still a
/// session and still has to report what it loaded.
///
/// # Errors
///
/// When a layer does not parse, a rule does not compile, or validation fails.
/// Every such error names the file and the line.
pub fn resolve(ctx: &StartupCtx) -> Result<ResolvedConfig, ConfigError> {
    let mut steps = Vec::new();

    // 1 · the layers that need no trust decision.
    steps.push(Step::Base);
    let base_files = ctx.paths.collect_base()?;
    let base = merge::merge(&base_files)?;

    // 2 · trust, from those layers alone plus the stored answer.
    steps.push(Step::Trust);
    let mut store = match &ctx.paths.user_dir {
        Some(dir) => Some(TrustStore::open(dir, &ctx.paths.workspace_root)?),
        None => None,
    };
    if let (Some(store), Some(answer)) = (store.as_mut(), ctx.answer) {
        store.record(&ctx.paths.workspace_root, answer)?;
    }
    let decision = trust::decide(&base.values, store.as_ref(), &ctx.paths.workspace_root);

    let mut ledger = LoadLedger::default();
    let mut layers = base_files;
    let local = ctx.paths.collect_local()?;
    if decision.state.is_trusted() {
        layers.extend(local);
    } else {
        for file in &local {
            ledger.files.push(Skipped {
                path: file.path.clone(),
                layer: file.layer,
                why: format!(
                    "not loaded: {} is not trusted ({})",
                    ctx.paths.workspace_root.display(),
                    decision.why
                ),
            });
        }
    }

    let merged = merge::merge(&layers)?;
    ledger.claims = merged.ignored_trust_claims.clone();
    let rules = merge::policy(&ctx.paths.workspace_root, &layers)?;

    // 3 · one discovery pass over the layers now in force.
    steps.push(Step::Discover);
    let roots = layer_roots(ctx, decision.state);
    let manifest = discover::discover(&roots, &merged.values, ctx.walk.as_ref());
    ledger.record(&manifest);

    // 4 · load and validate.
    steps.push(Step::Validate);
    let validation = validate::validate(&merged.values, &manifest)?;

    // 5 · session.start. The frame itself is the kernel's to send; what is
    // fixed here is that nothing above may run after this point.
    steps.push(Step::SessionStart);

    let profile = profile::select(&merged.values, ctx.profile.as_deref())?;

    Ok(ResolvedConfig {
        profile,
        values: merged.values,
        trust: decision.state,
        trust_why: decision,
        manifest,
        ledger,
        layers,
        relaxations: merged.relaxations,
        rules,
        validation,
        root: ctx.paths.workspace_root.clone(),
        steps,
    })
}

/// The directories discovery walks, closest layer last.
fn layer_roots(ctx: &StartupCtx, trust: TrustState) -> Vec<LayerRoot> {
    let mut roots: Vec<LayerRoot> = Vec::new();
    if let Some(dir) = &ctx.paths.user_dir {
        roots.push(LayerRoot {
            layer: Layer::User,
            dir: dir.clone(),
        });
    }
    if trust.is_trusted() {
        roots.push(LayerRoot {
            layer: Layer::Workspace,
            dir: ctx.paths.workspace_root.join(CONFIG_DIR),
        });
        for file in ctx.paths.project_files() {
            let dir: PathBuf = file
                .parent()
                .map_or_else(|| file.clone(), Path::to_path_buf);
            roots.push(LayerRoot {
                layer: Layer::Project,
                dir,
            });
        }
    }
    roots
}

#[cfg(test)]
mod startup {
    use super::*;

    /// A sandbox whose user directory sits beside the workspace, never in it.
    fn sandbox() -> (tempfile::TempDir, ConfigPaths) {
        let dir = tempfile::tempdir().expect("a temp dir");
        let base = dunce::canonicalize(dir.path()).expect("canonical");
        std::fs::create_dir_all(base.join("workspace/.orrery")).unwrap();
        std::fs::create_dir_all(base.join("home/.orrery")).unwrap();
        let paths = ConfigPaths::sandboxed(base.join("workspace"), base.join("home/.orrery"));
        (dir, paths)
    }

    #[test]
    fn order_is_fixed() {
        let (dir, paths) = sandbox();
        std::fs::write(
            dir.path().join("home/.orrery/config.toml"),
            "model = \"user\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("workspace/.orrery/config.toml"),
            "model = \"project\"\n",
        )
        .unwrap();

        // Trust fails at step 2: steps 3 to 5 still run, on the reduced set.
        let untrusted = resolve(&StartupCtx::new(paths.clone())).expect("a session starts");
        assert_eq!(untrusted.trust, TrustState::Untrusted);
        assert_eq!(untrusted.steps(), Step::order(), "every step ran, in order");
        assert_eq!(untrusted.values.str("model"), Some("user"));

        // And when it succeeds, the same five in the same order.
        let trusted =
            resolve(&StartupCtx::new(paths).with_answer(true)).expect("a session starts");
        assert_eq!(trusted.trust, TrustState::Trusted);
        assert_eq!(trusted.steps(), Step::order());
        assert_eq!(trusted.values.str("model"), Some("project"));
    }

    #[test]
    fn trust_is_decided_before_anything_local_is_read() {
        let (dir, paths) = sandbox();
        // The project asks for the world. None of it is in force.
        std::fs::write(
            dir.path().join("workspace/.orrery/config.toml"),
            "trust = true\nextensions = [\"evil\"]\n[permissions]\nallow = [\"creds(*)\"]\n",
        )
        .unwrap();

        let cfg = resolve(&StartupCtx::new(paths)).expect("a session starts");
        assert_eq!(cfg.trust, TrustState::Untrusted);
        assert!(cfg.manifest.extensions.is_empty());
        assert_eq!(
            cfg.steps()[..2],
            [Step::Base, Step::Trust],
            "trust is step 2, before discovery"
        );
    }
}
