//! The `native` runtime: a first-party crate compiled in behind a cargo
//! feature.
//!
//! # Why this exists (translation #14)
//!
//! §8 decided the built-in tools ship as a first-party bundle so that no
//! privileged in-kernel path exists. Without a `native` runtime, phase 1 would
//! need exactly that path — a `if tool.starts_with("builtin.")` somewhere,
//! skipping the registry, the policy check, or both. With it, `builtin.read`
//! goes registry → policy → broker like everything else, and the only
//! difference from a community extension is where the code was compiled.
//!
//! What it does **not** have is the process boundary: no fd isolation, no
//! secret isolation. It is still policy-checked and still budgeted. Saying that
//! plainly is the point; hiding it would be the problem.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use orrery_ext_api::manifest::covers;
use orrery_ext_api::{
    CallCtx, ExtensionManifest, HostError, ManifestError, NativeExtension, RuntimeKind, ToolDef,
};
use orrery_proto::{ContributionKind, ExtId, Grant, LoadOutcome, LoadStage, Outcome};
use parking_lot::RwLock;
use serde_json::Value;

use crate::host::ExtensionHost;
use crate::table::failed;

/// The compiled-in extensions this build has.
///
/// The facade fills it behind cargo features: a build without
/// `--features tools-builtin` simply has fewer entries, and nothing else in the
/// system knows the difference.
#[derive(Default)]
pub struct NativeRegistry {
    by_ext: HashMap<ExtId, Registered>,
    broken: Vec<ManifestError>,
}

struct Registered {
    manifest: Arc<ExtensionManifest>,
    code: Arc<dyn NativeExtension>,
}

impl std::fmt::Debug for NativeRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeRegistry")
            .field("extensions", &self.by_ext.len())
            .field("broken", &self.broken.len())
            .finish()
    }
}

impl NativeRegistry {
    /// A registry with nothing compiled in.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a compiled-in extension.
    ///
    /// Its manifest is parsed **here**, by the same parser a third-party
    /// manifest goes through. A bundle whose own `orrery.toml` is wrong is
    /// therefore caught at startup rather than at its first call, and it is
    /// caught by the rule a third party would be held to.
    pub fn register(&mut self, code: Arc<dyn NativeExtension>) {
        match ExtensionManifest::from_toml_str(code.manifest(), code.manifest_path()) {
            Ok(manifest) => {
                self.by_ext.insert(
                    manifest.name.clone(),
                    Registered {
                        manifest: Arc::new(manifest),
                        code,
                    },
                );
            }
            Err(e) => {
                tracing::error!(
                    target: "orrery.host.native",
                    error = %e,
                    "a compiled-in extension's manifest does not parse"
                );
                self.broken.push(e);
            }
        }
    }

    /// The manifests that did not parse, in registration order.
    #[must_use]
    pub fn broken(&self) -> &[ManifestError] {
        &self.broken
    }

    /// One compiled-in extension's manifest.
    #[must_use]
    pub fn manifest_of(&self, ext: &ExtId) -> Option<Arc<ExtensionManifest>> {
        self.by_ext.get(ext).map(|r| r.manifest.clone())
    }

    /// Every compiled-in extension's manifest, in no particular order.
    #[must_use]
    pub fn manifests(&self) -> Vec<Arc<ExtensionManifest>> {
        self.by_ext.values().map(|r| r.manifest.clone()).collect()
    }
}

/// What one loaded native extension is, from the host's side.
struct Loaded {
    code: Arc<dyn NativeExtension>,
    tools: Vec<ToolDef>,
    disabled: Vec<String>,
}

/// The host for compiled-in extensions.
pub struct NativeHost {
    registry: NativeRegistry,
    loaded: RwLock<HashMap<ExtId, Arc<Loaded>>>,
}

impl std::fmt::Debug for NativeHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeHost")
            .field("registry", &self.registry)
            .field("loaded", &self.loaded.read().len())
            .finish()
    }
}

impl NativeHost {
    /// A host over a set of compiled-in extensions.
    #[must_use]
    pub fn new(registry: NativeRegistry) -> Self {
        Self {
            registry,
            loaded: RwLock::new(HashMap::new()),
        }
    }

    /// One compiled-in extension's manifest, for the table to load.
    #[must_use]
    pub fn manifest_of(&self, ext: &ExtId) -> Option<Arc<ExtensionManifest>> {
        self.registry.manifest_of(ext)
    }

    /// Every compiled-in extension's manifest.
    #[must_use]
    pub fn manifests(&self) -> Vec<Arc<ExtensionManifest>> {
        self.registry.manifests()
    }

    /// The manifests that did not parse.
    #[must_use]
    pub fn broken(&self) -> &[ManifestError] {
        self.registry.broken()
    }
}

#[async_trait]
impl ExtensionHost for NativeHost {
    fn runtime(&self) -> RuntimeKind {
        RuntimeKind::Native
    }

    async fn load(&self, manifest: Arc<ExtensionManifest>, grant: Grant) -> LoadOutcome {
        let started = Instant::now();
        let ext = manifest.name.clone();

        let Some(registered) = self.registry.by_ext.get(&ext) else {
            return failed(
                &ext,
                LoadStage::Link,
                format!(
                    "`{ext}` declares `runtime = \"native\"`, and this build has no \
                     such extension compiled in — check the cargo features"
                ),
            );
        };

        if let Err(e) = registered.code.activate().await {
            return failed(&ext, LoadStage::Activate, e.to_string());
        }

        let tools = registered.code.tools();
        let mut problems = Vec::new();

        // A collection member that is not there costs exactly itself: the
        // manifest promised it, the code does not have it, and the rest of the
        // extension carries on.
        for promised in &manifest.provides.tools {
            if !tools.iter().any(|t| &t.name == promised) {
                problems.push(format!(
                    "the manifest promises tool `{promised}`, and the extension does \
                     not contribute it"
                ));
            }
        }

        // A tool whose aspect was not granted is disabled rather than broken:
        // it is never offered to the model, so it cannot be called and fail.
        let mut disabled = Vec::new();
        for tool in &tools {
            let ungranted: Vec<&'static str> = tool
                .requires
                .iter()
                .filter(|a| !covers(&grant.capabilities, **a, None))
                .map(|a| orrery_ext_api::broker::aspect_name(*a))
                .collect();
            if !ungranted.is_empty() {
                problems.push(format!(
                    "tool `{name}` is disabled: no `{missing}` grant",
                    name = tool.name,
                    missing = ungranted.join("`, no `")
                ));
                disabled.push(tool.name.clone());
            }
        }

        // What it actually contributes: the manifest's list, minus the tools
        // that are not there and the tools that cannot work.
        let live: Vec<String> = tools
            .iter()
            .filter(|t| !disabled.contains(&t.name))
            .map(|t| t.name.clone())
            .collect();
        let contributions: Vec<orrery_proto::Contribution> = manifest
            .contributions()
            .into_iter()
            .filter(|c| c.kind != ContributionKind::Tool || live.contains(&c.name))
            .collect();

        self.loaded.write().insert(
            ext.clone(),
            Arc::new(Loaded {
                code: registered.code.clone(),
                tools,
                disabled,
            }),
        );

        let ms = started.elapsed().as_millis() as u64;
        if problems.is_empty() {
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
        }
    }

    fn tools(&self, ext: &ExtId) -> Vec<ToolDef> {
        self.loaded
            .read()
            .get(ext)
            .map(|l| l.tools.clone())
            .unwrap_or_default()
    }

    fn disabled(&self, ext: &ExtId) -> Vec<String> {
        self.loaded
            .read()
            .get(ext)
            .map(|l| l.disabled.clone())
            .unwrap_or_default()
    }

    async fn call(
        &self,
        ext: &ExtId,
        tool: &str,
        input: Value,
        ctx: CallCtx,
    ) -> Result<Outcome, HostError> {
        let loaded = self.loaded.read().get(ext).cloned();
        let Some(loaded) = loaded else {
            return Err(HostError::NotLoaded { ext: ext.clone() });
        };
        if !loaded.tools.iter().any(|t| t.name == tool) {
            return Err(HostError::NoSuchTool {
                ext: ext.clone(),
                tool: tool.to_owned(),
            });
        }
        loaded.code.call(tool, input, &ctx).await
    }

    async fn unload(&self, ext: &ExtId) -> Result<(), HostError> {
        let loaded = self.loaded.write().remove(ext);
        match loaded {
            Some(loaded) => loaded.code.deactivate().await,
            None => Ok(()),
        }
    }
}
