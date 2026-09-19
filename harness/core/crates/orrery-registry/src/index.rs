//! The index document: what the organisation says exists, and at which hashes.
//!
//! The registry is a **pin list, not a package host**. This module owns the
//! document's shape and its two refusals — an unknown schema and a closed
//! validity window — because both have to happen before anything reads an
//! entry, let alone fetches one.
//!
//! # Time, without a clock
//!
//! Every expiry check takes the instant to compare against as an argument.
//! There is no `SystemTime::now()` anywhere under `src/`, so "the index
//! expired" is a test with a literal in it rather than a test that waits.

use std::collections::BTreeMap;
use std::fmt;

use orrery_proto::{Aspect, Capability};
use serde::{Deserialize, Serialize};

use crate::error::RegistryError;

/// The schema this build reads. An index that says anything else is refused.
pub const SCHEMA: u32 = 1;

/// An RFC 3339 instant in UTC, as `YYYYMMDDHHMMSS`.
///
/// Packed into one integer so comparison is comparison. The original string is
/// kept for messages, because `20261218000000` is not what anybody wrote.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Timestamp {
    packed: u64,
    text: String,
}

impl Timestamp {
    /// Parse `YYYY-MM-DDTHH:MM:SSZ`, with an optional fractional part.
    ///
    /// # Errors
    ///
    /// [`RegistryError::BadTimestamp`] for anything else. Offsets other than
    /// `Z` are refused rather than converted: an index is a document several
    /// people read, and a local-time expiry is a document that means different
    /// things to each of them.
    pub fn parse(field: &str, value: &str) -> Result<Self, RegistryError> {
        let bad = || RegistryError::BadTimestamp {
            field: field.to_owned(),
            value: value.to_owned(),
        };
        let body = value.strip_suffix('Z').ok_or_else(bad)?;
        // Drop a fractional second if there is one; seconds are the resolution
        // an index needs.
        let body = body.split('.').next().unwrap_or(body);
        let (date, time) = body.split_once('T').ok_or_else(bad)?;
        let date: Vec<&str> = date.split('-').collect();
        let time: Vec<&str> = time.split(':').collect();
        if date.len() != 3 || time.len() != 3 {
            return Err(bad());
        }
        let mut packed: u64 = 0;
        for (part, width) in [
            (date[0], 4u32),
            (date[1], 2),
            (date[2], 2),
            (time[0], 2),
            (time[1], 2),
            (time[2], 2),
        ] {
            if part.len() != width as usize || !part.bytes().all(|b| b.is_ascii_digit()) {
                return Err(bad());
            }
            let n: u64 = part.parse().map_err(|_| bad())?;
            packed = packed * 10u64.pow(width) + n;
        }
        Ok(Self {
            packed,
            text: value.to_owned(),
        })
    }

    /// The text it was parsed from.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.text
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

/// Where a package's bytes actually come from.
///
/// The registry indexes; it does not host. `kind` is pinned per variant because
/// `crates-io` is not what any `rename_all` rule produces from `CratesIo`.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum EntrySource {
    /// A crates.io crate.
    #[serde(rename = "crates-io")]
    CratesIo {
        /// The crate name.
        name: String,
    },
    /// An npm package.
    #[serde(rename = "npm")]
    Npm {
        /// The package name.
        name: String,
    },
    /// A tarball at a URL the organisation controls.
    #[serde(rename = "url")]
    Url {
        /// Where.
        url: String,
    },
}

impl std::str::FromStr for EntrySource {
    type Err = RegistryError;

    /// The same spelling [`EntrySource`] prints, so `orrery registry add
    /// --source crates-io:x` and what the index shows are one string.
    ///
    /// An index pins **three** of [`crate::VOCABULARY`] — the three whose bytes
    /// the registry can hash. The others are install-only, and saying so is not
    /// the same as having a second vocabulary: `crate:` means here exactly what
    /// it means at `orrery install`, and a refusal here prints the same list
    /// that one does.
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let bad = |message: &str| RegistryError::SourceSyntax {
            input: text.to_owned(),
            message: message.to_owned(),
        };
        let (kind, rest) = text.split_once(':').ok_or_else(|| {
            bad("expected `crates-io:<name>`, `npm:<name>` or `url:<url>`")
        })?;
        if rest.is_empty() {
            return Err(bad("names nothing after the `:`"));
        }
        Ok(match kind {
            // `crate` is the other spelling `orrery install` accepts for the
            // same concept. It parses to the same variant and is written back
            // as `crates-io:`, so an index holds one spelling.
            "crates-io" | "crate" => EntrySource::CratesIo {
                name: rest.to_owned(),
            },
            "npm" => EntrySource::Npm {
                name: rest.to_owned(),
            },
            "url" => EntrySource::Url {
                url: rest.to_owned(),
            },
            other => {
                return Err(bad(&format!(
                    "`{other}` is not something an index can pin; an index pins                      `crates-io:<name>`, `npm:<name>` and `url:<url>`. The full                      source vocabulary, which `orrery install` takes, is {vocabulary}",
                    vocabulary = crate::source::VOCABULARY
                )));
            }
        })
    }
}

impl fmt::Display for EntrySource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EntrySource::CratesIo { name } => write!(f, "crates-io:{name}"),
            EntrySource::Npm { name } => write!(f, "npm:{name}"),
            EntrySource::Url { url } => write!(f, "url:{url}"),
        }
    }
}

/// One pinned extension version.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// The extension's namespace.
    pub id: String,
    /// Its version, exactly. Not a range: a range is not a pin.
    pub version: semver::Version,
    /// Where the bytes come from.
    pub source: EntrySource,
    /// The hash the bytes must have, lowercase hex.
    pub sha256: String,
    /// The signature over this entry, lowercase hex, optionally `<key-id>:<hex>`.
    pub sig: String,
    /// What the manifest asks for, mirrored here on purpose.
    ///
    /// So an admin reviewing a version bump sees a capability change **without
    /// downloading the package**. A mismatch against the fetched manifest is a
    /// hard failure — see [`crate::verify::check_requires`].
    #[serde(default)]
    pub requires: Vec<String>,
}

impl Entry {
    /// The bytes the entry signature covers: `(id, version, source, sha256)`.
    ///
    /// Written out field by field rather than re-serialising, because a
    /// canonical form that depends on a serialiser's field order is a canonical
    /// form that changes when somebody reorders a struct.
    #[must_use]
    pub fn signed_bytes(&self) -> Vec<u8> {
        format!(
            "orrery-registry-entry/1\nid={}\nversion={}\nsource={}\nsha256={}\n",
            self.id, self.version, self.source, self.sha256
        )
        .into_bytes()
    }

    /// What this entry says the extension asks for.
    ///
    /// # Errors
    ///
    /// [`RegistryError::Syntax`] when a `requires` string is not
    /// `aspect` or `aspect(scope, …)`.
    pub fn capabilities(&self) -> Result<Vec<Capability>, RegistryError> {
        merge_capabilities(&self.requires, &self.id)
    }
}

/// The document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Index {
    /// The schema version.
    pub schema: u32,
    /// When it was issued.
    pub issued: String,
    /// When it stops being valid.
    pub expires: String,
    /// The pins, in the order the document lists them.
    #[serde(default, rename = "extension")]
    pub extensions: Vec<Entry>,
}

impl Index {
    /// Parse and validate the document's own invariants.
    ///
    /// Validates the schema and both timestamps. It does **not** check expiry
    /// or any signature: those need an instant and a keyring, and a parser that
    /// silently needed a clock would be a parser nobody could test.
    ///
    /// # Errors
    ///
    /// [`RegistryError::Syntax`], [`RegistryError::UnknownSchema`] or
    /// [`RegistryError::BadTimestamp`].
    pub fn parse(text: &str, file: impl Into<String>) -> Result<Self, RegistryError> {
        let file = file.into();
        let index: Index = toml::from_str(text).map_err(|e| RegistryError::Syntax {
            file: file.clone(),
            message: e.message().to_owned(),
        })?;
        if index.schema != SCHEMA {
            return Err(RegistryError::UnknownSchema {
                file,
                found: index.schema,
                supported: SCHEMA,
            });
        }
        let _ = index.issued_at()?;
        let _ = index.expires_at()?;
        Ok(index)
    }

    /// Render back to TOML.
    ///
    /// # Errors
    ///
    /// [`RegistryError::Syntax`] when the document cannot be serialised, which
    /// in practice means a `requires` string this build cannot round-trip.
    pub fn to_toml(&self) -> Result<String, RegistryError> {
        toml::to_string_pretty(self).map_err(|e| RegistryError::Syntax {
            file: "<in memory>".to_owned(),
            message: e.to_string(),
        })
    }

    /// When it was issued.
    ///
    /// # Errors
    ///
    /// [`RegistryError::BadTimestamp`].
    pub fn issued_at(&self) -> Result<Timestamp, RegistryError> {
        Timestamp::parse("issued", &self.issued)
    }

    /// When it stops being valid.
    ///
    /// # Errors
    ///
    /// [`RegistryError::BadTimestamp`].
    pub fn expires_at(&self) -> Result<Timestamp, RegistryError> {
        Timestamp::parse("expires", &self.expires)
    }

    /// Refuse an index whose validity window has closed.
    ///
    /// # Errors
    ///
    /// [`RegistryError::Expired`], naming the index URL so the message says
    /// what to do next.
    pub fn check_fresh(
        &self,
        now: &Timestamp,
        file: &str,
        index_url: &str,
    ) -> Result<(), RegistryError> {
        let expires = self.expires_at()?;
        if now > &expires {
            return Err(RegistryError::Expired {
                file: file.to_owned(),
                expires: expires.text,
                now: now.text.clone(),
                index: index_url.to_owned(),
            });
        }
        Ok(())
    }

    /// Every version of one id, newest last.
    #[must_use]
    pub fn versions_of(&self, id: &str) -> Vec<&Entry> {
        let mut found: Vec<&Entry> = self.extensions.iter().filter(|e| e.id == id).collect();
        found.sort_by(|a, b| a.version.cmp(&b.version));
        found
    }

    /// One exact pin.
    #[must_use]
    pub fn entry(&self, id: &str, version: &semver::Version) -> Option<&Entry> {
        self.extensions
            .iter()
            .find(|e| e.id == id && &e.version == version)
    }
}

/// Parse one `requires` string: `read`, or `read($WORKSPACE/**, ./x)`.
///
/// # Errors
///
/// [`RegistryError::Syntax`] for an unknown aspect or an unclosed bracket.
pub fn parse_requirement(text: &str, file: &str) -> Result<Capability, RegistryError> {
    let text = text.trim();
    let bad = |message: &str| RegistryError::Syntax {
        file: file.to_owned(),
        message: format!("`{text}`: {message}"),
    };
    let (aspect_text, scope) = match text.split_once('(') {
        None => (text, Vec::new()),
        Some((head, rest)) => {
            let inner = rest
                .strip_suffix(')')
                .ok_or_else(|| bad("unclosed `(` in a requires entry"))?;
            let scope: Vec<String> = inner
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(ToOwned::to_owned)
                .collect();
            (head.trim(), scope)
        }
    };
    let aspect = aspect_from_str(aspect_text).ok_or_else(|| bad("not a capability aspect"))?;
    Ok(Capability { aspect, scope })
}

/// Render a capability the way an index writes it.
#[must_use]
pub fn render_requirement(cap: &Capability) -> String {
    let aspect = aspect_str(cap.aspect);
    if cap.scope.is_empty() {
        aspect.to_owned()
    } else {
        format!("{aspect}({})", cap.scope.join(", "))
    }
}

/// Parse a whole `requires` list, folding repeats of one aspect together.
///
/// # Errors
///
/// [`RegistryError::Syntax`] from [`parse_requirement`].
pub fn merge_capabilities(
    requires: &[String],
    file: &str,
) -> Result<Vec<Capability>, RegistryError> {
    let mut by_aspect: BTreeMap<Aspect, Vec<String>> = BTreeMap::new();
    let mut unqualified: BTreeMap<Aspect, bool> = BTreeMap::new();
    for line in requires {
        let cap = parse_requirement(line, file)?;
        let slot = by_aspect.entry(cap.aspect).or_default();
        if cap.scope.is_empty() {
            unqualified.insert(cap.aspect, true);
        } else {
            for s in cap.scope {
                if !slot.contains(&s) {
                    slot.push(s);
                }
            }
        }
    }
    Ok(by_aspect
        .into_iter()
        .map(|(aspect, scope)| {
            if unqualified.get(&aspect).copied().unwrap_or(false) {
                Capability::all(aspect)
            } else {
                Capability { aspect, scope }
            }
        })
        .collect())
}

/// The aspect names, as the rule grammar spells them.
///
/// A second copy of `orrery-policy`'s table would be one copy too many, but
/// `orrery-policy`'s is private to its parser and this crate does not otherwise
/// depend on it. The set is closed and pinned by `index::every_aspect_round_trips`.
#[must_use]
pub fn aspect_from_str(s: &str) -> Option<Aspect> {
    Some(match s {
        "tool" => Aspect::Tool,
        "mcp" => Aspect::Mcp,
        "skill" => Aspect::Skill,
        "ext" => Aspect::Ext,
        "mode" => Aspect::Mode,
        "read" => Aspect::Read,
        "write" => Aspect::Write,
        "spawn" => Aspect::Spawn,
        "net" => Aspect::Net,
        "creds" => Aspect::Creds,
        "ui" => Aspect::Ui,
        "render" => Aspect::Render,
        "mem.read" => Aspect::MemRead,
        "mem.write" => Aspect::MemWrite,
        _ => return None,
    })
}

/// The name an aspect is written under.
#[must_use]
pub fn aspect_str(aspect: Aspect) -> &'static str {
    match aspect {
        Aspect::Tool => "tool",
        Aspect::Mcp => "mcp",
        Aspect::Skill => "skill",
        Aspect::Ext => "ext",
        Aspect::Mode => "mode",
        Aspect::Read => "read",
        Aspect::Write => "write",
        Aspect::Spawn => "spawn",
        Aspect::Net => "net",
        Aspect::Creds => "creds",
        Aspect::Ui => "ui",
        Aspect::Render => "render",
        Aspect::MemRead => "mem.read",
        Aspect::MemWrite => "mem.write",
        _ => "unknown",
    }
}
