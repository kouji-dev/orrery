//! The dependency-direction rule, enforced over `cargo metadata`.
//!
//! Three rules, from `harness/docs/plans/00-overview.md`:
//!
//! 1. No `core/` crate depends on an `extensions/` or `clients/` crate. Two
//!    exceptions, and both are composition roots rather than libraries:
//!    [`FACADE`], which links the first-party extension set behind cargo
//!    features, and any **binary-only** core crate — `orrery-cli`, which
//!    00-overview's crate table has linking `client-ratatui` and `client-json`.
//!    A binary is the one place where the wiring is allowed to be concrete;
//!    a core *library* that named a client would put a renderer underneath the
//!    kernel, which is the thing the rule exists to stop.
//! 2. Every `extensions/` → `core/` dependency names a `publish = true` crate
//!    **and** carries a `version`. `version` is what makes an extension
//!    publishable; `path` is only what makes it build in-tree.
//! 3. No `extensions/` crate dev-depends on an unpublished core crate. Extension
//!    tests use the mock broker in `orrery-ext-api::testing`, so a community
//!    author has the identical harness.
//!
//! The graph is only checkable when the whole thing exists, which is why this is
//! turned on in the scaffold rather than discovered later.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

/// The one crate allowed to break rule 1: the facade an embedder names.
pub const FACADE: &str = "orrery-harness";

/// Which half of the tree a workspace member lives in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Area {
    /// `harness/core/crates/*`
    Core,
    /// `harness/extensions/**`
    Extension,
    /// `harness/clients/*`
    Client,
    /// Anything else in the workspace: the ADE, `xtask`.
    Other,
}

impl Area {
    fn label(self) -> &'static str {
        match self {
            Area::Core => "core",
            Area::Extension => "extension",
            Area::Client => "client",
            Area::Other => "workspace",
        }
    }

    /// Classify a package by where it sits **relative to the workspace root**.
    ///
    /// Relative, not a substring search: this repo is often checked out into a
    /// directory that is itself called `harness`, so `contains("/harness/core/")`
    /// would be true for every crate in the tree, the ADE included.
    fn of(manifest_path: &str, workspace_root: &str) -> Area {
        let p = manifest_path.replace('\\', "/");
        let root = workspace_root.replace('\\', "/");
        let Some(rel) = p.strip_prefix(&root).map(|r| r.trim_start_matches('/')) else {
            return Area::Other;
        };
        if rel.starts_with("harness/core/") {
            Area::Core
        } else if rel.starts_with("harness/extensions/") {
            Area::Extension
        } else if rel.starts_with("harness/clients/") {
            Area::Client
        } else {
            Area::Other
        }
    }
}

#[derive(Debug, Deserialize)]
struct Metadata {
    packages: Vec<Package>,
    workspace_members: Vec<String>,
    workspace_root: String,
}

#[derive(Debug, Deserialize)]
struct Package {
    id: String,
    name: String,
    manifest_path: String,
    /// `null` when the crate is publishable, `[]` when `publish = false`,
    /// otherwise the allow-list of registries.
    #[serde(default)]
    publish: Option<Vec<String>>,
    dependencies: Vec<Dependency>,
    #[serde(default)]
    targets: Vec<Target>,
}

impl Package {
    /// Whether this crate produces no library at all: a binary, and therefore a
    /// composition root rather than something another crate can link.
    fn is_binary_only(&self) -> bool {
        !self.targets.iter().any(Target::is_library)
    }

    fn is_published(&self) -> bool {
        match &self.publish {
            None => true,
            Some(registries) => !registries.is_empty(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct Target {
    /// `["lib"]`, `["bin"]`, `["test"]`, `["custom-build"]`, …
    kind: Vec<String>,
}

impl Target {
    fn is_library(&self) -> bool {
        self.kind.iter().any(|k| {
            matches!(
                k.as_str(),
                "lib" | "rlib" | "dylib" | "cdylib" | "staticlib" | "proc-macro"
            )
        })
    }
}

#[derive(Debug, Deserialize)]
struct Dependency {
    name: String,
    /// The semver requirement. `*` means none was written down.
    req: String,
    /// `"normal"`, `"dev"` or `"build"`.
    kind: Option<String>,
}

impl Dependency {
    fn is_dev(&self) -> bool {
        self.kind.as_deref() == Some("dev")
    }

    fn has_version(&self) -> bool {
        self.req != "*"
    }
}

/// Run `cargo metadata --no-deps` in `root` and return every violation, in a
/// stable order. An empty vector means the workspace is green.
///
/// `--no-deps` keeps this offline: nothing is resolved against a registry, so
/// the check runs in CI and on a fixture workspace alike.
pub fn check(root: &Path) -> Result<Vec<String>, String> {
    let metadata = metadata(root)?;
    Ok(check_metadata(&metadata))
}

fn metadata(root: &Path) -> Result<Metadata, String> {
    let manifest: PathBuf = root.join("Cargo.toml");
    let out = Command::new(env!("CARGO"))
        .args([
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--manifest-path",
        ])
        .arg(&manifest)
        .output()
        .map_err(|e| {
            format!(
                "could not run cargo metadata on {}: {e}",
                manifest.display()
            )
        })?;
    if !out.status.success() {
        return Err(format!(
            "cargo metadata failed on {}:\n{}",
            manifest.display(),
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    serde_json::from_slice(&out.stdout).map_err(|e| format!("could not parse cargo metadata: {e}"))
}

fn check_metadata(metadata: &Metadata) -> Vec<String> {
    let members: BTreeMap<&str, &Package> = metadata
        .packages
        .iter()
        .filter(|p| metadata.workspace_members.contains(&p.id))
        .map(|p| (p.name.as_str(), p))
        .collect();

    let mut violations = Vec::new();

    for package in members.values() {
        let area = Area::of(&package.manifest_path, &metadata.workspace_root);
        for dep in &package.dependencies {
            let Some(target) = members.get(dep.name.as_str()) else {
                // A third-party crate. Not our business.
                continue;
            };
            let target_area = Area::of(&target.manifest_path, &metadata.workspace_root);

            // Rule 1. The facade may link an extension; a binary-only core
            // crate — the composition root — may link a client.
            let excused = match target_area {
                Area::Extension => package.name == FACADE,
                Area::Client => package.is_binary_only(),
                _ => false,
            };
            if area == Area::Core
                && matches!(target_area, Area::Extension | Area::Client)
                && !excused
            {
                violations.push(format!(
                    "rule 1: core crate `{}` depends on {} crate `{}` — \
                     no core library may depend on extensions/ or clients/ \
                     (the exceptions are `{FACADE}` and a binary-only crate)",
                    package.name,
                    target_area.label(),
                    target.name,
                ));
            }

            if area != Area::Extension || target_area != Area::Core {
                continue;
            }

            if dep.is_dev() {
                // Rule 3.
                if !target.is_published() {
                    violations.push(format!(
                        "rule 3: extension crate `{}` dev-depends on core crate `{}`, \
                         which is `publish = false` — extension tests use the mock broker \
                         in `orrery-ext-api::testing`, not kernel internals",
                        package.name, target.name,
                    ));
                }
                continue;
            }

            // Rule 2, both halves.
            if !target.is_published() {
                violations.push(format!(
                    "rule 2: extension crate `{}` depends on core crate `{}`, \
                     which is `publish = false` — extensions depend on core only \
                     through published crates",
                    package.name, target.name,
                ));
            }
            if !dep.has_version() {
                violations.push(format!(
                    "rule 2: extension crate `{}` depends on core crate `{}` \
                     without a `version` — declare both halves, \
                     `{} = {{ version = \"…\", path = \"…\" }}`, or the crate cannot be published",
                    package.name, target.name, target.name,
                ));
            }
        }
    }

    violations.sort();
    violations.dedup();
    violations
}
