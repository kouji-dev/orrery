//! Compiled selectors, and the normalisation that has to happen before they run.
//!
//! # Paths normalise before they match
//!
//! A rule that can be defeated by a symlink is not a rule. Both sides — the
//! pattern and the call's target — go through the same normalisation: made
//! absolute against the workspace root, symlinks resolved as far as the
//! filesystem will say, `..` collapsed, the Windows `\\?\` verbatim prefix
//! stripped by `dunce`, separators turned into `/`, and case-folded where the
//! filesystem is case-insensitive.
//!
//! # And it still is not enforcement
//!
//! **Rules decide whether to ask; the broker and the token decide what can be
//! touched.** A `spawn(curl *)` deny stops `curl https://x` and does not stop
//! `/usr/bin/curl https://x` or `sh -c 'curl https://x'`. Patterns describe
//! intent. A `spawn` grant is enforced where the process is created.

use std::path::{Component, Path, PathBuf};

use orrery_proto::Aspect;
use regex::Regex;

use crate::call::PendingCall;
use crate::error::PolicyError;
use crate::rule::{Rule, Selector, SelectorKind};

/// True where the filesystem folds case, so a rule must fold it too.
#[must_use]
pub const fn fs_is_case_insensitive() -> bool {
    cfg!(any(windows, target_os = "macos"))
}

/// One compiled pattern.
#[derive(Debug)]
enum Matcher {
    /// Matches anything, including an empty target.
    Any,
    /// `*` is any text. Used for ids, domains, command text and scope names.
    Name(Regex),
    /// gitignore-style: `*` within a segment, `**` any depth.
    Path(globset::GlobMatcher),
    /// The escape hatch.
    Raw(Regex),
}

impl Matcher {
    fn is_match(&self, haystack: &str) -> bool {
        match self {
            Matcher::Any => true,
            Matcher::Name(re) | Matcher::Raw(re) => re.is_match(haystack),
            Matcher::Path(glob) => glob.is_match(haystack),
        }
    }
}

/// A rule with its patterns compiled against a workspace root.
#[derive(Debug)]
pub struct Compiled {
    /// The rule this came from.
    pub rule: Rule,
    root: PathBuf,
    primary: Matcher,
    specifier: Option<Matcher>,
    params: Vec<(String, Matcher)>,
    regex: Option<Matcher>,
}

impl Compiled {
    /// Compile one rule's selector against a workspace root.
    ///
    /// # Errors
    ///
    /// When a glob or a `re:` pattern does not compile.
    pub fn new(rule: Rule, root: &Path) -> Result<Self, PolicyError> {
        let Selector {
            primary,
            specifier,
            params,
            regex,
        } = rule.selector.clone();

        let primary_m = match &primary {
            None => Matcher::Any,
            Some(p) => compile(rule.kind, p, root, &rule.text)?,
        };
        let specifier_m = specifier
            .as_deref()
            .map(|s| compile(SelectorKind::Command, s, root, &rule.text))
            .transpose()?;
        let mut params_m = Vec::with_capacity(params.len());
        for (k, v) in &params {
            params_m.push((k.clone(), compile(SelectorKind::Name, v, root, &rule.text)?));
        }
        let regex_m = regex
            .as_deref()
            .map(|r| {
                Regex::new(r)
                    .map(Matcher::Raw)
                    .map_err(|e| PolicyError::pattern(&rule.text, e.to_string()))
            })
            .transpose()?;

        Ok(Self {
            rule,
            root: root.to_path_buf(),
            primary: primary_m,
            specifier: specifier_m,
            params: params_m,
            regex: regex_m,
        })
    }

    /// Whether this rule selects the call.
    ///
    /// Every term has to hold: the aspect, the main pattern, the tool's own
    /// specifier when the rule names one, and every `param:` term. A `re:` rule
    /// matches on the regex alone.
    #[must_use]
    pub fn matches(&self, call: &PendingCall) -> bool {
        if call.aspect != self.rule.aspect {
            return false;
        }
        if let Some(regex) = &self.regex {
            return regex.is_match(&call.match_text());
        }
        let target = match self.rule.kind {
            SelectorKind::Path => normalise_path(&call.target, &self.root),
            _ => fold(&call.target),
        };
        if !self.primary.is_match(&target) {
            return false;
        }
        if let Some(spec) = &self.specifier {
            let Some(written) = call.specifier.as_deref() else {
                return false;
            };
            if !spec.is_match(&fold(written)) {
                return false;
            }
        }
        for (key, want) in &self.params {
            let Some(got) = call.params.get(key) else {
                return false;
            };
            if !want.is_match(&fold(got)) {
                return false;
            }
        }
        true
    }
}

fn compile(
    kind: SelectorKind,
    pattern: &str,
    root: &Path,
    whole: &str,
) -> Result<Matcher, PolicyError> {
    if pattern == "*" || pattern == "**" {
        return Ok(Matcher::Any);
    }
    match kind {
        SelectorKind::Path => {
            let absolute = normalise_pattern(pattern, root);
            globset::GlobBuilder::new(&absolute)
                .literal_separator(true)
                .case_insensitive(fs_is_case_insensitive())
                .build()
                .map(|g| Matcher::Path(g.compile_matcher()))
                .map_err(|e| PolicyError::pattern(whole, e.to_string()))
        }
        _ => {
            let mut re = String::from("^");
            for part in fold(pattern).split('*') {
                if !re.ends_with('^') {
                    re.push_str(".*");
                }
                re.push_str(&regex::escape(part));
            }
            if fold(pattern).ends_with('*') {
                re.push_str(".*");
            }
            re.push('$');
            Regex::new(&re)
                .map(Matcher::Name)
                .map_err(|e| PolicyError::pattern(whole, e.to_string()))
        }
    }
}

/// Case-fold where the filesystem does, so a rule is not defeated by a capital.
#[must_use]
pub fn fold(text: &str) -> String {
    if fs_is_case_insensitive() {
        text.to_lowercase()
    } else {
        text.to_owned()
    }
}

/// A rule's path pattern, made absolute against the root and normalised.
///
/// The glob metacharacters survive: only the literal head of the pattern is
/// resolved, because `canonicalize` on `./src/**` would fail.
#[must_use]
pub fn normalise_pattern(pattern: &str, root: &Path) -> String {
    let raw = pattern.replace('\\', "/");
    let joined = if is_absolute_ish(&raw) {
        PathBuf::from(&raw)
    } else {
        root.join(raw.trim_start_matches("./"))
    };
    fold(&to_slash(&lexical(&joined)))
}

/// A call's path target, resolved as far as the filesystem will say.
///
/// Symlinks are followed, `..` is collapsed, a UNC or drive-relative form is
/// reduced to the same shape a rule compiles to, and the result is case-folded
/// where the filesystem is case-insensitive. A path that does not exist yet —
/// a file about to be created — falls back to lexical normalisation of its
/// deepest existing ancestor.
#[must_use]
pub fn normalise_path(target: &str, root: &Path) -> String {
    let raw = target.replace('\\', "/");
    let joined = if is_absolute_ish(&raw) {
        PathBuf::from(&raw)
    } else {
        root.join(raw.trim_start_matches("./"))
    };
    let resolved = resolve_existing_prefix(&joined);
    fold(&to_slash(&resolved))
}

/// `dunce::canonicalize` the deepest existing ancestor, then re-attach the rest.
fn resolve_existing_prefix(path: &Path) -> PathBuf {
    let mut suffix: Vec<std::ffi::OsString> = Vec::new();
    let mut probe = path.to_path_buf();
    loop {
        if let Ok(real) = dunce::canonicalize(&probe) {
            let mut out = real;
            for part in suffix.iter().rev() {
                out.push(part);
            }
            return lexical(&out);
        }
        let Some(name) = probe.file_name().map(std::ffi::OsStr::to_os_string) else {
            return lexical(path);
        };
        suffix.push(name);
        if !probe.pop() {
            return lexical(path);
        }
    }
}

/// Collapse `.` and `..` without touching the filesystem, and reduce the
/// Windows extended-length form so every key has one shape.
fn lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    unverbatim(&out)
}

/// Reduce `\?\` to the plain form, **for comparison only**.
///
/// [`dunce::simplified`] deliberately refuses to do this once the result would
/// pass `MAX_PATH`, because the plain form could no longer be opened. That
/// caution is right for a path about to be opened and wrong for a key about to
/// be matched: it left a long target in verbatim form while every rule compiled
/// to the plain one, so nothing matched and a *length* came back out of the
/// engine as "no rule allows read(...)" — a filesystem limit wearing a
/// permission decision's clothes. Nothing in this module opens anything.
#[cfg(windows)]
fn unverbatim(path: &Path) -> PathBuf {
    use std::path::Prefix;

    let Some(Component::Prefix(prefix)) = path.components().next() else {
        return path.to_path_buf();
    };
    let text = path.as_os_str().to_string_lossy();
    match prefix.kind() {
        // `\?\C:\a` -> `C:\a`
        Prefix::VerbatimDisk(_) => PathBuf::from(text[4..].to_owned()),
        // `\?\UNC\server\share\a` -> `\server\share\a`
        Prefix::VerbatimUNC(..) => PathBuf::from(format!(r"\{}", &text[8..])),
        // `\?\` over something that is not a disk or a share has no plain form
        // to reduce to; leaving it alone keeps it comparable with itself.
        _ => path.to_path_buf(),
    }
}

#[cfg(not(windows))]
fn unverbatim(path: &Path) -> PathBuf {
    path.to_path_buf()
}

fn to_slash(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn is_absolute_ish(raw: &str) -> bool {
    raw.starts_with('/')
        || raw.starts_with("//")
        || raw
            .as_bytes()
            .get(1)
            .is_some_and(|b| *b == b':' && raw.as_bytes()[0].is_ascii_alphabetic())
}

/// Which aspects are about a path, and therefore normalise.
#[must_use]
pub const fn is_path_aspect(aspect: Aspect) -> bool {
    matches!(aspect, Aspect::Read | Aspect::Write)
}
