//! Pinning, and the managed switch an enterprise actually sets.
//!
//! # The one line
//!
//! ```toml
//! # managed.toml — not user-overridable
//! [registry]
//! index    = "https://registry.corp.internal/orrery/index.toml"
//! key      = "…"
//! unpinned = "refuse"        # refuse | warn | allow
//! ```
//!
//! # Why the override is impossible rather than merely disallowed
//!
//! [`effective_unpinned`] takes the managed setting and the user setting and,
//! when the managed layer said anything at all, returns the managed one. There
//! is no merge, no "the stricter wins", no precedence table to get backwards
//! later. A user layer that sets `unpinned = "allow"` under a managed
//! `"refuse"` is not an error and does not warn: it is simply not consulted,
//! which is what "not user-overridable" means.
//!
//! # Five of the seven sources bypass the registry
//!
//! That is the ergonomic choice plan 15 makes on purpose, and this module is
//! where it stays honest. Every non-registry install is recorded
//! `pinned: false` with the source named, visibly, in the supply-chain ledger —
//! and `--link` is unpinned even under `allow`, because a symlinked extension
//! can change under the harness between one run and the next.

use std::path::{Path, PathBuf};

use orrery_proto::{ExtId, LoadOutcome, SkipReason};

use crate::error::RegistryError;
use crate::index::Index;
use crate::source::Source;

/// What to do about a source the registry index does not cover.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum Unpinned {
    /// Refuse it. The single line an enterprise sets.
    Refuse,
    /// Install it, loudly.
    Warn,
    /// Install it. The developer default.
    #[default]
    Allow,
}

impl Unpinned {
    /// Parse the spelling a config file uses.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "refuse" => Unpinned::Refuse,
            "warn" => Unpinned::Warn,
            "allow" => Unpinned::Allow,
            _ => return None,
        })
    }

    /// How it is written back.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Unpinned::Refuse => "refuse",
            Unpinned::Warn => "warn",
            Unpinned::Allow => "allow",
        }
    }
}

/// The `[registry]` table, as the managed layer states it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManagedRegistry {
    /// Where the signed index lives.
    pub index: String,
    /// The signing key, hex, as distributed through the managed layer.
    pub key: Option<String>,
    /// What to do about everything the index does not cover.
    pub unpinned: Unpinned,
    /// The file that said so, so a refusal can name it.
    pub file: PathBuf,
}

impl ManagedRegistry {
    /// Read a `[registry]` table out of a managed config file's text.
    ///
    /// Returns `None` when the file has no `[registry]` table at all, which is
    /// the ordinary case on a developer machine.
    ///
    /// # Errors
    ///
    /// [`RegistryError::Syntax`] when the file does not parse, or when
    /// `unpinned` is not one of the three words.
    pub fn from_toml(text: &str, file: impl Into<PathBuf>) -> Result<Option<Self>, RegistryError> {
        let file = file.into();
        let doc: toml::Value = toml::from_str(text).map_err(|e| RegistryError::Syntax {
            file: file.display().to_string(),
            message: e.message().to_owned(),
        })?;
        let Some(table) = doc.get("registry").and_then(toml::Value::as_table) else {
            return Ok(None);
        };
        let index = table
            .get("index")
            .and_then(toml::Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let key = table
            .get("key")
            .and_then(toml::Value::as_str)
            .map(ToOwned::to_owned);
        let unpinned = match table.get("unpinned").and_then(toml::Value::as_str) {
            None => Unpinned::default(),
            Some(word) => Unpinned::parse(word).ok_or_else(|| RegistryError::Syntax {
                file: file.display().to_string(),
                message: format!(
                    "registry.unpinned = \"{word}\" is not one of refuse, warn, allow"
                ),
            })?,
        };
        Ok(Some(Self {
            index,
            key,
            unpinned,
            file,
        }))
    }

    /// Read it from a file, if the file is there.
    ///
    /// # Errors
    ///
    /// [`RegistryError::Io`] when the file exists and cannot be read, and
    /// whatever [`ManagedRegistry::from_toml`] returns.
    pub fn read(path: &Path) -> Result<Option<Self>, RegistryError> {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::from_toml(&text, path),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(RegistryError::io(path, &e)),
        }
    }
}

/// Which setting is in force.
///
/// The managed layer wins outright when it has spoken; otherwise the user's own
/// preference applies; otherwise the developer default.
#[must_use]
pub fn effective_unpinned(managed: Option<&ManagedRegistry>, user: Option<Unpinned>) -> Unpinned {
    match managed {
        Some(m) => m.unpinned,
        None => user.unwrap_or_default(),
    }
}

/// Why something is not pinned, in a closed set.
///
/// Closed for the same reason [`SkipReason`] is: "it was not pinned" is a thing
/// an auditor counts and groups, and free text cannot be counted.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum UnpinnedReason {
    /// The source is not the registry, so there is no signature to check.
    NotFromTheRegistry,
    /// A `--link` install: the code can change under the harness between runs.
    Development,
    /// In the registry's index, but with no signature that verifies.
    Unsigned,
}

impl UnpinnedReason {
    /// The word the ledger records.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            UnpinnedReason::NotFromTheRegistry => "not-from-the-registry",
            UnpinnedReason::Development => "development",
            UnpinnedReason::Unsigned => "unsigned",
        }
    }
}

/// What pinning decided about one install.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PinDecision {
    /// Whether the bytes were checked against something somebody signed.
    pub pinned: bool,
    /// The rule that verified it, when one did: the key id and the index URL.
    pub rule: Option<String>,
    /// Whether it is a `--link`, whose code can change between runs.
    pub development: bool,
    /// Why it is not pinned, when it is not.
    pub reason: Option<UnpinnedReason>,
    /// The loud line `warn` mode puts in the ledger.
    pub warning: Option<String>,
}

impl PinDecision {
    /// The decision for a verified registry install.
    #[must_use]
    pub fn verified(rule: impl Into<String>) -> Self {
        Self {
            pinned: true,
            rule: Some(rule.into()),
            development: false,
            reason: None,
            warning: None,
        }
    }
}

/// Decide whether an install may proceed, and how it is recorded.
///
/// `link` is passed separately from the source because `--link` is a flag on a
/// path install, not a seventh spelling of one.
///
/// # Errors
///
/// [`RegistryError::Unpinned`] under managed `refuse`, naming the managed file
/// and the index that is the one allowed path.
pub fn decide(
    source: &Source,
    managed: Option<&ManagedRegistry>,
    user: Option<Unpinned>,
    link: bool,
) -> Result<PinDecision, RegistryError> {
    if source.is_registry() && !link {
        // The pin itself is established by `fetch::fetch_and_verify`; what this
        // says is that the source is the one path on which that can happen.
        return Ok(PinDecision {
            pinned: true,
            rule: None,
            development: false,
            reason: None,
            warning: None,
        });
    }

    let reason = if link {
        UnpinnedReason::Development
    } else {
        UnpinnedReason::NotFromTheRegistry
    };
    let mode = effective_unpinned(managed, user);
    match mode {
        Unpinned::Refuse => Err(RegistryError::Unpinned {
            origin: source.label(),
            file: managed.map_or_else(
                || "the managed layer".to_owned(),
                |m| m.file.display().to_string(),
            ),
            index: managed.map_or_else(
                || "the registry index".to_owned(),
                |m| m.index.clone(),
            ),
        }),
        Unpinned::Warn => Ok(PinDecision {
            pinned: false,
            rule: None,
            development: link,
            reason: Some(reason),
            warning: Some(format!(
                "{}: installed unpinned ({}) — no signature and no hash were checked",
                source.label(),
                reason.as_str()
            )),
        }),
        Unpinned::Allow => Ok(PinDecision {
            pinned: false,
            rule: None,
            development: link,
            reason: Some(reason),
            warning: None,
        }),
    }
}

/// Resolve a registry source against the index, exactly.
///
/// Three refusals, and the middle one is the phase-8 criterion:
///
/// - an id the index does not have at all is [`RegistryError::NotInIndex`],
///   naming the index URL — never a fall-through to another host;
/// - a version the index does not have is [`RegistryError::VersionNotInIndex`],
///   listing what it does have, so a pinned 1.2.0 refuses 1.2.1;
/// - **no version, and the index holds several** is
///   [`RegistryError::VersionRequired`]. Not "the latest": an index is a pin
///   list, and picking newest for the user would make `orrery install x`
///   resolve differently on two machines on either side of a publish.
///
/// # Errors
///
/// The three above.
pub fn resolve_entry<'i>(
    index: &'i Index,
    source: &Source,
    index_url: &str,
) -> Result<&'i crate::index::Entry, RegistryError> {
    let Source::Registry { name, version } = source else {
        return Err(RegistryError::NotInIndex {
            id: source.label(),
            index: index_url.to_owned(),
        });
    };
    let available = index.versions_of(name);
    if available.is_empty() {
        return Err(RegistryError::NotInIndex {
            id: name.clone(),
            index: index_url.to_owned(),
        });
    }
    let listed = || {
        available
            .iter()
            .map(|e| e.version.to_string())
            .collect::<Vec<_>>()
            .join(", ")
    };
    match version {
        Some(want) => index
            .entry(name, want)
            .ok_or_else(|| RegistryError::VersionNotInIndex {
                id: name.clone(),
                version: want.to_string(),
                index: index_url.to_owned(),
                available: listed(),
            }),
        None if available.len() == 1 => Ok(available[0]),
        None => Err(RegistryError::VersionRequired {
            id: name.clone(),
            available: listed(),
        }),
    }
}

/// One line of the supply-chain ledger.
///
/// # Why this is not only a [`LoadOutcome`]
///
/// `LoadOutcome::Skipped` carries a [`SkipReason`], and that set — closed on
/// purpose — has no `Unsigned` in this build. Adding one is an `orrery-proto`
/// change with a generated-types tail behind it, so the supply-chain facts live
/// here, next to the code that establishes them, and [`Self::load_outcome`]
/// projects each record into the shape the load ledger already holds. The
/// record is what an auditor reads; the outcome is what a session reports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SupplyChainRecord {
    /// Which extension.
    pub ext: ExtId,
    /// The source, as the person typed it.
    pub source: String,
    /// Whether anything about it was signed.
    pub pinned: bool,
    /// Whether it is a `--link`.
    pub development: bool,
    /// The rule that verified it, when one did.
    pub rule: Option<String>,
    /// Why it is not pinned, when it is not.
    pub reason: Option<UnpinnedReason>,
    /// The index that was consulted, whether or not it had the answer.
    pub index: Option<String>,
    /// Set when the install was refused outright.
    pub refused: Option<String>,
}

impl SupplyChainRecord {
    /// The line a person reads.
    #[must_use]
    pub fn line(&self) -> String {
        let mut out = format!("{} from {}", self.ext.as_str(), self.source);
        if self.pinned {
            out.push_str(" — pinned");
            if let Some(rule) = &self.rule {
                out.push_str(&format!(" by {rule}"));
            }
        } else {
            out.push_str(" — UNPINNED");
            if let Some(reason) = self.reason {
                out.push_str(&format!(" ({})", reason.as_str()));
            }
        }
        if self.development {
            out.push_str(" [development]");
        }
        if let Some(index) = &self.index {
            out.push_str(&format!("; index {index}"));
        }
        if let Some(why) = &self.refused {
            out.push_str(&format!("; refused: {why}"));
        }
        out
    }

    /// The same fact, in the shape the load ledger holds.
    ///
    /// A refused install is [`LoadOutcome::Skipped`] with
    /// [`SkipReason::PolicyDenied`] — the managed layer is policy, and that is
    /// the closest true statement this build's closed set can make.
    #[must_use]
    pub fn load_outcome(&self) -> LoadOutcome {
        if self.refused.is_some() {
            LoadOutcome::Skipped {
                ext: self.ext.clone(),
                reason: SkipReason::PolicyDenied,
            }
        } else {
            LoadOutcome::Ok {
                ext: self.ext.clone(),
                contributions: Vec::new(),
                ms: 0,
            }
        }
    }
}

/// The file name a receipt is written under, inside [`RECEIPTS_DIR`].
///
/// # Why a receipt exists at all
///
/// Phase 8's criterion is that unpinned extensions refuse to **load**, not
/// merely to install. The load path has only a directory and a manifest to go
/// on, and neither says whether anybody ever checked a signature — so the
/// install writes down what it decided, and the load reads it back. Without
/// this, an extension installed before an admin set `unpinned = "refuse"` went
/// on loading forever afterwards, which is the hole this closes.
///
/// It lives **beside** the extension rather than inside it, under the layer
/// root, so that copying an extension directory onto a machine does not carry a
/// receipt with it.
pub const RECEIPTS_DIR: &str = "registry/receipts";

/// The receipt for an extension installed at `<layer>/extensions/<id>`.
///
/// `None` when the path is not shaped like an installed extension, which is how
/// an extension listed by a `[[extension]] path = …` entry — never installed,
/// so never pinned — is told apart from one the installer placed.
#[must_use]
pub fn receipt_beside(ext_dir: &Path) -> Option<PathBuf> {
    let id = ext_dir.file_name()?;
    let layer_root = ext_dir.parent()?.parent()?;
    Some(
        layer_root
            .join(RECEIPTS_DIR)
            .join(format!("{}.toml", id.to_string_lossy())),
    )
}

impl SupplyChainRecord {
    /// The receipt, as it is written to disk.
    ///
    /// Hand-written rather than derived: three scalars and three optional
    /// strings do not need a serde surface, and the format has to stay readable
    /// by a person auditing a machine.
    #[must_use]
    pub fn to_toml(&self) -> String {
        let mut out = String::from("# Written by `orrery install`. Read by the load path.\n");
        out.push_str(&format!("ext = {}\n", quote(self.ext.as_str())));
        out.push_str(&format!("source = {}\n", quote(&self.source)));
        out.push_str(&format!("pinned = {}\n", self.pinned));
        out.push_str(&format!("development = {}\n", self.development));
        if let Some(rule) = &self.rule {
            out.push_str(&format!("rule = {}\n", quote(rule)));
        }
        if let Some(reason) = self.reason {
            out.push_str(&format!("reason = {}\n", quote(reason.as_str())));
        }
        if let Some(index) = &self.index {
            out.push_str(&format!("index = {}\n", quote(index)));
        }
        out
    }

    /// Read one back.
    ///
    /// # Errors
    ///
    /// [`RegistryError::Syntax`] when the file does not parse or has no `ext`.
    pub fn from_toml(text: &str, file: impl Into<PathBuf>) -> Result<Self, RegistryError> {
        let file = file.into();
        let syntax = |message: String| RegistryError::Syntax {
            file: file.display().to_string(),
            message,
        };
        let doc: toml::Value =
            toml::from_str(text).map_err(|e| syntax(e.message().to_owned()))?;
        let string = |key: &str| {
            doc.get(key)
                .and_then(toml::Value::as_str)
                .map(ToOwned::to_owned)
        };
        let ext = string("ext").ok_or_else(|| syntax("no `ext` in the receipt".to_owned()))?;
        let ext: ExtId = ext
            .parse()
            .map_err(|e| syntax(format!("`ext` is not an extension id: {e}")))?;
        Ok(Self {
            ext,
            source: string("source").unwrap_or_default(),
            pinned: doc
                .get("pinned")
                .and_then(toml::Value::as_bool)
                .unwrap_or(false),
            development: doc
                .get("development")
                .and_then(toml::Value::as_bool)
                .unwrap_or(false),
            rule: string("rule"),
            reason: string("reason").and_then(|r| match r.as_str() {
                "not-from-the-registry" => Some(UnpinnedReason::NotFromTheRegistry),
                "development" => Some(UnpinnedReason::Development),
                "unsigned" => Some(UnpinnedReason::Unsigned),
                _ => None,
            }),
            index: string("index"),
            refused: None,
        })
    }

    /// Read the receipt for an extension installed at `ext_dir`, if there is
    /// one this build can make sense of.
    #[must_use]
    pub fn beside(ext_dir: &Path) -> Option<Self> {
        let path = receipt_beside(ext_dir)?;
        let text = std::fs::read_to_string(&path).ok()?;
        Self::from_toml(&text, path).ok()
    }
}

/// A TOML basic string, with the two characters that can appear in a path
/// escaped.
fn quote(s: &str) -> String {
    let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

/// Whether the **load** path must refuse this extension, and why.
///
/// The install-time gate ([`decide`]) cannot answer this on its own: it runs
/// once, when the extension arrives, and an admin who sets
/// `unpinned = "refuse"` afterwards is making a statement about what may run
/// from now on, not about what was installed last month. So the same managed
/// setting is consulted again at load, against the receipt the install left.
///
/// A missing receipt refuses under `refuse`, deliberately: the only way to have
/// no receipt is to predate this check or to have been placed by hand, and
/// neither is a signature.
#[must_use]
pub fn load_refusal(
    managed: Option<&ManagedRegistry>,
    receipt: Option<&SupplyChainRecord>,
) -> Option<String> {
    let managed = managed?;
    if managed.unpinned != Unpinned::Refuse {
        return None;
    }
    match receipt {
        Some(r) if r.pinned && !r.development => None,
        Some(r) => Some(format!(
            "unpinned ({}) — {} sets `registry.unpinned = \"refuse\"`, so only the signed \
             index at {} may be loaded from",
            r.reason.map_or("no signature was checked", UnpinnedReason::as_str),
            managed.file.display(),
            managed.index,
        )),
        None => Some(format!(
            "no install receipt, so nothing about it was ever verified — {} sets \
             `registry.unpinned = \"refuse\"`, so only the signed index at {} may be \
             loaded from",
            managed.file.display(),
            managed.index,
        )),
    }
}

/// Everything installed, and how it was checked.
#[derive(Clone, Debug, Default)]
pub struct SupplyChainLedger {
    records: Vec<SupplyChainRecord>,
}

impl SupplyChainLedger {
    /// Add one.
    pub fn record(&mut self, record: SupplyChainRecord) {
        self.records.push(record);
    }

    /// Everything, in the order it happened.
    #[must_use]
    pub fn all(&self) -> &[SupplyChainRecord] {
        &self.records
    }

    /// The unpinned ones, which is the question this ledger exists to answer.
    #[must_use]
    pub fn unpinned(&self) -> Vec<&SupplyChainRecord> {
        self.records.iter().filter(|r| !r.pinned).collect()
    }

    /// The line for one extension, if there is one.
    #[must_use]
    pub fn of(&self, ext: &str) -> Option<&SupplyChainRecord> {
        self.records.iter().find(|r| r.ext.as_str() == ext)
    }
}
