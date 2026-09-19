//! `orrery install <source>` and `orrery remove <name>`, under the skin.
//!
//! # Where an install lands
//!
//! **`~/.orrery/extensions/<id>/` by default** — the user layer, so an install
//! is personal and available in every project, exactly as Pi behaves.
//! `--workspace` puts it in `<workspace>/.orrery/extensions/<id>/`, for an
//! extension the repository itself needs and should commit. Discovery already
//! scans `<layer-root>/extensions/` at every layer (plan 10, landed), so nothing
//! new is needed on the read side and `install::defaults_to_the_user_layer`
//! asserts exactly that: it calls `orrery_config::discover` from a *different*
//! workspace and finds it.
//!
//! # The order is the type
//!
//! Nothing here can place a registry package without a [`Verified`], and
//! `Verified` comes only from [`crate::fetch::fetch_and_verify`]. The hook
//! runner is reached after placement and never before, which is the sentinel
//! test in `tests/fetch.rs`.
//!
//! # `remove` does not guess
//!
//! An extension installed at both the user and the workspace layer is a
//! question, not a preference. `remove` names both and refuses, because
//! removing the wrong one looks exactly like removing the right one until the
//! next session.

use std::path::{Path, PathBuf};

use orrery_ext_api::ExtensionManifest;
use orrery_proto::{Capability, ExtId, LoadOutcome};

use crate::diff::{Approval, Decision, GrantDiff};
use crate::error::RegistryError;
use crate::fetch::{PackageFetcher, PackageHooks, Verified, copy_tree, fetch_and_verify, tree_sha256};
use crate::index::{Index, Timestamp};
use crate::pin::{
    ManagedRegistry, PinDecision, SupplyChainRecord, Unpinned, UnpinnedReason, decide,
    resolve_entry,
};
use crate::source::{GitRunner, Source, SystemGit};
use crate::verify::{Keyring, check_requires};

/// The file an extension declares itself in.
pub const MANIFEST_FILE: &str = "orrery.toml";

/// Which layer an install goes to.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Target {
    /// `~/.orrery/extensions/<id>/`. The default.
    #[default]
    User,
    /// `<workspace>/.orrery/extensions/<id>/`.
    Workspace,
}

impl Target {
    /// The word a message uses.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Target::User => "user",
            Target::Workspace => "workspace",
        }
    }
}

/// Where the layers are on this machine.
///
/// Constructed from paths rather than discovered, because every test runs
/// against a sandboxed home — plan 10's `ConfigPaths::sandboxed` rule, kept
/// here so no test in this crate can touch a real `~`.
#[derive(Clone, Debug)]
pub struct Layout {
    /// `~/.orrery`.
    pub user_dir: PathBuf,
    /// The workspace root — the directory that holds `.orrery/`.
    pub workspace_root: PathBuf,
}

impl Layout {
    /// The two roots.
    #[must_use]
    pub fn new(user_dir: impl Into<PathBuf>, workspace_root: impl Into<PathBuf>) -> Self {
        Self {
            user_dir: user_dir.into(),
            workspace_root: workspace_root.into(),
        }
    }

    /// Take the roots plan 10 already resolved.
    #[must_use]
    pub fn from_config_paths(paths: &orrery_config::ConfigPaths) -> Option<Self> {
        Some(Self {
            user_dir: paths.user_dir.clone()?,
            workspace_root: paths.workspace_root.clone(),
        })
    }

    /// The layer directory: `~/.orrery`, or `<workspace>/.orrery`.
    #[must_use]
    pub fn layer_root(&self, target: Target) -> PathBuf {
        match target {
            Target::User => self.user_dir.clone(),
            Target::Workspace => self
                .workspace_root
                .join(orrery_config::layer::CONFIG_DIR),
        }
    }

    /// `<layer>/extensions`.
    #[must_use]
    pub fn extensions_dir(&self, target: Target) -> PathBuf {
        self.layer_root(target).join("extensions")
    }

    /// `<layer>/extensions/<id>`.
    #[must_use]
    pub fn dir_for(&self, target: Target, id: &str) -> PathBuf {
        self.extensions_dir(target).join(id)
    }

    /// `<layer>/registry/receipts/<id>.toml` — the install receipt the load
    /// path reads back. See [`crate::pin::RECEIPTS_DIR`].
    #[must_use]
    pub fn receipt_path(&self, target: Target, id: &str) -> PathBuf {
        self.layer_root(target)
            .join(crate::pin::RECEIPTS_DIR)
            .join(format!("{id}.toml"))
    }

    /// Every layer this id is installed at, user first.
    #[must_use]
    pub fn installed(&self, id: &str) -> Vec<(Target, PathBuf)> {
        [Target::User, Target::Workspace]
            .into_iter()
            .map(|t| (t, self.dir_for(t, id)))
            .filter(|(_, p)| p.symlink_metadata().is_ok())
            .collect()
    }
}

/// What `install` was asked for beyond the source.
#[derive(Clone, Debug, Default)]
pub struct InstallOptions {
    /// Which layer.
    pub target: Target,
    /// Symlink instead of copy: the development loop, never for real use.
    pub link: bool,
    /// Answers to the grant diff. Anything unanswered takes
    /// [`GrantDiff::defaults`], which denies whatever is new.
    pub answers: Vec<(String, Decision)>,
    /// Replace an existing install rather than refusing.
    pub force: bool,
    /// Grant everything the extension asks for without asking.
    ///
    /// What `--yes` means. It is a separate flag rather than a set of answers
    /// because the rows are not known until the manifest has been read, and the
    /// whole point of [`GrantDiff::defaults`] is that an unanswered row denies.
    pub allow_all: bool,
}

impl InstallOptions {
    /// Into a layer.
    #[must_use]
    pub fn to(target: Target) -> Self {
        Self {
            target,
            ..Self::default()
        }
    }

    /// Make it a development symlink.
    #[must_use]
    pub fn linked(mut self) -> Self {
        self.link = true;
        self
    }

    /// Answer the grant diff.
    #[must_use]
    pub fn answering(mut self, answers: Vec<(String, Decision)>) -> Self {
        self.answers = answers;
        self
    }

    /// Allow every capability the extension asks for.
    ///
    /// What a `--yes` flag means, and the only way a non-interactive install
    /// gets a capability at all — the defaults deny.
    #[must_use]
    pub fn allowing_everything(mut self) -> Self {
        self.allow_all = true;
        self
    }
}

/// What one install came to.
#[derive(Clone, Debug)]
pub struct InstallRecord {
    /// The extension's own id, from its manifest.
    pub ext: ExtId,
    /// The version installed.
    pub version: semver::Version,
    /// Where it landed.
    pub path: PathBuf,
    /// Which layer.
    pub target: Target,
    /// The source, as typed.
    pub source: Source,
    /// Whether it is pinned, and what verified it.
    pub pin: PinDecision,
    /// The commit sha a git install resolved to, so the install is auditable
    /// even when the ref was a mutable tag.
    pub resolved: Option<String>,
    /// The prompt that was shown.
    pub diff: GrantDiff,
    /// What was granted and what was refused.
    pub approval: Approval,
    /// How the load would go with that grant.
    pub outcome: LoadOutcome,
    /// The supply-chain line.
    pub record: SupplyChainRecord,
}

impl InstallRecord {
    /// Whether this install is signature-checked and hash-pinned.
    #[must_use]
    pub fn is_pinned(&self) -> bool {
        self.pin.pinned
    }
}

/// Everything an install needs that is not the source.
///
/// Built by the caller — the CLI, or a test — so that every collaborator that
/// could reach the network is a value somebody handed in. There is no default
/// that fetches from anywhere.
pub struct Installer<'a> {
    /// Where the layers are.
    pub layout: Layout,
    /// The signed index, when there is one.
    pub index: Option<&'a Index>,
    /// Where the index came from, for messages.
    pub index_url: String,
    /// The keys in force.
    pub keyring: &'a Keyring,
    /// The instant every expiry and key window is checked against.
    pub now: Timestamp,
    /// The managed `[registry]` table, when the managed layer has one.
    pub managed: Option<&'a ManagedRegistry>,
    /// The user's own preference, consulted only when the managed layer is
    /// silent.
    pub user_unpinned: Option<Unpinned>,
    /// How registry packages are fetched.
    pub fetcher: &'a dyn PackageFetcher,
    /// How git sources are fetched.
    pub git: &'a dyn GitRunner,
    /// What may run after a package is placed, and only after.
    pub hooks: &'a dyn PackageHooks,
    /// Where verified packages are kept so a second install needs no network.
    pub cache: PathBuf,
    /// Somewhere to stage a fetch before it has been verified. On no discovery
    /// path, by construction.
    pub quarantine: PathBuf,
}

impl Installer<'_> {
    /// Install one source.
    ///
    /// # Errors
    ///
    /// [`RegistryError::Unpinned`] under managed `refuse`; anything resolution,
    /// fetching, hashing, signature checking or the `requires` mirror returns;
    /// [`RegistryError::AlreadyInstalled`] without `force`.
    pub fn install(
        &self,
        source: &Source,
        options: &InstallOptions,
    ) -> Result<InstallRecord, RegistryError> {
        // 1. Pinning first. A refusal must not fetch anything: under managed
        //    `refuse`, nothing about the source should be contacted at all.
        let pin = decide(source, self.managed, self.user_unpinned, options.link)?;

        // 2. Get the files somewhere that is not a layer.
        let staged = self.stage(source, options, &pin)?;

        // 3. Read what it says about itself, and — for a registry install —
        //    check that against what the index said it would say.
        let manifest_path = staged.dir.join(MANIFEST_FILE);
        let manifest = ExtensionManifest::from_path(&manifest_path).map_err(|e| {
            RegistryError::Fetch {
                id: source.provisional_id(),
                origin: source.label(),
                message: e.to_string(),
            }
        })?;
        if let Some(verified) = &staged.verified {
            check_requires(&verified.entry, &manifest)?;
        }

        // 4. The grant diff, against whatever the installed version had.
        let ext = manifest.name.clone();
        let wants = manifest.capabilities();
        let existing = self.installed_manifest(options.target, ext.as_str());
        let diff = match &existing {
            Some(previous) => GrantDiff::upgrade(
                ext.clone(),
                previous.version.clone(),
                manifest.version.clone(),
                &wants,
                &previous.capabilities(),
            ),
            None => GrantDiff::fresh(ext.clone(), manifest.version.clone(), &wants),
        };
        let answers: Vec<(String, Decision)> = if options.allow_all {
            diff.rows
                .iter()
                .map(|r| (r.field(), Decision::Allow))
                .collect()
        } else {
            options.answers.clone()
        };
        let approval = diff.apply(&answers);

        // 5. Place it.
        let dest = self.layout.dir_for(options.target, ext.as_str());
        if dest.symlink_metadata().is_ok() {
            if !options.force && existing.is_none() {
                return Err(RegistryError::AlreadyInstalled { path: dest });
            }
            remove_path(&dest)?;
        }
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| RegistryError::io(parent, &e))?;
        }
        if options.link {
            let Source::Path { path } = source else {
                return Err(RegistryError::SourceSyntax {
                    input: source.label(),
                    message: "--link only applies to a local path".to_owned(),
                });
            };
            symlink_dir(&absolute(path), &dest)?;
        } else {
            copy_tree(&staged.dir, &dest)?;
        }

        // 6. Only now may anything the package brought be run.
        let mut problems = Vec::new();
        if let Err(message) = self.hooks.post_install(ext.as_str(), &dest) {
            problems.push(format!("post-install: {message}"));
        }

        // 7. Cache a verified package so the next install needs no network.
        if let Some(verified) = &staged.verified {
            let cached = self.cache_dir(ext.as_str(), &manifest.version);
            if cached.symlink_metadata().is_err() {
                copy_tree(&verified.dir, &cached)?;
            }
        }

        let mut pin = pin;
        if let Some(verified) = &staged.verified {
            pin = PinDecision::verified(format!(
                "key {} in {}",
                verified.key,
                if self.index_url.is_empty() {
                    "the registry index"
                } else {
                    &self.index_url
                }
            ));
        }

        let outcome = match approval.outcome(&manifest, 0) {
            LoadOutcome::Ok {
                ext,
                contributions,
                ms,
            } if !problems.is_empty() => LoadOutcome::Degraded {
                ext,
                contributions,
                ms,
                problems: problems.clone(),
            },
            LoadOutcome::Degraded {
                ext,
                contributions,
                ms,
                problems: mut already,
            } => {
                already.extend(problems.clone());
                LoadOutcome::Degraded {
                    ext,
                    contributions,
                    ms,
                    problems: already,
                }
            }
            other => other,
        };

        let record = SupplyChainRecord {
            ext: ext.clone(),
            source: source.label(),
            pinned: pin.pinned,
            development: pin.development,
            rule: pin.rule.clone(),
            reason: pin.reason,
            index: if pin.pinned {
                Some(self.index_url.clone())
            } else {
                None
            },
            refused: None,
        };

        // The load path reads this back: phase 8's criterion is that unpinned
        // extensions refuse to **load**, and a directory on disk says nothing
        // about whether a signature was ever checked. A receipt that cannot be
        // written is not fatal — it fails closed, because a missing receipt is
        // refused under managed `refuse`.
        let receipt = self.layout.receipt_path(options.target, ext.as_str());
        if let Some(parent) = receipt.parent() {
            std::fs::create_dir_all(parent).map_err(|e| RegistryError::io(parent, &e))?;
        }
        std::fs::write(&receipt, record.to_toml()).map_err(|e| RegistryError::io(&receipt, &e))?;

        Ok(InstallRecord {
            ext,
            version: manifest.version.clone(),
            path: dest,
            target: options.target,
            source: source.clone(),
            pin,
            resolved: staged.resolved,
            diff,
            approval,
            outcome,
            record,
        })
    }

    /// Where a verified package is kept between installs.
    #[must_use]
    fn cache_dir(&self, id: &str, version: &semver::Version) -> PathBuf {
        self.cache.join(id).join(version.to_string())
    }

    /// What the installed copy of an extension says about itself.
    fn installed_manifest(&self, target: Target, id: &str) -> Option<ExtensionManifest> {
        ExtensionManifest::from_path(self.layout.dir_for(target, id).join(MANIFEST_FILE)).ok()
    }

    /// Put the source's files in the quarantine directory.
    fn stage(
        &self,
        source: &Source,
        options: &InstallOptions,
        pin: &PinDecision,
    ) -> Result<Staged, RegistryError> {
        let _ = pin;
        let scratch = self.quarantine.join(sanitise(&source.provisional_id()));
        if scratch.symlink_metadata().is_ok() {
            remove_path(&scratch)?;
        }

        match source {
            Source::Registry { .. } => {
                let index = self.index.ok_or_else(|| RegistryError::NotInIndex {
                    id: source.label(),
                    index: if self.index_url.is_empty() {
                        "<no index configured>".to_owned()
                    } else {
                        self.index_url.clone()
                    },
                })?;
                let entry = resolve_entry(index, source, &self.index_url)?;

                // Offline first: a package this machine has already verified is
                // installed from the cache, with the pin re-checked against the
                // cached bytes rather than trusted because they are local.
                let cached = self.cache_dir(&entry.id, &entry.version);
                if cached.is_dir() && tree_sha256(&cached)? == entry.sha256.to_ascii_lowercase() {
                    copy_tree(&cached, &scratch)?;
                    let verified = fetch_and_verify(
                        entry,
                        &CachedFetcher,
                        self.keyring,
                        &self.now,
                        &scratch,
                    )?;
                    return Ok(Staged {
                        dir: scratch,
                        verified: Some(verified),
                        resolved: None,
                    });
                }

                let verified =
                    fetch_and_verify(entry, self.fetcher, self.keyring, &self.now, &scratch)?;
                Ok(Staged {
                    dir: scratch,
                    verified: Some(verified),
                    resolved: None,
                })
            }
            Source::Git { url, reference, .. } => {
                let sha = self.git.clone_at(url, reference, &scratch)?;
                Ok(Staged {
                    dir: scratch,
                    verified: None,
                    resolved: Some(sha),
                })
            }
            Source::Path { path } => {
                let from = absolute(path);
                if !from.is_dir() {
                    return Err(RegistryError::Fetch {
                        id: source.provisional_id(),
                        origin: source.label(),
                        message: format!("{} is not a directory", from.display()),
                    });
                }
                if options.link {
                    // Nothing is copied: the link is the install. The manifest
                    // is still read from the real directory.
                    return Ok(Staged {
                        dir: from,
                        verified: None,
                        resolved: None,
                    });
                }
                copy_tree(&from, &scratch)?;
                Ok(Staged {
                    dir: scratch,
                    verified: None,
                    resolved: None,
                })
            }
            Source::Crate { name, .. } | Source::Npm { name, .. } => {
                // crates.io and npm without an index entry: there is nothing to
                // verify against, so this is the unpinned path by definition.
                // The fetcher decides how the bytes arrive; offline, that is a
                // local mirror directory.
                let entry_source = match source {
                    Source::Crate { .. } => crate::index::EntrySource::CratesIo {
                        name: name.clone(),
                    },
                    _ => crate::index::EntrySource::Npm { name: name.clone() },
                };
                std::fs::create_dir_all(&scratch).map_err(|e| RegistryError::io(&scratch, &e))?;
                self.fetcher
                    .stage(&source.provisional_id(), &entry_source, &scratch)?;
                Ok(Staged {
                    dir: scratch,
                    verified: None,
                    resolved: None,
                })
            }
        }
    }
}

/// A fetcher for a package that is already in the quarantine directory.
///
/// Used when the cache hit: the bytes are there, and what is wanted is the rest
/// of [`fetch_and_verify`] — the hash and the signature — over them.
struct CachedFetcher;

impl PackageFetcher for CachedFetcher {
    fn stage(
        &self,
        _id: &str,
        _source: &crate::index::EntrySource,
        _into: &Path,
    ) -> Result<(), RegistryError> {
        Ok(())
    }
}

struct Staged {
    dir: PathBuf,
    verified: Option<Verified>,
    resolved: Option<String>,
}

/// Remove an installed extension.
///
/// # Errors
///
/// [`RegistryError::NotInstalled`] when it is nowhere, and
/// [`RegistryError::AmbiguousRemove`] when it is at more than one layer and
/// `target` did not say which.
pub fn remove(
    layout: &Layout,
    id: &str,
    target: Option<Target>,
) -> Result<PathBuf, RegistryError> {
    let found = layout.installed(id);
    if found.is_empty() {
        return Err(RegistryError::NotInstalled { id: id.to_owned() });
    }
    let chosen = match target {
        Some(t) => found
            .iter()
            .find(|(layer, _)| *layer == t)
            .ok_or_else(|| RegistryError::NotInstalled {
                id: format!("{id} at the {} layer", t.as_str()),
            })?
            .clone(),
        None if found.len() == 1 => found[0].clone(),
        None => {
            return Err(RegistryError::AmbiguousRemove {
                id: id.to_owned(),
                layers: found
                    .iter()
                    .map(|(t, p)| format!("{} ({})", t.as_str(), p.display()))
                    .collect::<Vec<_>>()
                    .join(" and "),
            });
        }
    };
    remove_path(&chosen.1)?;
    // The receipt outlives nothing: an id reinstalled by hand must not inherit
    // the pin decision of the copy that was removed.
    remove_path(&layout.receipt_path(chosen.0, id))?;
    Ok(chosen.1)
}

/// Delete a directory, a symlink to one, or a file.
fn remove_path(path: &Path) -> Result<(), RegistryError> {
    let meta = match path.symlink_metadata() {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(RegistryError::io(path, &e)),
    };
    let result = if meta.file_type().is_symlink() {
        // A symlink to a directory must be unlinked, not walked: removing its
        // contents would delete the developer's working copy.
        remove_symlink(path)
    } else if meta.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    };
    result.map_err(|e| RegistryError::io(path, &e))
}

#[cfg(windows)]
fn remove_symlink(path: &Path) -> std::io::Result<()> {
    std::fs::remove_dir(path).or_else(|_| std::fs::remove_file(path))
}

#[cfg(not(windows))]
fn remove_symlink(path: &Path) -> std::io::Result<()> {
    std::fs::remove_file(path)
}

/// Create a directory symlink, or say why it could not be created.
///
/// On Windows this needs Developer Mode or `SeCreateSymbolicLinkPrivilege`, and
/// a machine without either gets [`RegistryError::SymlinkUnavailable`] naming
/// both — an install that silently copied instead would be an install whose
/// `--link` did not link.
pub fn symlink_dir(target: &Path, link: &Path) -> Result<(), RegistryError> {
    #[cfg(windows)]
    let result = std::os::windows::fs::symlink_dir(target, link);
    #[cfg(not(windows))]
    let result = std::os::unix::fs::symlink(target, link);
    result.map_err(|e| RegistryError::SymlinkUnavailable {
        path: link.to_path_buf(),
        message: e.to_string(),
    })
}

/// Whether this machine can make a directory symlink at all.
///
/// Used by the tests, which have to assert one thing on a machine with
/// Developer Mode and another on a machine without: `--link` either links, or
/// says why it cannot. Silently copying is the one outcome that is wrong.
#[must_use]
pub fn symlinks_available(scratch: &Path) -> bool {
    let target = scratch.join("orrery-symlink-probe-target");
    let link = scratch.join("orrery-symlink-probe-link");
    let _ = std::fs::create_dir_all(&target);
    let _ = remove_path(&link);
    let ok = symlink_dir(&target, &link).is_ok();
    let _ = remove_path(&link);
    let _ = remove_path(&target);
    ok
}

fn absolute(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().map_or_else(|_| path.to_path_buf(), |cwd| cwd.join(path))
    }
}

fn sanitise(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect()
}

/// The capabilities an already-installed extension was approved for.
///
/// Approximated by what its manifest asks for, and the approximation is
/// deliberate: what was *granted* lives in the policy layer, and reading it
/// here would make an install need a session. The consequence is written down
/// rather than hidden — an upgrade diff can show a row as already-allowed that
/// the user had in fact denied, which errs towards *not* re-asking. When the
/// policy store can be read without a session, this is where it plugs in.
#[must_use]
pub fn previously_approved(manifest: &ExtensionManifest) -> Vec<Capability> {
    manifest.capabilities()
}

/// The record a refused install leaves behind.
///
/// A refusal is still a supply-chain event: "nothing happened" and "the managed
/// layer stopped it" must not look the same in an audit.
#[must_use]
pub fn refusal_record(ext: ExtId, source: &Source, why: &RegistryError) -> SupplyChainRecord {
    SupplyChainRecord {
        ext,
        source: source.label(),
        pinned: false,
        development: false,
        rule: None,
        reason: Some(UnpinnedReason::NotFromTheRegistry),
        index: None,
        refused: Some(why.to_string()),
    }
}

/// The system git, for a caller that has no reason to care which.
#[must_use]
pub fn default_git_runner() -> impl GitRunner {
    SystemGit
}
