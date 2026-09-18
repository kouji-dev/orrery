//! The rule grammar, as data.
//!
//! A [`Rule`] is the parsed form: raw pattern text, the list it was written in,
//! the layer and the file and line it came from. It is deliberately not
//! compiled — path patterns are relative to a workspace root that is not known
//! until the layers are resolved, so compilation happens in
//! [`crate::r#match`].

use std::path::PathBuf;

use orrery_proto::{Aspect, Layer, RuleId, Subject};
use serde::{Deserialize, Serialize};

/// Which of the three lists a rule was written in.
///
/// Evaluated in this order — deny, then ask, then allow — with **first match
/// wins and specificity deliberately irrelevant**. An allow can never carve an
/// exception out of a deny.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RuleList {
    /// Refused.
    Deny,
    /// The user is asked.
    Ask,
    /// Allowed without asking.
    Allow,
}

impl RuleList {
    /// The three lists in evaluation order.
    #[must_use]
    pub const fn order() -> [RuleList; 3] {
        [RuleList::Deny, RuleList::Ask, RuleList::Allow]
    }

    /// The keyword as written in a config file.
    #[must_use]
    pub const fn keyword(self) -> &'static str {
        match self {
            RuleList::Deny => "deny",
            RuleList::Ask => "ask",
            RuleList::Allow => "allow",
        }
    }
}

/// What kind of thing the main pattern is about, which decides how it matches.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum SelectorKind {
    /// A namespaced id: `ripgrep.search`, `github.get_*`. `*` is any text.
    Name,
    /// A filesystem path, gitignore-style: `*` within a segment, `**` any depth.
    Path,
    /// A host name: `*.corp.internal`.
    Domain,
    /// Command text: `bazel *`.
    Command,
    /// A memory scope name, which is never a path.
    Scope,
}

impl SelectorKind {
    /// The kind an aspect's specifier is written in.
    #[must_use]
    pub const fn of(aspect: Aspect) -> Self {
        match aspect {
            Aspect::Read | Aspect::Write => SelectorKind::Path,
            Aspect::Net => SelectorKind::Domain,
            Aspect::Spawn => SelectorKind::Command,
            Aspect::MemRead | Aspect::MemWrite => SelectorKind::Scope,
            _ => SelectorKind::Name,
        }
    }
}

/// What a rule selects, before compilation.
///
/// The operator set is small on purpose: `*` any text, `**` any depth in a path,
/// a trailing `prefix:*`, and `param:key=value` to match one named input. The
/// `re:` escape hatch is off by default and warned about at load, because a
/// permission rule nobody can read at a glance is a governance problem rather
/// than a feature.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Selector {
    /// The main pattern. `None` means unqualified: the whole aspect.
    pub primary: Option<String>,
    /// The tool's own specifier, after a `:` — `tool(shell.exec: npm run *)`.
    pub specifier: Option<String>,
    /// Named-input matches, `param:key=glob`.
    pub params: Vec<(String, String)>,
    /// The `re:` escape hatch, when one was written.
    pub regex: Option<String>,
}

impl Selector {
    /// The unqualified selector: the whole aspect.
    #[must_use]
    pub fn any() -> Self {
        Self::default()
    }

    /// A selector over one pattern.
    #[must_use]
    pub fn on(primary: impl Into<String>) -> Self {
        Self {
            primary: Some(primary.into()),
            ..Self::default()
        }
    }

    /// Add the tool's own specifier.
    #[must_use]
    pub fn with_specifier(mut self, specifier: impl Into<String>) -> Self {
        self.specifier = Some(specifier.into());
        self
    }

    /// Add a named-input match.
    #[must_use]
    pub fn with_param(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.params.push((key.into(), value.into()));
        self
    }

    /// The `re:` form.
    #[must_use]
    pub fn regex(pattern: impl Into<String>) -> Self {
        Self {
            regex: Some(pattern.into()),
            ..Self::default()
        }
    }
}

/// Where a rule was written.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Source {
    /// The file it came from.
    pub file: PathBuf,
    /// The line, 1-based. Zero when it was built in code rather than parsed.
    pub line: u32,
}

impl Source {
    /// A file and a line.
    #[must_use]
    pub fn new(file: impl Into<PathBuf>, line: u32) -> Self {
        Self {
            file: file.into(),
            line,
        }
    }
}

impl std::fmt::Display for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.line == 0 {
            write!(f, "{}", self.file.display())
        } else {
            write!(f, "{}:{}", self.file.display(), self.line)
        }
    }
}

/// One rule: what it is about, what it selects, and where it came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rule {
    /// So a denial can name what denied it.
    pub id: RuleId,
    /// Which list it was written in.
    pub list: RuleList,
    /// Which layer contributed it.
    pub layer: Layer,
    /// Who it is about.
    pub subject: Subject,
    /// What it is about.
    pub aspect: Aspect,
    /// How the specifier is read.
    pub kind: SelectorKind,
    /// What it selects.
    pub selector: Selector,
    /// The file and line it was written on.
    pub source: Source,
    /// Exactly as written, so an explanation can quote it.
    pub text: String,
}

impl Rule {
    /// A rule built in code rather than parsed, for tests and for defaults.
    #[must_use]
    pub fn new(list: RuleList, layer: Layer, subject: Subject, aspect: Aspect, text: &str) -> Self {
        let selector = crate::parse::selector_of(aspect, text).unwrap_or_default();
        Self {
            id: RuleId::new(),
            list,
            layer,
            subject,
            aspect,
            kind: SelectorKind::of(aspect),
            selector,
            source: Source::default(),
            text: text.to_owned(),
        }
    }
}
