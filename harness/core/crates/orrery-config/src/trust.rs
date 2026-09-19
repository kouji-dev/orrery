//! Trust gating: project-local config, extensions and interceptors do not load
//! until the project is trusted.
//!
//! # Nothing a project supplies gets a vote
//!
//! The decision is made at step 2 of the startup order, from the **managed,
//! organisation and user** layers plus the stored answer for this path, and
//! from nothing else. A workspace or project file that says `trust = true` is
//! not consulted, not merged and not believed — it is reported as an ignored
//! claim and dropped on the floor. That ordering is the whole point: an
//! extension cannot influence the decision that governs whether it loads.
//!
//! # Where the answers live
//!
//! Open question 3, decided: **the trust store is a file of its own in the user
//! directory**, `~/.orrery/trust.toml`, and [`TrustStore::open`] refuses to
//! operate if that directory is inside the workspace it is answering about. It
//! is not in `config.toml`, so a `config explain` dump, a shared dotfile repo or
//! a config-writing command cannot flip it by accident, and it is never
//! somewhere a hostile project can write.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::error::ConfigError;
use crate::provenance::Provenanced;

/// The file the answers live in, inside the user directory.
pub const TRUST_FILE: &str = "trust.toml";

/// Whether this workspace's own configuration and code may load.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TrustState {
    /// The workspace and project layers are in force.
    Trusted,
    /// They are not. The session runs with user-level config.
    Untrusted,
}

impl TrustState {
    /// Whether the local layers load.
    #[must_use]
    pub const fn is_trusted(self) -> bool {
        matches!(self, TrustState::Trusted)
    }
}

/// What decided it.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TrustSource {
    /// A managed `trust.mode` that no closer layer may relax.
    Managed,
    /// The stored answer for this path.
    Stored,
    /// A managed, organisation or user `trust.auto`.
    Auto,
    /// Nobody has answered, and the default is not to trust.
    Unanswered,
}

/// The decision, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrustDecision {
    /// What was decided.
    pub state: TrustState,
    /// What decided it.
    pub source: TrustSource,
    /// In words a person can act on.
    pub why: String,
}

/// The stored answers, one per path.
///
/// Held outside every project-writable path — see the module docs.
#[derive(Clone, Debug)]
pub struct TrustStore {
    path: PathBuf,
    answers: BTreeMap<String, bool>,
}

impl TrustStore {
    /// Open, or start, the store in a user directory.
    ///
    /// # Errors
    ///
    /// When `user_dir` is inside `workspace_root` — a store a project could
    /// write is not a store — or when the file exists and cannot be read or
    /// parsed.
    pub fn open(
        user_dir: impl AsRef<Path>,
        workspace_root: impl AsRef<Path>,
    ) -> Result<Self, ConfigError> {
        let user_dir = user_dir.as_ref();
        let root = workspace_root.as_ref();
        let resolved = dunce::canonicalize(user_dir).unwrap_or_else(|_| user_dir.to_path_buf());
        let root = dunce::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
        if resolved == root || resolved.starts_with(&root) {
            return Err(ConfigError::TrustStoreInsideProject { path: resolved });
        }

        let path = resolved.join(TRUST_FILE);
        let answers = match std::fs::read_to_string(&path) {
            Ok(text) => parse(&text, &path)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
            Err(source) => {
                return Err(ConfigError::Io {
                    file: path.clone(),
                    source,
                });
            }
        };
        Ok(Self { path, answers })
    }

    /// Where the answers are kept.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The stored answer for a path, if there is one.
    #[must_use]
    pub fn answer(&self, workspace: impl AsRef<Path>) -> Option<bool> {
        self.answers.get(&key(workspace.as_ref())).copied()
    }

    /// Every stored answer, path first, in path order.
    ///
    /// What `orrery trust list` prints. Without it the store was write-only
    /// from outside the crate: there was no way to ask it what had been
    /// answered, and therefore no way for a person to find out why a workspace
    /// they thought they had trusted was not loading its own configuration.
    pub fn answers(&self) -> impl Iterator<Item = (&str, bool)> {
        self.answers.iter().map(|(path, trusted)| (path.as_str(), *trusted))
    }

    /// The key a workspace path is stored under: canonical, as a string.
    ///
    /// The same normalisation [`answer`](Self::answer) and
    /// [`record`](Self::record) use, exposed so a caller that prints an entry
    /// can tell which one is *this* workspace.
    #[must_use]
    pub fn key_for(workspace: impl AsRef<Path>) -> String {
        key(workspace.as_ref())
    }

    /// Record an answer for a path, and persist it.
    ///
    /// # Errors
    ///
    /// When the store cannot be written.
    pub fn record(&mut self, workspace: impl AsRef<Path>, trusted: bool) -> Result<(), ConfigError> {
        self.answers.insert(key(workspace.as_ref()), trusted);
        self.flush()
    }

    /// Forget an answer, and persist that.
    ///
    /// # Errors
    ///
    /// When the store cannot be written.
    pub fn forget(&mut self, workspace: impl AsRef<Path>) -> Result<(), ConfigError> {
        self.answers.remove(&key(workspace.as_ref()));
        self.flush()
    }

    fn flush(&self) -> Result<(), ConfigError> {
        let mut doc = toml_edit::DocumentMut::new();
        let mut table = toml_edit::Table::new();
        for (path, trusted) in &self.answers {
            table.insert(path, toml_edit::value(*trusted));
        }
        doc.insert("paths", toml_edit::Item::Table(table));
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| ConfigError::Io {
                file: parent.to_path_buf(),
                source,
            })?;
        }
        std::fs::write(&self.path, doc.to_string()).map_err(|source| ConfigError::Io {
            file: self.path.clone(),
            source,
        })
    }
}

fn parse(text: &str, path: &Path) -> Result<BTreeMap<String, bool>, ConfigError> {
    let doc: toml::Value = text.parse().map_err(|e: toml::de::Error| ConfigError::Syntax {
        file: path.to_path_buf(),
        // Not 0: the store is a file a person can open, and a parse error that
        // will not say which line is a parse error nobody can act on.
        line: crate::layer::line_of_toml(text, &e),
        message: e.to_string(),
    })?;
    let mut out = BTreeMap::new();
    if let Some(paths) = doc.get("paths").and_then(toml::Value::as_table) {
        for (k, v) in paths {
            if let Some(b) = v.as_bool() {
                out.insert(k.clone(), b);
            }
        }
    }
    Ok(out)
}

fn key(path: &Path) -> String {
    dunce::canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .display()
        .to_string()
}

/// Decide trust for a workspace.
///
/// `base` is the merge of the **managed, organisation and user** layers and
/// nothing else. Passing it anything a project wrote would defeat the point, so
/// [`crate::resolve`] is the only caller and it merges the base layers first.
///
/// Precedence, furthest-reaching first:
///
/// 1. A managed `trust.mode = "never"` — final; no closer layer may relax it.
/// 2. The stored answer for this path.
/// 3. A managed, organisation or user `trust.auto = true`, or
///    `trust.mode = "always"`.
/// 4. Otherwise untrusted, because the default for code nobody has vouched for
///    is not to run it.
#[must_use]
pub fn decide(base: &Provenanced, store: Option<&TrustStore>, workspace: &Path) -> TrustDecision {
    if let Some(slot) = base.winner("trust.mode") {
        if slot.value.as_str() == Some("never") && slot.origin.layer == orrery_proto::Layer::Managed
        {
            return TrustDecision {
                state: TrustState::Untrusted,
                source: TrustSource::Managed,
                why: format!("`trust.mode = \"never\"` at {}", slot.origin),
            };
        }
    }

    if let Some(stored) = store.and_then(|s| s.answer(workspace)) {
        return TrustDecision {
            state: if stored {
                TrustState::Trusted
            } else {
                TrustState::Untrusted
            },
            source: TrustSource::Stored,
            why: format!(
                "the stored answer for `{}` is {stored}",
                workspace.display()
            ),
        };
    }

    let auto = base.bool("trust.auto") == Some(true)
        || base.str("trust.mode") == Some("always");
    if auto {
        let origin = base
            .winner("trust.auto")
            .or_else(|| base.winner("trust.mode"))
            .map(|s| s.origin.to_string())
            .unwrap_or_default();
        return TrustDecision {
            state: TrustState::Trusted,
            source: TrustSource::Auto,
            why: format!("trust is automatic here, set at {origin}"),
        };
    }

    TrustDecision {
        state: TrustState::Untrusted,
        source: TrustSource::Unanswered,
        why: format!(
            "nobody has answered for `{}`, and an unanswered workspace is not trusted",
            workspace.display()
        ),
    }
}

/// Whether a dotted key is a trust claim, which a local layer may never make.
#[must_use]
pub fn is_trust_claim(key: &str) -> bool {
    key == "trust" || key.starts_with("trust.")
}
