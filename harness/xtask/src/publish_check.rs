//! Can every `publish = true` crate actually be published? Offline.
//!
//! `cargo publish --dry-run` is the real answer and it needs the crates.io
//! index, which this repo's tests may not reach. So the rules below are the
//! ones cargo can evaluate from the manifests alone — and they are the ones we
//! can break by accident:
//!
//! 1. **A path dependency with no `version`.** `cargo package` rewrites path
//!    dependencies into version requirements; one with no version cannot be
//!    rewritten, and the upload fails. This is the same shape as
//!    [`deps_check`](crate::deps_check)'s rule 2, applied to *every* published
//!    crate rather than only to extensions.
//! 2. **A dependency on a `publish = false` crate.** Nobody outside this
//!    repository could ever resolve it.
//! 3. **The manifest fields crates.io requires**: `description`, a licence, and
//!    `repository`.
//!
//! Dev-dependencies are exempt: `cargo package` strips a path dev-dependency
//! that carries no version, and a published crate's tests are not built by the
//! people who depend on it.
//!
//! # Scope
//!
//! Only members under `harness/`. The ADE is a Tauri application shipped as an
//! installer, not a crates.io package; holding its manifests to a publishing
//! rule would make this check something people turn off rather than fix.
//!
//! Implementation plan: `harness/docs/plans/18-writing-an-extension.md` (Task 5)

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

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
    #[serde(default)]
    publish: Option<Vec<String>>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    license: Option<String>,
    #[serde(default)]
    license_file: Option<String>,
    #[serde(default)]
    repository: Option<String>,
    dependencies: Vec<Dependency>,
}

impl Package {
    fn is_published(&self) -> bool {
        match &self.publish {
            None => true,
            Some(registries) => !registries.is_empty(),
        }
    }

    /// Whether this crate sits under `harness/`, relative to the workspace
    /// root. Relative, not a substring search: this repo is often checked out
    /// into a directory that is itself called `harness`.
    fn in_harness(&self, workspace_root: &str) -> bool {
        let p = self.manifest_path.replace('\\', "/");
        let root = workspace_root.replace('\\', "/");
        p.strip_prefix(&root)
            .map(|rel| rel.trim_start_matches('/'))
            .is_some_and(|rel| rel.starts_with("harness/"))
    }

    /// The crates.io fields that are missing, in a stable order.
    fn missing_fields(&self) -> Vec<&'static str> {
        let mut missing = Vec::new();
        if self.description.as_deref().unwrap_or("").trim().is_empty() {
            missing.push("description");
        }
        if self.license.as_deref().unwrap_or("").trim().is_empty()
            && self.license_file.as_deref().unwrap_or("").trim().is_empty()
        {
            missing.push("license");
        }
        if self.repository.as_deref().unwrap_or("").trim().is_empty() {
            missing.push("repository");
        }
        missing
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
/// stable order. An empty vector means every published crate could be uploaded.
///
/// # Errors
///
/// When `cargo metadata` cannot be run or its output cannot be parsed.
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
        if !package.is_published() || !package.in_harness(&metadata.workspace_root) {
            continue;
        }

        let missing = package.missing_fields();
        if !missing.is_empty() {
            violations.push(format!(
                "publish: `{}` is `publish = true` but has no {} — crates.io refuses \
                 an upload without {}",
                package.name,
                missing.join(", no "),
                if missing.len() == 1 { "it" } else { "them" },
            ));
        }

        for dep in &package.dependencies {
            // A path dev-dependency with no version is stripped by
            // `cargo package`, so it cannot break an upload.
            if dep.is_dev() {
                continue;
            }
            let Some(target) = members.get(dep.name.as_str()) else {
                // A crates.io dependency. Already resolvable by definition.
                continue;
            };
            if !dep.has_version() {
                violations.push(format!(
                    "publish: `{}` depends on `{}` by path with no `version` — \
                     `cargo package` cannot rewrite it, so `{}` cannot be published",
                    package.name, target.name, package.name,
                ));
            }
            if !target.is_published() {
                violations.push(format!(
                    "publish: `{}` is `publish = true` but depends on `{}`, which is \
                     `publish = false` — nobody outside this repository could resolve it",
                    package.name, target.name,
                ));
            }
        }
    }

    violations.sort();
    violations.dedup();
    violations
}
