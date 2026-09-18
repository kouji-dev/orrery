//! Discovery: **one pass** over extensions, skills, prompts and MCP servers
//! across the layers in force.
//!
//! One pass is not an optimisation. Four separate walks over the same tree can
//! disagree — a directory that appears between two of them ends up in one set
//! and not another — and §4.11 says that if it is in the model's visible set it
//! is in the manifest. So the walk is instrumented and every directory is read
//! exactly once, which is what `discover::one_pass` asserts.

use std::path::{Path, PathBuf};

use orrery_proto::Layer;

use crate::merge::IgnoredClaim;
use crate::provenance::Provenanced;

/// What kind of thing was discovered.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ItemKind {
    /// An extension.
    Extension,
    /// A skill.
    Skill,
    /// A prompt.
    Prompt,
    /// An MCP server.
    McpServer,
}

impl ItemKind {
    /// The directory a layer keeps these in.
    #[must_use]
    pub const fn dir(self) -> &'static str {
        match self {
            ItemKind::Extension => "extensions",
            ItemKind::Skill => "skills",
            ItemKind::Prompt => "prompts",
            ItemKind::McpServer => "mcp",
        }
    }

    /// The config key that declares these by name.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            ItemKind::Extension => "extensions",
            ItemKind::Skill => "skills",
            ItemKind::Prompt => "prompts",
            ItemKind::McpServer => "mcp_servers",
        }
    }

    /// The kinds, in a fixed order.
    #[must_use]
    pub const fn all() -> [ItemKind; 4] {
        [
            ItemKind::Extension,
            ItemKind::Skill,
            ItemKind::Prompt,
            ItemKind::McpServer,
        ]
    }
}

/// One thing discovery found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Discovered {
    /// What it is.
    pub kind: ItemKind,
    /// Its name.
    pub name: String,
    /// Which layer contributed it.
    pub layer: Layer,
    /// Where it was found: a directory on disk, or the config file that named it.
    pub source: PathBuf,
}

/// What discovery produced.
///
/// This **is** the load ledger's input: what is in here is what the ledger
/// reports, and a thing in neither is not in the model's visible set.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DiscoveryManifest {
    /// Extensions, in layer order.
    pub extensions: Vec<Discovered>,
    /// Skills.
    pub skills: Vec<Discovered>,
    /// Prompts.
    pub prompts: Vec<Discovered>,
    /// MCP servers.
    pub mcp_servers: Vec<Discovered>,
}

impl DiscoveryManifest {
    /// Everything, in kind order.
    #[must_use]
    pub fn all(&self) -> Vec<&Discovered> {
        self.extensions
            .iter()
            .chain(&self.skills)
            .chain(&self.prompts)
            .chain(&self.mcp_servers)
            .collect()
    }

    /// Whether a name of a kind is in the manifest.
    #[must_use]
    pub fn has(&self, kind: ItemKind, name: &str) -> bool {
        self.of(kind).iter().any(|d| d.name == name)
    }

    /// One kind's list.
    #[must_use]
    pub fn of(&self, kind: ItemKind) -> &[Discovered] {
        match kind {
            ItemKind::Extension => &self.extensions,
            ItemKind::Skill => &self.skills,
            ItemKind::Prompt => &self.prompts,
            ItemKind::McpServer => &self.mcp_servers,
        }
    }

    fn push(&mut self, item: Discovered) {
        let list = match item.kind {
            ItemKind::Extension => &mut self.extensions,
            ItemKind::Skill => &mut self.skills,
            ItemKind::Prompt => &mut self.prompts,
            ItemKind::McpServer => &mut self.mcp_servers,
        };
        if !list.iter().any(|d| d.name == item.name) {
            list.push(item);
        }
    }
}

/// How discovery reaches the filesystem.
///
/// A trait so a test can count what was read: "the filesystem is walked once"
/// is an assertion, not a hope.
pub trait Walk {
    /// The entries of a directory, or an empty list when it is not there.
    fn read_dir(&self, dir: &Path) -> Vec<PathBuf>;
}

/// The real filesystem.
#[derive(Copy, Clone, Debug, Default)]
pub struct FsWalk;

impl Walk for FsWalk {
    fn read_dir(&self, dir: &Path) -> Vec<PathBuf> {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut out: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        out.sort();
        out
    }
}

/// A file that was not loaded, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Skipped {
    /// The file or directory.
    pub path: PathBuf,
    /// Which layer it would have been.
    pub layer: Layer,
    /// Why not, in words a person can act on.
    pub why: String,
}

/// How one discovered thing fared.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    /// It is in force.
    Loaded,
    /// It was found and not loaded.
    Skipped(String),
}

/// One line of the load ledger.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LedgerEntry {
    /// What it is.
    pub kind: ItemKind,
    /// Its name.
    pub name: String,
    /// Which layer.
    pub layer: Layer,
    /// How it fared.
    pub status: Status,
}

/// What the session will answer `query extensions` with.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LoadLedger {
    /// One line per discovered thing.
    pub entries: Vec<LedgerEntry>,
    /// Config files that were not loaded at all.
    pub files: Vec<Skipped>,
    /// Trust claims a local layer made, which were dropped.
    pub claims: Vec<IgnoredClaim>,
}

impl LoadLedger {
    /// The files trust kept out.
    #[must_use]
    pub fn skipped_for_trust(&self) -> Vec<&Skipped> {
        self.files
            .iter()
            .filter(|s| s.why.contains("trust"))
            .collect()
    }

    /// The trust claims that were ignored.
    #[must_use]
    pub fn ignored_trust_claims(&self) -> &[IgnoredClaim] {
        &self.claims
    }

    /// The names the ledger reports as loaded, for one kind.
    #[must_use]
    pub fn loaded(&self, kind: ItemKind) -> Vec<&str> {
        self.entries
            .iter()
            .filter(|e| e.kind == kind && e.status == Status::Loaded)
            .map(|e| e.name.as_str())
            .collect()
    }

    /// Record everything a manifest holds as loaded.
    pub fn record(&mut self, manifest: &DiscoveryManifest) {
        for item in manifest.all() {
            self.entries.push(LedgerEntry {
                kind: item.kind,
                name: item.name.clone(),
                layer: item.layer,
                status: Status::Loaded,
            });
        }
    }
}

/// A directory one layer contributes items from.
#[derive(Clone, Debug)]
pub struct LayerRoot {
    /// Which layer.
    pub layer: Layer,
    /// The directory — `~/.orrery`, `<workspace>/.orrery`, and so on.
    pub dir: PathBuf,
}

/// One pass over the layers in force.
///
/// Every directory is read **once**: the layer's own directory, then each of
/// the four item directories under it. Names declared in configuration are
/// folded in from `values`, which has already been merged, so nothing is read
/// twice there either.
#[must_use]
pub fn discover(roots: &[LayerRoot], values: &Provenanced, walk: &dyn Walk) -> DiscoveryManifest {
    let mut manifest = DiscoveryManifest::default();

    for root in roots {
        // One read of the layer directory, then one read per item directory
        // that it actually has. Nothing is read twice, and nothing is read
        // that does not exist.
        let present: Vec<PathBuf> = walk.read_dir(&root.dir);
        for kind in ItemKind::all() {
            let dir = root.dir.join(kind.dir());
            if !present.iter().any(|p| p == &dir) {
                continue;
            }
            for entry in walk.read_dir(&dir) {
                if let Some(name) = item_name(&entry) {
                    manifest.push(Discovered {
                        kind,
                        name,
                        layer: root.layer,
                        source: entry,
                    });
                }
            }
        }
    }

    // Declared by name in configuration, closest layer first.
    for kind in ItemKind::all() {
        let key = kind.key();
        if kind == ItemKind::McpServer {
            for full in values.keys_under(key) {
                let Some(rest) = full.strip_prefix(&format!("{key}.")) else {
                    continue;
                };
                let name = rest.split('.').next().unwrap_or(rest).to_owned();
                if let Some(slot) = values.winner(full) {
                    manifest.push(Discovered {
                        kind,
                        name,
                        layer: slot.origin.layer,
                        source: slot.origin.file.clone(),
                    });
                }
            }
            continue;
        }
        if let Some(slot) = values.winner(key) {
            for name in values.strings(key) {
                manifest.push(Discovered {
                    kind,
                    name,
                    layer: slot.origin.layer,
                    source: slot.origin.file.clone(),
                });
            }
        }
    }

    manifest
}

/// A directory's name, or a file's stem.
fn item_name(path: &Path) -> Option<String> {
    if path.is_dir() {
        return path.file_name().map(|n| n.to_string_lossy().into_owned());
    }
    path.file_stem().map(|n| n.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    use crate::layer::LayerFile;

    /// A walker that records every directory it is asked for, so "walked once"
    /// is an assertion rather than a hope.
    #[derive(Default)]
    struct Counting {
        inner: FsWalk,
        seen: RefCell<Vec<PathBuf>>,
    }

    impl Walk for Counting {
        fn read_dir(&self, dir: &Path) -> Vec<PathBuf> {
            self.seen.borrow_mut().push(dir.to_path_buf());
            self.inner.read_dir(dir)
        }
    }

    fn fixture() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("a temp dir");
        let root = dunce::canonicalize(dir.path()).unwrap().join(".orrery");
        for (sub, name) in [
            ("extensions", "git"),
            ("skills", "review-checklist"),
            ("prompts", "triage"),
            ("mcp", "ripgrep"),
        ] {
            std::fs::create_dir_all(root.join(sub).join(name)).unwrap();
        }
        // A directory discovery must not wander into.
        std::fs::create_dir_all(root.join("extensions/git/src")).unwrap();
        (dir, root)
    }

    fn values(text: &str) -> Provenanced {
        let file = LayerFile::new(Layer::Workspace, "config.toml", text);
        crate::merge::merge(&[file]).expect("the fixture parses").values
    }

    #[test]
    fn one_pass() {
        let (_guard, root) = fixture();
        let roots = vec![LayerRoot {
            layer: Layer::Workspace,
            dir: root.clone(),
        }];
        let walk = Counting::default();
        let manifest = discover(&roots, &Provenanced::new(), &walk);

        // All four kinds, from one traversal.
        assert_eq!(manifest.extensions.len(), 1, "{manifest:?}");
        assert!(manifest.has(ItemKind::Extension, "git"));
        assert!(manifest.has(ItemKind::Skill, "review-checklist"));
        assert!(manifest.has(ItemKind::Prompt, "triage"));
        assert!(manifest.has(ItemKind::McpServer, "ripgrep"));

        let seen = walk.seen.borrow().clone();
        let mut unique = seen.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(seen.len(), unique.len(), "no directory is read twice: {seen:?}");
        assert_eq!(
            seen.len(),
            5,
            "the layer directory and its four item directories, and nothing else: {seen:?}"
        );
        assert!(
            !seen.iter().any(|p| p.ends_with("src")),
            "discovery does not descend into an item: {seen:?}"
        );
    }

    #[test]
    fn manifest_matches_the_ledger() {
        let (_guard, root) = fixture();
        let roots = vec![LayerRoot {
            layer: Layer::Workspace,
            dir: root,
        }];
        let values = values("extensions = [\"lsp\"]\n[mcp_servers.github]\ncommand = \"gh\"\n");
        let manifest = discover(&roots, &values, &FsWalk);

        let mut ledger = LoadLedger::default();
        ledger.record(&manifest);

        // §4.11: if it is in the model's visible set, it is in the manifest —
        // and what the ledger reports is exactly that set, no more and no less.
        for kind in ItemKind::all() {
            let in_manifest: Vec<&str> =
                manifest.of(kind).iter().map(|d| d.name.as_str()).collect();
            assert_eq!(ledger.loaded(kind), in_manifest, "{kind:?}");
        }
        assert!(manifest.has(ItemKind::Extension, "lsp"), "declared by name");
        assert!(manifest.has(ItemKind::Extension, "git"), "found on disk");
        assert!(manifest.has(ItemKind::McpServer, "github"));
        assert_eq!(ledger.entries.len(), manifest.all().len());
    }
}
